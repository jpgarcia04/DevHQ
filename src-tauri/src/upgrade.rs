//! The explicit upgrade: the only way DevHQ changes the schema of an inventory that exists, and only
//! when the person asks for it in the window (Back up and upgrade). No opening, read, supervision,
//! backup or tool of the agent gate does it.
//!
//! What the window shows before the person decides (`inspect`) is read without changing any file,
//! on a connection that cannot write (`inventory::examine_where`): the version the file declares,
//! what it holds table by table, whether DevHQ can upgrade it and, if so, a fingerprint of its
//! content, which the window sends back with the person's consent. Upgrading (`upgrade`) then:
//!
//! 1. looks at the files again without changing anything, and opens the upgrade's own connection
//!    (`inventory::open_to_upgrade`) only if none needs recovering;
//! 2. takes SQLite's write lock (`BEGIN IMMEDIATE`), which no other connection, in this process or
//!    another, can take until this transaction ends, and inside it finds the inventory the person
//!    consented to: the same version and fingerprint, exactly the structure the migrations give
//!    that version, every page sound (`integrity_check`) and no broken relationship;
//! 3. backs it up from a second, read-only connection with `VACUUM INTO`, which reads the last
//!    committed state, the one the lock keeps; verifies the copy on its own against what the lock
//!    keeps (version, structure, integrity, and every row of every table, value by value); forces it
//!    to disk and names it (`backup::copy_verified`), writes its manifest beside it the same way, and
//!    forces both names to disk where the system can confirm them (`backup::names_on_disk`);
//! 4. applies the migrations in the same transaction;
//! 5. checks the result before committing: integrity, relationships, this build's structure, and
//!    every row still there with every value it had (`check_upgraded`);
//! 6. commits.
//!
//! SQLite's transaction makes 4 to 6 all or nothing: any failure, and the process ending for any
//! reason before the commit, leaves the inventory holding what it held, with the verified backup
//! beside it. Nothing is migrated before the backup and its manifest are forced to disk under their
//! names, nor once the system reported that any of that failed. Windows documents no way to force
//! a name to disk, only a file's content (see `backup::names_on_disk`).

use crate::backup;
use crate::inventory::{self, stored_at, ApiError, Found, SCHEMA_VERSION};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

/// How many rows a table holds.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TableRows {
    pub table: String,
    pub rows: i64,
}

/// What the window shows of the inventory before the person decides whether to upgrade it, read
/// without changing any of its files.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub database: String,
    /// Where the file system really keeps it, when that is not `database`.
    pub stored_at: Option<String>,
    /// What was found there (`outdated`, `current`, `needs_recovery`…): only `outdated` is upgraded.
    pub state: &'static str,
    /// The schema version the file declares, when one could be read.
    pub stored_version: Option<i64>,
    /// This DevHQ's schema version, the one it upgrades to.
    pub version: i64,
    /// What that means, in a sentence or two.
    pub message: String,
    /// Whether Back up and upgrade is offered: an inventory in an older schema with exactly that
    /// version's structure, sound, with no broken relationship, read whole.
    pub upgradable: bool,
    /// Why DevHQ will not upgrade it, one finding each; empty when it will.
    pub problems: Vec<String>,
    /// Each table with its rows, when the file could be read.
    pub tables: Vec<TableRows>,
    /// What the person consents to: the content as read, which the upgrade finds again under its
    /// lock before it writes anything. Only when `upgradable`.
    pub fingerprint: Option<String>,
    /// The folder the backup goes to.
    pub backups: String,
}

/// The verified copy the upgrade made before changing anything.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BeforeUpgrade {
    pub name: String,
    pub path: String,
    /// Where the file system really keeps it, when that is not `path`.
    pub stored_at: Option<String>,
    /// The manifest beside it, which says what it is.
    pub manifest: String,
    /// When the copy was taken (UTC), as its name says.
    pub taken_at: String,
    pub size_bytes: u64,
    /// SHA-256 of the file's bytes, as the manifest records it.
    pub sha256: String,
    /// The schema version it has: the inventory's before the upgrade.
    pub schema_version: i64,
    /// What `PRAGMA integrity_check` said of the copy.
    pub integrity_check: String,
    /// Each table with its rows, every one compared value by value with the inventory.
    pub tables: Vec<TableRows>,
}

/// How an upgrade ended, once committed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Upgraded {
    /// The schema version the inventory had: this build's when it already had it.
    pub from: i64,
    pub to: i64,
    /// The copy made before upgrading; None when the inventory was already in this build's
    /// schema, another DevHQ having upgraded it meanwhile, and nothing was written.
    pub backup: Option<BeforeUpgrade>,
    /// Tables and rows carried over, each row checked value by value before the commit.
    pub tables_kept: usize,
    pub rows_kept: i64,
}

impl Upgraded {
    /// Nothing upgraded: the inventory is already in this build's schema.
    pub fn already() -> Self {
        Self {
            from: SCHEMA_VERSION,
            to: SCHEMA_VERSION,
            backup: None,
            tables_kept: 0,
            rows_kept: 0,
        }
    }
}

/// Where an upgrade is, as it goes: each step is sent once DevHQ reached it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "step", rename_all = "camelCase")]
pub enum Step {
    /// Looking at the inventory again, and taking the write lock.
    Checking,
    /// Copying the inventory and verifying the copy. Nothing has been changed.
    BackingUp,
    /// The copy is verified and named, with its manifest. Nothing has been changed.
    BackedUp { backup: Box<BeforeUpgrade> },
    /// Applying the migrations, in a transaction nothing outside it sees yet.
    Upgrading,
    /// Checking the upgraded inventory before committing.
    Verifying,
}

/// The inspection of the inventory at `path` when the window serves it: in this build's schema,
/// with nothing to upgrade. Told without looking at the file, which the window's own connections
/// may hold (see `inspect`).
pub fn serving(path: &Path) -> Inspection {
    Inspection {
        database: path.display().to_string(),
        stored_at: stored_at(path),
        state: Found::Current.state(),
        stored_version: Some(SCHEMA_VERSION),
        version: SCHEMA_VERSION,
        message:
            "DevHQ serves this inventory: it is in this DevHQ's schema, with nothing to upgrade."
                .to_owned(),
        upgradable: false,
        problems: vec![],
        tables: vec![],
        fingerprint: None,
        backups: backup::folder_of(path).display().to_string(),
    }
}

/// What the inventory at `path` is, looked at without changing any file. On Unix it must not run
/// while this process holds a connection to the inventory (see `inventory::examine`).
pub fn inspect(path: &Path) -> Inspection {
    let (found, read) = inventory::examine_where(
        path,
        |found| matches!(found, Found::Outdated(_)),
        |db| {
            // One read transaction: one state, from the first table to the last.
            let tx = db.transaction().map_err(ApiError::database)?;
            Snapshot::read(&tx).map_err(ApiError::database)
        },
    );
    let mut inspection = Inspection {
        database: path.display().to_string(),
        stored_at: stored_at(path),
        state: found.state(),
        stored_version: found.stored(),
        version: SCHEMA_VERSION,
        message: found.meaning(),
        upgradable: false,
        problems: vec![],
        tables: vec![],
        fingerprint: None,
        backups: backup::folder_of(path).display().to_string(),
    };
    match (found, read) {
        (Found::Outdated(version), Some(Ok(snapshot))) => {
            inspection.message = format!(
                "A DevHQ inventory in schema version {version}, older than this DevHQ's ({SCHEMA_VERSION}). DevHQ read it without changing it, and serves nothing from it until it is upgraded."
            );
            inspection.tables = snapshot.table_rows();
            inspection.upgradable = snapshot.problems.is_empty();
            if inspection.upgradable {
                inspection.fingerprint = Some(snapshot.fingerprint());
            }
            inspection.problems = snapshot.problems;
        }
        (_, Some(Err(error))) => inspection.problems = vec![error.message],
        // What was found, what is not known, and what keeps every way back open. No backup is
        // presumed: the files may come from an upgrade that ended before its commit, from an older
        // DevHQ or another program that ended in the middle of a write, or from anywhere else.
        (Found::NeedsRecovery(_), _) => inspection.problems = vec![
            "Its files need recovering before anything in them can be read, and DevHQ recovers files only for an inventory in its own schema. So it neither backs this one up nor upgrades it, and it recovered, moved, deleted and replaced none of its files.".to_owned(),
            "What they hold is unknown until they are recovered, and so is whether any backup holds the same: the inventory's latest committed changes may be only in them. Without its -journal the file may be left half-written, and without its -wal committed changes are lost.".to_owned(),
            "Keep inventory.sqlite3 and, where they are, its -journal, -wal and -shm together and unchanged, and copy them all to another folder before anything else. Do not open them meanwhile with another program, which would recover them in place, nor with an older DevHQ, which may also upgrade them without a backup. A file's name does not make it this inventory's backup: with DevHQ closed, put a backup in their place only if its .json manifest names this inventory, its SHA-256 is the one the manifest records, and it holds the same as a copy of these files recovered apart. If DevHQ was upgrading this inventory when it closed, the backup that upgrade made is the one to compare.".to_owned(),
        ],
        _ => {}
    }
    inspection
}

/// When `upgrade_with` lets a test act, as a crash or another program could.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Moment {
    /// The migrations ran in the transaction, which is neither checked nor committed.
    Migrated,
    /// The upgrade committed; its connection is still open.
    Committed,
}

/// What a test can do to the copy, on its `.partial`, before it is verified (see `backup::write`).
pub(crate) type Damage<'a> = &'a mut dyn FnMut(&Path) -> std::io::Result<()>;

/// Upgrades the inventory at `path`, if the person consented to exactly what is there: an
/// inventory in schema version `from` with content `fingerprint`, as `inspect` showed it. See the
/// module's documentation for the order of things. On Unix it must not run while this process holds
/// another connection to the inventory (see `inventory::examine`).
pub fn upgrade(
    path: &Path,
    from: i64,
    fingerprint: &str,
    progress: &mut dyn FnMut(Step),
) -> Result<Upgraded, ApiError> {
    upgrade_with(
        path,
        (from, fingerprint),
        &inventory::MIGRATIONS,
        &crate::records::now_ms,
        progress,
        (&mut |_| Ok(()), backup::names_on_disk),
        &mut |_| Ok(()),
    )
}

/// `upgrade`, applying `migrations`, naming the backup by `clock`, letting a test `damage` the
/// backup's copy, force its `names` to disk its own way and act `meanwhile` (see `Moment`).
pub(crate) fn upgrade_with(
    path: &Path,
    (from, fingerprint): (i64, &str),
    migrations: &[(i64, &str)],
    clock: &dyn Fn() -> i64,
    progress: &mut dyn FnMut(Step),
    (damage, names): (Damage, backup::Names),
    meanwhile: &mut dyn FnMut(Moment) -> std::io::Result<()>,
) -> Result<Upgraded, ApiError> {
    progress(Step::Checking);
    // The connection below can write, so SQLite would recover, at its first read, files left
    // needing it: it opens only once a look that changes nothing found none.
    match inventory::examine(path, |_| Ok(())).0 {
        Found::Current => return Ok(Upgraded::already()),
        Found::Outdated(_) => {}
        found => return Err(found.refusal(path)),
    }
    let mut db = inventory::open_to_upgrade(path)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| {
            refused(&format!(
                "DevHQ could not take the inventory's write lock: {error}. Another program may \
                 be writing to it, perhaps another DevHQ upgrading it."
            ))
        })?;
    // From here on nothing else can change the file until this transaction ends.
    let before = match inventory::classify(&tx).map_err(ApiError::database)? {
        // Another DevHQ upgraded it since the look: nothing to do, and no second backup.
        Found::Current => return Ok(Upgraded::already()),
        Found::Outdated(_) => Snapshot::read(&tx).map_err(ApiError::database)?,
        found => return Err(found.refusal(path)),
    };
    if before.version != from || before.fingerprint() != fingerprint {
        return Err(ApiError {
            code: "upgrade_changed",
            message: format!(
                "The inventory at {} is not what DevHQ showed you any more (it now declares schema version {}, and its content changed or was replaced), so DevHQ did not upgrade it: nothing was written. Look at it again before deciding.",
                path.display(),
                before.version
            ),
        });
    }
    if !before.problems.is_empty() {
        return Err(refused(&format!(
            "DevHQ does not upgrade this inventory: {}.",
            before.problems.join("; ")
        )));
    }

    progress(Step::BackingUp);
    let backup = back_up(path, &before, clock(), damage, names)?;
    progress(Step::BackedUp {
        backup: Box::new(backup.clone()),
    });

    let failed = |cause: &str| rolled_back(cause, before.version, &backup.path);
    progress(Step::Upgrading);
    inventory::apply_migrations(&tx, before.version, migrations)
        .map_err(|error| failed(&format!("{}.", error.message)))?;
    meanwhile(Moment::Migrated).map_err(|error| failed(&error.to_string()))?;
    progress(Step::Verifying);
    let (tables_kept, rows_kept) = check_upgraded(&tx, &before).map_err(|cause| failed(&cause))?;
    tx.commit()
        .map_err(|error| failed(&format!("Committing the upgrade failed: {error}.")))?;
    meanwhile(Moment::Committed).map_err(|error| ApiError::database(error.to_string()))?;
    Ok(Upgraded {
        from: before.version,
        to: SCHEMA_VERSION,
        backup: Some(backup),
        tables_kept,
        rows_kept,
    })
}

/// Why an upgrade whose backup was made did not happen: `cause`, and that nothing of it was
/// committed, so that the inventory is still in schema `from`, with its backup at `kept`.
fn rolled_back(cause: &str, from: i64, kept: &str) -> ApiError {
    ApiError {
        code: "upgrade_failed",
        message: format!(
            "{cause} Nothing of the upgrade was committed: the inventory holds what it held \
             before, in schema version {from}. The backup made before it is kept at {kept}."
        ),
    }
}

/// Why DevHQ does not upgrade the inventory as it is; nothing was written.
fn refused(why: &str) -> ApiError {
    ApiError {
        code: "upgrade_refused",
        message: format!("{why} Nothing was written."),
    }
}

/// Makes the copy of the inventory at `path` that precedes its upgrade, while the upgrade's
/// transaction holds the write lock: the last committed state, `before`. The copy is verified
/// against `before` and named (see `backup::copy_verified`), then its manifest is written beside it,
/// and then `names` forces both names to disk.
fn back_up(
    path: &Path,
    before: &Snapshot,
    taken_at: i64,
    damage: Damage,
    names: backup::Names,
) -> Result<BeforeUpgrade, ApiError> {
    let nothing = |error: ApiError| ApiError {
        code: error.code,
        message: format!(
            "{} Nothing was upgraded: the inventory holds what it held before.",
            error.message
        ),
    };
    let name = backup::file_name(taken_at, &format!("-v{}", before.version)).ok_or_else(|| {
        nothing(ApiError::backup(
            "The system clock is out of range. Nothing was written.",
        ))
    })?;
    // A second connection, beside the one that holds the write lock, since VACUUM INTO cannot run
    // inside a transaction. It only reads, and what it reads is the last committed state, which the
    // lock keeps as `before` found it.
    let source = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .and_then(|db| db.busy_timeout(Duration::from_secs(5)).map(|()| db))
    .map_err(|error| {
        nothing(ApiError::backup(format!(
            "DevHQ could not read the inventory to copy it: {error}."
        )))
    })?;
    let copy = backup::copy_verified(
        &source,
        path,
        &name,
        |partial| damage(partial),
        |partial| verify_copy(partial, before),
    )
    .map_err(nothing)?;
    drop(source);
    let backup = BeforeUpgrade {
        stored_at: stored_at(&copy.path),
        path: copy.path.display().to_string(),
        manifest: String::new(),
        taken_at: crate::records::format_time(taken_at).unwrap_or_default(),
        size_bytes: copy.size_bytes,
        sha256: copy.verified,
        schema_version: before.version,
        integrity_check: "ok".to_owned(),
        tables: before.table_rows(),
        name,
    };
    let manifest = Manifest::of(path, &backup);
    let mut bytes = serde_json::to_vec_pretty(&manifest).map_err(ApiError::database)?;
    bytes.push(b'\n');
    let written = backup::write_new(path, &format!("{}.json", backup.name), &bytes)
        .map_err(|cause| {
            nothing(ApiError::backup(format!(
                "{cause} The backup {} was made and verified, but DevHQ upgrades nothing without its manifest.",
                backup.path
            )))
        })?;
    // Both names are in the backups folder: one trip to disk for the two. DevHQ does not upgrade
    // what it may not be able to put back.
    names(&backup::folder_of(path)).map_err(|error| {
        nothing(ApiError::backup(format!(
            "The backup {} and its manifest are complete and verified, but the system did not confirm that their names reached the disk ({error}), so a crash of the system or a power cut could still lose them, and DevHQ upgrades nothing it might not be able to put back. They were kept.",
            backup.path
        )))
    })?;
    Ok(BeforeUpgrade {
        manifest: written.display().to_string(),
        ..backup
    })
}

/// Reads the copy at `copy` back on its own and checks that it is `before`: the same schema version
/// with exactly its structure, sound, with no broken relationship, and every table with the same
/// rows, value by value. Gives the SHA-256 of its bytes.
fn verify_copy(copy: &Path, before: &Snapshot) -> Result<String, String> {
    let db = Connection::open_with_flags(
        copy,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("The copy cannot be read back: {error}."))?;
    let copied =
        Snapshot::read(&db).map_err(|error| format!("The copy cannot be read back: {error}."))?;
    if !copied.problems.is_empty() {
        return Err(format!(
            "The copy failed its verification: {}.",
            copied.problems.join("; ")
        ));
    }
    if copied.version != before.version {
        return Err(format!(
            "The copy has schema version {} instead of {}.",
            copied.version, before.version
        ));
    }
    let differing = differing_tables(&before.tables, &copied.tables);
    if !differing.is_empty() {
        return Err(format!(
            "The copy does not hold what the inventory holds: {}.",
            differing.join("; ")
        ));
    }
    drop(db);
    inventory::content(copy)
        .map(|digest| hex(&digest))
        .map_err(|error| format!("The copy cannot be read back: {error}."))
}

/// What a migration moves from one table to another, rows, rowids and values: v10 copies every row
/// of `agent_log` into the common `change_log` as an agent's change (`source` 'agent', `changes`
/// null) and drops `agent_log`. Every other table keeps its name and its rows.
const MOVED: [(&str, &str); 1] = [("agent_log", "change_log")];

/// What a migration adds to a table that did not exist before it, besides rows moved into it: v10's
/// `change_log_start` holds the moment of the upgrade, exactly when the inventory already held
/// applications or resources. Every other new table starts empty.
const STARTED: &str = "change_log_start";

/// Checks, in `tx` once the migrations ran and before anything commits, that the inventory is in
/// this build's schema, with its structure, sound and with no broken relationship, and that it
/// still holds everything `before` held. For each table `before` had:
///
/// - each row is there, with its rowid, and holds the same values in the columns the table had,
///   compared value by value and type by type (`rows_of`), in the same table or, for the one table
///   a migration moves (`MOVED`), in the table it moved to;
/// - in each column the table did not have, every row holds the column's declared default (null
///   when it has none), which is what a migration that adds a column or rebuilds a table gives the
///   rows it carries over; rows moved into `change_log` hold an agent's source and no changes.
///
/// Every table `before` did not have is empty, except where rows moved into it and v10's
/// `change_log_start` (`STARTED`). Gives how many tables and rows were checked.
fn check_upgraded(tx: &Transaction, before: &Snapshot) -> Result<(usize, i64), String> {
    let wrong = |what: String| format!("The upgraded inventory failed its verification: {what}.");
    let sqlite = |error: rusqlite::Error| wrong(error.to_string());
    let version: i64 = tx
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(sqlite)?;
    if version != SCHEMA_VERSION {
        return Err(wrong(format!(
            "it declares schema version {version}, not {SCHEMA_VERSION}"
        )));
    }
    let mut problems = inventory::schema_differences(tx, SCHEMA_VERSION).map_err(sqlite)?;
    let findings = integrity(tx);
    if findings != ["ok"] {
        problems.push(format!("integrity check: {}", findings.join("; ")));
    }
    let broken = inventory::broken_relationships(tx).map_err(sqlite)?;
    if broken > 0 {
        problems.push(format!("{broken} rows refer to a row that is not there"));
    }
    if !problems.is_empty() {
        return Err(wrong(problems.join("; ")));
    }
    let after = table_names(tx).map_err(sqlite)?;
    let mut kept = 0;
    let mut filled = Vec::new();
    for (table, (columns, rows)) in &before.tables {
        let into = MOVED
            .iter()
            .find(|(from, _)| from == table)
            .map_or(table.as_str(), |(_, to)| to);
        if !after.contains(&into.to_owned()) {
            return Err(wrong(format!("table {into} is gone")));
        }
        let now = columns_of(tx, into).map_err(sqlite)?;
        if let Some(missing) = columns.iter().find(|column| !now.contains(column)) {
            return Err(wrong(format!("column {into}.{missing} is gone")));
        }
        let found = rows_of(tx, into, columns).map_err(sqlite)?;
        if found != *rows {
            return Err(wrong(if found.count == rows.count {
                format!(
                    "{into} holds the same number of rows as {table} did ({}), with other values",
                    rows.count
                )
            } else {
                format!(
                    "{into} holds {} rows where {table} held {}",
                    found.count, rows.count
                )
            }));
        }
        let added: Vec<&String> = now
            .iter()
            .filter(|column| !columns.contains(column))
            .collect();
        for column in added {
            let expected = if into == table {
                default_of(tx, into, column).map_err(sqlite)?
            } else {
                // Rows moved into the change log: an agent's change, with nothing structured.
                match column.as_str() {
                    "source" => "'agent'".to_owned(),
                    _ => "NULL".to_owned(),
                }
            };
            let other: i64 = tx
                .query_row(
                    &format!(
                        "SELECT count(*) FROM {} WHERE {} IS NOT ({expected})",
                        quoted(into),
                        quoted(column)
                    ),
                    [],
                    |row| row.get(0),
                )
                .map_err(sqlite)?;
            if other > 0 {
                return Err(wrong(format!(
                    "{other} rows of {into} hold something other than {expected} in {column}, which {table} did not have"
                )));
            }
        }
        filled.push(into.to_owned());
        kept += rows.count;
    }
    for table in after.iter().filter(|table| !filled.contains(table)) {
        let rows: i64 = tx
            .query_row(
                &format!("SELECT count(*) FROM {}", quoted(table)),
                [],
                |row| row.get(0),
            )
            .map_err(sqlite)?;
        let expected = if table == STARTED {
            let held = |name: &str| {
                before
                    .tables
                    .get(name)
                    .is_some_and(|(_, rows)| rows.count > 0)
            };
            i64::from(before.version < 10 && (held("applications") || held("resources")))
        } else {
            0
        };
        if rows != expected {
            return Err(wrong(format!(
                "the new table {table} holds {rows} rows instead of {expected}"
            )));
        }
    }
    Ok((before.tables.len(), kept))
}

/// What the manifest beside a backup made before an upgrade says: enough to tell what the file is,
/// which inventory it copies, of which version, when, and to check that its bytes are still the
/// ones DevHQ verified. Written once, never rewritten.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest<'a> {
    /// What this file is, and the version of its format.
    format: &'static str,
    /// The backup's file name, in the same folder.
    backup: &'a str,
    /// SHA-256 of the backup's bytes.
    sha256: &'a str,
    size_bytes: u64,
    /// When the copy was taken (UTC).
    taken_at: &'a str,
    /// The inventory it is a copy of, where DevHQ opened it.
    inventory: String,
    /// The schema version of the copy: the inventory's before the upgrade.
    schema_version: i64,
    /// What it was made for.
    made_before: String,
    /// The DevHQ that made it.
    devhq: &'static str,
    /// What `PRAGMA integrity_check` said of the copy.
    integrity_check: &'a str,
    /// Rows by table, each compared value by value with the inventory before naming the copy.
    rows: BTreeMap<&'a str, i64>,
}

impl<'a> Manifest<'a> {
    fn of(path: &Path, backup: &'a BeforeUpgrade) -> Self {
        Self {
            format: "DevHQ backup manifest 1",
            backup: &backup.name,
            sha256: &backup.sha256,
            size_bytes: backup.size_bytes,
            taken_at: &backup.taken_at,
            inventory: path.display().to_string(),
            schema_version: backup.schema_version,
            made_before: format!("upgrading the inventory to schema version {SCHEMA_VERSION}"),
            devhq: env!("CARGO_PKG_VERSION"),
            integrity_check: &backup.integrity_check,
            rows: backup
                .tables
                .iter()
                .map(|table| (table.table.as_str(), table.rows))
                .collect(),
        }
    }
}

/// A table's rows as stored: how many, and a digest of each one's rowid and the values of the
/// columns compared, in rowid order, each value with its type, so that 1, 1.0 and '1' differ.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rows {
    count: i64,
    digest: [u8; 32],
}

/// An inventory as read in one transaction: its version, each table with its columns and rows, and
/// why DevHQ would not upgrade it.
#[derive(Debug, Clone)]
struct Snapshot {
    version: i64,
    /// Each table by name, with its columns in order and its rows.
    tables: BTreeMap<String, (Vec<String>, Rows)>,
    /// Empty when DevHQ upgrades it.
    problems: Vec<String>,
}

impl Snapshot {
    /// Reads the database on `db`: what it holds, and whether it has exactly the structure the
    /// migrations give its version, is sound and has no broken relationship.
    fn read(db: &Connection) -> rusqlite::Result<Self> {
        let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let mut problems: Vec<String> = inventory::schema_differences(db, version)?
            .into_iter()
            .map(|difference| {
                format!("its structure differs from schema version {version}'s: {difference}")
            })
            .collect();
        let findings = integrity(db);
        if findings != ["ok"] {
            problems.push(format!(
                "SQLite's integrity check found it damaged: {}",
                findings.join("; ")
            ));
        }
        match inventory::broken_relationships(db) {
            Ok(0) => {}
            Ok(broken) => problems.push(format!("{broken} rows refer to a row that is not there")),
            Err(error) => problems.push(format!("its relationships cannot be checked: {error}")),
        }
        let mut tables = BTreeMap::new();
        for table in table_names(db)? {
            let columns = columns_of(db, &table)?;
            match rows_of(db, &table, &columns) {
                Ok(rows) => {
                    tables.insert(table, (columns, rows));
                }
                Err(error) => problems.push(format!("table {table} cannot be read: {error}")),
            }
        }
        Ok(Self {
            version,
            tables,
            problems,
        })
    }

    /// A digest of everything it holds, by table, with its version: what the person consents to.
    fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"DevHQ inventory content 1\0");
        hasher.update(self.version.to_be_bytes());
        for (table, (columns, rows)) in &self.tables {
            text(&mut hasher, table);
            hasher.update((columns.len() as u64).to_be_bytes());
            for column in columns {
                text(&mut hasher, column);
            }
            hasher.update(rows.count.to_be_bytes());
            hasher.update(rows.digest);
        }
        hex(&hasher.finalize())
    }

    fn table_rows(&self) -> Vec<TableRows> {
        self.tables
            .iter()
            .map(|(table, (_, rows))| TableRows {
                table: table.clone(),
                rows: rows.count,
            })
            .collect()
    }
}

/// The tables in `before` and `after` that differ, said for a person.
fn differing_tables(
    before: &BTreeMap<String, (Vec<String>, Rows)>,
    after: &BTreeMap<String, (Vec<String>, Rows)>,
) -> Vec<String> {
    let mut differing = Vec::new();
    for (table, (columns, rows)) in before {
        match after.get(table) {
            None => differing.push(format!("no table {table}")),
            Some((other, _)) if other != columns => {
                differing.push(format!("table {table} has other columns"))
            }
            Some((_, other)) if other.count != rows.count => differing.push(format!(
                "table {table} has {} rows instead of {}",
                other.count, rows.count
            )),
            Some((_, other)) if other != rows => {
                differing.push(format!("table {table} holds other values"))
            }
            Some(_) => {}
        }
    }
    for table in after.keys().filter(|table| !before.contains_key(*table)) {
        differing.push(format!("table {table}, which the inventory does not have"));
    }
    differing
}

/// What `PRAGMA integrity_check` says of the database on `db`: ["ok"] when it is sound. Damage
/// shows up as rows describing it, or, when SQLite cannot even walk a page, as an error.
fn integrity(db: &Connection) -> Vec<String> {
    db.prepare("PRAGMA integrity_check")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap_or_else(|error| vec![error.to_string()])
}

/// The tables of the database on `db`, by name, but SQLite's own.
fn table_names(db: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut statement = db.prepare(
        "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name",
    )?;
    let names = statement.query_map([], |row| row.get(0))?.collect();
    names
}

/// The columns of `table`, in order.
fn columns_of(db: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    let mut statement = db.prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")?;
    let columns = statement.query_map([table], |row| row.get(0))?.collect();
    columns
}

/// The default `column` of `table` declares, as SQL: NULL when it declares none.
fn default_of(db: &Connection, table: &str, column: &str) -> rusqlite::Result<String> {
    db.query_row(
        "SELECT dflt_value FROM pragma_table_info(?1) WHERE name = ?2",
        [table, column],
        |row| row.get::<_, Option<String>>(0),
    )
    .map(|default| default.unwrap_or_else(|| "NULL".to_owned()))
}

/// The rows of `table`: their rowids and the values of `columns`.
fn rows_of(db: &Connection, table: &str, columns: &[String]) -> rusqlite::Result<Rows> {
    let selected: Vec<String> = std::iter::once("rowid".to_owned())
        .chain(columns.iter().map(|column| quoted(column)))
        .collect();
    let mut statement = db.prepare(&format!(
        "SELECT {} FROM {} ORDER BY rowid",
        selected.join(", "),
        quoted(table)
    ))?;
    let mut hasher = Sha256::new();
    let mut count = 0;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        count += 1;
        for index in 0..selected.len() {
            match row.get_ref(index)? {
                ValueRef::Null => hasher.update([0]),
                ValueRef::Integer(value) => {
                    hasher.update([1]);
                    hasher.update(value.to_be_bytes());
                }
                ValueRef::Real(value) => {
                    hasher.update([2]);
                    hasher.update(value.to_bits().to_be_bytes());
                }
                ValueRef::Text(bytes) => {
                    hasher.update([3]);
                    hasher.update((bytes.len() as u64).to_be_bytes());
                    hasher.update(bytes);
                }
                ValueRef::Blob(bytes) => {
                    hasher.update([4]);
                    hasher.update((bytes.len() as u64).to_be_bytes());
                    hasher.update(bytes);
                }
            }
        }
    }
    Ok(Rows {
        count,
        digest: hasher.finalize().into(),
    })
}

/// A name as an SQL identifier, whatever it holds: names come from the file, which is data.
fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Feeds `value` to `hasher` with its length first, so that no two lists of texts feed the same.
fn text(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::tests::{database_at, dump, helper, kill, resolved_temp_dir, writer};
    use crate::inventory::{beside, examine, start, MIGRATIONS};
    use std::path::PathBuf;

    // 2026-01-01T09:30:00.123Z
    const TAKEN_AT: i64 = 1_767_259_800_123;
    const BACKUP_V6: &str = "inventory-20260101T093000.123Z-v6.sqlite3";

    /// An inventory path in a folder of its own, which also holds its backups folder.
    fn path() -> PathBuf {
        let folder =
            resolved_temp_dir().join(format!("devhq-upgrade-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        folder.join("inventory.sqlite3")
    }

    fn remove(path: &Path) {
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    /// Every file in `folder` and below, by path relative to it, with its bytes.
    fn files(folder: &Path) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, folder: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
            for entry in std::fs::read_dir(folder).unwrap() {
                let entry = entry.unwrap().path();
                if entry.is_dir() {
                    walk(root, &entry, found);
                } else {
                    let name = entry.strip_prefix(root).unwrap().display().to_string();
                    found.insert(name.replace('\\', "/"), std::fs::read(&entry).unwrap());
                }
            }
        }
        let mut found = BTreeMap::new();
        walk(folder, folder, &mut found);
        found
    }

    fn names(folder: &Path) -> Vec<String> {
        files(folder).into_keys().collect()
    }

    /// What a release of schema `version` could have written, with relationships between rows: a
    /// row or more in every table that version has, with text that needs quoting and every kind of
    /// value SQLite stores, so that the upgrade has something to lose in each.
    fn seed(db: &Connection, version: i64) {
        let mut sql = String::new();
        if version >= 2 {
            sql.push_str(
                "INSERT INTO resources (id, name, kind, description, operating_system)
                   VALUES ('host', 'Host', 'server', 'Rack 2', 'Linux'), ('spare', 'Spare', 'workstation', '', '');
                 INSERT INTO applications (id, name, lifecycle, description, version)
                   VALUES ('shop', 'Shop', 'production', 'Store ''front''', '2.0'), ('draft', 'Ünïcödé', 'development', '', NULL);",
            );
        } else {
            sql.push_str(
                "INSERT INTO resources (id, name, kind) VALUES ('host', 'Host', 'server'), ('spare', 'Spare', 'workstation');
                 INSERT INTO applications (id, name, lifecycle) VALUES ('shop', 'Shop', 'production'), ('draft', 'Ünïcödé', 'development');",
            );
        }
        sql.push_str(
            "INSERT INTO components (id, application_id, name, kind, resource_id)
               VALUES ('api', 'shop', 'API', 'backend', 'host'), ('ui', 'shop', 'UI', 'frontend', NULL);",
        );
        if version >= 3 {
            sql.push_str(
                "INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at)
                   VALUES ('obs-manual', 'host', 'degraded', 'Slow disk', 'manual', 500, 900, 600);
                 INSERT INTO incidents (id, application_id, title, description, severity, status, source, opened_at, updated_at, resolved_at)
                   VALUES ('inc-open', 'shop', 'Checkout down', '', 'critical', 'open', 'manual', 700, 700, NULL),
                          ('inc-done', 'shop', 'Old outage', 'Fixed', 'warning', 'resolved', 'manual', 100, 200, 200);
                 INSERT INTO events (id, resource_id, kind, outcome, title, detail, source, observation_id, occurred_at, recorded_at)
                   VALUES ('evt-obs', 'host', 'observation', 'warning', 'Observation: Attention required', 'Slow disk', 'manual', 'obs-manual', 500, 600);
                 INSERT INTO events (id, application_id, kind, outcome, title, source, incident_id, occurred_at, recorded_at)
                   VALUES ('evt-inc', 'shop', 'incident', 'failure', 'Incident opened: Checkout down', 'manual', 'inc-open', 700, 700);
                 INSERT INTO events (id, application_id, kind, outcome, title, detail, source, occurred_at, recorded_at)
                   VALUES ('evt-note', 'shop', 'note', 'info', 'A note', 'With ''quotes'' and a
line break', 'manual', 800, 800);",
            );
        }
        if version >= 4 {
            sql.push_str(
                "INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at,
                                          http_url, http_expected_status, http_status, http_latency_ms, http_error, http_redirects)
                   VALUES ('obs-http', 'host', 'unhealthy', 'HTTP 500, expected 200', 'http', 1000, 2000, 1000,
                           'http://host/health', 200, 500, 9, NULL, '[{\"status\":301,\"location\":\"http://host/health\"}]');
                 INSERT INTO events (id, resource_id, kind, outcome, title, detail, source, observation_id, occurred_at, recorded_at)
                   VALUES ('evt-http', 'host', 'observation', 'failure', 'HTTP check: Unhealthy', 'HTTP 500', 'http', 'obs-http', 1000, 1000);",
            );
            sql.push_str(if version >= 5 {
                "INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at, monitor, monitor_interval_minutes)
                   VALUES ('host', 'http://host/health', 200, 5, 15, 800, 'active', 5);
                 INSERT INTO http_checks (application_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at)
                   VALUES ('shop', 'https://shop.example/', 204, 3, 60, 900);"
            } else {
                "INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at)
                   VALUES ('host', 'http://host/health', 200, 5, 15, 800);
                 INSERT INTO http_checks (application_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at)
                   VALUES ('shop', 'https://shop.example/', 204, 3, 60, 900);"
            });
        }
        if version >= 7 {
            sql.push_str(
                "INSERT INTO observations (id, application_id, result, summary, source, observed_at, valid_until, recorded_at, agent)
                   VALUES ('obs-agent', 'shop', 'healthy', 'Looks fine', 'agent', 1100, 1200, 1100, 'claude-code');
                 INSERT INTO events (id, application_id, kind, outcome, title, detail, source, observation_id, occurred_at, recorded_at, agent)
                   VALUES ('evt-agent', 'shop', 'observation', 'success', 'Observation: Healthy', 'Looks fine', 'agent', 'obs-agent', 1100, 1100, 'claude-code');",
            );
        }
        if version >= 8 {
            sql.push_str(
                "INSERT INTO agent_access (id, mode, changed_at) VALUES (1, 'read', 1200);",
            );
        }
        if version == 9 {
            sql.push_str(
                "INSERT INTO agent_log (id, application_id, resource_id, subject_name, agent, action, summary, occurred_at)
                   VALUES ('log-1', 'shop', NULL, 'Shop', 'claude-code', 'updated', 'Changed the version', 1150),
                          ('log-2', NULL, 'host', 'Host', 'claude-code', 'check_run', 'Ran the HTTP check', 1160);",
            );
        }
        if version >= 10 {
            sql.push_str(
                "INSERT INTO change_log (id, application_id, subject_name, source, agent, action, summary, changes, occurred_at)
                   VALUES ('change-1', 'shop', 'Shop', 'manual', NULL, 'updated', 'Changed the version',
                           '[{\"field\":\"version\",\"before\":\"1.0\",\"after\":\"2.0\"}]', 1150);
                 INSERT INTO change_log_start (id, started_at) VALUES (1, 1000);",
            );
        }
        if version >= 11 {
            sql.push_str(
                "INSERT INTO system_references (id, application_id, kind, target, label, machine_id)
                   VALUES ('shop-repo', 'shop', 'repository', 'https://github.com/me/shop', NULL, NULL),
                          ('shop-copy', 'shop', 'local_copy', 'C:\\src\\shop', 'Laptop clone', 'spare');",
            );
        }
        if version >= 12 {
            sql.push_str(
                "INSERT INTO copy_readings (id, application_id, reference_id, machine_id, path, source, reader, agent,
                                           observed_at, recorded_at, outcome, detail)
                   VALUES ('reading-1', 'shop', 'shop-copy', 'spare', 'C:\\src\\shop', 'read', 'libgit2 1.9.1',
                           'claude-code', 1300, 1300, 'missing', 'Nothing at C:\\src\\shop');",
            );
        }
        db.execute_batch(&sql).unwrap();
    }

    /// A real number and a blob where SQLite takes any value, which no DevHQ writes and its window
    /// does not read: an upgrade must not turn one kind of value into another. In a v3 inventory or
    /// later, with an event of its own.
    fn odd(path: &Path) {
        Connection::open(path)
            .unwrap()
            .execute_batch(
                "INSERT INTO events (id, application_id, kind, outcome, title, detail, source, occurred_at, recorded_at)
                   VALUES ('evt-odd', 'draft', 'note', 'info', 'Odd values', x'00ff', 'manual', 1.5, 900);",
            )
            .unwrap();
    }

    /// An inventory as a release of schema `version` left it, holding what `seed` writes, with a
    /// rollback journal or, `logged`, a write-ahead log, all of it folded into the file.
    fn historical(path: &Path, version: i64, logged: bool) {
        let db = database_at(path, version);
        seed(&db, version);
        if logged {
            let mode: String = db
                .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
                .unwrap();
            assert_eq!(mode, "wal");
        }
    }

    /// A connection through which a test only reads the database at `path`, as the window's
    /// connections do: reading writes nothing, and the last connection to close folds and removes
    /// the log it opened, so that the files are as they were.
    fn readonly(path: &Path) -> Connection {
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap()
    }

    /// Every table, each with its rows: rowid first, then every column, as text.
    fn whole(db: &Connection) -> BTreeMap<String, Vec<String>> {
        table_names(db)
            .unwrap()
            .into_iter()
            .map(|table| {
                let rows = dump(db, &table);
                (table, rows)
            })
            .collect()
    }

    /// The rows of `table` on `db`: rowid first, then the `columns` named, as text.
    fn picked(db: &Connection, table: &str, columns: &[String]) -> Vec<String> {
        let list: Vec<String> = columns.iter().map(|column| quoted(column)).collect();
        let mut statement = db
            .prepare(&format!(
                "SELECT rowid, {} FROM {table} ORDER BY rowid",
                list.join(", ")
            ))
            .unwrap();
        let width = statement.column_count();
        statement
            .query_map([], |row| {
                (0..width)
                    .map(|index| Ok(format!("{:?}", row.get_ref(index)?)))
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map(|values| values.join(" | "))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// Upgrades with the person's consent to what `inspect` showed, a clock that says `TAKEN_AT`,
    /// `migrations`, and `damage` on the backup's copy; collects the steps sent.
    fn upgrade_as_shown(
        path: &Path,
        migrations: &[(i64, &str)],
        damage: Damage,
    ) -> (Result<Upgraded, ApiError>, Vec<Step>) {
        let inspection = inspect(path);
        let consent = (
            inspection.stored_version.unwrap_or(0),
            inspection.fingerprint.unwrap_or_default(),
        );
        let mut steps = Vec::new();
        let outcome = upgrade_with(
            path,
            (consent.0, &consent.1),
            migrations,
            &|| TAKEN_AT,
            &mut |step| steps.push(step),
            (damage, backup::names_on_disk),
            &mut |_| Ok(()),
        );
        (outcome, steps)
    }

    /// The fingerprint of the database at `path`, whatever it is, read as `inspect` reads it: what
    /// a person could have consented to, so that the upgrade's own checks are what refuses it.
    fn fingerprint_of(path: &Path) -> String {
        Snapshot::read(&readonly(path)).unwrap().fingerprint()
    }

    /// The upgrade, with the person's consent to `fingerprint` of version `from`.
    fn upgrade_to(path: &Path, from: i64, fingerprint: &str) -> Result<Upgraded, ApiError> {
        upgrade_at(path, from, fingerprint, TAKEN_AT)
    }

    /// `upgrade_to`, with a clock that says `taken_at`.
    fn upgrade_at(
        path: &Path,
        from: i64,
        fingerprint: &str,
        taken_at: i64,
    ) -> Result<Upgraded, ApiError> {
        upgrade_with(
            path,
            (from, fingerprint),
            &MIGRATIONS,
            &|| taken_at,
            &mut |_| {},
            (&mut |_| Ok(()), backup::names_on_disk),
            &mut |_| Ok(()),
        )
    }

    /// Every older schema, with either journal, upgraded with the person's consent: each step is
    /// reported once reached, the inventory ends up in this build's schema holding every row it
    /// held, value by value, rowids included, and the backup holds exactly what the inventory held
    /// before, in its version, with a manifest that says so; put in place of the inventory, it is
    /// what the person was shown before upgrading, and it upgrades again to the same inventory.
    #[test]
    fn upgrades_every_older_schema_keeping_every_row_after_a_verified_backup() {
        for version in 1..SCHEMA_VERSION {
            for logged in [false, true] {
                let case = format!("v{version}, {}", if logged { "WAL" } else { "rollback" });
                let path = path();
                let folder = path.parent().unwrap().to_owned();
                historical(&path, version, logged);
                if version >= 3 {
                    odd(&path);
                }
                let before = whole(&readonly(&path));
                let columns: BTreeMap<String, Vec<String>> = before
                    .keys()
                    .map(|table| (table.clone(), columns_of(&readonly(&path), table).unwrap()))
                    .collect();
                let on_disk = files(&folder);

                // What the person sees first changes nothing.
                let inspection = inspect(&path);
                assert_eq!(
                    files(&folder),
                    on_disk,
                    "{case}: the inspection changed files"
                );
                assert_eq!(
                    (
                        inspection.state,
                        inspection.stored_version,
                        inspection.upgradable
                    ),
                    ("outdated", Some(version), true),
                    "{case}: {inspection:?}"
                );
                assert!(
                    inspection.problems.is_empty(),
                    "{case}: {:?}",
                    inspection.problems
                );
                let tables: Vec<TableRows> = before
                    .iter()
                    .map(|(table, rows)| TableRows {
                        table: table.clone(),
                        rows: rows.len() as i64,
                    })
                    .collect();
                assert_eq!(inspection.tables, tables, "{case}");
                let shown = inspection.fingerprint.clone().unwrap();

                let (outcome, steps) = upgrade_as_shown(&path, &MIGRATIONS, &mut |_| Ok(()));
                let upgraded = outcome.unwrap_or_else(|error| panic!("{case}: {error:?}"));
                let backup = upgraded.backup.clone().unwrap();
                assert_eq!(
                    steps,
                    [
                        Step::Checking,
                        Step::BackingUp,
                        Step::BackedUp {
                            backup: Box::new(backup.clone())
                        },
                        Step::Upgrading,
                        Step::Verifying
                    ],
                    "{case}"
                );
                let rows: i64 = before.values().map(|rows| rows.len() as i64).sum();
                assert_eq!(
                    (
                        upgraded.from,
                        upgraded.to,
                        upgraded.tables_kept,
                        upgraded.rows_kept
                    ),
                    (version, SCHEMA_VERSION, before.len(), rows),
                    "{case}"
                );

                // The inventory: this build's schema, every row and value kept.
                assert_eq!(examine(&path, |_| Ok(())).0, Found::Current, "{case}");
                let after = readonly(&path);
                assert_eq!(integrity(&after), ["ok"], "{case}");
                for (table, rows) in &before {
                    let into = if table == "agent_log" {
                        "change_log"
                    } else {
                        table
                    };
                    assert_eq!(
                        &picked(&after, into, &columns[table]),
                        rows,
                        "{case}: {table} as {into}"
                    );
                }
                // What the migrations add: every check unsupervised, the gate closed, and the
                // moment the change log starts, since the inventory held systems before it had one.
                let count =
                    |sql: &str| -> i64 { after.query_row(sql, [], |row| row.get(0)).unwrap() };
                if version < 5 {
                    assert_eq!(
                        count("SELECT count(*) FROM http_checks WHERE monitor <> 'off'"),
                        0
                    );
                }
                if version < 8 {
                    assert_eq!(count("SELECT count(*) FROM agent_access"), 0, "{case}");
                }
                assert_eq!(
                    count("SELECT count(*) FROM change_log_start"),
                    1,
                    "{case}: the log is complete only from the upgrade on"
                );
                drop(after);

                // The backup: the inventory as it was, in its own version, named for it.
                let name = format!("inventory-20260101T093000.123Z-v{version}.sqlite3");
                assert_eq!(
                    names(&folder),
                    [
                        format!("backups/{name}"),
                        format!("backups/{name}.json"),
                        "inventory.sqlite3".to_owned()
                    ],
                    "{case}: no .partial, no log left behind"
                );
                let copy = folder.join("backups").join(&name);
                assert_eq!(
                    (
                        backup.name.as_str(),
                        backup.path.clone(),
                        backup.schema_version,
                        backup.integrity_check.as_str()
                    ),
                    (name.as_str(), copy.display().to_string(), version, "ok"),
                    "{case}"
                );
                assert_eq!(backup.tables, tables, "{case}");
                assert_eq!(
                    whole(&readonly(&copy)),
                    before,
                    "{case}: the backup is not the inventory as it was"
                );
                let bytes = std::fs::read(&copy).unwrap();
                assert_eq!(backup.size_bytes, bytes.len() as u64);
                assert_eq!(backup.sha256, hex(&Sha256::digest(&bytes)), "{case}");
                assert_eq!(
                    bytes[18..20],
                    [1, 1],
                    "{case}: the copy uses a rollback journal, one file complete by itself"
                );

                // The manifest: what the backup is, checkable without DevHQ.
                let manifest: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&backup.manifest).unwrap()).unwrap();
                assert_eq!(manifest["format"], "DevHQ backup manifest 1");
                assert_eq!(manifest["backup"], name.as_str());
                assert_eq!(manifest["sha256"], backup.sha256.as_str());
                assert_eq!(manifest["sizeBytes"], backup.size_bytes);
                assert_eq!(manifest["takenAt"], "2026-01-01T09:30:00.123Z");
                assert_eq!(manifest["inventory"], path.display().to_string());
                assert_eq!(manifest["schemaVersion"], version);
                assert_eq!(manifest["integrityCheck"], "ok");
                for (table, rows) in &before {
                    assert_eq!(manifest["rows"][table], rows.len(), "{case}: {table}");
                }

                // Put in place of the inventory, it is what the person was shown, and it upgrades
                // again to the same rows.
                let mut first = whole(&readonly(&path));
                std::fs::remove_dir_all(folder.join("backups")).unwrap();
                std::fs::write(&path, &bytes).unwrap();
                assert_eq!(
                    inspect(&path).fingerprint.as_deref(),
                    Some(shown.as_str()),
                    "{case}"
                );
                let again = upgrade_as_shown(&path, &MIGRATIONS, &mut |_| Ok(()))
                    .0
                    .unwrap();
                assert_eq!(again.rows_kept, rows);
                let mut redone = whole(&readonly(&path));
                if version < 10 {
                    // The moment the change log starts is the moment of each upgrade.
                    redone.remove("change_log_start");
                    first.remove("change_log_start");
                }
                assert_eq!(redone, first, "{case}");
                remove(&path);
            }
        }
    }

    /// Nothing is written unless the inventory is exactly what the person consented to: another
    /// version, other content or a file replaced since are refused under the lock, and neither the
    /// inspection nor a refusal changes, creates or removes a file, not even the backups folder.
    #[test]
    fn writes_nothing_without_consent_to_what_is_there() {
        for logged in [false, true] {
            let path = path();
            let folder = path.parent().unwrap().to_owned();
            historical(&path, 6, logged);
            let shown = inspect(&path).fingerprint.unwrap();
            let on_disk = files(&folder);
            let other = {
                let mut other = shown.clone();
                other.replace_range(..1, if other.starts_with('0') { "1" } else { "0" });
                other
            };
            for (from, fingerprint) in [
                (6, other.as_str()),
                (5, shown.as_str()),
                (11, shown.as_str()),
                (6, ""),
            ] {
                let refused = upgrade_to(&path, from, fingerprint).unwrap_err();
                assert_eq!(refused.code, "upgrade_changed", "{}", refused.message);
                assert!(
                    refused.message.contains("nothing was written"),
                    "{}",
                    refused.message
                );
                assert_eq!(files(&folder), on_disk);
            }
            // Content written after the person looked, by a DevHQ that still writes v6.
            {
                let db = Connection::open(&path).unwrap();
                db.execute(
                    "UPDATE resources SET name = 'Renamed meanwhile' WHERE id = 'host'",
                    [],
                )
                .unwrap();
            }
            let on_disk = files(&folder);
            let refused = upgrade_to(&path, 6, &shown).unwrap_err();
            assert_eq!(refused.code, "upgrade_changed");
            assert_eq!(files(&folder), on_disk);
            assert!(!folder.join("backups").exists());
            // Looked at again, it upgrades.
            assert!(upgrade_as_shown(&path, &MIGRATIONS, &mut |_| Ok(()))
                .0
                .is_ok());
            remove(&path);
        }
    }

    /// What DevHQ cannot vouch for is never upgraded, however the person consented: a structure
    /// that is not exactly the one the migrations give its version (a missing index, a table or a
    /// trigger of its own), pages SQLite finds damaged, a broken relationship, or a schema this
    /// DevHQ does not know. The inspection says why, and offers nothing to consent to.
    #[test]
    fn never_upgrades_what_it_cannot_vouch_for() {
        type Case = (&'static str, fn(&Path), &'static str);
        let cases: [Case; 6] = [
            (
                "a missing index",
                |path| {
                    Connection::open(path)
                        .unwrap()
                        .execute_batch("DROP INDEX observations_manual;")
                        .unwrap()
                },
                "no index observations_manual",
            ),
            (
                "a table of its own",
                |path| {
                    Connection::open(path)
                        .unwrap()
                        .execute_batch("CREATE TABLE notes (body TEXT);")
                        .unwrap()
                },
                "table notes, which schema version 6 does not have",
            ),
            (
                "a trigger that would fire while rows are copied",
                |path| {
                    Connection::open(path)
                        .unwrap()
                        .execute_batch(
                            "CREATE TRIGGER on_rename AFTER UPDATE ON resources BEGIN SELECT 1; END;",
                        )
                        .unwrap()
                },
                "trigger on_rename, which schema version 6 does not have",
            ),
            (
                "a component on a resource that is not there",
                |path| {
                    let db = Connection::open(path).unwrap();
                    db.pragma_update(None, "foreign_keys", "OFF").unwrap();
                    db.execute_batch(
                        "INSERT INTO components (id, application_id, name, kind, resource_id)
                           VALUES ('ghost', 'shop', 'Ghost', 'docker', 'nowhere');",
                    )
                    .unwrap();
                },
                "1 rows refer to a row that is not there",
            ),
            (
                "an index that no longer matches its table",
                |path| {
                    // Rows changed with the index left behind, as damage leaves them.
                    let db = Connection::open(path).unwrap();
                    db.execute_batch("PRAGMA writable_schema = ON;").unwrap();
                    db.execute_batch(
                        "UPDATE sqlite_schema SET sql = 'CREATE INDEX components_resource_id ON components(name)' WHERE name = 'components_resource_id';",
                    )
                    .unwrap();
                },
                "SQLite's integrity check found it damaged",
            ),
            (
                "a column declared otherwise",
                |path| {
                    let db = Connection::open(path).unwrap();
                    db.execute_batch("PRAGMA writable_schema = ON;").unwrap();
                    db.execute_batch(
                        "UPDATE sqlite_schema SET sql = replace(sql, 'name TEXT NOT NULL', 'name BLOB') WHERE name = 'resources';",
                    )
                    .unwrap();
                },
                "column resources.name declared as",
            ),
        ];
        for (case, make, problem) in cases {
            let path = path();
            let folder = path.parent().unwrap().to_owned();
            historical(&path, 6, false);
            make(&path);
            let on_disk = files(&folder);
            let inspection = inspect(&path);
            assert_eq!(inspection.state, "outdated", "{case}");
            assert!(!inspection.upgradable, "{case}");
            assert_eq!(inspection.fingerprint, None, "{case}");
            assert!(
                inspection
                    .problems
                    .iter()
                    .any(|found| found.contains(problem)),
                "{case}: {:?}",
                inspection.problems
            );
            let refused = upgrade_to(&path, 6, &fingerprint_of(&path)).unwrap_err();
            assert_eq!(
                refused.code, "upgrade_refused",
                "{case}: {}",
                refused.message
            );
            assert!(
                refused.message.contains(problem),
                "{case}: {}",
                refused.message
            );
            assert_eq!(files(&folder), on_disk, "{case}");
            remove(&path);
        }

        // A newer schema, and files that are not an inventory, are what they are.
        let path = path();
        let folder = path.parent().unwrap().to_owned();
        type Other = (fn(&Path), &'static str, &'static str);
        let others: [Other; 2] = [
            (
                |path| {
                    let db = database_at(path, SCHEMA_VERSION);
                    db.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                        .unwrap();
                },
                "newer",
                "schema_newer",
            ),
            (
                |path| {
                    Connection::open(path)
                        .unwrap()
                        .execute_batch("CREATE TABLE notes (body TEXT);")
                        .unwrap()
                },
                "not_an_inventory",
                "not_an_inventory",
            ),
        ];
        for (make, state, code) in others {
            make(&path);
            let on_disk = files(&folder);
            let inspection = inspect(&path);
            assert_eq!((inspection.state, inspection.upgradable), (state, false));
            let refused = upgrade_to(&path, 6, &fingerprint_of(&path)).unwrap_err();
            assert_eq!(refused.code, code, "{}", refused.message);
            assert_eq!(files(&folder), on_disk);
            std::fs::remove_file(&path).unwrap();
        }
        remove(&path);
    }

    /// An inventory already in this build's schema, upgraded meanwhile by another DevHQ or new, is
    /// left as it is: nothing is backed up or written, and it serves as before.
    /// The upgrade to v13 keeps the access the person chose, none included, and only an inventory
    /// in v13 takes the fourth mode, Read and maintain: a v12 one refuses it.
    #[test]
    fn the_upgrade_to_v13_keeps_the_access_the_person_chose() {
        use rusqlite::OptionalExtension;
        for mode in [None, Some("off"), Some("read"), Some("write")] {
            let path = path();
            let db = database_at(&path, 12);
            if let Some(mode) = mode {
                db.execute(
                    "INSERT INTO agent_access (id, mode, changed_at) VALUES (1, ?1, 1200)",
                    [mode],
                )
                .unwrap();
            }
            assert!(
                db.execute(
                    "INSERT OR REPLACE INTO agent_access (id, mode, changed_at) VALUES (1, 'maintain', 1300)",
                    [],
                )
                .is_err(),
                "{mode:?}: v12 took maintain"
            );
            drop(db);
            let shown = fingerprint_of(&path);
            let upgraded = upgrade_to(&path, 12, &shown).unwrap();
            assert_eq!((upgraded.from, upgraded.to), (12, 13));
            let kept: Option<(String, i64)> = readonly(&path)
                .query_row("SELECT mode, changed_at FROM agent_access", [], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })
                .optional()
                .unwrap();
            assert_eq!(kept, mode.map(|mode| (mode.to_owned(), 1200)), "{mode:?}");
            assert_eq!(
                crate::access::agent_access(&path).unwrap().mode,
                mode.unwrap_or("off")
            );
            assert_eq!(
                crate::access::set_agent_access(&path, "maintain")
                    .unwrap()
                    .mode,
                "maintain"
            );
            remove(&path);
        }
    }

    #[test]
    fn an_inventory_in_this_schema_is_left_as_it_is() {
        let path = path();
        let folder = path.parent().unwrap().to_owned();
        historical(&path, SCHEMA_VERSION, false);
        let on_disk = files(&folder);
        assert_eq!(
            upgrade_to(&path, 6, "anything").unwrap(),
            Upgraded::already()
        );
        assert_eq!(files(&folder), on_disk);
        start(&path).unwrap();
        assert_eq!(crate::inventory::list(&path).unwrap().applications.len(), 2);
        remove(&path);
    }

    /// No migration is committed without a verified backup on disk under its name, with its
    /// manifest: a copy that fails its verification, a backup name or a manifest name already taken,
    /// names the system does not confirm on disk and a backups folder that cannot be made each stop
    /// the upgrade before it changes anything, remove what they half wrote, keep what they finished
    /// and never touch another file.
    #[test]
    fn no_migration_commits_without_a_verified_backup() {
        let path = path();
        let folder = path.parent().unwrap().to_owned();
        historical(&path, 6, true);
        let on_disk = files(&folder);
        let backups = folder.join("backups");

        // Damaged pages, and rows changed or added in the copy: SQLite's check, then the copy's
        // comparison with the inventory under the lock, refuse it.
        type Damaging = (fn(&Path) -> std::io::Result<()>, &'static str);
        let damages: [Damaging; 4] = [
            (
                |partial| {
                    use std::io::{Seek, SeekFrom, Write};
                    let mut file = std::fs::File::options().write(true).open(partial)?;
                    file.seek(SeekFrom::Start(4096))?;
                    file.write_all(&[0xFF; 64])
                },
                "The copy",
            ),
            (
                |partial| {
                    Connection::open(partial)
                        .and_then(|copy| {
                            copy.execute(
                                "UPDATE resources SET name = 'Other' WHERE id = 'host'",
                                [],
                            )
                        })
                        .map(|_| ())
                        .map_err(std::io::Error::other)
                },
                "table resources holds other values",
            ),
            (
                |partial| {
                    Connection::open(partial)
                        .and_then(|copy| {
                            copy.execute(
                                "INSERT INTO resources (id, name, kind) VALUES ('extra', 'Extra', 'server')",
                                [],
                            )
                        })
                        .map(|_| ())
                        .map_err(std::io::Error::other)
                },
                "table resources has 3 rows instead of 2",
            ),
            // The same rows, and something of its own that would run on the inventory once
            // restored: only the copy's own check of its structure sees it.
            (
                |partial| {
                    Connection::open(partial)
                        .and_then(|copy| {
                            copy.execute_batch(
                                "CREATE TRIGGER planted AFTER INSERT ON resources BEGIN SELECT 1; END;",
                            )
                        })
                        .map_err(std::io::Error::other)
                },
                "trigger planted, which schema version 6 does not have",
            ),
        ];
        for (mut damage, why) in damages {
            let (outcome, steps) = upgrade_as_shown(&path, &MIGRATIONS, &mut damage);
            let refused = outcome.unwrap_err();
            assert_eq!(refused.code, "backup_error", "{}", refused.message);
            for said in [
                why,
                "The incomplete copy was removed",
                "Nothing was upgraded",
            ] {
                assert!(refused.message.contains(said), "{}", refused.message);
            }
            assert_eq!(steps, [Step::Checking, Step::BackingUp]);
            // An empty backups folder, and the inventory's file as it was.
            assert!(std::fs::read_dir(&backups).unwrap().next().is_none());
            assert_eq!(files(&folder), on_disk);
        }

        // A file already has the backup's name, or the manifest's.
        std::fs::write(backups.join(BACKUP_V6), "an earlier file").unwrap();
        let refused = upgrade_as_shown(&path, &MIGRATIONS, &mut |_| Ok(()))
            .0
            .unwrap_err();
        assert!(
            refused
                .message
                .contains("already exists in the backups folder"),
            "{}",
            refused.message
        );
        assert_eq!(
            std::fs::read(backups.join(BACKUP_V6)).unwrap(),
            b"an earlier file"
        );
        std::fs::remove_file(backups.join(BACKUP_V6)).unwrap();
        let manifest = format!("{BACKUP_V6}.json");
        std::fs::write(backups.join(&manifest), "an earlier file").unwrap();
        let (outcome, steps) = upgrade_as_shown(&path, &MIGRATIONS, &mut |_| Ok(()));
        let refused = outcome.unwrap_err();
        assert_eq!(refused.code, "backup_error");
        assert!(
            refused
                .message
                .contains("upgrades nothing without its manifest"),
            "{}",
            refused.message
        );
        assert_eq!(steps, [Step::Checking, Step::BackingUp]);
        // The verified copy stays: it is a backup, only the upgrade did not happen.
        assert_eq!(names(&backups), [BACKUP_V6.to_owned(), manifest.clone()]);
        assert_eq!(
            std::fs::read(backups.join(&manifest)).unwrap(),
            b"an earlier file"
        );
        std::fs::remove_dir_all(&backups).unwrap();

        // Both are named, and the system does not confirm that the names reached the disk: they
        // are kept, complete and verified, and nothing is migrated.
        let shown = inspect(&path).fingerprint.unwrap();
        let mut steps = Vec::new();
        let refused = upgrade_with(
            &path,
            (6, &shown),
            &MIGRATIONS,
            &|| TAKEN_AT,
            &mut |step| steps.push(step),
            (&mut |_| Ok(()), |_| {
                Err(std::io::Error::other("the disk said no"))
            }),
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert_eq!(refused.code, "backup_error", "{}", refused.message);
        for said in [
            "did not confirm that their names reached the disk (the disk said no)",
            "They were kept",
            "Nothing was upgraded",
        ] {
            assert!(refused.message.contains(said), "{}", refused.message);
        }
        assert_eq!(steps, [Step::Checking, Step::BackingUp]);
        assert_eq!(names(&backups), [BACKUP_V6.to_owned(), manifest.clone()]);
        // The backup is the inventory as it is, and the manifest says what it is.
        assert_eq!(
            whole(&readonly(&backups.join(BACKUP_V6))),
            whole(&readonly(&path))
        );
        let said: serde_json::Value =
            serde_json::from_slice(&std::fs::read(backups.join(&manifest)).unwrap()).unwrap();
        assert_eq!(
            said["sha256"],
            hex(&Sha256::digest(
                std::fs::read(backups.join(BACKUP_V6)).unwrap()
            ))
        );
        std::fs::remove_dir_all(&backups).unwrap();
        assert_eq!(files(&folder), on_disk);

        // No backups folder can be made: a file has its name.
        std::fs::write(&backups, "not a folder").unwrap();
        let refused = upgrade_as_shown(&path, &MIGRATIONS, &mut |_| Ok(()))
            .0
            .unwrap_err();
        assert_eq!(refused.code, "backup_error");
        assert!(
            refused.message.contains("could not start the backup"),
            "{}",
            refused.message
        );
        std::fs::remove_file(&backups).unwrap();

        // Throughout, the inventory stayed as it was.
        assert_eq!(files(&folder), on_disk);
        assert!(examine(&path, |_| Ok(())).0 == Found::Outdated(6));
        remove(&path);
    }

    /// A migration that fails, or that loses or changes anything, commits nothing: the inventory
    /// holds what it held, and the backup made before it is kept and named.
    #[test]
    fn a_failed_or_lossy_migration_commits_nothing() {
        // This build's migrations, with migration `number` replaced by `sql`.
        let with = |number: i64, sql: String| -> Vec<(i64, String)> {
            MIGRATIONS
                .iter()
                .map(|(version, migration)| {
                    let migration = if *version == number {
                        sql.clone()
                    } else {
                        (*migration).to_owned()
                    };
                    (*version, migration)
                })
                .collect()
        };
        let migration = |number: usize| MIGRATIONS[number - 1].1;
        // What is tried, from which version, with which migrations, and what it must say.
        type Case = (&'static str, i64, Vec<(i64, String)>, &'static str);
        let cases: [Case; 6] = [
            (
                "a migration that fails",
                6,
                with(12, format!("{} INSERT INTO nowhere VALUES (1);", migration(12))),
                "migration 012 failed",
            ),
            (
                "a row lost",
                6,
                with(12, format!("DELETE FROM events WHERE id = 'evt-note'; {}", migration(12))),
                "events holds 4 rows where events held 5",
            ),
            (
                "a value changed",
                6,
                with(12, format!("UPDATE resources SET name = 'Changed' WHERE id = 'host'; {}", migration(12))),
                "resources holds the same number of rows as resources did (2), with other values",
            ),
            (
                "a value turned into another type, with the same bytes",
                6,
                with(12, format!("UPDATE events SET detail = CAST(detail AS TEXT) WHERE id = 'evt-odd'; {}", migration(12))),
                "events holds the same number of rows as events did (5), with other values",
            ),
            (
                "a new table that does not start empty: the agent gate opened",
                6,
                with(8, format!("{} INSERT INTO agent_access VALUES (1, 'write', 0);", migration(8))),
                "the new table agent_access holds 1 rows instead of 0",
            ),
            (
                "a new column that does not hold its default: supervision started",
                4,
                with(5, migration(5).replace(
                    "DROP TABLE http_checks;",
                    "UPDATE http_checks_v5 SET monitor = 'paused', monitor_interval_minutes = 5; DROP TABLE http_checks;",
                )),
                "2 rows of http_checks hold something other than 'off' in monitor",
            ),
        ];
        for (case, version, migrations, why) in cases {
            let migrations: Vec<(i64, &str)> = migrations
                .iter()
                .map(|(number, sql)| (*number, sql.as_str()))
                .collect();
            for logged in [false, true] {
                let path = path();
                let folder = path.parent().unwrap().to_owned();
                historical(&path, version, logged);
                odd(&path);
                let before = whole(&readonly(&path));
                let on_disk = files(&folder);
                let (outcome, steps) = upgrade_as_shown(&path, &migrations, &mut |_| Ok(()));
                let failed = outcome.unwrap_err();
                assert_eq!(failed.code, "upgrade_failed", "{case}: {}", failed.message);
                let name = format!("inventory-20260101T093000.123Z-v{version}.sqlite3");
                let kept = folder.join("backups").join(&name);
                for said in [
                    why,
                    "Nothing of the upgrade was committed",
                    &format!("in schema version {version}"),
                    &kept.display().to_string(),
                ] {
                    assert!(failed.message.contains(said), "{case}: {}", failed.message);
                }
                assert!(
                    matches!(steps.last(), Some(Step::Upgrading | Step::Verifying)),
                    "{case}"
                );
                // The inventory as it was, byte for byte, with nothing left to recover; beside it,
                // only the backup and its manifest.
                let mut now = files(&folder);
                assert!(now.remove(&format!("backups/{name}")).is_some(), "{case}");
                assert!(
                    now.remove(&format!("backups/{name}.json")).is_some(),
                    "{case}"
                );
                assert_eq!(now, on_disk, "{case}");
                assert_eq!(
                    examine(&path, |_| Ok(())).0,
                    Found::Outdated(version),
                    "{case}"
                );
                assert_eq!(whole(&readonly(&path)), before, "{case}");
                assert_eq!(whole(&readonly(&kept)), before, "{case}");
                remove(&path);
            }
        }
    }

    /// The environment variable that makes `upgrades_in_a_process_of_its_own` upgrade.
    const UPGRADER: &str = "DEVHQ_TEST_UPGRADER";

    /// Run by `upgrader` in a process of its own; on its own, it does nothing. `migrated` and
    /// `committed`: upgrades the inventory, with a cache so small that the migrations reach its files
    /// before they commit, and stops at that moment, until the test kills it as a crash would.
    /// `race`: waits for the file `go`, upgrades the inventory and writes how it went to `outcome`.
    #[test]
    #[ignore = "a helper that other tests run in a process of its own"]
    fn upgrades_in_a_process_of_its_own() {
        let Some(spec) = std::env::var_os(UPGRADER) else {
            return;
        };
        let spec = spec.into_string().unwrap();
        let [what, path, from, fingerprint, go, outcome, ready] =
            spec.split('\n').collect::<Vec<_>>()[..]
        else {
            panic!("{spec}")
        };
        let path = Path::new(path);
        let from = from.parse().unwrap();
        let small: Vec<(i64, String)> = MIGRATIONS
            .iter()
            .map(|(version, sql)| (*version, format!("PRAGMA cache_size = 2; {sql}")))
            .collect();
        let small: Vec<(i64, &str)> = small.iter().map(|(v, sql)| (*v, sql.as_str())).collect();
        let stop = |at: Moment| {
            move |moment: Moment| {
                if moment == at {
                    std::fs::write(ready, "").unwrap();
                    loop {
                        std::thread::sleep(Duration::from_secs(60));
                    }
                }
                Ok(())
            }
        };
        match what {
            "migrated" | "committed" => {
                let at = if what == "migrated" {
                    Moment::Migrated
                } else {
                    Moment::Committed
                };
                upgrade_with(
                    path,
                    (from, fingerprint),
                    &small,
                    &|| TAKEN_AT,
                    &mut |_| {},
                    (&mut |_| Ok(()), backup::names_on_disk),
                    &mut stop(at),
                )
                .unwrap();
                panic!("it did not stop");
            }
            "race" => {
                std::fs::write(ready, "").unwrap();
                while !Path::new(go).exists() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                let result = upgrade(path, from, fingerprint, &mut |_| {});
                let said = match result {
                    Ok(upgraded) => format!("upgraded {}", upgraded.backup.is_some()),
                    Err(refused) => format!("refused {}: {}", refused.code, refused.message),
                };
                std::fs::write(outcome, said).unwrap();
            }
            other => panic!("{other}"),
        }
    }

    /// A process of its own that upgrades the inventory at `path` with the person's consent to
    /// `fingerprint` of version `from`, as `what` says (see `upgrades_in_a_process_of_its_own`).
    fn upgrader(
        what: &str,
        path: &Path,
        (from, fingerprint): (i64, &str),
        go: &Path,
        outcome: &Path,
    ) -> crate::inventory::tests::Writer {
        helper(
            "upgrade::tests::upgrades_in_a_process_of_its_own",
            UPGRADER,
            &format!(
                "{what}\n{}\n{from}\n{fingerprint}\n{}\n{}",
                path.display(),
                go.display(),
                outcome.display()
            ),
        )
    }

    /// A DevHQ that ends, for any reason, in the middle of an upgrade leaves the inventory holding
    /// what it held and its verified backup: once SQLite recovers its files, the inventory is again
    /// what the person was shown, and so is the backup. DevHQ does not recover an inventory in an
    /// older schema on its own, so it refuses it, changing none of its files, and says how to tell
    /// whether a backup holds what they hold; checked as the README says and put in place of the
    /// inventory, the backup upgrades.
    #[test]
    fn an_upgrade_that_ends_before_its_commit_leaves_its_backup_to_put_back() {
        for logged in [false, true] {
            let path = path();
            let folder = path.parent().unwrap().to_owned();
            historical(&path, 6, logged);
            let shown = inspect(&path).fingerprint.unwrap();
            let original = std::fs::read(&path).unwrap();
            let nowhere = folder.join("unused");
            kill(
                upgrader("migrated", &path, (6, &shown), &nowhere, &nowhere),
                &path,
            );

            let left = files(&folder);
            let backups = [
                format!("backups/{BACKUP_V6}"),
                format!("backups/{BACKUP_V6}.json"),
            ];
            for name in &backups {
                assert!(left.contains_key(name), "{:?}", left.keys());
            }
            let unfinished = if logged {
                "inventory.sqlite3-wal"
            } else {
                "inventory.sqlite3-journal"
            };
            assert!(left.contains_key(unfinished), "{:?}", left.keys());
            assert!(matches!(
                examine(&path, |_| Ok(())).0,
                Found::NeedsRecovery(_)
            ));
            if !logged {
                assert_ne!(
                    left["inventory.sqlite3"], original,
                    "the migrations reached the file"
                );
            }

            // The window refuses it, and says what to do, without touching a file.
            let refused = start(&path).unwrap_err();
            assert_eq!(refused.code, "schema_outdated", "{}", refused.message);
            assert!(
                refused.message.contains("also need recovering"),
                "{}",
                refused.message
            );
            let inspection = inspect(&path);
            assert_eq!(
                (inspection.state, inspection.upgradable),
                ("needs_recovery", false)
            );
            presumes_no_backup(&inspection.problems);
            let refused = upgrade_to(&path, 6, &shown).unwrap_err();
            assert_eq!(refused.code, "needs_recovery");
            assert_eq!(files(&folder), left);

            // The README's checks. The manifest names this inventory, and the backup's bytes are
            // the ones it records.
            let backup = folder.join("backups").join(BACKUP_V6);
            let manifest: serde_json::Value = serde_json::from_slice(
                &std::fs::read(folder.join("backups").join(format!("{BACKUP_V6}.json"))).unwrap(),
            )
            .unwrap();
            assert_eq!(manifest["inventory"], path.display().to_string());
            assert_eq!(manifest["schemaVersion"], 6);
            assert_eq!(
                manifest["sha256"],
                hex(&Sha256::digest(std::fs::read(&backup).unwrap()))
            );
            // A copy of the inventory's files, recovered apart by SQLite, holds what the backup
            // holds: what the person was shown.
            let apart = folder.join("apart");
            std::fs::create_dir(&apart).unwrap();
            for name in ["inventory.sqlite3", unfinished] {
                std::fs::write(apart.join(name), &left[name]).unwrap();
            }
            let recovered = whole(&readonly(&apart.join("inventory.sqlite3")));
            assert_eq!(recovered, whole(&readonly(&backup)));
            assert_eq!(fingerprint_of(&apart.join("inventory.sqlite3")), shown);
            std::fs::remove_dir_all(&apart).unwrap();
            assert_eq!(files(&folder), left);

            // The way back: the inventory's files out of the folder, the backup in.
            let aside = folder.join("set aside");
            std::fs::create_dir(&aside).unwrap();
            for name in ["inventory.sqlite3", unfinished, "inventory.sqlite3-shm"] {
                if folder.join(name).exists() {
                    std::fs::rename(folder.join(name), aside.join(name)).unwrap();
                }
            }
            std::fs::copy(folder.join("backups").join(BACKUP_V6), &path).unwrap();
            assert_eq!(start(&path).unwrap_err().code, "schema_outdated");
            assert_eq!(inspect(&path).fingerprint.as_deref(), Some(shown.as_str()));
            // A minute later: the interrupted upgrade's backup keeps its name.
            let upgraded = upgrade_at(&path, 6, &shown, TAKEN_AT + 60_000).unwrap();
            assert_eq!(upgraded.from, 6);
            start(&path).unwrap();
            assert_eq!(crate::inventory::list(&path).unwrap().resources.len(), 2);
            remove(&path);
        }
    }

    /// What the inspection says of an inventory in an older schema whose files need recovering:
    /// what DevHQ found and did not do, what is unknown, and how to keep every way back open, with no
    /// backup presumed to exist or to hold what the files hold, and none chosen by its name.
    fn presumes_no_backup(problems: &[String]) {
        let said = problems.join(" ");
        for found in [
            "Its files need recovering",
            "it recovered, moved, deleted and replaced none of its files",
            "whether any backup holds the same",
            "copy them all to another folder before anything else",
            "A file's name does not make it this inventory's backup",
            "its .json manifest names this inventory",
            "it holds the same as a copy of these files recovered apart",
        ] {
            assert!(said.contains(found), "{found}: {problems:?}");
        }
        for presumed in [
            "holds everything",
            "the backup it made first",
            "copy that backup",
        ] {
            assert!(!said.contains(presumed), "{presumed}: {problems:?}");
        }
    }

    /// An inventory in an older schema left needing recovery by something other than an upgrade (an
    /// older DevHQ, or another program, that ended in the middle of a write, or committed changes
    /// that are only in its log), with no backup anywhere: the window's start, the inspection and an
    /// upgrade the person asks for presume none, make none, and recover, move, delete or replace
    /// none of its files.
    #[test]
    fn an_older_inventory_left_needing_recovery_is_kept_as_it_is_with_no_backup_presumed() {
        for logged in [false, true] {
            let path = path();
            let folder = path.parent().unwrap().to_owned();
            historical(&path, 6, logged);
            if logged {
                // Copies of the files taken while a connection held them: a change committed only
                // in the log, which is left without its index.
                let held = Connection::open(&path).unwrap();
                held.query_row("SELECT count(*) FROM resources", [], |_| Ok(()))
                    .unwrap();
                Connection::open(&path)
                    .unwrap()
                    .execute(
                        "UPDATE resources SET name = 'Renamed by an older DevHQ' WHERE id = 'host'",
                        [],
                    )
                    .unwrap();
                let file = std::fs::read(&path).unwrap();
                let log = std::fs::read(beside(&path, "-wal")).unwrap();
                drop(held);
                for gone in ["-wal", "-shm"] {
                    let _ = std::fs::remove_file(beside(&path, gone));
                }
                std::fs::write(&path, file).unwrap();
                std::fs::write(beside(&path, "-wal"), log).unwrap();
            } else {
                kill(writer(&path, "journal"), &path);
            }
            assert!(matches!(
                examine(&path, |_| Ok(())).0,
                Found::NeedsRecovery(_)
            ));
            let on_disk = files(&folder);

            let refused = start(&path).unwrap_err();
            assert_eq!(refused.code, "schema_outdated", "{}", refused.message);
            let inspection = inspect(&path);
            assert_eq!(
                (
                    inspection.state,
                    inspection.upgradable,
                    inspection.fingerprint.as_deref()
                ),
                ("needs_recovery", false, None)
            );
            presumes_no_backup(&inspection.problems);
            let refused = upgrade_to(&path, 6, "anything").unwrap_err();
            assert_eq!(refused.code, "needs_recovery", "{}", refused.message);
            assert_eq!(files(&folder), on_disk);
            assert!(!folder.join("backups").exists());
            remove(&path);
        }
    }

    /// A DevHQ that ends right after the upgrade committed leaves an inventory in this build's
    /// schema, which the window serves when it starts, recovering a log left behind.
    #[test]
    fn an_upgrade_that_ends_after_its_commit_is_served() {
        for logged in [false, true] {
            let path = path();
            historical(&path, 6, logged);
            let before = whole(&readonly(&path));
            let shown = inspect(&path).fingerprint.unwrap();
            let nowhere = path.with_file_name("unused");
            kill(
                upgrader("committed", &path, (6, &shown), &nowhere, &nowhere),
                &path,
            );
            if logged {
                assert_eq!(
                    examine(&path, |_| Ok(())).0,
                    Found::NeedsRecovery(crate::inventory::Recovery::Log)
                );
            }
            start(&path).unwrap();
            assert_eq!(examine(&path, |_| Ok(())).0, Found::Current);
            let after = readonly(&path);
            for table in ["resources", "applications", "components"] {
                assert_eq!(dump(&after, table), before[table], "{table}");
            }
            drop(after);
            assert!(path.with_file_name("backups").join(BACKUP_V6).exists());
            remove(&path);
        }
    }

    /// Several DevHQ processes asked to upgrade the same inventory at once: one backs it up and
    /// upgrades it, the others find it upgraded once they get the lock, or are refused while it is
    /// held, and none makes a second backup or upgrades it twice.
    #[test]
    fn concurrent_upgrades_make_one_backup_and_one_upgrade() {
        for logged in [false, true] {
            let path = path();
            let folder = path.parent().unwrap().to_owned();
            historical(&path, 6, logged);
            let before = whole(&readonly(&path));
            let shown = inspect(&path).fingerprint.unwrap();
            let go = folder.join("go");
            let outcomes: Vec<PathBuf> = (0..3)
                .map(|n| folder.join(format!("outcome-{n}")))
                .collect();
            let racers: Vec<_> = outcomes
                .iter()
                .map(|outcome| upgrader("race", &path, (6, &shown), &go, outcome))
                .collect();
            std::fs::write(&go, "").unwrap();
            for racer in racers {
                assert!(racer.finished());
            }
            let said: Vec<String> = outcomes
                .iter()
                .map(|outcome| std::fs::read_to_string(outcome).unwrap())
                .collect();
            assert_eq!(
                said.iter().filter(|said| *said == "upgraded true").count(),
                1,
                "{said:?}"
            );
            for other in said.iter().filter(|said| *said != "upgraded true") {
                assert!(
                    other == "upgraded false" || other.starts_with("refused upgrade_refused"),
                    "{said:?}"
                );
            }
            let backups: Vec<String> = names(&folder.join("backups"));
            assert_eq!(backups.len(), 2, "one backup and its manifest: {backups:?}");
            assert_eq!(examine(&path, |_| Ok(())).0, Found::Current);
            let after = readonly(&path);
            for table in ["resources", "applications", "components"] {
                assert_eq!(dump(&after, table)[..], before[table][..], "{table}");
            }
            drop(after);
            remove(&path);
        }
    }

    /// The window's look at an outdated inventory changes none of its files, whatever state they
    /// are in: with a log another connection holds, at rest in write-ahead mode, or needing
    /// recovery.
    #[test]
    fn the_inspection_changes_no_file() {
        let path = path();
        let folder = path.parent().unwrap().to_owned();
        historical(&path, 11, true);
        let on_disk = files(&folder);
        assert!(inspect(&path).upgradable);
        assert_eq!(files(&folder), on_disk);
        // Held by another connection, with its log and index.
        let held = Connection::open(&path).unwrap();
        held.query_row("SELECT count(*) FROM resources", [], |_| Ok(()))
            .unwrap();
        assert!(beside(&path, "-wal").exists());
        let on_disk = files(&folder);
        assert!(inspect(&path).upgradable);
        assert_eq!(files(&folder), on_disk);
        drop(held);
        // Left needing recovery.
        let nowhere = folder.join("unused");
        kill(
            upgrader(
                "migrated",
                &path,
                (11, &fingerprint_of(&path)),
                &nowhere,
                &nowhere,
            ),
            &path,
        );
        let on_disk = files(&folder);
        assert_eq!(inspect(&path).state, "needs_recovery");
        assert_eq!(files(&folder), on_disk);
        remove(&path);
    }
}
