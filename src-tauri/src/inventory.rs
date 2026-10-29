use crate::local_copies::readings::{self, LocalCopy};
use crate::records::{Actor, Subject};
use crate::references::{self, Reference, ReferenceInput, SharedRepository};
use crate::{access, change_log, http_check, records};
use rusqlite::{
    params, Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::File;
#[cfg(windows)]
use std::fs::TryLockError;
use std::io::{ErrorKind, Read, Write as _};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_input",
            message: message.into(),
        }
    }
    pub(crate) fn database(error: impl std::fmt::Display) -> Self {
        Self {
            code: "database_error",
            message: format!("Inventory database error: {error}"),
        }
    }
    /// DevHQ itself could not run a check. It is never recorded as an observation.
    pub(crate) fn check(error: impl std::fmt::Display) -> Self {
        Self {
            code: "check_error",
            message: format!("DevHQ could not run the HTTP check: {error}"),
        }
    }
    /// The check was removed, or DevHQ closed, while it ran. Nothing was recorded.
    pub(crate) fn cancelled(message: impl Into<String>) -> Self {
        Self {
            code: "cancelled",
            message: message.into(),
        }
    }
    /// The person has not opened the agent gate for this (see `access`). Nothing was done.
    pub(crate) fn denied(message: impl Into<String>) -> Self {
        Self {
            code: "access_denied",
            message: message.into(),
        }
    }
    /// A backup did not succeed: none was made, one is already being made, or the one made is kept
    /// without DevHQ vouching for it, as the message says. The inventory itself is untouched.
    pub(crate) fn backup(message: impl Into<String>) -> Self {
        Self {
            code: "backup_error",
            message: message.into(),
        }
    }
    /// DevHQ itself could not read a local copy (see `local_copies`). It is never kept as a
    /// reading: a copy that is missing or unreadable is a reading, this is not.
    pub(crate) fn read(message: impl Into<String>) -> Self {
        Self {
            code: "read_error",
            message: message.into(),
        }
    }
    /// An agent's edit was based on a state of the system that is no longer its state: someone
    /// changed it after the agent read it. Nothing was done; reading it again tells what is there.
    pub(crate) fn changed(message: impl Into<String>) -> Self {
        Self {
            code: "changed_since_read",
            message: message.into(),
        }
    }
    /// An agent asked to add what is already there, by its name. Nothing was added.
    pub(crate) fn exists(message: impl Into<String>) -> Self {
        Self {
            code: "already_exists",
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub description: String,
    pub operating_system: String,
    /// Where it is: its documentation, its logs, its dashboard… in the order they were saved.
    pub references: Vec<Reference>,
}

#[derive(Debug, Deserialize)]
// Serialized only by tests: ipc_contract finds the fixture keys Rust never reads.
#[cfg_attr(test, derive(Serialize))]
#[serde(rename_all = "camelCase")]
pub struct ResourceInput {
    pub id: Option<String>,
    pub name: String,
    pub kind: String,
    pub description: String,
    pub operating_system: String,
    /// The complete list: the save replaces the stored one.
    pub references: Vec<ReferenceInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub resource_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
// Serialized only by tests: ipc_contract finds the fixture keys Rust never reads.
#[cfg_attr(test, derive(Serialize))]
#[serde(rename_all = "camelCase")]
pub struct ComponentInput {
    pub id: Option<String>,
    pub name: String,
    pub kind: String,
    pub resource_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Application {
    pub id: String,
    pub name: String,
    pub description: String,
    pub lifecycle: String,
    pub version: Option<String>,
    pub components: Vec<Component>,
    /// Where it is: its repository, its local copies, its documentation… in the order they were
    /// saved.
    pub references: Vec<Reference>,
}

#[derive(Debug, Deserialize)]
// Serialized only by tests: ipc_contract finds the fixture keys Rust never reads.
#[cfg_attr(test, derive(Serialize))]
#[serde(rename_all = "camelCase")]
pub struct ApplicationInput {
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub lifecycle: String,
    pub version: Option<String>,
    pub components: Vec<ComponentInput>,
    /// The complete list: the save replaces the stored one.
    pub references: Vec<ReferenceInput>,
}

/// One consistent read of the local workspace: inventory plus its operational records.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub applications: Vec<Application>,
    pub resources: Vec<Resource>,
    /// Each repository more than one system declares, derived from their references at this read.
    pub shared_repositories: Vec<SharedRepository>,
    /// Each local copy the systems declare, with its newest reading and what DevHQ derives from it
    /// at this read. Older readings are paged per system.
    pub local_copies: Vec<LocalCopy>,
    pub observations: Vec<records::Observation>,
    pub incidents: Vec<records::Incident>,
    pub events: Vec<records::Event>,
    pub http_checks: Vec<http_check::HttpCheck>,
    /// Every change to the configuration, by the person or by an agent, newest first: all of it.
    pub changes: Vec<change_log::Change>,
    /// From when the person's changes are in `changes`: the upgrade that started the log on an
    /// inventory that already held systems. None when the log was always complete.
    pub changes_since: Option<String>,
    /// Which automated results the snapshot leaves out. Manual records are always complete.
    pub automated_history: records::AutomatedHistory,
}

/// Where the inventory lives on this device, so that a person can back it up.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DataLocation {
    /// The SQLite file. Once DevHQ is closed it holds everything on its own.
    pub database: String,
    /// The folder it is in, which also holds its write-ahead log while DevHQ has it open.
    pub directory: String,
    /// Where the file system really keeps the file, when that is not `database` (see `stored_at`).
    pub stored_at: Option<String>,
}

impl DataLocation {
    pub fn of(path: &Path) -> Self {
        Self {
            database: path.display().to_string(),
            directory: path
                .parent()
                .map(|folder| folder.display().to_string())
                .unwrap_or_default(),
            stored_at: stored_at(path),
        }
    }
}

/// Where the file system really keeps the file at `path`, when that is another path; `None` when
/// it is the same one or the file cannot be resolved (it does not exist yet, say).
///
/// A symbolic link or a junction leads elsewhere. So does Windows for an app started from a
/// packaged app, such as the Claude or Codex desktop app: it redirects the files that process
/// creates under AppData to the packaged app's own folder, and shows that process a merge of both.
/// DevHQ then reads and writes there, while File Explorer, the installed DevHQ and every other
/// program see the real AppData, where the file is missing or is another one. Windows resolves
/// each file on its own (a folder that exists on both sides resolves to the real one), so this
/// asks about the file itself, never its folder.
pub fn stored_at(path: &Path) -> Option<String> {
    let resolved = plain(&std::fs::canonicalize(path).ok()?);
    let asked = path.display().to_string();
    // Windows compares names regardless of case, and answers with the case the files have.
    let same = if cfg!(windows) {
        resolved.eq_ignore_ascii_case(&asked)
    } else {
        resolved == asked
    };
    (!same).then_some(resolved)
}

/// Windows resolves to a verbatim path (`\\?\C:\…`, `\\?\UNC\server\share\…`); File Explorer and
/// people use the plain form.
pub(crate) fn plain(path: &Path) -> String {
    let text = path.display().to_string();
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{share}")
    } else if let Some(drive) = text.strip_prefix(r"\\?\") {
        drive.to_owned()
    } else {
        text
    }
}

pub(crate) const SCHEMA_VERSION: i64 = 13;
/// Each schema version with the migration that brings an inventory to it from the one before.
/// They build a new inventory (`create`) and upgrade an older one (`upgrade.rs`), and are never
/// edited once released: an inventory left at any version has the structure they give it there.
pub(crate) const MIGRATIONS: [(i64, &str); 13] = [
    (1, include_str!("../migrations/001_inventory.sql")),
    (2, include_str!("../migrations/002_inventory_details.sql")),
    (3, include_str!("../migrations/003_operational_records.sql")),
    (4, include_str!("../migrations/004_http_checks.sql")),
    (5, include_str!("../migrations/005_http_monitoring.sql")),
    (6, include_str!("../migrations/006_history_window.sql")),
    (7, include_str!("../migrations/007_agent_provenance.sql")),
    (8, include_str!("../migrations/008_agent_access.sql")),
    (9, include_str!("../migrations/009_agent_log.sql")),
    (10, include_str!("../migrations/010_change_log.sql")),
    (11, include_str!("../migrations/011_references.sql")),
    (12, include_str!("../migrations/012_copy_readings.sql")),
    (13, include_str!("../migrations/013_maintain_access.sql")),
];

// How DevHQ reaches its inventory. No opening changes its schema: an inventory in an older schema
// is refused, never upgraded, except by the explicit upgrade the person starts in the window
// (`upgrade.rs`), which backs it up and verifies the copy first.
//
// - `create` makes a new inventory, only where there is no file at all, and is called only by the
//   window when it starts (`start`), never while it is open and never by the gate.
// - `start` is what the window does before anything else opens the inventory: create one where
//   none is, recover one in this build's schema that a crash left needing it, and switch it to
//   write-ahead logging. It leaves anything else as it found it.
// - `open_current` opens an existing file for a use case, and `begin` and `begin_write` start the
//   use case's transaction only once they found, inside it, an inventory in this build's schema
//   (and, for an agent's write, the person's leave): what a use case reads or writes is always
//   the current schema, whatever happened to the file since anyone last looked.
// - `examine` and `read_as_found` are the agent gate's reads, which change no file; the window
//   looks at an outdated inventory the same way before offering to upgrade it.
// - `open_to_upgrade` is the explicit upgrade's own connection, and the only one that applies a
//   migration to an inventory that exists.

/// How long a use case's connection waits for another writer before SQLite refuses with "database
/// is locked", and how long an agent's write waits for readers to let it empty the log.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// What a use case's connection does on top of SQLite's defaults: waits up to five seconds for
/// another writer, and enforces relationships.
fn prepared(db: Connection) -> Result<Connection, ApiError> {
    db.busy_timeout(BUSY_TIMEOUT).map_err(ApiError::database)?;
    db.pragma_update(None, "foreign_keys", "ON")
        .map_err(ApiError::database)?;
    Ok(db)
}

/// A connection to the inventory at `path`, for one of the window's use cases or one of the gate's
/// writes: only if a file is there, which it never creates, upgrades or converts to another journal
/// mode. It does not say what the file is: every use goes through `begin` or `begin_write`, which
/// refuse anything but an inventory in this build's schema inside the transaction.
///
/// It can write, as every use case may, so SQLite recovers what a crash left in the file the first
/// time the connection reads it, as any program that writes to a SQLite file does: it plays back a
/// rollback journal or folds a log that no connection holds. The window recovers at `start` only
/// an inventory in this build's schema; one that a crash leaves while the window is open, or that
/// another program puts in its place, is recovered here before it is classified.
pub(crate) fn open_current(path: &Path) -> Result<Connection, ApiError> {
    let opened = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    );
    match opened {
        Ok(db) => prepared(db),
        // Without SQLITE_OPEN_CREATE a missing file cannot be opened: say what is missing.
        Err(_) if !path.exists() => Err(Found::Missing.refusal(path)),
        Err(error) => Err(ApiError::database(error)),
    }
}

/// A transaction on `db` (`behavior`: Deferred to read, Immediate to write), started only once a
/// look inside it found an inventory in this build's schema, with its structure: what the
/// transaction reads is that inventory, whatever replaced, upgraded or emptied the file before.
/// Anything else is refused with what was found, and the transaction is rolled back before it read
/// or wrote anything else. A read transaction sees one state from its first read, the look, on.
pub(crate) fn begin(
    db: &mut Connection,
    behavior: TransactionBehavior,
) -> Result<Transaction<'_>, ApiError> {
    begin_on(db, behavior)
}

/// `begin` on a connection borrowed for as long as the transaction lives: `begin_write` needs the
/// connection again once the transaction has committed. The borrow comes from `&mut`, so nothing
/// else uses the connection meanwhile, as `Connection::transaction_with_behavior` ensures.
fn begin_on(db: &Connection, behavior: TransactionBehavior) -> Result<Transaction<'_>, ApiError> {
    let path = PathBuf::from(db.path().unwrap_or_default());
    let tx = Transaction::new_unchecked(db, behavior).map_err(ApiError::database)?;
    match classify(&tx).map_err(ApiError::database)? {
        Found::Current => Ok(tx),
        found => Err(found.refusal(&path)),
    }
}

/// A write transaction on `db` for `by`, with SQLite's write lock held, started only once it found
/// inside it an inventory in this build's schema and, for an agent, that the person lets agents
/// write (see `access`): that decides, so that closing the agent gate in Settings, or replacing,
/// upgrading or emptying the file, can never race a write that was asked for before.
pub(crate) fn begin_write<'a>(db: &'a mut Connection, by: &Actor) -> Result<Writing<'a>, ApiError> {
    begin_write_for(db, by, "write")
}

/// `begin_write` for a change that needs `need` of the agent gate's access (see
/// `access::permitted`): `maintain` to add or edit applications and resources, `write` for
/// anything else. The person's own changes need none.
pub(crate) fn begin_write_for<'a>(
    db: &'a mut Connection,
    by: &Actor,
    need: &str,
) -> Result<Writing<'a>, ApiError> {
    let db: &'a Connection = db;
    let tx = begin_on(db, TransactionBehavior::Immediate)?;
    let agent = matches!(by, Actor::Agent(_));
    if agent {
        access::permitted(&access::mode(&tx)?, need)?;
    }
    Ok(Writing { tx, db, agent })
}

/// A write transaction from `begin_write`, used as the `Transaction` it holds. Dropped without
/// `commit`, it rolls back, as a `Transaction` does.
#[derive(Debug)]
pub(crate) struct Writing<'a> {
    tx: Transaction<'a>,
    db: &'a Connection,
    agent: bool,
}

impl<'a> std::ops::Deref for Writing<'a> {
    type Target = Transaction<'a>;

    fn deref(&self) -> &Transaction<'a> {
        &self.tx
    }
}

impl Writing<'_> {
    /// Commits the transaction and, for an agent, then tries to empty the log (see `empty_log`):
    /// once the commit succeeded, the write succeeded, whether or not the log could be emptied.
    pub(crate) fn commit(self) -> rusqlite::Result<()> {
        self.tx.commit()?;
        if self.agent {
            empty_log(self.db, BUSY_TIMEOUT);
        }
        Ok(())
    }
}

/// Folds what the write-ahead log holds into the file and empties the log, on `db`, a connection
/// that has just committed an agent's write, before it closes; true once the log is empty.
///
/// SQLite folds the log into the file and deletes it only when the last connection to the database
/// closes, and only if that connection can: one that closes while another reads (a gate's read is
/// read-only, and in WAL mode a reader holds the file shared) leaves its changes in the log, and the
/// read-only connection that closes after it cannot fold them. With the window closed nothing holds
/// that log, and every gate, which never recovers what it finds (see `examine`), refuses the
/// inventory with `needs_recovery`, reads and writes alike, since every write first reads: only the
/// window's start recovers it. Two agent sessions, one writing while the other reads, were enough.
/// Folding here, before the connection closes, leaves that log empty instead, which the gate reads
/// as it reads a file without a log.
///
/// `TRUNCATE` can empty the log only once no read in progress still uses it, which a gate's read
/// does for milliseconds, and it empties it to zero bytes: a log that is only reset keeps its size,
/// and the gate tells a log with changes by its size. Waiting with the busy timeout would hold
/// SQLite's write lock all the while, and another write, an agent's or the window's, would wait
/// behind it and could be refused with "database is locked". So each try waits for nothing and
/// lets go of the lock at once, and the tries repeat, with SQLite's own pauses between tries of a
/// lock, for up to `patience` (the busy timeout, as any lock): another write takes the lock between
/// two tries, and only the agent's answer waits.
///
/// Nothing it does is a decision: the write has committed, and what it folds are committed pages,
/// so a fold that does not finish (a read kept the log past `patience`) is not an error, and leaves
/// the log as SQLite would have. While a connection still holds it, a later agent's write folds it
/// the same way; if the last to close is a gate's read, nothing holds it, every gate refuses the
/// inventory before any write could fold it, and the window recovers it when it starts.
/// The window's own writes are left as they were: while it is open its own connection keeps the
/// log, and it folds it when it closes.
fn empty_log(db: &Connection, patience: Duration) -> bool {
    /// The pauses SQLite's default busy handler makes between tries of a lock, in milliseconds;
    /// the last one repeats.
    const PAUSES: [u64; 12] = [1, 2, 5, 10, 15, 20, 25, 25, 25, 50, 50, 100];
    let deadline = std::time::Instant::now() + patience;
    // Without a busy timeout a try that finds a read in progress, or another writer, gives up at
    // once, and lets go of the write lock, instead of waiting with it held.
    if db.busy_timeout(Duration::ZERO).is_err() {
        return false;
    }
    let mut tries = 0;
    let emptied = loop {
        // Its first column is 1 when the log could not be emptied because something used it.
        match db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            row.get::<_, i64>(0)
        }) {
            Ok(0) => break true,
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(failure, _))
                if failure.code == rusqlite::ErrorCode::DatabaseBusy => {}
            Err(_) => break false,
        }
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            break false;
        }
        let pause = Duration::from_millis(PAUSES[tries.min(PAUSES.len() - 1)]);
        std::thread::sleep(pause.min(left));
        tries += 1;
    };
    let _ = db.busy_timeout(BUSY_TIMEOUT);
    emptied
}

/// Runs `work` in a write transaction for `by` on the inventory at `path` (see `begin_write`), and
/// commits what it did unless it failed.
pub(crate) fn write<T>(
    path: &Path,
    by: &Actor,
    work: impl FnOnce(&Transaction) -> Result<T, ApiError>,
) -> Result<T, ApiError> {
    let mut db = open_current(path)?;
    let tx = begin_write(&mut db, by)?;
    let value = work(&tx)?;
    tx.commit().map_err(ApiError::database)?;
    Ok(value)
}

/// Creates a new, empty inventory in this build's schema at `path`, only where there is no file:
/// false when a file was there, which it leaves as it is. It builds the inventory under a name of
/// its own beside `path`, in one transaction, and then gives it `path` with a second name, which
/// the file system refuses if anything has that name: `path` never holds half an inventory, and a
/// file that appears there meanwhile, another DevHQ's new inventory or anything else, is never
/// replaced or written to. A folder that cannot hold a second name for a file (a hard link) cannot
/// hold a new inventory either; DevHQ's backups need the same.
///
/// Nor where a file SQLite keeps beside a database is there without the database (`leftovers`):
/// SQLite pairs a database with its rollback journal and its write-ahead log by name alone, so it
/// would play another database's journal back into the new inventory, damaging it, or read
/// another database's log as part of it. Those files may be all that is left of an inventory's
/// last writes, and they are refused and left as they are, never deleted.
///
/// The new file uses a rollback journal: `start` switches it to write-ahead logging once it is in
/// place, as it does for any current inventory.
pub(crate) fn create(path: &Path) -> Result<bool, ApiError> {
    let failed = |error: &dyn std::fmt::Display| {
        ApiError::database(format!(
            "DevHQ could not create a new inventory at {}: {error}",
            path.display()
        ))
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| failed(&error))?;
    }
    let building = beside(path, &format!(".new-{}", Uuid::new_v4()));
    let built = (|| {
        let mut db = Connection::open_with_flags(
            &building,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| failed(&error))?;
        // Rebuilding tables needs enforcement off; migrate() checks every relationship.
        db.pragma_update(None, "foreign_keys", "OFF")
            .map_err(|error| failed(&error))?;
        migrate(&mut db)?;
        drop(db);
        // On disk before it is named, so that `path` never names data still in a cache. Windows
        // flushes only through a handle that can write.
        File::options()
            .write(true)
            .open(&building)
            .and_then(|file| file.sync_all())
            .map_err(|error| failed(&error))?;
        // Looked at just before naming it. An inventory's own files appear only once the
        // inventory has its name (another window's new one, say), so with them there `path` is
        // taken; without it, they belong to a database that is not there.
        let left = leftovers(path).map_err(|error| failed(&error))?;
        if !left.is_empty() {
            return match std::fs::symlink_metadata(path) {
                Ok(_) => Ok(false),
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    Err(left_without_a_database(path, &left))
                }
                Err(error) => Err(failed(&error)),
            };
        }
        match std::fs::hard_link(&building, path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(failed(&error)),
        }
    })();
    // Named or not, the building name goes: the inventory keeps `path`.
    let _ = std::fs::remove_file(&building);
    built
}

/// The files SQLite keeps beside the database at `path` that are there: its rollback journal, its
/// write-ahead log and the log's index, whatever they hold (anything with the name counts).
fn leftovers(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for suffix in ["-journal", "-wal", "-shm"] {
        let file = beside(path, suffix);
        match std::fs::symlink_metadata(&file) {
            Ok(_) => found.push(file),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(found)
}

/// Why the window creates no inventory at `path`, where `left` are there without a database.
fn left_without_a_database(path: &Path, left: &[PathBuf]) -> ApiError {
    let names: Vec<_> = left.iter().map(|file| file.display().to_string()).collect();
    ApiError {
        code: "leftover_files",
        message: format!(
            "There is no DevHQ inventory at {}, yet files SQLite keeps beside a database are there ({}): they may hold the last writes of an inventory that is no longer there. SQLite would apply them to any new file with that name, so DevHQ creates no inventory there and leaves them as they are. Put back the inventory they belong to, or move them out of the folder, and start DevHQ again. Nothing was read or changed.",
            path.display(),
            listed(&names),
        ),
    }
}

/// What the window does with the inventory at `path` when it starts, before its commands or its
/// supervision open it: Ok when it is an inventory in this build's schema, ready to use; otherwise
/// why the window serves nothing from it, which every command answers until DevHQ starts again.
///
/// - No file there: it creates a new inventory (`create`). A file that is there, even an empty
///   one, is never initialized.
/// - An inventory in this build's schema: it switches it to write-ahead logging, the only place
///   DevHQ sets a journal mode.
/// - A file left needing recovery (a write that did not finish, or a log no connection holds):
///   it lets SQLite recover it only once a copy of its files, recovered apart in a temporary
///   folder, proved to be an inventory in this build's schema (`recovered_copy`), and only if,
///   with the connection that recovers it open, its files still hold the bytes that were copied.
///   Anything else, older, newer or not an inventory, keeps its files as they are.
/// - Anything else is left as it is, and refused.
///
/// It looks with `examine`, which on Unix must never run while this process holds a connection
/// to the inventory: the window calls it before anything opens one.
pub(crate) fn start(path: &Path) -> Result<(), ApiError> {
    start_with(path, &mut |_| {})
}

/// The moments at which `start_with` lets a test change the files under it, as another program
/// could.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Moment {
    /// A copy of the files was recovered and found current; nothing has opened them.
    Copied,
    /// The connection that would recover them is open and has read nothing.
    Opened,
}

/// `start`, calling `meanwhile` at each `Moment`.
fn start_with(path: &Path, meanwhile: &mut dyn FnMut(Moment)) -> Result<(), ApiError> {
    let mut unreadable = "its files kept changing while DevHQ opened them".to_owned();
    for _ in 0..2 * LOOKS {
        match examine(path, |_| Ok(())).0 {
            Found::Missing => {
                // Whoever names it first, this window or another starting at the same moment,
                // made a current inventory: look again either way.
                create(path)?;
            }
            Found::NeedsRecovery(recovery) => match recovered_copy(path) {
                (Found::Current, Some(copied)) => {
                    meanwhile(Moment::Copied);
                    let mut db = open_current(path)?;
                    meanwhile(Moment::Opened);
                    // SQLite recovers at the connection's first read, inside the look in `begin`,
                    // from whatever files then have these names: only from the ones the copy was
                    // made of, looked at again once the connection holds the database file, which
                    // on Windows nothing can then rename or delete. A journal or a log is opened
                    // by name at that read, so one put in place between this look and it is the
                    // only kind of change this does not see. Files that changed are not
                    // recovered: the connection goes without reading, before `examine` looks at
                    // them again (which on Unix it must never do beside a connection of its own).
                    if RecoveredFrom::of(path).ok() != Some(copied) {
                        drop(db);
                        unreadable = "its files changed between the copy DevHQ recovered apart and their own recovery".to_owned();
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    begin(&mut db, TransactionBehavior::Deferred)?;
                }
                (found, _) => return Err(left_needing_recovery(path, &found, recovery)),
            },
            Found::Current => {
                let db = open_current(path)?;
                // Classified again on this connection, which is bound to the file it opened.
                match classify(&db).map_err(ApiError::database)? {
                    Found::Current => {}
                    found => return Err(found.refusal(path)),
                }
                return log_ahead(&db);
            }
            // Its files changed under every look: another window starting at the same moment
            // creates, recovers or switches it, and opens and closes its log as it goes. Looked at
            // again once it has had a moment; a file that is unreadable for good stays so.
            Found::Unreadable(why) => {
                unreadable = why;
                std::thread::sleep(Duration::from_millis(100));
            }
            found => return Err(found.refusal(path)),
        }
    }
    Err(Found::Unreadable(unreadable).refusal(path))
}

/// Switches the inventory on `db` to write-ahead logging: a reader works on the last committed
/// state and never waits for a writer, and a writer never waits for readers. Supervision writes
/// from the async runtime while the window reads, and some commands run on the main thread, so with
/// a rollback journal a reader could stall the window behind a commit in progress. Writers still
/// take turns. The mode is stored in the file, so setting it again is a no-op; when the last
/// connection closes SQLite folds the log back into `inventory.sqlite3`.
fn log_ahead(db: &Connection) -> Result<(), ApiError> {
    // Switching needs the file to itself, and SQLite does not wait for that: while another
    // connection holds a lock (the agent gate writing), the next start switches it. Until then,
    // and where the file system cannot share memory between connections, the rollback journal
    // stays: just as correct, only readers and writers wait for each other.
    match db.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0)) {
        Err(rusqlite::Error::SqliteFailure(error, _))
            if error.code == rusqlite::ErrorCode::DatabaseBusy =>
        {
            Ok(())
        }
        result => result.map(|_| ()).map_err(ApiError::database),
    }
}

/// What the file at `path`, which needs recovering, is once recovered, found on a copy of its
/// files that SQLite recovers in a temporary folder of its own, which is then removed: the files
/// at `path` are only read. With it, what the copy was made of, when it could be made.
fn recovered_copy(path: &Path) -> (Found, Option<RecoveredFrom>) {
    let folder = std::env::temp_dir().join(format!("devhq-recovering-{}", Uuid::new_v4()));
    let found = (|| {
        let unreadable = |error: std::io::Error| Found::Unreadable(error.to_string());
        let changed = || Found::Unreadable("its files changed while DevHQ copied them".to_owned());
        let before = OnDisk::look(path)?;
        std::fs::create_dir(&folder).map_err(unreadable)?;
        let copy = folder.join("inventory.sqlite3");
        // The log's index is rebuilt from the log; a journal is copied whatever it holds.
        let copy_beside = |suffix: &str| copy_of(&beside(path, suffix), &beside(&copy, suffix));
        let copied = RecoveredFrom {
            database: copy_beside("").map_err(unreadable)?.ok_or_else(changed)?,
            journal: copy_beside("-journal").map_err(unreadable)?,
            log: copy_beside("-wal").map_err(unreadable)?,
        };
        if OnDisk::look(path).as_ref() != Ok(&before) {
            return Err(changed());
        }
        let db = Connection::open_with_flags(
            &copy,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| Found::Unreadable(error.to_string()))?;
        let found = classify(&db).map_err(|error| Found::Unreadable(error.to_string()))?;
        Ok((found, Some(copied)))
    })()
    .unwrap_or_else(|found| (found, None));
    let _ = std::fs::remove_dir_all(&folder);
    found
}

/// The bytes of the files SQLite recovers a database from: the database file, and the rollback
/// journal and the write-ahead log beside it, each None when it is not there. Not the log's index
/// (`-shm`), which SQLite rebuilds from the log when no connection holds it. Two sets of files with
/// the same bytes recover into the same database, wherever and whenever SQLite recovers them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RecoveredFrom {
    database: [u8; 32],
    journal: Option<[u8; 32]>,
    log: Option<[u8; 32]>,
}

impl RecoveredFrom {
    /// What the files at `path` hold now.
    fn of(path: &Path) -> std::io::Result<Self> {
        let digest = |suffix: &str| match content(&beside(path, suffix)) {
            Ok(digest) => Ok(Some(digest)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        };
        Ok(Self {
            database: digest("")?.ok_or_else(|| std::io::Error::from(ErrorKind::NotFound))?,
            journal: digest("-journal")?,
            log: digest("-wal")?,
        })
    }
}

/// Copies the file at `from` to `to`, which must not exist, and gives the digest of the bytes it
/// copied, read once: what the copy holds is what was digested. None when there is no file.
fn copy_of(from: &Path, to: &Path) -> std::io::Result<Option<[u8; 32]>> {
    let mut source = match File::open(from) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut copy = File::create_new(to)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        copy.write_all(&buffer[..read])?;
    }
    Ok(Some(hasher.finalize().into()))
}

/// Why the window serves nothing from a file left needing recovery whose recovered copy was
/// `found`, not an inventory in this build's schema: its files are left as they are.
fn left_needing_recovery(path: &Path, found: &Found, recovery: Recovery) -> ApiError {
    let refused = found.refusal(path);
    let unfinished = Found::NeedsRecovery(recovery).meaning();
    ApiError {
        code: refused.code,
        message: format!(
            "{} Its files also need recovering ({unfinished}) DevHQ found that out on a copy, which it recovered apart and removed, and left the files themselves as they are.",
            refused.message
        ),
    }
}

/// Upgrades the inventory at `path` from an older schema to this build's, with no backup and no
/// check of what it carries over. Only tests call it, to prove what each migration keeps; DevHQ
/// upgrades an inventory only through `upgrade.rs`, which backs it up and checks both.
#[cfg(test)]
pub(crate) fn upgrade(path: &Path) -> Result<(), ApiError> {
    let mut db = open_to_upgrade(path)?;
    migrate(&mut db)
}

/// The connection through which the explicit upgrade changes the inventory at `path` (see
/// `upgrade.rs`), the only one that applies a migration to an inventory that exists: only if a
/// file is there, which it never creates or converts to another journal mode. It waits up to five
/// seconds for another writer, and leaves relationships unenforced, since migrations rebuild
/// tables that others refer to: they are checked once the migrations ran, before anything commits.
/// Like `open_current`, it can write, so SQLite recovers what a crash left the first time it reads:
/// the upgrade opens it only once a look that changes nothing found no file to recover.
pub(crate) fn open_to_upgrade(path: &Path) -> Result<Connection, ApiError> {
    let db = open_current(path)?;
    // Only outside a transaction does SQLite let this change.
    db.pragma_update(None, "foreign_keys", "OFF")
        .map_err(ApiError::database)?;
    Ok(db)
}

/// A connection for a test to the inventory at `path`, which it makes as DevHQ would have had it:
/// a new one where there is no file, and one in an older schema upgraded (see `upgrade`). It has
/// read the inventory once, so that it holds a write-ahead log open as the window's connections do.
/// Only tests have it: anything that opens the inventory for DevHQ itself goes through
/// `open_current`.
#[cfg(test)]
pub(crate) fn open(path: &Path) -> Result<Connection, ApiError> {
    if !path.exists() {
        start(path)?;
    }
    upgrade(path)?;
    let mut db = open_current(path)?;
    begin(&mut db, TransactionBehavior::Deferred)?;
    Ok(db)
}

/// The schema version stored in the file. A newer one than this build knows is refused
/// rather than guessed at.
fn schema_version(db: &Connection) -> Result<i64, ApiError> {
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(ApiError::database)?;
    if version > SCHEMA_VERSION {
        return Err(ApiError::database(format!(
            "unsupported inventory schema version {version}"
        )));
    }
    Ok(version)
}

/// Brings the schema on `db` up to this build's in one transaction: from nothing for `create`,
/// and, in tests only, from an older one (`upgrade`). Every relationship is checked before the
/// commit; anything that fails rolls the whole of it back.
fn migrate(db: &mut Connection) -> Result<(), ApiError> {
    // Whoever gets the lock second finds the schema already there.
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    let version = schema_version(&tx)?;
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    apply_migrations(&tx, version, &MIGRATIONS)?;
    let broken = broken_relationships(&tx).map_err(ApiError::database)?;
    if broken > 0 {
        // Dropping the transaction rolls the whole upgrade back.
        return Err(ApiError::database(format!(
            "upgrading to schema version {SCHEMA_VERSION} would leave {broken} broken relationships; the database was left unchanged"
        )));
    }
    tx.commit().map_err(ApiError::database)?;
    Ok(())
}

/// Runs, in `tx`, each of `migrations` past schema `version`, in order, stamping the version each
/// one brings: what `migrate` and the explicit upgrade (`upgrade.rs`) both apply. It commits
/// nothing; a migration that fails leaves the rest to the transaction's rollback.
pub(crate) fn apply_migrations(
    tx: &Transaction,
    version: i64,
    migrations: &[(i64, &str)],
) -> Result<(), ApiError> {
    for (number, sql) in migrations {
        if version < *number {
            tx.execute_batch(sql).map_err(|error| {
                ApiError::database(format!("migration {number:03} failed: {error}"))
            })?;
            tx.pragma_update(None, "user_version", number)
                .map_err(ApiError::database)?;
        }
    }
    Ok(())
}

/// How many rows refer to a row that is not there, as `PRAGMA foreign_key_check` finds them.
pub(crate) fn broken_relationships(db: &Connection) -> rusqlite::Result<usize> {
    let mut statement = db.prepare("PRAGMA foreign_key_check")?;
    let rows = statement.query_map([], |_| Ok(()))?.count();
    Ok(rows)
}

/// What the agent gate finds at an inventory's path, told without changing anything there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Found {
    /// No file there.
    Missing,
    /// A DevHQ inventory that declares this build's schema and has its tables, columns and
    /// indexes: the only kind the gate serves.
    Current,
    /// A file that declares this build's schema without having its structure: what differs.
    Mismatch(Vec<String>),
    /// A DevHQ inventory that declares an older schema, which nothing in this build upgrades.
    Outdated(i64),
    /// A DevHQ inventory that declares a schema newer than this build knows.
    Newer(i64),
    /// A SQLite database without DevHQ's tables: `empty` when it has no table at all.
    Foreign { empty: bool },
    /// A file SQLite would have to recover, changing its files, before reading anything in it;
    /// the gate never does.
    NeedsRecovery(Recovery),
    /// Not readable as a SQLite database, and why: not one, damaged, locked, a folder…
    Unreadable(String),
}

/// What SQLite would have to recover before a file could be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Recovery {
    /// A rollback journal that a write left behind (a hot journal): until SQLite plays it back,
    /// the file may hold half of that write.
    Journal,
    /// A write-ahead log holding changes that no connection holds open.
    Log,
    /// A write-ahead log without the index (`-shm`) through which SQLite reads it.
    LogWithoutIndex,
}

impl Found {
    /// The schema version the file declares, when it is a DevHQ inventory that could be read:
    /// never a guess.
    pub(crate) fn stored(&self) -> Option<i64> {
        match self {
            Self::Current | Self::Mismatch(_) => Some(SCHEMA_VERSION),
            Self::Outdated(version) | Self::Newer(version) => Some(*version),
            Self::Missing | Self::Foreign { .. } | Self::NeedsRecovery(_) | Self::Unreadable(_) => {
                None
            }
        }
    }

    pub(crate) fn state(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Current => "current",
            Self::Mismatch(_) => "mismatch",
            Self::Outdated(_) => "outdated",
            Self::Newer(_) => "newer",
            Self::Foreign { .. } => "not_an_inventory",
            Self::NeedsRecovery(_) => "needs_recovery",
            Self::Unreadable(_) => "unreadable",
        }
    }

    /// Whether the file has the structure of the schema it declares: `matches` and `differs` only
    /// for this build's schema, the one DevHQ can compare; `not_checked` for any other version;
    /// None when no schema could be read.
    pub(crate) fn structure(&self) -> Option<&'static str> {
        match self {
            Self::Current => Some("matches"),
            Self::Mismatch(_) => Some("differs"),
            Self::Outdated(_) | Self::Newer(_) => Some("not_checked"),
            Self::Missing | Self::Foreign { .. } | Self::NeedsRecovery(_) | Self::Unreadable(_) => {
                None
            }
        }
    }

    /// What it means for the person and for an agent, in a sentence or two.
    pub(crate) fn meaning(&self) -> String {
        match self {
            Self::Missing => "There is no file there. The gate never creates one, and DevHQ's window creates a new, empty inventory only when it starts and finds no file, never while it is open.".to_owned(),
            Self::Current => format!("A DevHQ inventory that declares schema version {SCHEMA_VERSION}, the one this DevHQ reads, and has every table, column and index of that schema. Its integrity (each page and row) was not checked."),
            Self::Mismatch(differences) => format!("A file that declares schema version {SCHEMA_VERSION}, the one this DevHQ reads, without that schema's structure: {}. The gate serves nothing from it, so what it holds is unknown.", differences.join("; ")),
            Self::Outdated(version) => format!("A DevHQ inventory that declares schema version {version}, older than this DevHQ's ({SCHEMA_VERSION}). This DevHQ never upgrades an inventory when it opens it: neither its window nor the gate serves anything from this one, and both leave it as it is. Only the person upgrades it, from DevHQ's window (Back up and upgrade), which first makes and verifies a backup of it; the gate never does."),
            Self::Newer(version) => format!("A DevHQ inventory that declares schema version {version}, written by a DevHQ newer than this one (schema {SCHEMA_VERSION}). This DevHQ cannot read it, and the gate serves nothing from it."),
            Self::Foreign { empty: true } => "An empty file or a SQLite database without tables: not a DevHQ inventory. DevHQ never turns a file into one: it creates a new inventory only where there is no file at all.".to_owned(),
            Self::Foreign { empty: false } => "A SQLite database without DevHQ's tables: not a DevHQ inventory. DevHQ never turns a file into one.".to_owned(),
            Self::NeedsRecovery(Recovery::Journal) => "A rollback journal (-journal) beside the file, left by a write that did not finish: until SQLite plays it back, the file may hold half of that write. Playing it back changes the file, and the gate never does; SQLite does it the next time a program that writes opens the file; DevHQ's window does it when it starts only for an inventory in this DevHQ's schema, which it finds out first on a copy. Until then what the file holds, its schema version included, is unknown.".to_owned(),
            Self::NeedsRecovery(Recovery::Log) => "A write-ahead log (-wal) with changes that no connection holds open: whoever wrote them, DevHQ's window or another program, ended without folding them into the file (it crashed, was killed, or closed while another connection was reading). Reading them needs SQLite to recover the log, which changes the files, and the gate never does; DevHQ's window recovers and folds it when it starts, only for an inventory in this DevHQ's schema, which it finds out first on a copy. Until then what the inventory holds, its schema version included, is unknown.".to_owned(),
            Self::NeedsRecovery(Recovery::LogWithoutIndex) => "A write-ahead log (-wal) without its index (-shm): SQLite would have to rebuild the index from the log, creating a file, before reading anything, and the gate never does; DevHQ's window does it when it starts, only for an inventory in this DevHQ's schema, which it finds out first on a copy. Until then what the inventory holds, its schema version included, is unknown.".to_owned(),
            Self::Unreadable(why) => format!("DevHQ could not read the file as a SQLite database ({why}), so what it holds, its schema included, is unknown."),
        }
    }

    /// Why the gate serves nothing from the file at `path`; never asked of `Current`.
    pub(crate) fn refusal(&self, path: &Path) -> ApiError {
        let at = path.display();
        let (code, message) = match self {
            Self::Missing => {
                return ApiError::invalid(format!(
                    "There is no DevHQ inventory at {at}. DevHQ's window creates a new one only when it starts and finds no file there, never while it is open; the gate never creates one (start it with --database <path> for another inventory)."
                ))
            }
            Self::Current => {
                return ApiError::database(format!("{at} is a current inventory, served, not refused."))
            }
            Self::Mismatch(_) => ("schema_mismatch", self.meaning()),
            Self::Outdated(_) => ("schema_outdated", self.meaning()),
            Self::Newer(_) => ("schema_newer", self.meaning()),
            Self::Foreign { .. } => ("not_an_inventory", self.meaning()),
            Self::NeedsRecovery(_) => ("needs_recovery", self.meaning()),
            Self::Unreadable(_) => ("database_error", self.meaning()),
        };
        ApiError {
            code,
            message: format!("{at}: {message} Nothing was read or changed."),
        }
    }
}

/// How many times the gate looks again at files that changed while it looked or read.
const LOOKS: usize = 3;

/// How long the gate waits, each time, before it looks again at files that need recovering, to
/// tell files left so from files caught in the instant a connection closes. The last connection to
/// a write-ahead-logged database lets go of the log's index, deletes the index and only then
/// deletes the log: a look in between finds a log with changes that nothing holds, or one without
/// its index, which a moment later is gone.
const SETTLE: [Duration; 2] = [Duration::from_millis(25), Duration::from_millis(100)];

/// Whether the files at `path` are still `disk` after each pause of `SETTLE`: what needs recovering
/// was left so, and is not a connection closing, or a writer committing, in that instant.
fn settled(path: &Path, disk: &OnDisk) -> bool {
    for pause in SETTLE {
        #[cfg(test)]
        tests::while_settling();
        std::thread::sleep(pause);
        if OnDisk::look(path).as_ref() != Ok(disk) {
            return false;
        }
    }
    true
}

/// What is at `path`, told without changing any of its files, and, when it is an inventory in this
/// build's schema, what `read` made of it on a connection that cannot change them either.
///
/// SQLite changes a database's files just by opening it in some states, whatever the connection
/// is allowed to do: the first connection to a write-ahead log left by a process that ended
/// rebuilds its index (`-shm`), the last one to close folds the log back into the file and deletes
/// it, one that finds a rollback journal left by an interrupted write plays it back, and any but an
/// `immutable` one creates the log of a write-ahead-logged file that has none. So the gate first
/// looks at the files without SQLite (`OnDisk`) and only opens a connection where SQLite will not
/// write: a read-only one that reads a log through its index without writing either
/// (`readonly_shm`), or, for a logged file without a log, an `immutable` one, whose read counts only
/// if the files are still as they were afterwards. A file that needs recovering is refused with
/// what was found, and so is one SQLite reports so when opening it.
///
/// One race remains: if the last connection to the database closes between the look and the
/// opening, it folds the log into the file and deletes it, and a `readonly_shm` connection then
/// creates an empty log. Nothing the database holds changes, but "without changing any of its
/// files" holds for everything except that empty `-wal`.
pub(crate) fn examine<T>(
    path: &Path,
    read: impl FnMut(&mut Connection) -> Result<T, ApiError>,
) -> (Found, Option<Result<T, ApiError>>) {
    examine_where(path, |found| *found == Found::Current, read)
}

/// `examine`, reading with `read` what `wanted` accepts of what it found instead of only an
/// inventory in this build's schema: the window reads an outdated inventory this way before
/// offering to upgrade it, on a connection that cannot change it.
pub(crate) fn examine_where<T>(
    path: &Path,
    wanted: impl Fn(&Found) -> bool,
    mut read: impl FnMut(&mut Connection) -> Result<T, ApiError>,
) -> (Found, Option<Result<T, ApiError>>) {
    let mut changing = String::new();
    for _ in 0..LOOKS {
        let disk = match OnDisk::look(path) {
            Ok(disk) => disk,
            Err(found) => return (found, None),
        };
        let way = match disk.way(path) {
            Ok(way) => way,
            // Taken at its word only once the files stay as they are: a connection closing in that
            // instant changes them, and then the gate looks again.
            Err(Some(found @ Found::NeedsRecovery(_))) => {
                if settled(path, &disk) {
                    return (found, None);
                }
                changing = "what needed recovering changed a moment later".to_owned();
                continue;
            }
            Err(Some(found)) => return (found, None),
            Err(None) => {
                changing = "the log's index went away".to_owned();
                continue;
            }
        };
        // What a snapshot read has to find again afterwards for the read to count: the file's
        // bytes, since a write within the same tick of the clock can leave its size and time.
        let before = match way {
            Way::Snapshot => match content(path) {
                Ok(content) => Some(content),
                Err(error) => return (Found::Unreadable(error.to_string()), None),
            },
            Way::Shared => None,
        };
        // A writer that changed the file under a snapshot can make anything read from it wrong,
        // an error included: then neither counts, and the gate looks again.
        let changed = || {
            way == Way::Snapshot
                && (OnDisk::look(path).as_ref() != Ok(&disk) || content(path).ok() != before)
        };
        let opened = connect(path, way).and_then(|db| classify(&db).map(|found| (db, found)));
        let (mut db, found) = match opened {
            Ok(opened) => opened,
            Err(error) => match found_by(&error) {
                Some(found) if !changed() => return (found, None),
                _ => {
                    changing = error.to_string();
                    continue;
                }
            },
        };
        let outcome = wanted(&found).then(|| read(&mut db));
        drop(db);
        if changed() {
            changing = "another connection wrote to it while DevHQ read it".to_owned();
            continue;
        }
        return (found, outcome);
    }
    (
        Found::Unreadable(format!(
            "its files kept changing while DevHQ looked at them: {changing}"
        )),
        None,
    )
}

/// Reads the inventory at `path` as it is (see `examine`): only an inventory in this build's
/// schema is read, and anything else is refused with what was found there.
pub(crate) fn read_as_found<T>(
    path: &Path,
    read: impl FnMut(&mut Connection) -> Result<T, ApiError>,
) -> Result<T, ApiError> {
    match examine(path, read) {
        (_, Some(outcome)) => outcome,
        (found, None) => Err(found.refusal(path)),
    }
}

/// A connection for the gate to change the inventory at `path` in its own transaction, opened
/// only once a look that changes nothing (see `examine`) found it in this build's schema and
/// needing no recovery: the gate never creates, upgrades or converts an inventory to another
/// journal mode. Its transaction (`begin_write`) classifies the file again, so that one replaced
/// meanwhile is not written; one left needing recovery in that moment is recovered by SQLite, as
/// any write does (see `open_current`).
pub(crate) fn open_to_write(path: &Path) -> Result<Connection, ApiError> {
    read_as_found(path, |_| Ok(()))?;
    open_current(path)
}

/// The bytes every SQLite database file starts with.
const SQLITE_HEADER: &[u8] = b"SQLite format 3\0";
/// The size of a write-ahead log's own header: a longer log holds changes.
const LOG_HEADER: u64 = 32;

/// How the gate reads a database without changing any of its files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    /// Read-only, beside whatever connections hold it, with SQLite's usual locks: a rollback-journal
    /// file, or a log that a connection holds open, read through its index without writing to
    /// either (`readonly_shm`).
    Shared,
    /// A write-ahead-logged file whose log is gone or empty, so that the file holds everything:
    /// read as a file that does not change (`immutable`), since any other connection would create
    /// the log and its index. Nothing locks that read against a writer that opens the file
    /// meanwhile, so the read counts only if afterwards the file holds the same bytes and no log,
    /// index or journal came or went: a writer writes the file only by folding a log back into it.
    Snapshot,
}

/// A database's files as they are on disk, looked at without SQLite: what decides how the gate may
/// open it without changing any of them, and, compared before and after a snapshot, whether
/// anything changed them meanwhile.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OnDisk {
    /// The database file's size and last write.
    len: u64,
    modified: Option<SystemTime>,
    /// Its first 100 bytes: SQLite's header, which says whether it uses a write-ahead log.
    header: Vec<u8>,
    /// The size of its write-ahead log (`-wal`), when there is one.
    log: Option<u64>,
    /// Whether the log's index (`-shm`) is there.
    index: bool,
    /// Whether a rollback journal (`-journal`) that SQLite could play back is there: one that is
    /// not empty and whose header is not zeroed.
    journal: bool,
}

impl OnDisk {
    fn look(path: &Path) -> Result<Self, Found> {
        let metadata = match std::fs::metadata(path) {
            Err(error) if error.kind() == ErrorKind::NotFound => return Err(Found::Missing),
            Err(error) => return Err(Found::Unreadable(error.to_string())),
            Ok(metadata) if !metadata.is_file() => {
                return Err(Found::Unreadable("it is not a file".to_owned()))
            }
            Ok(metadata) => metadata,
        };
        let unreadable = |error: std::io::Error| Found::Unreadable(error.to_string());
        let mut header = Vec::with_capacity(100);
        File::open(path)
            .and_then(|file| file.take(100).read_to_end(&mut header))
            .map_err(unreadable)?;
        let size = |suffix| match std::fs::metadata(beside(path, suffix)) {
            Ok(metadata) => Ok(Some(metadata.len())),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(unreadable(error)),
        };
        let mut first = [0];
        let journal = match File::open(beside(path, "-journal")) {
            Ok(mut journal) => match journal.read(&mut first) {
                Ok(read) => read == 1 && first[0] != 0,
                Err(error) => return Err(unreadable(error)),
            },
            Err(error) if error.kind() == ErrorKind::NotFound => false,
            Err(error) => return Err(unreadable(error)),
        };
        Ok(Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            header,
            log: size("-wal")?,
            index: size("-shm")?.is_some(),
            journal,
        })
    }

    /// How the gate may read the file at `path` without changing it; or what it found, when it
    /// may not read it; or None when the files changed while it looked, to look again.
    fn way(&self, path: &Path) -> Result<Way, Option<Found>> {
        if self.len == 0 {
            return Err(Some(Found::Foreign { empty: true }));
        }
        if !self.header.starts_with(SQLITE_HEADER) {
            return Err(Some(Found::Unreadable("file is not a database".to_owned())));
        }
        // Byte 19 of the header is 2 for a write-ahead-logged file: SQLite then opens its log.
        let logged = self.header.get(19) == Some(&2);
        let held = || match log_held(path) {
            Ok(held) => Ok(held),
            Err(error) if error.kind() == ErrorKind::NotFound => Err(None),
            Err(error) => Err(Some(Found::Unreadable(error.to_string()))),
        };
        let snapshot = || {
            if self.journal {
                Err(Some(Found::NeedsRecovery(Recovery::Journal)))
            } else {
                Ok(Way::Snapshot)
            }
        };
        match self.log {
            // A log with changes in it: SQLite opens one whatever the header says.
            Some(len) if len > LOG_HEADER => {
                if !self.index {
                    Err(Some(Found::NeedsRecovery(Recovery::LogWithoutIndex)))
                } else if held()? {
                    Ok(Way::Shared)
                } else {
                    Err(Some(Found::NeedsRecovery(Recovery::Log)))
                }
            }
            // A connection that has not written yet holds an empty log; otherwise it was left
            // empty, and the file holds everything.
            Some(_) if self.index && held()? => Ok(Way::Shared),
            Some(_) => snapshot(),
            None if logged => snapshot(),
            None => Ok(Way::Shared),
        }
    }
}

/// A digest of the bytes of the file at `path`.
pub(crate) fn content(path: &Path) -> std::io::Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    std::io::copy(&mut File::open(path)?, &mut hasher)?;
    Ok(hasher.finalize().into())
}

/// The file SQLite keeps beside the database at `path`: `-wal`, `-shm` or `-journal`.
pub(crate) fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Whether a connection in another process holds open the write-ahead log of the database at
/// `path` (on Windows, one in this process counts too). Every connection to a logged database
/// holds a shared lock on byte 128 of the log's index (`-shm`), SQLite's dead man's switch, for as
/// long as it is open, and the system drops it when the process ends, however it ends: SQLite asks
/// the same question, the same way, to decide whether a log needs recovering. This only asks: on
/// Windows the exclusive lock it tries is let go at once, and on Unix `F_GETLK` only tests.
///
/// On Unix closing any descriptor of a file drops every lock this process holds on it, so this
/// must never run while this process has a connection to the same database: the gate looks before
/// it opens one, and holds one at a time.
#[cfg(windows)]
fn log_held(path: &Path) -> std::io::Result<bool> {
    let index = File::open(beside(path, "-shm"))?;
    match index.try_lock() {
        Ok(()) => {
            index.unlock()?;
            Ok(false)
        }
        Err(TryLockError::WouldBlock) => Ok(true),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

#[cfg(unix)]
fn log_held(path: &Path) -> std::io::Result<bool> {
    use std::os::fd::AsRawFd;
    let index = File::open(beside(path, "-shm"))?;
    // SAFETY: flock is a plain C struct, for which all zeroes is a valid value.
    let mut lock: libc::flock = unsafe { std::mem::zeroed() };
    // libc declares the lock types and whences as c_int, and flock's fields as c_short (Linux,
    // macOS, the BSDs), which holds every one of them. Naming the type keeps the comparison below
    // inferable, and makes a platform whose fields differ fail to build instead of casting silently.
    lock.l_type = libc::F_WRLCK as libc::c_short;
    lock.l_whence = libc::SEEK_SET as libc::c_short;
    lock.l_start = 128;
    lock.l_len = 1;
    // SAFETY: the descriptor is open for the call, and F_GETLK only reads and writes `lock`.
    if unsafe { libc::fcntl(index.as_raw_fd(), libc::F_GETLK, &raw mut lock) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    // F_GETLK leaves F_UNLCK in l_type when no other process holds a lock that would conflict.
    Ok(lock.l_type != libc::F_UNLCK as libc::c_short)
}

/// A read-only connection to the file at `path`, opened the `way` its files allow.
fn connect(path: &Path, way: Way) -> rusqlite::Result<Connection> {
    let parameter = match way {
        Way::Shared => "readonly_shm=1",
        Way::Snapshot => "immutable=1",
    };
    let uri = uri(path, parameter).ok_or_else(|| rusqlite::Error::InvalidPath(path.to_owned()))?;
    let db = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    db.busy_timeout(Duration::from_secs(5))?;
    Ok(db)
}

/// The file at `path` as a SQLite URI carrying `parameter`: SQLite takes `immutable` and
/// `readonly_shm` only that way. Every byte but letters, digits and `/-._~:` is percent-encoded,
/// so that a `?`, `#` or `%` in a folder's name stays part of the path. None for a Windows path
/// that is not valid Unicode, which SQLite cannot be given.
fn uri(path: &Path, parameter: &str) -> Option<String> {
    #[cfg(windows)]
    let bytes = {
        path.to_str()?;
        plain(path).replace('\\', "/").into_bytes()
    };
    #[cfg(unix)]
    let bytes = std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str()).to_vec();
    let mut encoded = String::with_capacity(bytes.len());
    for byte in bytes {
        if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    // An absolute path gets the empty authority: file:///home/…, file:///C:/…, file:////server/….
    let drive =
        encoded.as_bytes().get(1..3) == Some(b":/") && encoded.as_bytes()[0].is_ascii_alphabetic();
    let authority = if encoded.starts_with('/') {
        "//"
    } else if cfg!(windows) && drive {
        "///"
    } else {
        ""
    };
    Some(format!("file:{authority}{encoded}?{parameter}"))
}

/// What a SQLite error opening or reading a file says it is; None when its files changed between
/// the look and the opening (an index or a log went away), to look again.
fn found_by(error: &rusqlite::Error) -> Option<Found> {
    use rusqlite::ffi;
    if let rusqlite::Error::SqliteFailure(failure, _) = error {
        match failure.extended_code {
            // A hot journal, which a read-only connection does not play back.
            ffi::SQLITE_READONLY_ROLLBACK => return Some(Found::NeedsRecovery(Recovery::Journal)),
            // A log whose index needs rebuilding, or that no connection holds any more.
            ffi::SQLITE_READONLY_RECOVERY | ffi::SQLITE_READONLY_CANTINIT => return None,
            _ if failure.code == rusqlite::ErrorCode::CannotOpen => return None,
            _ => {}
        }
    }
    Some(Found::Unreadable(error.to_string()))
}

/// Which file the connection reads. Only a DevHQ inventory has `applications` and `resources`
/// (since its first schema) and a `user_version` from 1 on, which its upgrades set; one that
/// declares this build's version is current only if it also has this schema's structure.
pub(crate) fn classify(db: &Connection) -> rusqlite::Result<Found> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let (tables, devhq): (i64, i64) = db.query_row(
        "SELECT count(*), count(*) FILTER (WHERE name IN ('applications', 'resources'))
         FROM sqlite_master WHERE type = 'table'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(if version < 1 || devhq < 2 {
        Found::Foreign { empty: tables == 0 }
    } else if version < SCHEMA_VERSION {
        Found::Outdated(version)
    } else if version > SCHEMA_VERSION {
        Found::Newer(version)
    } else {
        let differences = Structure::of(db)?.differences_from(Structure::expected());
        if differences.is_empty() {
            Found::Current
        } else {
            Found::Mismatch(differences)
        }
    })
}

/// The tables of a schema, each with its columns, and its indexes, each with its table: what
/// tells whether a file has the schema it declares. Names and declarations only, never contents:
/// whether each page and row is sound is SQLite's integrity check, which the gate never runs.
#[derive(Debug, Default, PartialEq, Eq)]
struct Structure {
    /// Each column as `PRAGMA table_xinfo` gives it: its name, then its declaration.
    tables: BTreeMap<String, Vec<(String, String)>>,
    indexes: BTreeMap<String, String>,
}

impl Structure {
    fn of(db: &Connection) -> rusqlite::Result<Self> {
        let mut structure = Self::default();
        let mut objects = db.prepare(
            "SELECT type, name, tbl_name FROM sqlite_schema
             WHERE type IN ('table', 'index') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'",
        )?;
        let mut columns = db.prepare(
            "SELECT name, type, \"notnull\", dflt_value, pk, hidden FROM pragma_table_xinfo(?1)",
        )?;
        let objects = objects
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<(String, String, String)>>>()?;
        for (kind, name, table) in objects {
            if kind == "index" {
                structure.indexes.insert(name, table);
                continue;
            }
            let declared = columns
                .query_map([&name], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        format!(
                            "{} notnull={} default={:?} pk={} hidden={}",
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, i64>(4)?,
                            row.get::<_, i64>(5)?,
                        ),
                    ))
                })?
                .collect::<rusqlite::Result<_>>()?;
            structure.tables.insert(name, declared);
        }
        Ok(structure)
    }

    /// This build's schema as its migrations make it, built once, in memory.
    fn expected() -> &'static Self {
        static EXPECTED: OnceLock<Structure> = OnceLock::new();
        EXPECTED.get_or_init(|| {
            let db = Connection::open_in_memory().expect("an in-memory database opens");
            for (_, sql) in MIGRATIONS {
                db.execute_batch(sql)
                    .expect("this build's migrations build its schema");
            }
            Self::of(&db).expect("this build's schema can be read back")
        })
    }

    /// What this structure lacks, or has otherwise, of `expected`'s. Tables and indexes that
    /// `expected` does not have are not DevHQ's to judge, and are left out.
    fn differences_from(&self, expected: &Self) -> Vec<String> {
        let mut differences = Vec::new();
        for (table, columns) in &expected.tables {
            let Some(found) = self.tables.get(table) else {
                differences.push(format!("no table {table}"));
                continue;
            };
            for (column, declaration) in columns {
                match found.iter().find(|(name, _)| name == column) {
                    None => differences.push(format!("no column {table}.{column}")),
                    Some((_, other)) if other != declaration => differences.push(format!(
                        "column {table}.{column} declared as {other}, not {declaration}"
                    )),
                    Some(_) => {}
                }
            }
            for (column, _) in found {
                if !columns.iter().any(|(name, _)| name == column) {
                    differences.push(format!(
                        "column {table}.{column}, which the schema does not have"
                    ));
                }
            }
        }
        for (index, table) in &expected.indexes {
            match self.indexes.get(index) {
                None => differences.push(format!("no index {index}")),
                Some(other) if other != table => {
                    differences.push(format!("index {index} on {other}, not {table}"))
                }
                Some(_) => {}
            }
        }
        differences
    }
}

/// What the structure of the database on `db` lacks, or has otherwise or besides, of the one this
/// build's migrations give an inventory at schema `version`: each table with its columns, each
/// index, and nothing of its own (no other table, index, view or trigger). Stricter than
/// `classify`, which leaves out what DevHQ's schema does not name: the explicit upgrade changes only
/// a file that is exactly what DevHQ made of it, so that nothing DevHQ does not know rides along,
/// such as a trigger that would fire while the migrations copy rows.
pub(crate) fn schema_differences(db: &Connection, version: i64) -> rusqlite::Result<Vec<String>> {
    let expected = Connection::open_in_memory()?;
    for (number, sql) in MIGRATIONS {
        if number <= version {
            expected.execute_batch(sql)?;
        }
    }
    let mut differences = Structure::of(db)?.differences_from(&Structure::of(&expected)?);
    let known = objects(&expected)?;
    for (kind, name) in objects(db)?.difference(&known) {
        differences.push(format!(
            "{kind} {name}, which schema version {version} does not have"
        ));
    }
    Ok(differences)
}

/// Each table, index, view and trigger of the database on `db`, by kind and name, but SQLite's own.
fn objects(db: &Connection) -> rusqlite::Result<BTreeSet<(String, String)>> {
    let mut statement = db.prepare(
        "SELECT type, name FROM sqlite_schema WHERE name NOT LIKE 'sqlite\\_%' ESCAPE '\\'",
    )?;
    let objects = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect();
    objects
}

/// Longest texts DevHQ accepts. The window enforces the same limits as you type.
pub(crate) const MAX_NAME: usize = 120;
pub(crate) const MAX_TITLE: usize = 160;
pub(crate) const MAX_SUMMARY: usize = 500;
pub(crate) const MAX_DESCRIPTION: usize = 1000;
const MAX_VERSION: usize = 60;

/// What a person typed, without surrounding whitespace, and no longer than `max` characters,
/// counted as the window counts them (UTF-16 code units), so both agree on every text.
pub(crate) fn bounded_text(value: &str, max: usize, label: &str) -> Result<String, ApiError> {
    let text = value.trim();
    if text.encode_utf16().count() > max {
        return Err(ApiError::invalid(format!(
            "{label} is longer than {max} characters."
        )));
    }
    Ok(text.to_owned())
}

pub(crate) fn required_text(value: &str, max: usize, label: &str) -> Result<String, ApiError> {
    let text = bounded_text(value, max, label)?;
    if text.is_empty() {
        Err(ApiError::invalid(format!("{label} is required.")))
    } else {
        Ok(text)
    }
}

/// Like `bounded_text`, but the text already stored is always accepted unchanged: limits apply
/// to what is typed, so something saved before a limit existed can still be saved.
fn bounded_or_kept(value: &str, kept: &str, max: usize, label: &str) -> Result<String, ApiError> {
    let text = value.trim();
    if !text.is_empty() && text == kept.trim() {
        return Ok(text.to_owned());
    }
    bounded_text(value, max, label)
}

fn required_or_kept(value: &str, kept: &str, max: usize, label: &str) -> Result<String, ApiError> {
    let text = bounded_or_kept(value, kept, max, label)?;
    if text.is_empty() {
        Err(ApiError::invalid(format!("{label} is required.")))
    } else {
        Ok(text)
    }
}

pub(crate) fn allowed(value: &str, choices: &[&str], label: &str) -> Result<(), ApiError> {
    if choices.contains(&value) {
        Ok(())
    } else {
        Err(ApiError::invalid(format!("Invalid {label}: {value}.")))
    }
}

pub fn list(path: &Path) -> Result<Inventory, ApiError> {
    list_from(&mut open_current(path)?)
}

/// One consistent read of the inventory on `db`, in one read transaction: `list` on a connection
/// the caller opened, such as the agent gate's, which opens the inventory as it finds it.
pub(crate) fn list_from(db: &mut Connection) -> Result<Inventory, ApiError> {
    let tx = begin(db, TransactionBehavior::Deferred)?;
    let inventory = list_in(&tx)?;
    tx.commit().map_err(ApiError::database)?;
    Ok(inventory)
}

/// `list_from` inside a read transaction the caller holds, so that whatever else it reads there
/// (the agent gate's `latestChange`) is of the same state of the inventory.
pub(crate) fn list_in(tx: &Transaction) -> Result<Inventory, ApiError> {
    let mut resources = {
        let mut statement = tx.prepare("SELECT id, name, kind, description, operating_system FROM resources ORDER BY name COLLATE NOCASE, id").map_err(ApiError::database)?;
        let result = statement
            .query_map([], |row| {
                Ok(Resource {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    kind: row.get(2)?,
                    description: row.get(3)?,
                    operating_system: row.get(4)?,
                    references: Vec::new(),
                })
            })
            .map_err(ApiError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(ApiError::database)?;
        result
    };
    for resource in &mut resources {
        resource.references = references::of(
            tx,
            &Subject::Resource {
                id: resource.id.clone(),
            },
        )?;
    }
    let mut applications = {
        let mut statement = tx.prepare("SELECT id, name, description, lifecycle, version FROM applications ORDER BY name COLLATE NOCASE, id").map_err(ApiError::database)?;
        let result = statement
            .query_map([], |row| {
                Ok(Application {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    lifecycle: row.get(3)?,
                    version: row.get(4)?,
                    components: Vec::new(),
                    references: Vec::new(),
                })
            })
            .map_err(ApiError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(ApiError::database)?;
        result
    };
    {
        let mut statement = tx.prepare("SELECT id, name, kind, resource_id FROM components WHERE application_id = ?1 ORDER BY rowid").map_err(ApiError::database)?;
        for app in &mut applications {
            app.components = statement
                .query_map([&app.id], |row| {
                    Ok(Component {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        kind: row.get(2)?,
                        resource_id: row.get(3)?,
                    })
                })
                .map_err(ApiError::database)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(ApiError::database)?;
            app.references = references::of(tx, &Subject::Application { id: app.id.clone() })?;
        }
    }
    let shared_repositories = references::shared_repositories(&applications, &resources);
    let local_copies = readings::local_copies(tx, &applications, &resources)?;
    let subjects: Vec<_> = applications
        .iter()
        .map(|app| records::Subject::Application { id: app.id.clone() })
        .chain(resources.iter().map(|resource| records::Subject::Resource {
            id: resource.id.clone(),
        }))
        .collect();
    let records::Records {
        observations,
        incidents,
        events,
        automated_history,
    } = records::load(tx, &subjects)?;
    let http_checks = http_check::load(tx)?;
    let changes = change_log::load(tx)?;
    let changes_since = change_log::complete_since(tx)?;
    Ok(Inventory {
        applications,
        resources,
        shared_repositories,
        local_copies,
        observations,
        incidents,
        events,
        http_checks,
        changes,
        changes_since,
        automated_history,
    })
}

pub fn save_resource(path: &Path, input: ResourceInput) -> Result<Resource, ApiError> {
    write(path, &Actor::Person, |tx| {
        save_resource_in(tx, input, &Actor::Person).map(|(resource, _)| resource)
    })
}

/// Saves a resource and its complete reference list inside the caller's write transaction, which
/// the caller commits, and logs the change there as `by`'s: None when the save changed nothing.
pub(crate) fn save_resource_in(
    tx: &Transaction,
    input: ResourceInput,
    by: &Actor,
) -> Result<(Resource, Option<change_log::Change>), ApiError> {
    allowed(&input.kind, &["server", "workstation"], "resource kind")?;
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    if id.is_empty() {
        return Err(ApiError::invalid("Resource ID is required."));
    }
    let before = resource_in(tx, &id)?;
    let kept = |field: fn(&Resource) -> &str| before.as_ref().map_or("", field);
    let name = required_or_kept(
        &input.name,
        kept(|item| &item.name),
        MAX_NAME,
        "Resource name",
    )?;
    let description = bounded_or_kept(
        &input.description,
        kept(|item| &item.description),
        MAX_DESCRIPTION,
        "Resource description",
    )?;
    let operating_system = bounded_or_kept(
        &input.operating_system,
        kept(|item| &item.operating_system),
        MAX_NAME,
        "Operating system",
    )?;
    let subject = Subject::Resource { id: id.clone() };
    // Its own paths can be on itself, as it is about to be.
    let references = references::validated(
        tx,
        &subject,
        input.references,
        Some(&references::Machine {
            id: &id,
            name: &name,
            kind: &input.kind,
        }),
    )?;
    if input.kind != "workstation" {
        // Other systems' local copies on it would be left on something that is no workstation.
        let copies = references::paths_on(tx, &id, Some("local_copy"))?;
        if !copies.is_empty() {
            return Err(ApiError::invalid(format!(
                "{name} holds local copies of other systems: {}. A local copy is on a workstation, so move them to another workstation, or remove them, before making {name} a {}.",
                listed(&copies),
                input.kind
            )));
        }
    }
    tx.execute(
        "INSERT INTO resources (id, name, kind, description, operating_system) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, kind=excluded.kind, description=excluded.description, operating_system=excluded.operating_system",
        params![id, name, input.kind, description, operating_system],
    )
    .map_err(ApiError::database)?;
    // The submitted reference list is the complete set, written once the resource it can be the
    // machine of exists.
    references::replace(tx, &subject, &references)?;
    let resource = Resource {
        id,
        name,
        kind: input.kind,
        description,
        operating_system,
        references,
    };
    let change = change_log::resource_saved(tx, by, before.as_ref(), &resource)?;
    Ok((resource, change))
}

pub fn save_application(path: &Path, input: ApplicationInput) -> Result<Application, ApiError> {
    write(path, &Actor::Person, |tx| {
        save_application_in(tx, input, &Actor::Person).map(|(application, _)| application)
    })
}

/// Saves an application and its complete component list inside the caller's write
/// transaction, which the caller commits, and logs the change there as `by`'s: None when the
/// save changed nothing.
pub(crate) fn save_application_in(
    tx: &Transaction,
    input: ApplicationInput,
    by: &Actor,
) -> Result<(Application, Option<change_log::Change>), ApiError> {
    allowed(
        &input.lifecycle,
        &["development", "testing", "production", "archived"],
        "lifecycle",
    )?;
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    if id.is_empty() {
        return Err(ApiError::invalid("Application ID is required."));
    }
    let mut parts: Vec<ComponentInput> = Vec::with_capacity(input.components.len());
    for mut component in input.components {
        allowed(
            &component.kind,
            &["backend", "database", "docker", "frontend"],
            "component kind",
        )?;
        let component_id = component
            .id
            .take()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        if component_id.is_empty() {
            return Err(ApiError::invalid("Component ID is required."));
        }
        if parts
            .iter()
            .any(|item| item.id.as_deref() == Some(component_id.as_str()))
        {
            return Err(ApiError::invalid(format!(
                "Component {component_id} appears twice. Each component can be listed once."
            )));
        }
        component.id = Some(component_id);
        parts.push(component);
    }
    let before = application_in(tx, &id)?;
    let name = required_or_kept(
        &input.name,
        before.as_ref().map_or("", |item| &item.name),
        MAX_NAME,
        "Application name",
    )?;
    let description = bounded_or_kept(
        &input.description,
        before.as_ref().map_or("", |item| &item.description),
        MAX_DESCRIPTION,
        "Application description",
    )?;
    // A blank version is no version.
    let version = match &input.version {
        Some(version) => Some(bounded_or_kept(
            version,
            before
                .as_ref()
                .and_then(|item| item.version.as_deref())
                .unwrap_or_default(),
            MAX_VERSION,
            "Version",
        )?),
        None => None,
    }
    .filter(|version| !version.is_empty());
    let mut components = Vec::with_capacity(parts.len());
    for part in parts {
        let component_id = part.id.unwrap_or_default();
        // A component belongs to one application; reusing its ID elsewhere would move it.
        let stored: Option<(String, String)> = tx
            .query_row(
                "SELECT application_id, name FROM components WHERE id = ?1",
                [&component_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(ApiError::database)?;
        let kept = match stored {
            Some((owner, _)) if owner != id => {
                return Err(ApiError::invalid(format!(
                    "Component {component_id} belongs to another application."
                )))
            }
            Some((_, kept)) => kept,
            None => String::new(),
        };
        let name = required_or_kept(&part.name, &kept, MAX_NAME, "Component name")?;
        if let Some(resource_id) = &part.resource_id {
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM resources WHERE id = ?1)",
                    [resource_id],
                    |row| row.get(0),
                )
                .map_err(ApiError::database)?;
            if !exists {
                return Err(ApiError::invalid(format!("Resource {resource_id} does not exist. Choose an existing resource or leave the component unlinked.")));
            }
        }
        components.push(Component {
            id: component_id,
            name,
            kind: part.kind,
            resource_id: part.resource_id,
        });
    }
    let subject = Subject::Application { id: id.clone() };
    let references = references::validated(tx, &subject, input.references, None)?;
    tx.execute(
        "INSERT INTO applications (id, name, lifecycle, description, version) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, lifecycle=excluded.lifecycle, description=excluded.description, version=excluded.version",
        params![id, name, input.lifecycle, description, version],
    ).map_err(ApiError::database)?;
    // The submitted component list is the complete set. Omitted components are removed atomically.
    tx.execute("DELETE FROM components WHERE application_id = ?1", [&id])
        .map_err(ApiError::database)?;
    for component in &components {
        tx.execute("INSERT INTO components (id, application_id, name, kind, resource_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![component.id, id, component.name, component.kind, component.resource_id]).map_err(ApiError::database)?;
    }
    // So is the reference list.
    references::replace(tx, &subject, &references)?;
    let application = Application {
        id,
        name,
        description,
        lifecycle: input.lifecycle,
        version,
        components,
        references,
    };
    let change = change_log::application_saved(tx, by, before.as_ref(), &application)?;
    Ok((application, change))
}

/// The application as stored, with its components in their order, read inside the caller's
/// transaction; None if there is none with that ID.
pub(crate) fn application_in(tx: &Transaction, id: &str) -> Result<Option<Application>, ApiError> {
    let Some(mut application) = tx
        .query_row(
            "SELECT id, name, description, lifecycle, version FROM applications WHERE id = ?1",
            [id],
            |row| {
                Ok(Application {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    lifecycle: row.get(3)?,
                    version: row.get(4)?,
                    components: Vec::new(),
                    references: Vec::new(),
                })
            },
        )
        .optional()
        .map_err(ApiError::database)?
    else {
        return Ok(None);
    };
    let mut statement = tx
        .prepare("SELECT id, name, kind, resource_id FROM components WHERE application_id = ?1 ORDER BY rowid")
        .map_err(ApiError::database)?;
    application.components = statement
        .query_map([id], |row| {
            Ok(Component {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                resource_id: row.get(3)?,
            })
        })
        .map_err(ApiError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApiError::database)?;
    application.references = references::of(tx, &Subject::Application { id: id.to_owned() })?;
    Ok(Some(application))
}

/// The resource as stored, with its references, read inside the caller's transaction; None if
/// there is none.
pub(crate) fn resource_in(tx: &Transaction, id: &str) -> Result<Option<Resource>, ApiError> {
    let Some(mut resource) = tx
        .query_row(
            "SELECT id, name, kind, description, operating_system FROM resources WHERE id = ?1",
            [id],
            |row| {
                Ok(Resource {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    kind: row.get(2)?,
                    description: row.get(3)?,
                    operating_system: row.get(4)?,
                    references: Vec::new(),
                })
            },
        )
        .optional()
        .map_err(ApiError::database)?
    else {
        return Ok(None);
    };
    resource.references = references::of(tx, &Subject::Resource { id: id.to_owned() })?;
    Ok(Some(resource))
}

/// Which kinds of record reference a subject. History is never deleted, so any keeps it.
fn history_of(tx: &Transaction, column: &str, id: &str) -> Result<Vec<String>, ApiError> {
    let mut found = Vec::new();
    for (table, kind) in [
        ("observations", "observations"),
        ("incidents", "incidents"),
        ("events", "events"),
        ("copy_readings", "readings of its local copies"),
    ] {
        // EXISTS stops at the first one: a long supervised history costs no more than one.
        let exists: bool = tx
            .query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {column} = ?1)"),
                [id],
                |row| row.get(0),
            )
            .map_err(ApiError::database)?;
        if exists {
            found.push(kind.to_owned());
        }
    }
    Ok(found)
}

fn listed(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Deletes an application that has no records, with its components, references and HTTP check.
/// One with observations, incidents or events is kept: DevHQ never erases history. Archive it
/// instead.
pub fn delete_application(path: &Path, id: &str) -> Result<(), ApiError> {
    write(path, &Actor::Person, |tx| {
        delete_application_in(tx, id, &Actor::Person).map(|_| ())
    })
}

/// Deletes the application inside the caller's write transaction, and logs it there as `by`'s
/// with every field it had; returns the name it had and that entry.
pub(crate) fn delete_application_in(
    tx: &Transaction,
    id: &str,
    by: &Actor,
) -> Result<(String, change_log::Change), ApiError> {
    let before = application_in(tx, id)?
        .ok_or_else(|| ApiError::invalid(format!("The application {id} does not exist.")))?;
    let name = before.name.clone();
    let history = history_of(tx, "application_id", id)?;
    if !history.is_empty() {
        return Err(ApiError::invalid(format!(
            "{name} has recorded {}. DevHQ keeps history, so it cannot be deleted; set its lifecycle to Archived instead.",
            listed(&history)
        )));
    }
    // Its components, its references and its HTTP check go with it (ON DELETE CASCADE).
    tx.execute("DELETE FROM applications WHERE id = ?1", [id])
        .map_err(ApiError::database)?;
    let change = change_log::application_deleted(tx, by, &before)?;
    Ok((name, change))
}

/// Deletes a resource that no component runs on, that holds no other system's paths and that has
/// no records, with its references and its HTTP check.
pub fn delete_resource(path: &Path, id: &str) -> Result<(), ApiError> {
    write(path, &Actor::Person, |tx| {
        delete_resource_in(tx, id, &Actor::Person).map(|_| ())
    })
}

/// Deletes the resource inside the caller's write transaction, and logs it there as `by`'s with
/// every field it had; returns the name it had and that entry.
pub(crate) fn delete_resource_in(
    tx: &Transaction,
    id: &str,
    by: &Actor,
) -> Result<(String, change_log::Change), ApiError> {
    let before = resource_in(tx, id)?
        .ok_or_else(|| ApiError::invalid(format!("The resource {id} does not exist.")))?;
    let name = before.name.clone();
    let users = {
        let mut statement = tx
            .prepare(
                "SELECT a.name || ' (' || c.name || ')' FROM components c
                 JOIN applications a ON a.id = c.application_id
                 WHERE c.resource_id = ?1 ORDER BY a.name COLLATE NOCASE, c.rowid",
            )
            .map_err(ApiError::database)?;
        let rows = statement
            .query_map([id], |row| row.get::<_, String>(0))
            .map_err(ApiError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(ApiError::database)?;
        rows
    };
    if !users.is_empty() {
        return Err(ApiError::invalid(format!(
            "{name} is used by {}. Link those components to another resource, or leave them unlinked, first.",
            listed(&users)
        )));
    }
    // Removing other systems' paths would change their configuration without their own entry in
    // the change log; the schema refuses it too (RESTRICT).
    let paths = references::paths_on(tx, id, None)?;
    if !paths.is_empty() {
        return Err(ApiError::invalid(format!(
            "{name} holds paths of other systems: {}. Remove those references, or put them on another machine, first.",
            listed(&paths)
        )));
    }
    let history = history_of(tx, "resource_id", id)?;
    if !history.is_empty() {
        return Err(ApiError::invalid(format!(
            "{name} has recorded {}. DevHQ keeps history, so it cannot be deleted.",
            listed(&history)
        )));
    }
    // Its own references go first: some can be on itself, and SQLite applies the machine's
    // RESTRICT at once, before the subject's CASCADE would remove them.
    references::remove_all(tx, &Subject::Resource { id: id.to_owned() })?;
    // Its HTTP check goes with it (ON DELETE CASCADE).
    tx.execute("DELETE FROM resources WHERE id = ?1", [id])
        .map_err(ApiError::database)?;
    let change = change_log::resource_deleted(tx, by, &before)?;
    Ok((name, change))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Where no file is yet.
    fn nothing() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("devhq-test-{}.sqlite3", Uuid::new_v4()))
    }

    /// A new inventory, as the window creates one when it starts.
    fn path() -> std::path::PathBuf {
        let path = nothing();
        start(&path).unwrap();
        path
    }

    fn app(resource_id: Option<String>) -> ApplicationInput {
        ApplicationInput {
            id: None,
            name: "Example".into(),
            description: "Context".into(),
            lifecycle: "production".into(),
            version: Some("1.0".into()),
            components: vec![ComponentInput {
                id: None,
                name: "API".into(),
                kind: "backend".into(),
                resource_id,
            }],
            references: vec![],
        }
    }

    #[test]
    fn persists_relationships_across_reopen_and_edits() {
        let path = path();
        assert!(list(&path).unwrap().applications.is_empty());
        let resource = save_resource(
            &path,
            ResourceInput {
                id: None,
                name: "Host".into(),
                kind: "server".into(),
                description: String::new(),
                operating_system: "Linux".into(),
                references: vec![],
            },
        )
        .unwrap();
        let saved = save_application(&path, app(Some(resource.id.clone()))).unwrap();
        let reopened = list(&path).unwrap();
        assert_eq!(reopened.resources[0], resource);
        assert_eq!(reopened.applications[0], saved);
        let edited = save_application(
            &path,
            ApplicationInput {
                id: Some(saved.id.clone()),
                name: "Renamed".into(),
                description: String::new(),
                lifecycle: "development".into(),
                version: None,
                components: vec![ComponentInput {
                    id: Some(saved.components[0].id.clone()),
                    name: "API v2".into(),
                    kind: "backend".into(),
                    resource_id: Some(resource.id),
                }],
                references: vec![],
            },
        )
        .unwrap();
        assert_eq!(list(&path).unwrap().applications, vec![edited]);
        save_application(
            &path,
            ApplicationInput {
                id: Some(saved.id),
                name: "Renamed".into(),
                description: String::new(),
                lifecycle: "development".into(),
                version: None,
                components: vec![],
                references: vec![],
            },
        )
        .unwrap();
        let after_removal = list(&path).unwrap();
        assert!(after_removal.applications[0].components.is_empty());
        assert_eq!(after_removal.resources.len(), 1);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_invalid_resource_without_partial_write() {
        let path = path();
        let error = save_application(&path, app(Some("missing".into()))).unwrap_err();
        assert_eq!(error.code, "invalid_input");
        assert!(error.message.contains("missing"));
        assert!(list(&path).unwrap().applications.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn invalid_edit_preserves_existing_relationship() {
        let path = path();
        let resource = save_resource(
            &path,
            ResourceInput {
                id: None,
                name: "Host".into(),
                kind: "server".into(),
                description: String::new(),
                operating_system: String::new(),
                references: vec![],
            },
        )
        .unwrap();
        let original = save_application(&path, app(Some(resource.id))).unwrap();
        let error = save_application(
            &path,
            ApplicationInput {
                id: Some(original.id.clone()),
                name: "Changed".into(),
                description: String::new(),
                lifecycle: "testing".into(),
                version: None,
                components: vec![ComponentInput {
                    id: Some(original.components[0].id.clone()),
                    name: "API".into(),
                    kind: "backend".into(),
                    resource_id: Some("gone".into()),
                }],
                references: vec![],
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "invalid_input");
        assert_eq!(list(&path).unwrap().applications, vec![original]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn stores_text_as_typed_without_surrounding_whitespace() {
        let path = path();
        let resource = save_resource(
            &path,
            ResourceInput {
                id: None,
                name: "  Host  ".into(),
                kind: "server".into(),
                description: "  Rack 2 \n".into(),
                operating_system: " Debian 13 ".into(),
                references: vec![],
            },
        )
        .unwrap();
        let saved = save_application(
            &path,
            ApplicationInput {
                description: "  Store  ".into(),
                // A blank version is no version.
                version: Some("   ".into()),
                ..app(Some(resource.id.clone()))
            },
        )
        .unwrap();
        assert_eq!(
            (
                resource.name.as_str(),
                resource.description.as_str(),
                resource.operating_system.as_str()
            ),
            ("Host", "Rack 2", "Debian 13")
        );
        assert_eq!(
            (saved.description.as_str(), saved.version.as_deref()),
            ("Store", None)
        );
        let reopened = list(&path).unwrap();
        assert_eq!(reopened.resources, vec![resource]);
        assert_eq!(reopened.applications, vec![saved]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_oversized_or_conflicting_input_without_partial_writes() {
        let path = path();
        let original = save_application(&path, app(None)).unwrap();
        let other = save_application(&path, app(None)).unwrap();
        let long = |count: usize| "é".repeat(count);
        let component = |id: Option<&str>| ComponentInput {
            id: id.map(Into::into),
            name: "API".into(),
            kind: "backend".into(),
            resource_id: None,
        };
        let edit = |change: fn(&mut ApplicationInput)| {
            let mut input = ApplicationInput {
                id: Some(original.id.clone()),
                ..app(None)
            };
            input.components = vec![component(Some(&original.components[0].id))];
            change(&mut input);
            input
        };
        let attempts = [
            (
                edit(|input| input.name = "é".repeat(121)),
                "longer than 120",
            ),
            (
                edit(|input| input.description = "é".repeat(1001)),
                "longer than 1000",
            ),
            (
                edit(|input| input.version = Some("1".repeat(61))),
                "longer than 60",
            ),
            (
                edit(|input| input.components[0].name = "é".repeat(121)),
                "Component name is longer",
            ),
            (
                edit(|input| input.components.push(input.components[0].clone())),
                "appears twice",
            ),
        ];
        for (attempt, message) in attempts {
            let error = save_application(&path, attempt).unwrap_err();
            assert_eq!(error.code, "invalid_input");
            assert!(error.message.contains(message), "{}", error.message);
        }
        // Reusing another application's component would move it; it is refused instead.
        let mut stolen = edit(|_| {});
        stolen.components = vec![component(Some(&other.components[0].id))];
        let error = save_application(&path, stolen).unwrap_err();
        assert_eq!(error.code, "invalid_input");
        assert!(error.message.contains("belongs to another application"));
        // Exactly at the limits is fine, counted in characters rather than bytes.
        assert!(save_resource(
            &path,
            ResourceInput {
                id: None,
                name: long(120),
                kind: "server".into(),
                description: long(1000),
                operating_system: long(120),
                references: vec![],
            },
        )
        .is_ok());
        let error = save_resource(
            &path,
            ResourceInput {
                id: None,
                name: "Host".into(),
                kind: "server".into(),
                description: String::new(),
                operating_system: long(121),
                references: vec![],
            },
        )
        .unwrap_err();
        assert!(error
            .message
            .contains("Operating system is longer than 120"));
        let snapshot = list(&path).unwrap();
        assert_eq!(snapshot.applications.len(), 2);
        assert!(snapshot.applications.contains(&original));
        assert!(snapshot.applications.contains(&other));
        assert_eq!(snapshot.resources.len(), 1);
        std::fs::remove_file(path).unwrap();
    }

    fn resource(path: &Path, name: &str) -> Resource {
        save_resource(
            path,
            ResourceInput {
                id: None,
                name: name.into(),
                kind: "server".into(),
                description: String::new(),
                operating_system: String::new(),
                references: vec![],
            },
        )
        .unwrap()
    }

    fn http_check(subject: records::Subject) -> http_check::HttpCheckInput {
        http_check::HttpCheckInput {
            subject,
            url: "https://example.com/health".into(),
            expected_status: 200,
            timeout_seconds: 5,
            valid_for_minutes: 60,
            monitor_interval_minutes: Some(5),
        }
    }

    /// The log as (source, action, summary), oldest first, and each entry's field changes.
    fn logged(path: &Path) -> Vec<(String, String, String)> {
        let mut changes = list(path).unwrap().changes;
        changes.reverse();
        changes
            .into_iter()
            .map(|change| (change.source, change.action, change.summary))
            .collect()
    }

    fn fields(change: &change_log::Change) -> Vec<(String, serde_json::Value, serde_json::Value)> {
        change
            .changes
            .as_ref()
            .unwrap()
            .iter()
            .map(|item| (item.field.clone(), item.before.clone(), item.after.clone()))
            .collect()
    }

    #[test]
    fn logs_each_change_the_person_makes_with_what_changed_and_nothing_else() {
        use serde_json::{json, Value};
        let path = path();
        let manual = |action: &str, summary: &str| {
            ("manual".to_owned(), action.to_owned(), summary.to_owned())
        };
        let host = resource(&path, "Host");
        let mut shop = app(Some(host.id.clone()));
        shop.name = "Shop".into();
        let shop = save_application(&path, shop).unwrap();
        assert_eq!(
            logged(&path),
            [
                manual("created", "Added resource «Host» (server)"),
                manual("created", "Added application «Shop» (production, version 1.0) with API (backend, on Host)"),
            ]
        );
        let created = list(&path).unwrap().changes.remove(0);
        assert_eq!(created.agent, None);
        assert_eq!(
            fields(&created),
            [
                ("name".into(), Value::Null, json!("Shop")),
                ("lifecycle".into(), Value::Null, json!("production")),
                ("version".into(), Value::Null, json!("1.0")),
                ("description".into(), Value::Null, json!("Context")),
                ("components".into(), Value::Null, json!(shop.components)),
            ]
        );

        // An edit keeps each changed field, before and after; saving unchanged logs nothing.
        let edit = |version: Option<&str>| ApplicationInput {
            id: Some(shop.id.clone()),
            name: shop.name.clone(),
            description: shop.description.clone(),
            lifecycle: shop.lifecycle.clone(),
            version: version.map(str::to_owned),
            components: shop
                .components
                .iter()
                .map(|component| ComponentInput {
                    id: Some(component.id.clone()),
                    name: component.name.clone(),
                    kind: component.kind.clone(),
                    resource_id: component.resource_id.clone(),
                })
                .collect(),
            references: vec![],
        };
        save_application(&path, edit(Some("1.1"))).unwrap();
        save_application(&path, edit(Some("1.1"))).unwrap();
        let mut renamed = list(&path).unwrap().resources.remove(0);
        renamed.name = "Web host".into();
        save_resource(
            &path,
            ResourceInput {
                id: Some(renamed.id.clone()),
                name: renamed.name.clone(),
                kind: renamed.kind.clone(),
                description: renamed.description.clone(),
                operating_system: renamed.operating_system.clone(),
                references: vec![],
            },
        )
        .unwrap();
        let changes = list(&path).unwrap().changes;
        assert_eq!(changes.len(), 4, "an unchanged save is no change");
        assert_eq!(
            fields(&changes[1]),
            [("version".into(), json!("1.0"), json!("1.1"))]
        );
        assert_eq!(
            fields(&changes[0]),
            [("name".into(), json!("Host"), json!("Web host"))]
        );

        // Refused changes log nothing: the entry is written in the change's own transaction.
        let mut ghost = edit(None);
        ghost.components[0].resource_id = Some("nowhere".into());
        assert!(save_application(&path, ghost).is_err());
        let subject = records::Subject::Application {
            id: shop.id.clone(),
        };
        let mut invalid = http_check(subject.clone());
        invalid.expected_status = 302;
        assert!(http_check::save_http_check(&path, invalid).is_err());
        assert!(http_check::remove_http_check(&path, subject.clone()).is_err());
        assert_eq!(list(&path).unwrap().changes.len(), 4);

        // Its HTTP check: set up, saved unchanged, paused twice, resumed and removed.
        http_check::save_http_check(&path, http_check(subject.clone())).unwrap();
        http_check::save_http_check(&path, http_check(subject.clone())).unwrap();
        http_check::set_http_check_paused(&path, &subject, true).unwrap();
        http_check::set_http_check_paused(&path, &subject, true).unwrap();
        http_check::set_http_check_paused(&path, &subject, false).unwrap();
        http_check::remove_http_check(&path, subject).unwrap();
        let every = list(&path).unwrap().changes;
        let removed = &every[0];
        assert_eq!(
            fields(removed),
            [
                (
                    "url".into(),
                    json!("https://example.com/health"),
                    Value::Null
                ),
                ("expectedStatus".into(), json!(200), Value::Null),
                ("timeoutSeconds".into(), json!(5), Value::Null),
                ("validForMinutes".into(), json!(60), Value::Null),
                ("monitor".into(), json!("active"), Value::Null),
                ("monitorIntervalMinutes".into(), json!(5), Value::Null),
            ]
        );
        assert_eq!(
            fields(&every[2]),
            [("monitor".into(), json!("active"), json!("paused"))]
        );

        // Deleting keeps every field it had, and the entry outlives the application.
        delete_application(&path, &shop.id).unwrap();
        let log = logged(&path);
        assert_eq!(
            log[4..],
            [
                manual("check_saved", "Set up the HTTP check of «Shop»: GET https://example.com/health expecting 200 within 5 s, valid 60 min, supervised every 5 min while DevHQ is open"),
                manual("check_paused", "Paused the supervision of «Shop»"),
                manual("check_resumed", "Resumed the supervision of «Shop»: GET https://example.com/health expecting 200 within 5 s, valid 60 min, supervised every 5 min while DevHQ is open"),
                manual("check_removed", "Removed the HTTP check of «Shop» (https://example.com/health); the evidence it recorded stays"),
                manual("deleted", "Deleted application «Shop», which had no history"),
            ]
        );
        let deleted = list(&path).unwrap().changes.remove(0);
        assert_eq!(deleted.subject_name, "Shop");
        assert_eq!(
            fields(&deleted)
                .iter()
                .map(|(field, _, after)| (field.as_str(), after.is_null()))
                .collect::<Vec<_>>(),
            [
                ("name", true),
                ("lifecycle", true),
                ("version", true),
                ("description", true),
                ("components", true)
            ]
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn deletes_what_has_no_history_with_its_components_and_check() {
        let path = path();
        let host = resource(&path, "Host");
        let spare = resource(&path, "Spare");
        let shop = save_application(&path, app(Some(host.id.clone()))).unwrap();
        let kept = save_application(&path, app(Some(host.id.clone()))).unwrap();
        http_check::save_http_check(
            &path,
            http_check(records::Subject::Application {
                id: shop.id.clone(),
            }),
        )
        .unwrap();
        http_check::save_http_check(
            &path,
            http_check(records::Subject::Resource {
                id: spare.id.clone(),
            }),
        )
        .unwrap();

        delete_application(&path, &shop.id).unwrap();
        delete_resource(&path, &spare.id).unwrap();
        let snapshot = list(&path).unwrap();
        assert_eq!(snapshot.applications, vec![kept]);
        assert_eq!(snapshot.resources, vec![host]);
        assert!(snapshot.http_checks.is_empty());
        let db = open(&path).unwrap();
        let components: i64 = db
            .query_row(
                "SELECT count(*) FROM components WHERE application_id = ?1",
                [&shop.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(components, 0);
        drop(db);
        let missing = delete_application(&path, &shop.id).unwrap_err();
        assert_eq!(missing.code, "invalid_input");
        assert!(missing.message.contains("does not exist"));
        assert_eq!(
            delete_resource(&path, "ghost").unwrap_err().code,
            "invalid_input"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn keeps_anything_with_history_or_in_use_and_says_why() {
        let path = path();
        let host = resource(&path, "Host");
        let shop = save_application(&path, app(Some(host.id.clone()))).unwrap();
        let application = records::Subject::Application {
            id: shop.id.clone(),
        };
        let used = delete_resource(&path, &host.id).unwrap_err();
        assert_eq!(used.code, "invalid_input");
        assert!(
            used.message
                .contains("Host is used by Example (API). Link those components"),
            "{}",
            used.message
        );

        records::open_incident(
            &path,
            records::IncidentInput {
                subject: application.clone(),
                title: "Down".into(),
                description: String::new(),
                severity: "critical".into(),
            },
        )
        .unwrap();
        records::record_event(
            &path,
            records::EventInput {
                subject: application.clone(),
                kind: "note".into(),
                outcome: "info".into(),
                title: "Noted".into(),
                detail: String::new(),
                occurred_at: None,
            },
        )
        .unwrap();
        let error = delete_application(&path, &shop.id).unwrap_err();
        assert_eq!(error.code, "invalid_input");
        assert_eq!(
            error.message,
            "Example has recorded incidents and events. DevHQ keeps history, so it cannot be deleted; set its lifecycle to Archived instead."
        );

        // Unlinked, the resource is still kept by its own history.
        save_application(
            &path,
            ApplicationInput {
                id: Some(shop.id.clone()),
                components: vec![],
                ..app(None)
            },
        )
        .unwrap();
        records::record_observation(
            &path,
            records::ObservationInput {
                subject: records::Subject::Resource {
                    id: host.id.clone(),
                },
                result: "healthy".into(),
                summary: "Fine".into(),
                observed_at: None,
                valid_for_minutes: 60,
            },
        )
        .unwrap();
        let error = delete_resource(&path, &host.id).unwrap_err();
        assert_eq!(
            error.message,
            "Host has recorded observations and events. DevHQ keeps history, so it cannot be deleted."
        );
        let snapshot = list(&path).unwrap();
        assert_eq!(snapshot.applications.len(), 1);
        assert_eq!(snapshot.resources.len(), 1);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_result_arriving_after_its_subject_was_deleted_is_not_recorded() {
        let path = path();
        let host = resource(&path, "Host");
        let subject = records::Subject::Resource {
            id: host.id.clone(),
        };
        http_check::save_http_check(&path, http_check(subject.clone())).unwrap();
        // A run started before the deletion finishes after it.
        delete_resource(&path, &host.id).unwrap();
        let late = records::store_observation(
            &path,
            records::NewObservation {
                subject: &subject,
                result: "healthy",
                summary: "HTTP 200 as expected",
                observed_at: records::now_ms(),
                valid_for_minutes: 60,
                recorded_at: records::now_ms(),
                http: Some(&records::HttpEvidence {
                    url: "https://example.com/health".into(),
                    expected_status: 200,
                    status: Some(200),
                    latency_ms: 5,
                    error: None,
                    redirects: vec![],
                }),
                reporter: None,
                asked_by: None,
                lifeline: None,
            },
        )
        .unwrap_err();
        assert_eq!(late.code, "invalid_input");
        let snapshot = list(&path).unwrap();
        assert!(snapshot.observations.is_empty() && snapshot.resources.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn text_saved_before_the_limits_can_still_be_saved_unchanged() {
        let path = path();
        let long = |count: usize| "a".repeat(count);
        // As a version without limits could have left them.
        let db = open(&path).unwrap();
        db.execute(
            "INSERT INTO resources (id, name, kind, description, operating_system) VALUES ('host', 'Host', 'server', ?1, ?2)",
            params![long(1500), long(200)],
        )
        .unwrap();
        db.execute(
            "INSERT INTO applications (id, name, lifecycle, description, version) VALUES ('shop', ?1, 'production', ?2, ?3)",
            params![long(130), long(1500), long(80)],
        )
        .unwrap();
        db.execute(
            "INSERT INTO components (id, application_id, name, kind, resource_id) VALUES ('api', 'shop', ?1, 'docker', 'host')",
            [long(150)],
        )
        .unwrap();
        drop(db);
        let stored = list(&path).unwrap();
        let shop = &stored.applications[0];
        let archive = |description: String| ApplicationInput {
            id: Some(shop.id.clone()),
            name: shop.name.clone(),
            description,
            lifecycle: "archived".into(),
            version: shop.version.clone(),
            components: vec![ComponentInput {
                id: Some("api".into()),
                name: shop.components[0].name.clone(),
                kind: "docker".into(),
                resource_id: Some("host".into()),
            }],
            references: vec![],
        };
        // Archiving, as a refused deletion suggests, keeps working.
        let archived = save_application(&path, archive(shop.description.clone())).unwrap();
        assert_eq!(archived.lifecycle, "archived");
        assert_eq!(archived.description.len(), 1500);
        let host = &stored.resources[0];
        assert!(save_resource(
            &path,
            ResourceInput {
                id: Some(host.id.clone()),
                name: "Renamed host".into(),
                kind: "server".into(),
                description: host.description.clone(),
                operating_system: host.operating_system.clone(),
                references: vec![],
            },
        )
        .is_ok());
        // New text, though, must fit.
        let error = save_application(&path, archive(long(1400))).unwrap_err();
        assert!(
            error.message.contains("longer than 1000"),
            "{}",
            error.message
        );
        // A component's kept name is its own: another component cannot borrow it.
        let mut borrowed = archive(shop.description.clone());
        borrowed.components.push(ComponentInput {
            id: None,
            ..borrowed.components[0].clone()
        });
        let error = save_application(&path, borrowed).unwrap_err();
        assert!(
            error.message.contains("Component name is longer than 120"),
            "{}",
            error.message
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn counts_length_as_the_window_does() {
        let path = path();
        // Outside the Basic Multilingual Plane: one character, two UTF-16 code units.
        let emoji = |count: usize| "🛰".repeat(count);
        let named = |name: String| ResourceInput {
            id: None,
            name,
            kind: "server".into(),
            description: String::new(),
            operating_system: String::new(),
            references: vec![],
        };
        assert!(save_resource(&path, named(emoji(60))).is_ok());
        let error = save_resource(&path, named(emoji(61))).unwrap_err();
        assert!(
            error.message.contains("longer than 120"),
            "{}",
            error.message
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn says_where_the_database_file_is() {
        let folder = std::env::temp_dir().join("com.devhq.desktop");
        assert_eq!(
            DataLocation::of(&folder.join("inventory.sqlite3")),
            DataLocation {
                database: folder.join("inventory.sqlite3").display().to_string(),
                directory: folder.display().to_string(),
                // Not created yet, so there is nothing to resolve.
                stored_at: None,
            }
        );
    }

    /// The temporary folder as the file system names it, for tests that compare paths it
    /// resolves: on a Windows runner `TMP` is a short 8.3 name (`C:\Users\RUNNER~1\…`), and on
    /// macOS `/var` is a link to `/private/var`.
    pub(crate) fn resolved_temp_dir() -> PathBuf {
        PathBuf::from(plain(&std::fs::canonicalize(std::env::temp_dir()).unwrap()))
    }

    /// A folder of its own and, beside it, a link to it: the data folder reached through the
    /// link stands for one that Windows redirects for a packaged app, which a test cannot set up.
    /// A junction on Windows, which needs no privilege; a symbolic link elsewhere.
    pub(crate) fn linked_folder() -> (PathBuf, PathBuf) {
        let base = resolved_temp_dir();
        let name = format!("devhq-linked-{}", Uuid::new_v4());
        let (real, link) = (base.join(&name), base.join(format!("{name}-link")));
        std::fs::create_dir(&real).unwrap();
        #[cfg(windows)]
        assert!(std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&real)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap()
            .success());
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        (real, link)
    }

    /// Removes the link alone, then the folder and what it holds.
    pub(crate) fn remove_linked((real, link): (PathBuf, PathBuf)) {
        #[cfg(windows)]
        std::fs::remove_dir(&link).unwrap();
        #[cfg(unix)]
        std::fs::remove_file(&link).unwrap();
        std::fs::remove_dir_all(real).unwrap();
    }

    #[test]
    fn says_where_the_file_system_really_keeps_the_database_file() {
        let (real, link) = linked_folder();
        let asked = link.join("inventory.sqlite3");
        start(&asked).unwrap();
        let kept = real.join("inventory.sqlite3");
        assert_eq!(
            DataLocation::of(&asked),
            DataLocation {
                database: asked.display().to_string(),
                directory: link.display().to_string(),
                stored_at: Some(kept.display().to_string()),
            }
        );
        // Asked for where it really is, it has nothing else to say.
        assert_eq!(DataLocation::of(&kept).stored_at, None);
        remove_linked((real, link));
    }

    /// Two windows starting at once on no inventory: one creates it, the other finds it, and
    /// nothing either built under its own name is left behind. Their log may be: SQLite folds and
    /// deletes it only when the connection that closes last can take the file to itself, and one
    /// that closes while another window still looks at the file cannot. Whether that happens is a
    /// race, so this checks what the next window does with what they leave: it opens it as the
    /// one inventory, in this build's schema.
    #[test]
    fn concurrent_starts_create_one_inventory() {
        let folder = std::env::temp_dir().join(format!("devhq-starts-{}", Uuid::new_v4()));
        let path = folder.join("inventory.sqlite3");
        let starts: Vec<_> = (0..4)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || start(&path))
            })
            .collect();
        for started in starts {
            started.join().unwrap().unwrap();
        }
        let names: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        // The inventory and, perhaps, its log and the log's index: no building name, no journal.
        let own = [
            "inventory.sqlite3",
            "inventory.sqlite3-wal",
            "inventory.sqlite3-shm",
        ];
        assert!(
            names.iter().all(|name| own.contains(&name.as_str())),
            "{names:?}"
        );
        assert!(
            names.iter().any(|name| name == "inventory.sqlite3"),
            "{names:?}"
        );
        start(&path).unwrap();
        assert_eq!(examine(&path, |_| Ok(())).0, Found::Current);
        std::fs::remove_dir_all(folder).unwrap();
    }

    fn host() -> ResourceInput {
        ResourceInput {
            id: None,
            name: "Host".into(),
            kind: "server".into(),
            description: String::new(),
            operating_system: String::new(),
            references: vec![],
        }
    }

    /// A new inventory is made only where there is no file: an empty file, which DevHQ did not
    /// make (it never names half an inventory), is never initialized, by the window or anything.
    /// With SQLite's files beside it, it is still a file that is there, not files left without one.
    #[test]
    fn never_turns_an_existing_file_into_an_inventory() {
        let path = nothing();
        std::fs::write(&path, b"").unwrap();
        assert!(!create(&path).unwrap(), "a file is there");
        std::fs::write(beside(&path, "-journal"), b"").unwrap();
        assert!(!create(&path).unwrap(), "a file is there, with its journal");
        let refused = start(&path).unwrap_err();
        assert_eq!(refused.code, "not_an_inventory");
        assert_eq!(std::fs::read(beside(&path, "-journal")).unwrap(), b"");
        // A use case's connection may write, and SQLite deletes a journal beside an empty file
        // when it opens it; the window opens none once `start` refused the file.
        std::fs::remove_file(beside(&path, "-journal")).unwrap();
        assert!(list(&path).is_err());
        assert!(save_resource(&path, host()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"");
        std::fs::remove_file(path).unwrap();
    }

    /// Every file beside the inventory at `path` (its log, its index, its journal), with what it
    /// holds.
    fn files_of(path: &Path) -> Vec<(String, Vec<u8>)> {
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        let mut found: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap())
            .filter(|entry| entry.file_name().to_str().unwrap().starts_with(&name))
            .map(|entry| {
                (
                    entry.file_name().into_string().unwrap(),
                    std::fs::read(entry.path()).unwrap(),
                )
            })
            .collect();
        found.sort();
        found
    }

    /// A use case that opens a file in another schema, older or newer, refuses it inside its
    /// transaction and leaves it as it was, journal mode included: nothing upgrades or converts it.
    #[test]
    fn a_use_case_refuses_another_schema_and_leaves_it_as_it_is() {
        for (version, logged, code) in [
            (6, false, "schema_outdated"),
            (11, true, "schema_outdated"),
            (SCHEMA_VERSION + 1, false, "schema_newer"),
            (SCHEMA_VERSION + 1, true, "schema_newer"),
        ] {
            let case = format!("v{version}, logged: {logged}");
            let folder = std::env::temp_dir().join(format!("devhq-other-{}", Uuid::new_v4()));
            std::fs::create_dir(&folder).unwrap();
            let path = folder.join("inventory.sqlite3");
            let db = database_at(&path, version.min(SCHEMA_VERSION));
            db.pragma_update(None, "user_version", version).unwrap();
            if logged {
                db.pragma_update(None, "journal_mode", "WAL").unwrap();
            }
            drop(db);
            let before = files_of(&path);
            let refusals = [
                list(&path).map(|_| ()),
                save_resource(&path, host()).map(|_| ()),
                access::agent_access(&path).map(|_| ()),
                access::set_agent_access(&path, "write").map(|_| ()),
                http_check::supervision(&path).map(|_| ()),
                crate::backup::back_up(&path).map(|_| ()),
            ];
            for refusal in refusals {
                assert_eq!(refusal.unwrap_err().code, code, "{case}");
            }
            assert_eq!(files_of(&path), before, "{case}");
            assert!(!folder.join("backups").exists(), "{case}: a backups folder");
            std::fs::remove_dir_all(folder).unwrap();
        }
    }

    /// What a use case reads or writes is decided inside its transaction: a file whose schema
    /// changed after it was opened (another DevHQ, another program) is refused there, as it is,
    /// and never written; one in this build's schema is written.
    #[test]
    fn checks_the_schema_inside_the_transaction() {
        let path = path();
        let version = || -> i64 {
            Connection::open(&path)
                .unwrap()
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap()
        };
        let mut db = open_current(&path).unwrap();
        // Another connection makes it an older inventory after this one opened it.
        Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", 11)
            .unwrap();
        let refused = begin_write(&mut db, &Actor::Person).unwrap_err();
        assert_eq!(refused.code, "schema_outdated");
        let refused = begin(&mut db, TransactionBehavior::Deferred).unwrap_err();
        assert_eq!(refused.code, "schema_outdated");
        assert_eq!(version(), 11, "never upgraded");
        // Back in this build's schema, the same connection writes.
        Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .unwrap();
        let tx = begin_write(&mut db, &Actor::Person).unwrap();
        tx.execute("DELETE FROM agent_access", []).unwrap();
        tx.commit().unwrap();
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    /// An agent writes only while the person lets agents write, asked inside the write
    /// transaction with SQLite's write lock held: the gate closed after the agent's connection
    /// opened refuses the write. The person's writes never ask.
    #[test]
    fn asks_for_the_persons_leave_inside_an_agents_write() {
        let path = path();
        access::set_agent_access(&path, "write").unwrap();
        let agent = Actor::agent("claude-code").unwrap();
        let mut db = open_current(&path).unwrap();
        // Closed in Settings after the agent's connection opened.
        access::set_agent_access(&path, "read").unwrap();
        assert_eq!(
            begin_write(&mut db, &agent).unwrap_err().code,
            "access_denied"
        );
        drop(begin_write(&mut db, &Actor::Person).unwrap());
        access::set_agent_access(&path, "write").unwrap();
        drop(begin_write(&mut db, &agent).unwrap());
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    /// An inventory that goes while the window is open is not made again by any use case, which
    /// says so.
    #[test]
    fn never_creates_an_inventory_that_went_while_open() {
        let path = path();
        resource(&path, "Host");
        std::fs::remove_file(&path).unwrap();
        let refused = list(&path).unwrap_err();
        assert_eq!(refused.code, "invalid_input");
        assert!(refused.message.contains("There is no DevHQ inventory at"));
        assert!(save_resource(&path, host()).is_err());
        assert!(access::agent_access(&path).is_err());
        assert!(http_check::supervision(&path).is_err());
        assert!(!path.exists());
    }

    /// Runs `work` on its own connection in another thread and waits until it holds what it
    /// took; the thread lets go when the returned sender is dropped or used.
    pub(crate) fn holding<T: Send + 'static>(
        path: &Path,
        work: impl FnOnce(&Path, &dyn Fn()) -> T + Send + 'static,
    ) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<T>) {
        let (held, wait_held) = std::sync::mpsc::channel();
        let (release, wait_release) = std::sync::mpsc::channel::<()>();
        let path = path.to_path_buf();
        let thread = std::thread::spawn(move || {
            work(&path, &|| {
                held.send(()).unwrap();
                // Far longer than anything here would wait before giving up.
                let _ = wait_release.recv_timeout(Duration::from_secs(30));
            })
        });
        wait_held.recv().unwrap();
        (release, thread)
    }

    #[test]
    fn an_up_to_date_database_is_read_without_waiting_for_a_write_in_progress() {
        let path = path();
        let host = resource(&path, "Host");
        let id = host.id.clone();
        let (release, writer) = holding(&path, move |path, hold| {
            let mut db = Connection::open(path).unwrap();
            let tx = db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            tx.execute("UPDATE resources SET name = 'Renamed' WHERE id = ?1", [&id])
                .unwrap();
            hold();
            tx.commit().unwrap();
        });
        // Every open used to take the write lock just to check the schema version, so this
        // waited for the writer and gave up after five seconds.
        let snapshot = list(&path);
        release.send(()).unwrap();
        writer.join().unwrap();
        // The last committed state, then the new one.
        assert_eq!(snapshot.unwrap().resources, vec![host]);
        assert_eq!(list(&path).unwrap().resources[0].name, "Renamed");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_write_never_waits_for_a_read_in_progress() {
        let path = path();
        resource(&path, "Host");
        let (release, reader) = holding(&path, |path, hold| {
            let mut db = open(path).unwrap();
            let tx = db.transaction().unwrap();
            let count = || -> i64 {
                tx.query_row("SELECT count(*) FROM resources", [], |row| row.get(0))
                    .unwrap()
            };
            let before = count();
            hold();
            (before, count())
        });
        // With a rollback journal a commit needs every reader gone, so this waited for the
        // reader and gave up after five seconds.
        let saved = save_resource(
            &path,
            ResourceInput {
                id: None,
                name: "Spare".into(),
                kind: "server".into(),
                description: String::new(),
                operating_system: String::new(),
                references: vec![],
            },
        );
        release.send(()).unwrap();
        // The reader kept the state it started from.
        assert_eq!(reader.join().unwrap(), (1, 1));
        assert!(saved.is_ok(), "{saved:?}");
        assert_eq!(list(&path).unwrap().resources.len(), 2);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn writes_ahead_and_leaves_one_complete_file_once_closed() {
        let path = nothing();
        // As earlier releases left it, and as every backup is: a rollback journal.
        let db = Connection::open(&path).unwrap();
        db.pragma_update(None, "foreign_keys", "OFF").unwrap();
        for (_, sql) in &MIGRATIONS {
            db.execute_batch(sql).unwrap();
        }
        db.pragma_update(None, "user_version", SCHEMA_VERSION)
            .unwrap();
        let mode = |db: &Connection| -> String {
            db.query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .unwrap()
        };
        assert_eq!(mode(&db), "delete");
        drop(db);
        // Reading and writing never convert it: only the window does, when it starts.
        resource(&path, "Host");
        assert_eq!(list(&path).unwrap().resources.len(), 1);
        assert_eq!(mode(&Connection::open(&path).unwrap()), "delete");
        // Switching needs the file to itself and SQLite does not wait for it: while another
        // connection writes, the window still starts on it, in the mode it has.
        let (release, writer) = holding(&path, |path, hold| {
            let mut db = Connection::open(path).unwrap();
            let tx = db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            hold();
            tx.commit().unwrap();
        });
        let contended = start(&path);
        release.send(()).unwrap();
        writer.join().unwrap();
        assert!(contended.is_ok(), "{:?}", contended.err());
        assert_eq!(mode(&Connection::open(&path).unwrap()), "delete");
        // The next start has it to itself and switches it, for every later connection.
        start(&path).unwrap();
        resource(&path, "Spare");
        let db = Connection::open(&path).unwrap();
        assert_eq!(mode(&db), "wal");
        drop(db);
        // Every connection is closed: the log was folded back into the database file, which
        // alone holds everything, so a copy of it is a complete backup.
        assert!(!path.with_extension("sqlite3-wal").exists());
        let copy = path.with_extension("copy.sqlite3");
        std::fs::copy(&path, &copy).unwrap();
        assert_eq!(list(&copy).unwrap().resources.len(), 2);
        std::fs::remove_file(copy).unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn upgrades_v1_schema_without_losing_inventory() {
        let path = nothing();
        let db = Connection::open(&path).unwrap();
        db.execute_batch(include_str!("../migrations/001_inventory.sql"))
            .unwrap();
        db.pragma_update(None, "user_version", 1).unwrap();
        db.execute("INSERT INTO applications (id, name, lifecycle) VALUES ('legacy', 'Old app', 'testing')", []).unwrap();
        drop(db);
        upgrade(&path).unwrap();
        let upgraded = list(&path).unwrap();
        assert_eq!(upgraded.applications[0].id, "legacy");
        assert_eq!(upgraded.applications[0].description, "");
        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn upgrades_v2_inventory_to_operational_records() {
        let path = nothing();
        let db = Connection::open(&path).unwrap();
        db.execute_batch(include_str!("../migrations/001_inventory.sql"))
            .unwrap();
        db.execute_batch(include_str!("../migrations/002_inventory_details.sql"))
            .unwrap();
        db.pragma_update(None, "user_version", 2).unwrap();
        db.execute_batch(
            "INSERT INTO resources (id, name, kind, description, operating_system) VALUES ('host', 'Host', 'server', 'Rack', 'Linux');
             INSERT INTO applications (id, name, lifecycle, description, version) VALUES ('shop', 'Shop', 'production', 'Store', '2.0');
             INSERT INTO components (id, application_id, name, kind, resource_id) VALUES ('api', 'shop', 'API', 'backend', 'host');",
        )
        .unwrap();
        drop(db);
        upgrade(&path).unwrap();
        let upgraded = list(&path).unwrap();
        assert_eq!(upgraded.resources[0].operating_system, "Linux");
        assert_eq!(upgraded.applications[0].version.as_deref(), Some("2.0"));
        assert_eq!(
            upgraded.applications[0].components[0]
                .resource_id
                .as_deref(),
            Some("host")
        );
        assert!(upgraded.observations.is_empty());
        assert!(upgraded.incidents.is_empty());
        assert!(upgraded.events.is_empty());
        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let violations = db
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query_map([], |_| Ok(()))
            .unwrap()
            .count();
        assert_eq!(violations, 0);
        drop(db);
        // Pre-existing inventory can receive records after the upgrade.
        records::record_observation(
            &path,
            records::ObservationInput {
                subject: records::Subject::Resource { id: "host".into() },
                result: "degraded".into(),
                summary: "Disk at 85%".into(),
                observed_at: None,
                valid_for_minutes: 60,
            },
        )
        .unwrap();
        assert_eq!(list(&path).unwrap().observations.len(), 1);
        std::fs::remove_file(path).unwrap();
    }

    /// A database as the previous release (schema v3) left it, with enforcement off.
    fn v3_database(path: &Path) -> Connection {
        let db = Connection::open(path).unwrap();
        // The bundled SQLite enforces foreign keys by default.
        db.pragma_update(None, "foreign_keys", "OFF").unwrap();
        for (_, sql) in &MIGRATIONS[..3] {
            db.execute_batch(sql).unwrap();
        }
        db.pragma_update(None, "user_version", 3).unwrap();
        db
    }

    fn strings(db: &Connection, sql: &str) -> Vec<String> {
        db.prepare(sql)
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn upgrades_v3_records_for_http_checks_without_losing_anything() {
        let path = nothing();
        let db = v3_database(&path);
        db.execute_batch(
            "INSERT INTO resources (id, name, kind, description, operating_system) VALUES ('host', 'Host', 'server', '', 'Linux');
             INSERT INTO applications (id, name, lifecycle, description, version) VALUES ('shop', 'Shop', 'production', 'Store', '2.0');
             INSERT INTO components (id, application_id, name, kind, resource_id) VALUES ('api', 'shop', 'API', 'backend', 'host');
             -- Equal times: only the rowid orders these two, and it must survive the rebuild.
             INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at)
               VALUES ('obs-b', 'host', 'healthy', 'First', 'manual', 1000, 2000, 1000),
                      ('obs-a', 'host', 'unhealthy', 'Second', 'manual', 1000, 2000, 1000);
             INSERT INTO observations (id, application_id, result, summary, source, observed_at, valid_until, recorded_at)
               VALUES ('obs-app', 'shop', 'degraded', 'Slow', 'manual', 500, 900, 600);
             INSERT INTO incidents (id, application_id, title, description, severity, status, source, opened_at, updated_at)
               VALUES ('inc', 'shop', 'Down', 'Checkout', 'critical', 'open', 'manual', 700, 700);
             INSERT INTO events (id, resource_id, kind, outcome, title, detail, source, observation_id, occurred_at, recorded_at)
               VALUES ('evt-b', 'host', 'observation', 'success', 'Observation: Healthy', 'First', 'manual', 'obs-b', 1000, 1000),
                      ('evt-a', 'host', 'observation', 'failure', 'Observation: Unhealthy', 'Second', 'manual', 'obs-a', 1000, 1000);
             INSERT INTO events (id, application_id, kind, outcome, title, detail, source, observation_id, incident_id, occurred_at, recorded_at)
               VALUES ('evt-app', 'shop', 'observation', 'warning', 'Observation: Attention required', 'Slow', 'manual', 'obs-app', NULL, 500, 600),
                      ('evt-inc', 'shop', 'incident', 'failure', 'Incident opened: Down', '', 'manual', NULL, 'inc', 700, 700),
                      ('evt-note', 'shop', 'note', 'info', 'Noted', 'By hand', 'manual', NULL, NULL, 800, 800);",
        )
        .unwrap();
        let events_sql = "SELECT id || ' ' || coalesce(observation_id, '-') || ' ' || coalesce(incident_id, '-') || ' ' || title || ' ' || detail FROM events ORDER BY occurred_at DESC, recorded_at DESC, rowid DESC";
        let events_before = strings(&db, events_sql);
        drop(db);

        upgrade(&path).unwrap();
        let upgraded = list(&path).unwrap();
        assert_eq!(
            upgraded
                .observations
                .iter()
                .map(|item| (item.id.as_str(), item.summary.as_str()))
                .collect::<Vec<_>>(),
            [("obs-a", "Second"), ("obs-b", "First"), ("obs-app", "Slow")]
        );
        assert!(upgraded
            .observations
            .iter()
            .all(|item| item.source == "manual" && item.http.is_none()));
        assert_eq!(
            upgraded.observations[2].valid_until,
            "1970-01-01T00:00:00.9Z"
        );
        assert_eq!(
            upgraded
                .events
                .iter()
                .map(|item| format!(
                    "{} {} {} {} {}",
                    item.id,
                    item.observation_id.as_deref().unwrap_or("-"),
                    item.incident_id.as_deref().unwrap_or("-"),
                    item.title,
                    item.detail
                ))
                .collect::<Vec<_>>(),
            events_before
        );
        assert!(upgraded.events.iter().all(|item| item.source == "manual"));
        assert_eq!(upgraded.incidents.len(), 1);
        assert_eq!(upgraded.incidents[0].description, "Checkout");
        assert_eq!(
            upgraded.applications[0].components[0]
                .resource_id
                .as_deref(),
            Some("host")
        );
        assert_eq!(upgraded.resources[0].operating_system, "Linux");
        assert!(upgraded.http_checks.is_empty());

        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert!(strings(&db, "SELECT \"table\" FROM pragma_foreign_key_check").is_empty());
        assert_eq!(
            strings(&db, "SELECT name FROM sqlite_master WHERE type = 'index' AND name NOT LIKE 'sqlite_autoindex%' ORDER BY name"),
            [
                "change_log_application_id",
                "change_log_application_latest",
                "change_log_resource_id",
                "change_log_resource_latest",
                "components_application_id",
                "components_resource_id",
                "copy_readings_application_id",
                "copy_readings_reference_id",
                "copy_readings_resource_id",
                "events_application_id",
                "events_incident_id",
                "events_observation_id",
                "events_reported",
                "events_resource_id",
                "incidents_application_id",
                "incidents_resource_id",
                "observations_application_id",
                "observations_reported",
                "observations_resource_id",
                "system_references_application_id",
                "system_references_machine_id",
                "system_references_resource_id",
            ]
        );
        // Events point at the rebuilt observations table; nothing is left half renamed.
        assert_eq!(
            strings(
                &db,
                "SELECT DISTINCT \"table\" FROM pragma_foreign_key_list('events') ORDER BY 1"
            ),
            ["applications", "incidents", "observations", "resources"]
        );
        assert!(strings(&db, "SELECT name FROM sqlite_master WHERE name LIKE '%_v4'").is_empty());
        drop(db);
        // The rebuilt relationships are enforced again.
        let db = open(&path).unwrap();
        assert!(db
            .execute("DELETE FROM observations WHERE id = 'obs-a'", [])
            .is_err());
        assert!(db
            .execute(
                "INSERT INTO events (id, resource_id, kind, outcome, title, source, observation_id, occurred_at, recorded_at)
                 VALUES ('ghost', 'host', 'observation', 'success', 'Observed', 'manual', 'missing', 1, 1)",
                [],
            )
            .is_err());
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn refuses_an_upgrade_that_would_keep_broken_relationships() {
        let path = nothing();
        let db = v3_database(&path);
        // Written with enforcement off, as another tool could have done.
        db.execute_batch(
            "INSERT INTO resources (id, name, kind) VALUES ('host', 'Host', 'server');
             INSERT INTO events (id, resource_id, kind, outcome, title, source, observation_id, occurred_at, recorded_at)
               VALUES ('orphan', 'host', 'observation', 'success', 'Observed', 'manual', 'missing', 1, 1);",
        )
        .unwrap();
        drop(db);
        let error = upgrade(&path).unwrap_err();
        assert_eq!(error.code, "database_error");
        assert!(
            error.message.contains("1 broken relationships"),
            "{}",
            error.message
        );
        // The whole upgrade was rolled back.
        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);
        assert_eq!(strings(&db, "SELECT id FROM events"), ["orphan"]);
        assert!(strings(
            &db,
            "SELECT name FROM sqlite_master WHERE name = 'http_checks'"
        )
        .is_empty());
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    /// Every row of a table, rowid first, as text: a lossless picture to compare across upgrades.
    pub(crate) fn dump(db: &Connection, table: &str) -> Vec<String> {
        let mut statement = db
            .prepare(&format!("SELECT rowid, * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = statement.column_count();
        statement
            .query_map([], |row| {
                (0..columns)
                    .map(|index| Ok(format!("{:?}", row.get_ref(index)?)))
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map(|values| values.join(" | "))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// The tables of reported and automated records, which v7 rebuilds with an `agent` column.
    const RECORDS: [&str; 3] = ["observations", "incidents", "events"];

    /// Rows dumped before v7, as v7 keeps them: the same values, followed by an empty agent.
    fn no_agent(rows: &[String]) -> Vec<String> {
        rows.iter().map(|row| format!("{row} | Null")).collect()
    }

    /// A database as an earlier release (schema `version`) left it, with enforcement off.
    pub(crate) fn database_at(path: &Path, version: i64) -> Connection {
        let db = Connection::open(path).unwrap();
        db.pragma_update(None, "foreign_keys", "OFF").unwrap();
        for (number, sql) in &MIGRATIONS {
            if *number <= version {
                db.execute_batch(sql).unwrap();
            }
        }
        db.pragma_update(None, "user_version", version).unwrap();
        db
    }

    /// A database as the previous release (schema v4) left it, with enforcement off.
    fn v4_database(path: &Path) -> Connection {
        let db = Connection::open(path).unwrap();
        db.pragma_update(None, "foreign_keys", "OFF").unwrap();
        for (_, sql) in &MIGRATIONS[..4] {
            db.execute_batch(sql).unwrap();
        }
        db.pragma_update(None, "user_version", 4).unwrap();
        db
    }

    #[test]
    fn upgrades_v4_http_checks_for_supervision_without_losing_anything() {
        let path = nothing();
        let db = v4_database(&path);
        db.execute_batch(
            "INSERT INTO resources (id, name, kind, description, operating_system) VALUES ('host', 'Host', 'server', '', 'Linux'), ('db', 'DB', 'server', '', '');
             INSERT INTO applications (id, name, lifecycle, description, version) VALUES ('shop', 'Shop', 'production', 'Store', '2.0');
             INSERT INTO components (id, application_id, name, kind, resource_id) VALUES ('api', 'shop', 'API', 'backend', 'host');
             INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at)
               VALUES ('obs-manual', 'db', 'degraded', 'Slow', 'manual', 500, 900, 600);
             INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at,
                                       http_url, http_expected_status, http_status, http_latency_ms, http_error, http_redirects)
               VALUES ('obs-http', 'host', 'unhealthy', 'HTTP 500, expected 200, in 9 ms', 'http', 1000, 2000, 1000,
                       'http://host/health', 200, 500, 9, NULL, '[{\"status\":301,\"location\":\"http://host/health\"}]');
             INSERT INTO incidents (id, application_id, title, description, severity, status, source, opened_at, updated_at)
               VALUES ('inc', 'shop', 'Down', 'Checkout', 'critical', 'open', 'manual', 700, 700);
             INSERT INTO events (id, resource_id, kind, outcome, title, detail, source, observation_id, occurred_at, recorded_at)
               VALUES ('evt-http', 'host', 'observation', 'failure', 'HTTP check: Unhealthy', 'HTTP 500', 'http', 'obs-http', 1000, 1000),
                      ('evt-manual', 'db', 'observation', 'warning', 'Observation: Attention required', 'Slow', 'manual', 'obs-manual', 500, 600);
             INSERT INTO events (id, application_id, kind, outcome, title, source, incident_id, occurred_at, recorded_at)
               VALUES ('evt-inc', 'shop', 'incident', 'failure', 'Incident opened: Down', 'manual', 'inc', 700, 700);
             -- Inserted out of name order: the rowid, not the subject, orders the list.
             INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at)
               VALUES ('host', 'http://host/health', 200, 5, 15, 800);
             INSERT INTO http_checks (application_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at)
               VALUES ('shop', 'https://shop.example/ready', 204, 10, 60, 900);",
        )
        .unwrap();
        let unchanged = ["resources", "applications", "components"];
        let before: Vec<_> = unchanged.iter().map(|table| dump(&db, table)).collect();
        let records_before: Vec<_> = RECORDS.iter().map(|table| dump(&db, table)).collect();
        let checks_before = dump(&db, "http_checks");
        drop(db);

        upgrade(&path).unwrap();
        let upgraded = list(&path).unwrap();
        assert_eq!(upgraded.http_checks.len(), 2);
        assert_eq!(
            upgraded.http_checks[0].subject,
            records::Subject::Resource { id: "host".into() }
        );
        assert_eq!(upgraded.http_checks[1].url, "https://shop.example/ready");
        // Supervision never starts by itself: every existing check runs only when asked.
        assert!(upgraded
            .http_checks
            .iter()
            .all(|check| check.monitor == "off" && check.monitor_interval_minutes.is_none()));
        assert_eq!(upgraded.observations.len(), 2);
        assert_eq!(upgraded.events.len(), 3);

        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        for (table, rows) in unchanged.iter().zip(&before) {
            assert_eq!(&dump(&db, table), rows, "{table}");
        }
        for (table, rows) in RECORDS.iter().zip(&records_before) {
            assert_eq!(dump(&db, table), no_agent(rows), "{table}");
        }
        // The same rows and values, followed by the two new columns.
        assert_eq!(
            dump(&db, "http_checks"),
            checks_before
                .iter()
                .map(|row| format!(
                    "{row} | {:?} | Null",
                    rusqlite::types::ValueRef::Text(b"off")
                ))
                .collect::<Vec<_>>()
        );
        assert!(strings(&db, "SELECT \"table\" FROM pragma_foreign_key_check").is_empty());
        assert!(strings(&db, "SELECT name FROM sqlite_master WHERE name LIKE '%_v5'").is_empty());
        assert_eq!(
            strings(
                &db,
                "SELECT DISTINCT \"table\" || ' ' || on_delete FROM pragma_foreign_key_list('http_checks') ORDER BY 1"
            ),
            ["applications CASCADE", "resources CASCADE"]
        );
        drop(db);
        // The rebuilt table enforces its relationships and the new rules.
        let db = open(&path).unwrap();
        let supervise = |interval: Option<i64>, monitor: &str, valid: i64| {
            db.execute(
                "UPDATE http_checks SET monitor = ?1, monitor_interval_minutes = ?2, valid_for_minutes = ?3 WHERE resource_id = 'host'",
                rusqlite::params![monitor, interval, valid],
            )
        };
        assert!(
            supervise(Some(15), "active", 15).is_err(),
            "validity must outlast the interval"
        );
        assert!(
            supervise(Some(4), "active", 60).is_err(),
            "interval below 5 min"
        );
        assert!(
            supervise(Some(61), "active", 120).is_err(),
            "interval above 60 min"
        );
        assert!(
            supervise(None, "active", 60).is_err(),
            "active without interval"
        );
        assert!(
            supervise(Some(15), "off", 60).is_err(),
            "off keeps no interval"
        );
        assert!(
            supervise(Some(15), "sometimes", 60).is_err(),
            "unknown state"
        );
        assert!(supervise(Some(15), "paused", 60).is_ok());
        assert!(supervise(Some(15), "active", 16).is_ok());
        assert!(db
            .execute(
                "INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at)
                 VALUES ('ghost', 'http://x/', 200, 5, 60, 0)",
                [],
            )
            .is_err());
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn refuses_a_v5_upgrade_that_would_keep_broken_relationships() {
        let path = nothing();
        let db = v4_database(&path);
        // A check for a resource that no longer exists, written with enforcement off.
        db.execute(
            "INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at)
             VALUES ('gone', 'http://x/', 200, 5, 60, 0)",
            [],
        )
        .unwrap();
        drop(db);
        let error = upgrade(&path).unwrap_err();
        assert_eq!(error.code, "database_error");
        assert!(
            error.message.contains("1 broken relationships"),
            "{}",
            error.message
        );
        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);
        assert_eq!(dump(&db, "http_checks").len(), 1);
        assert!(strings(
            &db,
            "SELECT name FROM pragma_table_info('http_checks') WHERE name = 'monitor'"
        )
        .is_empty());
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn upgrades_v5_with_history_indexes_without_changing_any_row() {
        let path = nothing();
        let db = Connection::open(&path).unwrap();
        db.pragma_update(None, "foreign_keys", "OFF").unwrap();
        for (_, sql) in &MIGRATIONS[..5] {
            db.execute_batch(sql).unwrap();
        }
        db.pragma_update(None, "user_version", 5).unwrap();
        db.execute_batch(
            "INSERT INTO resources (id, name, kind, description, operating_system) VALUES ('host', 'Host', 'server', '', 'Linux');
             INSERT INTO applications (id, name, lifecycle, description, version) VALUES ('shop', 'Shop', 'production', 'Store', '2.0');
             INSERT INTO components (id, application_id, name, kind, resource_id) VALUES ('api', 'shop', 'API', 'backend', 'host');
             INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at)
               VALUES ('obs-manual', 'host', 'degraded', 'Slow', 'manual', 500, 900, 600);
             INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at,
                                       http_url, http_expected_status, http_status, http_latency_ms, http_error, http_redirects)
               VALUES ('obs-http', 'host', 'healthy', 'HTTP 200 as expected', 'http', 1000, 2000, 1000,
                       'http://host/health', 200, 200, 9, NULL, '[]');
             INSERT INTO incidents (id, application_id, title, description, severity, status, source, opened_at, updated_at)
               VALUES ('inc', 'shop', 'Down', 'Checkout', 'critical', 'open', 'manual', 700, 700);
             INSERT INTO events (id, resource_id, kind, outcome, title, detail, source, observation_id, occurred_at, recorded_at)
               VALUES ('evt-http', 'host', 'observation', 'success', 'HTTP check: Healthy', '', 'http', 'obs-http', 1000, 1000),
                      ('evt-manual', 'host', 'observation', 'warning', 'Observation: Attention required', 'Slow', 'manual', 'obs-manual', 500, 600);
             INSERT INTO events (id, application_id, kind, outcome, title, source, incident_id, occurred_at, recorded_at)
               VALUES ('evt-inc', 'shop', 'incident', 'failure', 'Incident opened: Down', 'manual', 'inc', 700, 700);
             INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at, monitor, monitor_interval_minutes)
               VALUES ('host', 'http://host/health', 200, 5, 15, 800, 'active', 5);",
        )
        .unwrap();
        let tables = ["resources", "applications", "components", "http_checks"];
        let before: Vec<_> = tables.iter().map(|table| dump(&db, table)).collect();
        let records_before: Vec<_> = RECORDS.iter().map(|table| dump(&db, table)).collect();
        drop(db);

        upgrade(&path).unwrap();
        let upgraded = list(&path).unwrap();
        assert_eq!(upgraded.observations.len(), 2);
        assert_eq!(upgraded.events.len(), 3);
        assert_eq!(upgraded.http_checks[0].monitor, "active");
        assert!(upgraded.automated_history.truncated.is_empty());

        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        for (table, rows) in tables.iter().zip(&before) {
            assert_eq!(&dump(&db, table), rows, "{table}");
        }
        // v7 rebuilds the records, and with them these indexes (see its own test).
        for (table, rows) in RECORDS.iter().zip(&records_before) {
            assert_eq!(dump(&db, table), no_agent(rows), "{table}");
        }
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn upgrades_v6_records_for_agents_without_losing_anything() {
        let path = nothing();
        let db = database_at(&path, 6);
        db.execute_batch(
            "INSERT INTO resources (id, name, kind, description, operating_system) VALUES ('host', 'Host', 'server', '', 'Linux');
             INSERT INTO applications (id, name, lifecycle, description, version) VALUES ('shop', 'Shop', 'production', 'Store', '2.0');
             INSERT INTO components (id, application_id, name, kind, resource_id) VALUES ('api', 'shop', 'API', 'backend', 'host');
             -- Equal times: only the rowid orders these two, and it must survive the rebuild.
             INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at)
               VALUES ('obs-b', 'host', 'healthy', 'First', 'manual', 1000, 2000, 1000),
                      ('obs-a', 'host', 'unhealthy', 'Second', 'manual', 1000, 2000, 1000);
             INSERT INTO observations (id, application_id, result, summary, source, observed_at, valid_until, recorded_at,
                                       http_url, http_expected_status, http_status, http_latency_ms, http_error, http_redirects)
               VALUES ('obs-http', 'shop', 'unhealthy', 'Timed out after 5 s', 'http', 1100, 2000, 1100,
                       'https://shop.example/', 200, NULL, 5000, 'Timed out after 5 s', '[]');
             INSERT INTO incidents (id, application_id, title, description, severity, status, source, opened_at, updated_at, resolved_at)
               VALUES ('inc-done', 'shop', 'Slow', '', 'warning', 'resolved', 'manual', 600, 650, 650),
                      ('inc', 'shop', 'Down', 'Checkout', 'critical', 'open', 'manual', 700, 700, NULL);
             INSERT INTO events (id, resource_id, kind, outcome, title, detail, source, observation_id, occurred_at, recorded_at)
               VALUES ('evt-b', 'host', 'observation', 'success', 'Observation: Healthy', 'First', 'manual', 'obs-b', 1000, 1000),
                      ('evt-a', 'host', 'observation', 'failure', 'Observation: Unhealthy', 'Second', 'manual', 'obs-a', 1000, 1000);
             INSERT INTO events (id, application_id, kind, outcome, title, detail, source, observation_id, incident_id, occurred_at, recorded_at)
               VALUES ('evt-http', 'shop', 'observation', 'failure', 'HTTP check: Unhealthy', 'Timed out after 5 s', 'http', 'obs-http', NULL, 1100, 1100),
                      ('evt-inc', 'shop', 'incident', 'failure', 'Incident opened: Down', '', 'manual', NULL, 'inc', 700, 700),
                      ('evt-note', 'shop', 'note', 'info', 'Noted', 'By hand', 'manual', NULL, NULL, 800, 800);
             INSERT INTO http_checks (application_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at, monitor, monitor_interval_minutes)
               VALUES ('shop', 'https://shop.example/', 200, 5, 15, 800, 'active', 5);",
        )
        .unwrap();
        let tables = ["resources", "applications", "components", "http_checks"];
        let before: Vec<_> = tables.iter().map(|table| dump(&db, table)).collect();
        let records_before: Vec<_> = RECORDS.iter().map(|table| dump(&db, table)).collect();
        drop(db);

        upgrade(&path).unwrap();
        let upgraded = list(&path).unwrap();
        assert_eq!(
            upgraded
                .observations
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["obs-http", "obs-a", "obs-b"]
        );
        assert!(upgraded
            .observations
            .iter()
            .all(|item| item.agent.is_none()));
        assert!(upgraded.incidents.iter().all(|item| item.agent.is_none()));
        assert!(upgraded.events.iter().all(|item| item.agent.is_none()));
        assert_eq!(upgraded.events.len(), 5);

        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        for (table, rows) in tables.iter().zip(&before) {
            assert_eq!(&dump(&db, table), rows, "{table}");
        }
        // Every row keeps its rowid, ID, source and values, and names no agent.
        for (table, rows) in RECORDS.iter().zip(&records_before) {
            assert_eq!(dump(&db, table), no_agent(rows), "{table}");
        }
        assert!(strings(&db, "SELECT \"table\" FROM pragma_foreign_key_check").is_empty());
        assert!(strings(&db, "SELECT name FROM sqlite_master WHERE name LIKE '%_v7'").is_empty());
        assert_eq!(
            strings(
                &db,
                "SELECT DISTINCT \"table\" || ' ' || on_delete FROM pragma_foreign_key_list('events') ORDER BY 1"
            ),
            [
                "applications RESTRICT",
                "incidents RESTRICT",
                "observations RESTRICT",
                "resources RESTRICT",
            ]
        );
        for table in ["observations", "incidents"] {
            assert_eq!(
                strings(
                    &db,
                    &format!("SELECT DISTINCT \"table\" || ' ' || on_delete FROM pragma_foreign_key_list('{table}') ORDER BY 1")
                ),
                ["applications RESTRICT", "resources RESTRICT"],
                "{table}"
            );
        }
        // Partial: they hold the reported records, by the person or an agent.
        assert_eq!(
            strings(
                &db,
                "SELECT name || ': ' || substr(sql, instr(sql, 'WHERE')) FROM sqlite_master WHERE name LIKE '%_reported' ORDER BY name"
            ),
            [
                "events_reported: WHERE source IN ('manual', 'agent')",
                "observations_reported: WHERE source IN ('manual', 'agent')",
            ]
        );
        assert!(strings(
            &db,
            "SELECT name FROM sqlite_master WHERE name LIKE '%_manual'"
        )
        .is_empty());
        drop(db);
        // The rebuilt tables enforce their relationships and the new rules.
        let db = open(&path).unwrap();
        assert!(db
            .execute("DELETE FROM observations WHERE id = 'obs-a'", [])
            .is_err());
        assert!(db
            .execute("DELETE FROM incidents WHERE id = 'inc'", [])
            .is_err());
        let incident = |source: &str, agent: Option<&str>| {
            db.execute(
                "INSERT INTO incidents (id, application_id, title, severity, status, source, opened_at, updated_at, agent)
                 VALUES (?1, 'shop', 'Reported', 'info', 'open', ?2, 1, 1, ?3)",
                rusqlite::params![uuid::Uuid::new_v4().to_string(), source, agent],
            )
        };
        assert!(incident("http", None).is_err(), "a check opens no incident");
        assert!(incident("agent", None).is_err(), "an agent names itself");
        assert!(incident("manual", Some("codex")).is_err());
        assert!(incident("agent", Some("codex")).is_ok());
        let observation = |source: &str, agent: Option<&str>| {
            db.execute(
                "INSERT INTO observations (id, application_id, result, summary, source, observed_at, valid_until, recorded_at, agent)
                 VALUES (?1, 'shop', 'healthy', 'Looked fine', ?2, 1, 2, 1, ?3)",
                rusqlite::params![uuid::Uuid::new_v4().to_string(), source, agent],
            )
        };
        assert!(observation("agent", None).is_err());
        assert!(
            observation("agent", Some(&"a".repeat(121))).is_err(),
            "name too long"
        );
        assert!(observation("agent", Some("claude-code")).is_ok());
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn refuses_a_v7_upgrade_that_would_keep_broken_relationships() {
        let path = nothing();
        let db = database_at(&path, 6);
        // An event of an incident that no longer exists, written with enforcement off.
        db.execute_batch(
            "INSERT INTO applications (id, name, lifecycle) VALUES ('shop', 'Shop', 'production');
             INSERT INTO events (id, application_id, kind, outcome, title, source, incident_id, occurred_at, recorded_at)
               VALUES ('orphan', 'shop', 'incident', 'failure', 'Incident opened: Gone', 'manual', 'gone', 1, 1);",
        )
        .unwrap();
        drop(db);
        let error = upgrade(&path).unwrap_err();
        assert_eq!(error.code, "database_error");
        assert!(
            error.message.contains("1 broken relationships"),
            "{}",
            error.message
        );
        // The whole upgrade was rolled back.
        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 6);
        assert_eq!(strings(&db, "SELECT id FROM events"), ["orphan"]);
        assert!(strings(
            &db,
            "SELECT name FROM pragma_table_info('events') WHERE name = 'agent'"
        )
        .is_empty());
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn upgrades_v7_closed_to_agents_without_changing_any_row() {
        let path = nothing();
        let db = database_at(&path, 7);
        db.execute_batch(
            "INSERT INTO resources (id, name, kind, description, operating_system) VALUES ('host', 'Host', 'server', '', 'Linux');
             INSERT INTO applications (id, name, lifecycle, description, version) VALUES ('shop', 'Shop', 'production', 'Store', '2.0');
             INSERT INTO components (id, application_id, name, kind, resource_id) VALUES ('api', 'shop', 'API', 'backend', 'host');
             INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at, agent)
               VALUES ('obs-agent', 'host', 'healthy', 'Looked fine', 'agent', 500, 900, 600, 'claude-code');
             INSERT INTO events (id, resource_id, kind, outcome, title, detail, source, observation_id, occurred_at, recorded_at, agent)
               VALUES ('evt-agent', 'host', 'observation', 'success', 'Observation: Healthy', 'Looked fine', 'agent', 'obs-agent', 500, 600, 'claude-code');
             INSERT INTO incidents (id, application_id, title, description, severity, status, source, opened_at, updated_at)
               VALUES ('inc', 'shop', 'Down', '', 'critical', 'open', 'manual', 700, 700);
             INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at, monitor, monitor_interval_minutes)
               VALUES ('host', 'http://host/health', 200, 5, 15, 800, 'active', 5);",
        )
        .unwrap();
        let tables = [
            "resources",
            "applications",
            "components",
            "observations",
            "incidents",
            "events",
            "http_checks",
        ];
        let before: Vec<_> = tables.iter().map(|table| dump(&db, table)).collect();
        drop(db);

        // Upgraded, the inventory is closed to agents until the person opens it.
        upgrade(&path).unwrap();
        assert_eq!(crate::access::agent_access(&path).unwrap().mode, "off");
        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        for (table, rows) in tables.iter().zip(&before) {
            assert_eq!(&dump(&db, table), rows, "{table}");
        }
        assert!(dump(&db, "agent_access").is_empty());
        drop(db);
        let db = open(&path).unwrap();
        assert!(db
            .execute("INSERT INTO agent_access VALUES (1, 'admin', 0)", [])
            .is_err());
        assert!(db
            .execute("INSERT INTO agent_access VALUES (1, 'read', 0)", [])
            .is_ok());
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn upgrades_v8_with_an_empty_agent_log_without_changing_any_row() {
        let path = nothing();
        let db = database_at(&path, 8);
        db.execute_batch(
            "INSERT INTO resources (id, name, kind, description, operating_system) VALUES ('host', 'Host', 'server', '', 'Linux');
             INSERT INTO applications (id, name, lifecycle, description, version) VALUES ('shop', 'Shop', 'production', 'Store', '2.0');
             INSERT INTO components (id, application_id, name, kind, resource_id) VALUES ('api', 'shop', 'API', 'backend', 'host');
             INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at, agent)
               VALUES ('obs-agent', 'host', 'healthy', 'Looked fine', 'agent', 500, 900, 600, 'claude-code');
             INSERT INTO events (id, resource_id, kind, outcome, title, detail, source, observation_id, occurred_at, recorded_at, agent)
               VALUES ('evt-agent', 'host', 'observation', 'success', 'Observation: Healthy', 'Looked fine', 'agent', 'obs-agent', 500, 600, 'claude-code');
             INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at)
               VALUES ('host', 'http://host/health', 200, 5, 15, 800);
             INSERT INTO agent_access VALUES (1, 'write', 900);",
        )
        .unwrap();
        let tables = [
            "resources",
            "applications",
            "components",
            "observations",
            "incidents",
            "events",
            "http_checks",
            "agent_access",
        ];
        let before: Vec<_> = tables.iter().map(|table| dump(&db, table)).collect();
        drop(db);

        upgrade(&path).unwrap();
        list(&path).unwrap();
        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        for (table, rows) in tables.iter().zip(&before) {
            assert_eq!(&dump(&db, table), rows, "{table}");
        }
        // v9's agent_log, empty, became v10's change log (see its own test).
        assert!(dump(&db, "change_log").is_empty());
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    /// A database at v9 with two agent_log entries, one about a system that is gone.
    fn v9_with_agent_log(path: &Path, systems: bool) -> Connection {
        let db = database_at(path, 9);
        if systems {
            db.execute_batch(
                "INSERT INTO resources (id, name, kind, description, operating_system) VALUES ('host', 'Host', 'server', '', 'Linux');
                 INSERT INTO applications (id, name, lifecycle, description, version) VALUES ('shop', 'Shop', 'production', 'Store', '2.0');
                 INSERT INTO components (id, application_id, name, kind, resource_id) VALUES ('api', 'shop', 'API', 'backend', 'host');
                 INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at)
                   VALUES ('host', 'http://host/health', 200, 5, 15, 800);
                 INSERT INTO agent_access VALUES (1, 'write', 900);",
            )
            .unwrap();
        }
        // Out of order, so that keeping each rowid (the tie-break at equal times) is visible.
        db.execute_batch(
            "INSERT INTO agent_log (rowid, id, application_id, resource_id, subject_name, agent, action, summary, occurred_at)
               VALUES (7, 'log-version', 'shop', NULL, 'Shop', 'claude-code', 'updated', 'Updated application «Shop»: version «1.9» → «2.0»', 1000);
             INSERT INTO agent_log (rowid, id, application_id, resource_id, subject_name, agent, action, summary, occurred_at)
               VALUES (3, 'log-gone', NULL, 'spare', 'Spare box', 'codex', 'deleted', 'Deleted resource «Spare box», which had no history', 1000);",
        )
        .unwrap();
        db
    }

    /// SQLite's clock now, read as migration 010 reads it to stamp `change_log_start`. Only this
    /// clock can bound that stamp: on Windows SQLite reads the system time that moves once per
    /// timer tick (about 15.6 ms), behind the precise time `records::now_ms` reads, and anywhere it
    /// divides by 1000 in floating point, which the cast back to milliseconds may truncate by one.
    /// Both put the stamp before the precise time; read the same way, it never goes backwards.
    fn sqlite_now_ms() -> i64 {
        Connection::open_in_memory()
            .unwrap()
            .query_row(
                "SELECT CAST(unixepoch('subsec') * 1000 AS INTEGER)",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn upgrades_v9_agent_log_into_the_change_log_keeping_every_row() {
        let path = nothing();
        let db = v9_with_agent_log(&path, true);
        let tables = [
            "resources",
            "applications",
            "components",
            "observations",
            "incidents",
            "events",
            "http_checks",
            "agent_access",
        ];
        let before: Vec<_> = tables.iter().map(|table| dump(&db, table)).collect();
        let log = dump(&db, "agent_log");
        drop(db);

        let started = sqlite_now_ms();
        upgrade(&path).unwrap();
        let upgraded = list(&path).unwrap();
        let finished = sqlite_now_ms();
        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        for (table, rows) in tables.iter().zip(&before) {
            assert_eq!(&dump(&db, table), rows, "{table}");
        }
        // Every entry, with its rowid and values, as an agent's change with only its summary.
        let agent = format!("{:?}", rusqlite::types::ValueRef::Text(b"agent"));
        let carried: Vec<String> = log
            .iter()
            .map(|row| {
                let mut values: Vec<&str> = row.split(" | ").collect();
                // rowid, id, application_id, resource_id, subject_name, [source], agent, action,
                // summary, [changes], occurred_at.
                values.insert(5, &agent);
                values.insert(9, "Null");
                values.join(" | ")
            })
            .collect();
        assert_eq!(dump(&db, "change_log"), carried);
        assert!(strings(
            &db,
            "SELECT name FROM sqlite_master WHERE name = 'agent_log'"
        )
        .is_empty());
        // Nothing references or is referenced: an entry never keeps a subject from going.
        assert!(strings(
            &db,
            "SELECT \"table\" FROM pragma_foreign_key_list('change_log')"
        )
        .is_empty());
        assert_eq!(
            strings(&db, "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'change_log' AND name NOT LIKE 'sqlite_autoindex%' ORDER BY name"),
            [
                "change_log_application_id",
                "change_log_application_latest",
                "change_log_resource_id",
                "change_log_resource_latest"
            ]
        );
        // The inventory held systems whose earlier changes by the person left no trace: the log
        // is complete from the upgrade on, and the snapshot says so.
        let since: i64 = db
            .query_row("SELECT started_at FROM change_log_start", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!((started..=finished).contains(&since), "{since}");
        assert_eq!(upgraded.changes_since, records::format_time(since));
        let ids: Vec<_> = upgraded
            .changes
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(ids, ["log-version", "log-gone"]);
        assert!(upgraded
            .changes
            .iter()
            .all(|item| item.source == "agent" && item.changes.is_none()));
        drop(db);

        let db = open(&path).unwrap();
        let entry = |subject: (Option<&str>, Option<&str>),
                     source: &str,
                     agent: Option<&str>,
                     action: &str,
                     changes: Option<&str>| {
            db.execute(
                "INSERT INTO change_log (id, application_id, resource_id, subject_name, source, agent, action, summary, changes, occurred_at)
                 VALUES (?1, ?2, ?3, 'Shop', ?4, ?5, ?6, 'Changed', ?7, 1)",
                rusqlite::params![Uuid::new_v4().to_string(), subject.0, subject.1, source, agent, action, changes],
            )
        };
        let shop = (Some("shop"), None);
        assert!(
            entry(
                (Some("shop"), Some("host")),
                "manual",
                None,
                "updated",
                None
            )
            .is_err(),
            "two subjects"
        );
        assert!(
            entry((None, None), "manual", None, "updated", None).is_err(),
            "no subject"
        );
        assert!(
            entry(shop, "agent", None, "updated", None).is_err(),
            "an agent names itself"
        );
        assert!(
            entry(shop, "agent", Some(" "), "updated", None).is_err(),
            "with a name"
        );
        assert!(
            entry(shop, "manual", Some("codex"), "updated", None).is_err(),
            "the person is no agent"
        );
        assert!(
            entry(shop, "http", None, "updated", None).is_err(),
            "only the person or an agent changes configuration"
        );
        assert!(
            entry(shop, "manual", None, "dropped", None).is_err(),
            "unknown action"
        );
        assert!(
            entry(shop, "manual", None, "updated", Some("not json")).is_err(),
            "changes are JSON"
        );
        assert!(
            entry(shop, "manual", None, "updated", Some(r#"{"field":"name"}"#)).is_err(),
            "a list of field changes"
        );
        assert!(entry(
            shop,
            "manual",
            None,
            "updated",
            Some(r#"[{"field":"name","before":"A","after":"B"}]"#)
        )
        .is_ok());
        // A deleted subject's entry stays.
        assert!(entry(
            (Some("gone"), None),
            "agent",
            Some("codex"),
            "deleted",
            None
        )
        .is_ok());
        assert!(db
            .execute("INSERT INTO change_log_start VALUES (2, 0)", [])
            .is_err());
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_change_log_with_nothing_earlier_to_miss_is_complete() {
        // An inventory with no systems when v10 arrives: whatever changes it had concerned
        // systems that are gone, and agents' were logged, so nothing is missing from now on.
        let path = nothing();
        drop(v9_with_agent_log(&path, false));
        upgrade(&path).unwrap();
        let upgraded = list(&path).unwrap();
        assert_eq!(upgraded.changes_since, None);
        assert_eq!(upgraded.changes.len(), 2);
        let db = Connection::open(&path).unwrap();
        assert!(dump(&db, "change_log_start").is_empty());
        drop(db);
        std::fs::remove_file(&path).unwrap();
        // A new inventory starts with every migration at once, and its log is complete.
        start(&path).unwrap();
        let fresh = list(&path).unwrap();
        assert_eq!(fresh.changes_since, None);
        assert!(fresh.changes.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn upgrades_v10_with_references_without_changing_any_row() {
        let path = nothing();
        let db = database_at(&path, 10);
        db.execute_batch(
            "INSERT INTO resources (id, name, kind, description, operating_system)
               VALUES ('host', 'Host', 'server', '', 'Linux'), ('laptop', 'Laptop', 'workstation', '', 'Windows 11');
             INSERT INTO applications (id, name, lifecycle, description, version) VALUES ('shop', 'Shop', 'production', 'Store', '2.0');
             INSERT INTO components (id, application_id, name, kind, resource_id) VALUES ('api', 'shop', 'API', 'backend', 'host');
             INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at, agent)
               VALUES ('obs-agent', 'host', 'healthy', 'Looked fine', 'agent', 500, 900, 600, 'claude-code');
             INSERT INTO events (id, resource_id, kind, outcome, title, detail, source, observation_id, occurred_at, recorded_at, agent)
               VALUES ('evt-agent', 'host', 'observation', 'success', 'Observation: Healthy', 'Looked fine', 'agent', 'obs-agent', 500, 600, 'claude-code');
             INSERT INTO incidents (id, application_id, title, description, severity, status, source, opened_at, updated_at)
               VALUES ('inc', 'shop', 'Down', '', 'critical', 'open', 'manual', 700, 700);
             INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at, monitor, monitor_interval_minutes)
               VALUES ('host', 'http://host/health', 200, 5, 15, 800, 'active', 5);
             INSERT INTO agent_access VALUES (1, 'write', 900);
             INSERT INTO change_log (id, application_id, resource_id, subject_name, source, agent, action, summary, changes, occurred_at)
               VALUES ('change-version', 'shop', NULL, 'Shop', 'manual', NULL, 'updated', 'Updated application «Shop»: version «1.9» → «2.0»',
                       '[{\"field\":\"version\",\"before\":\"1.9\",\"after\":\"2.0\"}]', 1000),
                      ('change-host', NULL, 'host', 'Host', 'agent', 'codex', 'created', 'Added resource «Host» (server, Linux)', NULL, 400);
             INSERT INTO change_log_start VALUES (1, 300);",
        )
        .unwrap();
        let tables = [
            "resources",
            "applications",
            "components",
            "observations",
            "incidents",
            "events",
            "http_checks",
            "agent_access",
            "change_log",
            "change_log_start",
        ];
        let before: Vec<_> = tables.iter().map(|table| dump(&db, table)).collect();
        drop(db);

        upgrade(&path).unwrap();
        let upgraded = list(&path).unwrap();
        assert!(upgraded
            .applications
            .iter()
            .all(|item| item.references.is_empty()));
        assert!(upgraded
            .resources
            .iter()
            .all(|item| item.references.is_empty()));
        assert!(upgraded.shared_repositories.is_empty());
        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        for (table, rows) in tables.iter().zip(&before) {
            assert_eq!(&dump(&db, table), rows, "{table}");
        }
        assert!(dump(&db, "system_references").is_empty());
        // Each reference goes with its system; a machine holding a path stays.
        assert_eq!(
            strings(
                &db,
                "SELECT \"from\" || ' → ' || \"table\" || ' ' || on_delete FROM pragma_foreign_key_list('system_references') ORDER BY 1"
            ),
            [
                "application_id → applications CASCADE",
                "machine_id → resources RESTRICT",
                "resource_id → resources CASCADE",
            ]
        );
        assert_eq!(
            strings(&db, "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'system_references' AND name NOT LIKE 'sqlite_autoindex%' ORDER BY name"),
            [
                "system_references_application_id",
                "system_references_machine_id",
                "system_references_resource_id",
            ]
        );
        drop(db);

        // The schema holds what SQL can say on its own, whatever wrote the row.
        let db = open(&path).unwrap();
        let reference = |subject: (Option<&str>, Option<&str>),
                         kind: &str,
                         target: &str,
                         label: Option<&str>,
                         machine: Option<&str>| {
            db.execute(
                "INSERT INTO system_references (id, application_id, resource_id, kind, target, label, machine_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![Uuid::new_v4().to_string(), subject.0, subject.1, kind, target, label, machine],
            )
        };
        let shop = (Some("shop"), None);
        let refused = [
            (
                reference(
                    (Some("shop"), Some("host")),
                    "dashboard",
                    "https://x.example",
                    None,
                    None,
                ),
                "two systems",
            ),
            (
                reference((None, None), "dashboard", "https://x.example", None, None),
                "no system",
            ),
            (
                reference(
                    (Some("gone"), None),
                    "dashboard",
                    "https://x.example",
                    None,
                    None,
                ),
                "a system that is gone",
            ),
            (
                reference(shop, "wiki", "https://x.example", None, None),
                "unknown kind",
            ),
            (reference(shop, "dashboard", "", None, None), "no target"),
            (
                reference(shop, "dashboard", " https://x.example", None, None),
                "padded target",
            ),
            (
                reference(
                    shop,
                    "logs",
                    &format!("/{}", "a".repeat(2048)),
                    None,
                    Some("host"),
                ),
                "target too long",
            ),
            (
                reference(shop, "dashboard", "https://x.example", Some(""), None),
                "empty label",
            ),
            (
                reference(shop, "dashboard", "https://x.example", Some("Ops "), None),
                "padded label",
            ),
            (
                reference(
                    shop,
                    "dashboard",
                    "https://x.example",
                    Some(&"a".repeat(121)),
                    None,
                ),
                "label too long",
            ),
            (
                reference(shop, "logs", "https://logs.example", None, Some("host")),
                "a URL on a machine",
            ),
            (
                reference(shop, "logs", "/var/log/shop", None, None),
                "a path on no machine",
            ),
            (
                reference(shop, "logs", "logs/shop", None, Some("host")),
                "a relative path",
            ),
            (
                reference(shop, "repository", "/srv/git/shop", None, Some("host")),
                "a repository path",
            ),
            (
                reference(shop, "dashboard", "/srv/grafana", None, Some("host")),
                "a dashboard path",
            ),
            (
                reference(shop, "local_copy", "https://github.com/me/shop", None, None),
                "a local copy URL",
            ),
            (
                reference(shop, "logs", "/var/log/shop", None, Some("ghost")),
                "a machine that is not there",
            ),
        ];
        for (result, why) in refused {
            assert!(result.is_err(), "{why}");
        }
        for (subject, kind, target, machine) in [
            (shop, "repository", "https://github.com/me/shop", None),
            (shop, "documentation", "HTTPS://Docs.example/shop", None),
            (shop, "local_copy", r"C:\src\shop", Some("laptop")),
            (shop, "local_copy", "c:/src/shop", Some("laptop")),
            (shop, "logs", r"\\nas\logs\shop", Some("host")),
            (
                (None, Some("host")),
                "logs",
                "/var/log/syslog",
                Some("host"),
            ),
        ] {
            assert!(
                reference(subject, kind, target, None, machine).is_ok(),
                "{kind} {target}"
            );
        }
        // A machine that holds a path stays; deleting a system takes its references with it.
        assert!(db
            .execute("DELETE FROM resources WHERE id = 'laptop'", [])
            .is_err());
        db.execute("DELETE FROM incidents", []).unwrap();
        db.execute("DELETE FROM applications WHERE id = 'shop'", [])
            .unwrap();
        assert_eq!(dump(&db, "system_references").len(), 1);
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn upgrades_v11_with_copy_readings_without_changing_any_row() {
        let path = nothing();
        let db = database_at(&path, 11);
        db.execute_batch(
            "INSERT INTO resources (id, name, kind, description, operating_system)
               VALUES ('host', 'Host', 'server', '', 'Linux'), ('laptop', 'Laptop', 'workstation', '', 'Windows 11');
             INSERT INTO applications (id, name, lifecycle, description, version) VALUES ('shop', 'Shop', 'production', 'Store', '2.0');
             INSERT INTO components (id, application_id, name, kind, resource_id) VALUES ('api', 'shop', 'API', 'backend', 'host');
             INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at, agent)
               VALUES ('obs-agent', 'host', 'healthy', 'Looked fine', 'agent', 500, 900, 600, 'claude-code');
             INSERT INTO events (id, resource_id, kind, outcome, title, detail, source, observation_id, occurred_at, recorded_at, agent)
               VALUES ('evt-agent', 'host', 'observation', 'success', 'Observation: Healthy', 'Looked fine', 'agent', 'obs-agent', 500, 600, 'claude-code');
             INSERT INTO incidents (id, application_id, title, description, severity, status, source, opened_at, updated_at)
               VALUES ('inc', 'shop', 'Down', '', 'critical', 'open', 'manual', 700, 700);
             INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at, monitor, monitor_interval_minutes)
               VALUES ('host', 'http://host/health', 200, 5, 15, 800, 'active', 5);
             INSERT INTO agent_access VALUES (1, 'write', 900);
             INSERT INTO change_log (id, application_id, resource_id, subject_name, source, agent, action, summary, changes, occurred_at)
               VALUES ('change-version', 'shop', NULL, 'Shop', 'manual', NULL, 'updated', 'Updated application «Shop»: version «1.9» → «2.0»',
                       '[{\"field\":\"version\",\"before\":\"1.9\",\"after\":\"2.0\"}]', 1000);
             INSERT INTO change_log_start VALUES (1, 300);
             INSERT INTO system_references (id, application_id, resource_id, kind, target, label, machine_id)
               VALUES ('shop-repo', 'shop', NULL, 'repository', 'https://github.com/me/shop', NULL, NULL),
                      ('shop-copy', 'shop', NULL, 'local_copy', 'C:\\src\\shop', 'Work', 'laptop'),
                      ('host-logs', NULL, 'host', 'logs', '/var/log/syslog', NULL, 'host');",
        )
        .unwrap();
        let tables = [
            "resources",
            "applications",
            "components",
            "observations",
            "incidents",
            "events",
            "http_checks",
            "agent_access",
            "change_log",
            "change_log_start",
            "system_references",
        ];
        let before: Vec<_> = tables.iter().map(|table| dump(&db, table)).collect();
        drop(db);

        upgrade(&path).unwrap();
        let upgraded = list(&path).unwrap();
        // The declared copy is there, never read.
        assert_eq!(upgraded.local_copies.len(), 1);
        assert_eq!(upgraded.local_copies[0].reference_id, "shop-copy");
        assert_eq!(upgraded.local_copies[0].latest, None);
        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        for (table, rows) in tables.iter().zip(&before) {
            assert_eq!(&dump(&db, table), rows, "{table}");
        }
        assert!(dump(&db, "copy_readings").is_empty());
        // A reading keeps its system (RESTRICT) and only copies what it read: no other key.
        assert_eq!(
            strings(
                &db,
                "SELECT \"from\" || ' → ' || \"table\" || ' ' || on_delete FROM pragma_foreign_key_list('copy_readings') ORDER BY 1"
            ),
            [
                "application_id → applications RESTRICT",
                "resource_id → resources RESTRICT",
            ]
        );
        assert_eq!(
            strings(&db, "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'copy_readings' AND name NOT LIKE 'sqlite_autoindex%' ORDER BY name"),
            [
                "copy_readings_application_id",
                "copy_readings_reference_id",
                "copy_readings_resource_id",
            ]
        );
        drop(db);
        // Its reference can go, and its machine with it, without touching the reading.
        let db = open(&path).unwrap();
        db.execute(
            "INSERT INTO copy_readings (id, application_id, reference_id, machine_id, path, source, reader, observed_at, recorded_at, outcome, detail)
             VALUES ('reading', 'shop', 'shop-copy', 'laptop', 'C:\\src\\shop', 'read', 'libgit2 1.9.7', 1000, 1000, 'missing', 'Nothing is at this path.')",
            [],
        )
        .unwrap();
        db.execute("DELETE FROM system_references WHERE id = 'shop-copy'", [])
            .unwrap();
        db.execute("DELETE FROM resources WHERE id = 'laptop'", [])
            .unwrap();
        assert_eq!(dump(&db, "copy_readings").len(), 1);
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    /// The agent gate's reading connections never create a file and cannot write; its writing one
    /// writes only an inventory a reading look found current, and never converts it.
    #[test]
    fn opens_an_inventory_as_it_finds_it() {
        let path = nothing();
        // Nothing there: no file, also when a connection itself is asked.
        assert_eq!(examine(&path, |_| Ok(())).0, Found::Missing);
        for way in [Way::Shared, Way::Snapshot] {
            assert!(connect(&path, way).is_err(), "{way:?}");
        }
        assert!(!path.exists());
        let refused = read_as_found(&path, |_| Ok(())).unwrap_err();
        assert!(refused.message.contains("There is no DevHQ inventory at"));
        assert!(open_to_write(&path).is_err());
        assert!(!path.exists());
        start(&path).unwrap();
        resource(&path, "Host");
        let refused = read_as_found(&path, |db| {
            db.execute("DELETE FROM resources", [])
                .map_err(ApiError::database)
        })
        .unwrap_err();
        assert!(refused.message.contains("readonly"), "{}", refused.message);
        let writer = open_to_write(&path).unwrap();
        assert_eq!(writer.execute("DELETE FROM resources", []).unwrap(), 1);
        drop(writer);
        // Never converted: a rollback journal stays one, also after a write.
        let db = Connection::open(&path).unwrap();
        db.pragma_update(None, "journal_mode", "DELETE").unwrap();
        drop(db);
        let refused = read_as_found(&path, |db| {
            db.execute("DELETE FROM agent_access", [])
                .map_err(ApiError::database)
        })
        .unwrap_err();
        assert!(refused.message.contains("readonly"), "{}", refused.message);
        open_to_write(&path)
            .unwrap()
            .execute("DELETE FROM agent_access", [])
            .unwrap();
        let mode: String = Connection::open(&path)
            .unwrap()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "delete");
        std::fs::remove_file(path).unwrap();
    }

    /// Only a file with this schema's tables, columns and indexes is current: declaring its version
    /// is not enough, and what differs is named. Tables and indexes of its own are not DevHQ's to
    /// judge.
    #[test]
    fn tells_a_current_schema_from_one_that_only_declares_it() {
        let found_after = |sql: &str| {
            let path = nothing();
            let db = database_at(&path, SCHEMA_VERSION);
            db.execute_batch(sql).unwrap();
            drop(db);
            let found = examine(&path, |_| Ok(())).0;
            std::fs::remove_file(path).unwrap();
            found
        };
        assert_eq!(found_after(""), Found::Current);
        assert_eq!(
            found_after("CREATE TABLE notes (body TEXT); CREATE INDEX notes_body ON notes (body);"),
            Found::Current
        );
        assert_eq!(
            found_after("DROP TABLE agent_access"),
            Found::Mismatch(vec!["no table agent_access".to_owned()])
        );
        assert_eq!(
            found_after("ALTER TABLE resources ADD COLUMN extra TEXT"),
            Found::Mismatch(vec![
                "column resources.extra, which the schema does not have".to_owned()
            ])
        );
        assert_eq!(
            found_after("DROP INDEX events_reported"),
            Found::Mismatch(vec!["no index events_reported".to_owned()])
        );
        // Stamped 13 with only the two tables that tell a DevHQ inventory apart.
        let stamped = nothing();
        let db = Connection::open(&stamped).unwrap();
        db.execute_batch(
            "CREATE TABLE applications (id TEXT PRIMARY KEY);
             CREATE TABLE resources (id TEXT PRIMARY KEY);
             PRAGMA user_version = 13;",
        )
        .unwrap();
        drop(db);
        let found = examine(&stamped, |_| Ok(())).0;
        let Found::Mismatch(differences) = &found else {
            panic!("{found:?}")
        };
        assert!(differences.contains(&"no table agent_access".to_owned()));
        assert!(differences.contains(&"no column resources.name".to_owned()));
        assert_eq!(
            (found.state(), found.stored(), found.structure()),
            ("mismatch", Some(SCHEMA_VERSION), Some("differs"))
        );
        assert_eq!(found.refusal(&stamped).code, "schema_mismatch");
        std::fs::remove_file(stamped).unwrap();
        // Upgraded by the window from an older schema, it has the same structure as a new one.
        let upgraded = nothing();
        drop(database_at(&upgraded, 6));
        drop(open(&upgraded).unwrap());
        assert_eq!(examine(&upgraded, |_| Ok(())).0, Found::Current);
        std::fs::remove_file(upgraded).unwrap();
    }

    /// The gate gives SQLite the path as a URI, the only way to ask it to read without writing:
    /// every byte that a URI reserves stays part of the path.
    #[test]
    fn gives_sqlite_any_path_as_a_uri() {
        #[cfg(windows)]
        {
            let uri = |path: &str| uri(Path::new(path), "immutable=1").unwrap();
            assert_eq!(
                uri(r"C:\Data 1\a#b%c?d é\inventory.sqlite3"),
                "file:///C:/Data%201/a%23b%25c%3Fd%20%C3%A9/inventory.sqlite3?immutable=1"
            );
            assert_eq!(
                uri(r"\\?\C:\Data\inventory.sqlite3"),
                "file:///C:/Data/inventory.sqlite3?immutable=1"
            );
            assert_eq!(
                uri(r"\\?\UNC\server\share\inventory.sqlite3"),
                "file:////server/share/inventory.sqlite3?immutable=1"
            );
            assert_eq!(
                uri(r"\\server\share\inventory.sqlite3"),
                "file:////server/share/inventory.sqlite3?immutable=1"
            );
            assert_eq!(
                uri("inventory.sqlite3"),
                "file:inventory.sqlite3?immutable=1"
            );
        }
        #[cfg(unix)]
        {
            let uri = |path: &str| uri(Path::new(path), "readonly_shm=1").unwrap();
            assert_eq!(
                uri("/home/me/a#b%c?d é/inventory.sqlite3"),
                "file:///home/me/a%23b%25c%3Fd%20%C3%A9/inventory.sqlite3?readonly_shm=1"
            );
            assert_eq!(
                uri("inventory.sqlite3"),
                "file:inventory.sqlite3?readonly_shm=1"
            );
        }
        // A real folder with such a name, through both ways of reading.
        let name = if cfg!(windows) {
            "a b#c%d é"
        } else {
            "a b#c%d?e é"
        };
        let folder = std::env::temp_dir().join(format!("devhq-{name}-{}", Uuid::new_v4()));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("inventory.sqlite3");
        start(&path).unwrap();
        resource(&path, "Host");
        let count = |path: &Path| {
            read_as_found(path, |db| {
                db.query_row("SELECT count(*) FROM resources", [], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(ApiError::database)
            })
        };
        assert_eq!(count(&path).unwrap(), 1, "a snapshot of a logged file");
        Connection::open(&path)
            .unwrap()
            .pragma_update(None, "journal_mode", "DELETE")
            .unwrap();
        assert_eq!(count(&path).unwrap(), 1, "a rollback-journal file, shared");
        let names: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["inventory.sqlite3"]);
        std::fs::remove_dir_all(folder).unwrap();
    }

    /// A snapshot read counts only if no writer changed the file meanwhile: one that did is read
    /// again, and what is answered is what the file holds after that write.
    #[test]
    fn reads_a_snapshot_again_when_a_writer_changed_the_file_meanwhile() {
        let path = path();
        resource(&path, "Host");
        let mut reads = 0;
        let (found, outcome) = examine(&path, |db| {
            reads += 1;
            let count: i64 = db
                .query_row("SELECT count(*) FROM resources", [], |row| row.get(0))
                .map_err(ApiError::database)?;
            if reads == 1 {
                // Its own connection writes, folds the log into the file and deletes it.
                resource(&path, "Written meanwhile");
            }
            Ok(count)
        });
        assert_eq!(found, Found::Current);
        assert_eq!((reads, outcome.unwrap().unwrap()), (2, 2));
        std::fs::remove_file(path).unwrap();
    }

    /// Two agent sessions with the window closed: one's write ends while the other's gate reads.
    /// The writer's connection is not the last to close, so SQLite does not fold its log, and the
    /// reader's, read-only, cannot: without `empty_log` the change stayed in a log nothing held,
    /// and every gate refused the inventory with `needs_recovery` until the window started. The
    /// change is in the file and the log is empty; a write by the person still leaves it to SQLite.
    #[test]
    fn an_agents_write_leaves_no_changes_in_a_log_that_another_read_outlives() {
        let folder = Scratch::new("outlived");
        let path = folder.0.join("inventory.sqlite3");
        start(&path).unwrap();
        access::set_agent_access(&path, "write").unwrap();
        let release = folder.0.join("release");
        for (by, name) in [
            (Actor::agent("Claude Code").unwrap(), "Agent's box"),
            (Actor::Person, "Person's box"),
        ] {
            let mut db = open_current(&path).unwrap();
            let tx = begin_write(&mut db, &by).unwrap();
            // The other session's gate looks while this connection holds the log, so it reads
            // beside it, and goes on holding its connection until it is released.
            let reader = reader(&path, &release, "connection");
            let input = ResourceInput {
                name: name.into(),
                ..host()
            };
            save_resource_in(&tx, input, &by).unwrap();
            tx.commit().unwrap();
            drop(db);
            std::fs::write(&release, "").unwrap();
            assert!(reader.finished(), "{name}");
            std::fs::remove_file(&release).unwrap();
            let log = std::fs::metadata(beside(&path, "-wal")).map_or(0, |log| log.len());
            let (found, names) = examine(&path, |db| {
                let mut statement = db.prepare("SELECT name FROM resources").unwrap();
                let names = statement
                    .query_map([], |row| row.get::<_, String>(0))
                    .unwrap();
                Ok(names.collect::<Result<Vec<_>, _>>().unwrap())
            });
            if let Actor::Agent(_) = by {
                assert_eq!(log, 0);
                assert_eq!(found, Found::Current);
                assert_eq!(names.unwrap().unwrap(), [name]);
            } else {
                // As before: the window recovers it when it starts.
                assert!(log > LOG_HEADER, "{log}");
                assert_eq!(found, Found::NeedsRecovery(Recovery::Log));
                start(&path).unwrap();
                assert_eq!(list(&path).unwrap().resources.len(), 2);
            }
        }
    }

    /// A gate's read in the middle of its transaction while an agent's write commits, as `list_from`
    /// is for its milliseconds: unlike an open connection, it keeps the log in use, so `empty_log`
    /// cannot empty it until the read ends. Released while the commit waits, the read ends, the log
    /// is emptied before the writer's connection closes, and the gate reads the inventory; without
    /// the wait, the change stayed in a log nothing held once the reader closed after the writer.
    #[test]
    fn an_agents_write_waits_for_a_read_in_progress_and_then_empties_the_log() {
        let folder = Scratch::new("read-ends");
        let path = folder.0.join("inventory.sqlite3");
        start(&path).unwrap();
        access::set_agent_access(&path, "write").unwrap();
        let agent = Actor::agent("Claude Code").unwrap();
        let release = folder.0.join("release");
        let mut db = open_current(&path).unwrap();
        let tx = begin_write(&mut db, &agent).unwrap();
        let reader = reader(&path, &release, "transaction");
        let input = ResourceInput {
            name: "Agent's box".into(),
            ..host()
        };
        save_resource_in(&tx, input, &agent).unwrap();
        // The read ends a moment into the commit's wait, well before the patience runs out.
        let held = Duration::from_millis(400);
        let releasing = {
            let release = release.clone();
            std::thread::spawn(move || {
                std::thread::sleep(held);
                std::fs::write(release, "").unwrap();
            })
        };
        let committing = std::time::Instant::now();
        tx.commit().unwrap();
        let waited = committing.elapsed();
        drop(db);
        releasing.join().unwrap();
        assert!(reader.finished());
        assert!(waited >= held, "{waited:?}");
        assert!(waited < BUSY_TIMEOUT, "{waited:?}");
        let log = std::fs::metadata(beside(&path, "-wal")).map_or(0, |log| log.len());
        assert_eq!(log, 0);
        let (found, names) = examine(&path, |db| Ok(list_from(db)?.resources.len()));
        assert_eq!(found, Found::Current);
        assert_eq!(names.unwrap().unwrap(), 1);
    }

    /// A read that outlasts the patience: `empty_log` gives up without an error, the write it
    /// followed stays committed, and while it waits another write takes SQLite's write lock and
    /// commits, since no try holds it while waiting. What remains is the case the fold cannot
    /// cover: the reader, read-only, closes last, nothing holds the log, the gate refuses the
    /// inventory before any write could fold it, and only the window's start recovers it, with both
    /// writes. The patience is shortened here to keep the test quick; the window recovers the same.
    #[test]
    fn a_read_that_outlasts_the_fold_blocks_no_write_and_leaves_the_log_to_the_window() {
        let folder = Scratch::new("read-outlasts");
        let path = folder.0.join("inventory.sqlite3");
        start(&path).unwrap();
        access::set_agent_access(&path, "write").unwrap();
        let release = folder.0.join("release");
        let mut db = open_current(&path).unwrap();
        // The person's write, whose commit leaves the fold to this test, to try with less patience.
        let tx = begin_write(&mut db, &Actor::Person).unwrap();
        let reader = reader(&path, &release, "transaction");
        let input = ResourceInput {
            name: "First box".into(),
            ..host()
        };
        save_resource_in(&tx, input, &Actor::Person).unwrap();
        tx.commit().unwrap();
        let patience = Duration::from_millis(1500);
        let folding = std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let emptied = empty_log(&db, patience);
            (emptied, started.elapsed())
        });
        // Another write starts while the fold waits, and waits for the write lock far less than
        // the fold does: holding the lock while waiting would refuse it with "database is locked".
        std::thread::sleep(Duration::from_millis(200));
        let mut other = open_current(&path).unwrap();
        other.busy_timeout(Duration::from_millis(500)).unwrap();
        let tx = begin_write(&mut other, &Actor::Person).unwrap();
        let input = ResourceInput {
            name: "Second box".into(),
            ..host()
        };
        save_resource_in(&tx, input, &Actor::Person).unwrap();
        tx.commit().unwrap();
        drop(other);
        let (emptied, waited) = folding.join().unwrap();
        assert!(!emptied);
        assert!(waited >= patience, "{waited:?}");
        std::fs::write(&release, "").unwrap();
        assert!(reader.finished());
        let log = std::fs::metadata(beside(&path, "-wal")).map_or(0, |log| log.len());
        assert!(log > LOG_HEADER, "{log}");
        assert_eq!(
            examine(&path, |_| Ok(())).0,
            Found::NeedsRecovery(Recovery::Log)
        );
        assert_eq!(open_to_write(&path).unwrap_err().code, "needs_recovery");
        start(&path).unwrap();
        assert_eq!(list(&path).unwrap().resources.len(), 2);
    }

    /// A folder of the temporary folder for one test, removed with everything in it when the test
    /// ends, also when it fails. Declared before what uses it, so that the connections and helper
    /// processes that hold its files are gone when it is removed.
    /// `inventory::tests::disposable` (pull request #12) replaces it once that merges.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(what: &str) -> Self {
            let folder = std::env::temp_dir().join(format!("devhq-{what}-{}", Uuid::new_v4()));
            std::fs::create_dir(&folder).unwrap();
            Self(folder)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // Never panics: a panic while a failed test unwinds would abort every test running.
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Another agent's gate reading the inventory at `path` (see `reads_until_released`), started
    /// once it reads, and holding its `connection` or its read `transaction` until `release` appears.
    fn reader(path: &Path, release: &Path, hold: &str) -> Writer {
        helper(
            "inventory::tests::reads_until_released",
            READER,
            &format!("{}\n{}\n{hold}", path.display(), release.display()),
        )
    }

    /// The environment variable that makes `reads_until_released` read: the inventory's path, the
    /// file whose appearance ends the read, what it holds meanwhile (`connection` or `transaction`)
    /// and the file to create once it reads, one per line.
    const READER: &str = "DEVHQ_TEST_INVENTORY_READER";

    /// Run by `helper` in a process of its own, as another agent's gate: reads the inventory as the
    /// gate does (`examine`) and keeps its connection open, or a read transaction in progress, until
    /// the release file appears; on its own, it does nothing.
    #[test]
    #[ignore = "a helper that other tests run in a process of its own"]
    fn reads_until_released() {
        let Some(spec) = std::env::var_os(READER) else {
            return;
        };
        let spec = spec.into_string().unwrap();
        let [path, release, hold, ready] = spec.split('\n').collect::<Vec<_>>()[..] else {
            panic!("{spec}")
        };
        let (found, outcome) = examine(Path::new(path), |db| {
            // `begin` reads inside the transaction, as `list_from` does: from then on the read
            // keeps its snapshot, and the log it was taken from, in use.
            let read = match hold {
                "connection" => None,
                "transaction" => Some(begin(db, TransactionBehavior::Deferred)?),
                other => panic!("{other}"),
            };
            std::fs::write(ready, "").unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            while !Path::new(release).exists() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            drop(read);
            Ok(())
        });
        assert_eq!(found, Found::Current);
        outcome.unwrap().unwrap();
    }

    /// The environment variable that makes `writes_until_killed` write: what to leave behind, the
    /// inventory's path and the file to create once it has, one per line.
    const WRITER: &str = "DEVHQ_TEST_INVENTORY_WRITER";

    /// Run by `writer` in a process of its own, which the test then kills as a crash would; on its
    /// own, it does nothing. `log`: with a connection held open, as a window holds one, adds the
    /// resource «Live box» to a write-ahead-logged inventory, which stays in the log. `journal`:
    /// starts a write to a rollback-journal inventory, large enough to have reached the file
    /// already, and never ends it. `connection`: holds the log of a write-ahead-logged inventory
    /// open, as a window holds it, and writes nothing; on Unix a test that needs a window's
    /// connection beside the gate needs it from another process, since `log_held` never sees a
    /// lock its own process holds.
    #[test]
    #[ignore = "a helper that other tests run in a process of its own"]
    fn writes_until_killed() {
        let Some(spec) = std::env::var_os(WRITER) else {
            return;
        };
        let spec = spec.into_string().unwrap();
        let [what, path, ready] = spec.split('\n').collect::<Vec<_>>()[..] else {
            panic!("{spec}")
        };
        let path = Path::new(path);
        // A plain connection: it neither upgrades the file nor changes its journal mode.
        let held = Connection::open(path).unwrap();
        match what {
            "log" => {
                held.query_row("SELECT count(*) FROM resources", [], |_| Ok(()))
                    .unwrap();
                // Its own connection commits and closes, and cannot fold the log while `held`
                // is open.
                resource(path, "Live box");
            }
            "connection" => {
                // Its first read opens the log and takes the lock that says it is held.
                held.query_row("SELECT count(*) FROM resources", [], |_| Ok(()))
                    .unwrap();
            }
            "journal" => {
                held.pragma_update(None, "cache_size", 2).unwrap();
                held.execute_batch(
                    "BEGIN;
                     CREATE TABLE ballast (bytes BLOB);
                     WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 2000)
                     INSERT INTO ballast SELECT randomblob(200) FROM n;",
                )
                .unwrap();
            }
            other => panic!("{other}"),
        }
        std::fs::write(ready, "").unwrap();
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }

    /// A process of its own that has done `what` (see `writes_until_killed`) to the inventory at
    /// `path` and goes on holding it until `kill` ends it, or until it is dropped, so that a test
    /// that fails before killing it leaves nothing running.
    pub(crate) struct Writer(std::process::Child);

    impl Drop for Writer {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    pub(crate) fn writer(path: &Path, what: &str) -> Writer {
        helper(
            "inventory::tests::writes_until_killed",
            WRITER,
            &format!("{what}\n{}", path.display()),
        )
    }

    /// Runs the ignored test `test` of this executable as a process of its own, with the
    /// environment variable `variable` set to `spec` and, on a last line, a file it creates once it
    /// has done what it was started for; returns once the file is there.
    pub(crate) fn helper(test: &str, variable: &str, spec: &str) -> Writer {
        let ready = std::env::temp_dir().join(format!("devhq-helper-{}", Uuid::new_v4()));
        let mut child = Writer(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", test, "--nocapture"])
                .env(variable, format!("{spec}\n{}", ready.display()))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        while !ready.exists() {
            if let Some(status) = child.0.try_wait().unwrap() {
                panic!("{test} ended before it was ready: {status}");
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{test} was never ready"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        std::fs::remove_file(ready).unwrap();
        child
    }

    impl Writer {
        /// Waits until the process ends by itself, and says whether it succeeded.
        pub(crate) fn finished(mut self) -> bool {
            let deadline = std::time::Instant::now() + Duration::from_secs(120);
            loop {
                if let Some(status) = self.0.try_wait().unwrap() {
                    return status.success();
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the helper never ended"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    /// Kills `writer` as a crash ends a process, so that nothing in it gets to fold, roll back or
    /// release anything, and waits until the system lets go of its locks on the inventory at
    /// `path`, which Windows does as resources allow. Only the files' locks are asked about: no
    /// connection is opened that SQLite could let change them.
    pub(crate) fn kill(mut writer: Writer, path: &Path) {
        writer.0.kill().unwrap();
        writer.0.wait().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let released = || {
            if beside(path, "-shm").exists() {
                return !log_held(path).unwrap();
            }
            // A plain read-only connection to a rollback-journal file creates nothing, and is
            // refused with SQLITE_BUSY until the dead writer's lock is gone.
            let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            match db.query_row("PRAGMA user_version", [], |_| Ok(())) {
                Err(rusqlite::Error::SqliteFailure(failure, _)) => {
                    failure.code != rusqlite::ErrorCode::DatabaseBusy
                }
                _ => true,
            }
        };
        while !released() {
            assert!(
                std::time::Instant::now() < deadline,
                "the system never released the killed writer's locks"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// What a writer killed in the middle leaves is refused as needing recovery, and nothing the
    /// gate does to find out writes, rolls back or folds any of the files.
    #[test]
    fn never_recovers_what_a_killed_writer_left() {
        let folder = std::env::temp_dir().join(format!("devhq-killed-{}", Uuid::new_v4()));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("inventory.sqlite3");
        let files = || {
            let mut found: Vec<_> = std::fs::read_dir(&folder)
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    (entry.file_name(), std::fs::read(entry.path()).unwrap())
                })
                .collect();
            found.sort();
            found
        };
        let names = |found: &[(std::ffi::OsString, Vec<u8>)]| {
            found
                .iter()
                .map(|(name, _)| name.to_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };

        start(&path).unwrap();
        resource(&path, "Host");
        kill(writer(&path, "log"), &path);
        let before = files();
        assert_eq!(
            names(&before),
            [
                "inventory.sqlite3",
                "inventory.sqlite3-shm",
                "inventory.sqlite3-wal"
            ]
        );
        assert!(
            before[2].1.len() as u64 > LOG_HEADER,
            "the log holds the change"
        );
        assert_eq!(
            examine(&path, |_| Ok(())).0,
            Found::NeedsRecovery(Recovery::Log)
        );
        assert_eq!(files(), before);
        // Without its index.
        std::fs::remove_file(beside(&path, "-shm")).unwrap();
        let before = files();
        assert_eq!(
            examine(&path, |_| Ok(())).0,
            Found::NeedsRecovery(Recovery::LogWithoutIndex)
        );
        assert_eq!(files(), before);
        std::fs::remove_dir_all(&folder).unwrap();

        std::fs::create_dir(&folder).unwrap();
        drop(database_at(&path, 6));
        kill(writer(&path, "journal"), &path);
        let before = files();
        assert_eq!(
            names(&before),
            ["inventory.sqlite3", "inventory.sqlite3-journal"]
        );
        assert_eq!(
            examine(&path, |_| Ok(())).0,
            Found::NeedsRecovery(Recovery::Journal)
        );
        assert_eq!(files(), before);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    thread_local! {
        /// What a test does while `examine` waits to see whether files that need recovering stay
        /// so (see `settled`): the instant in which another connection finishes closing.
        static SETTLING: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
            const { std::cell::RefCell::new(None) };
    }

    pub(crate) fn while_settling() {
        SETTLING.with(|hook| {
            if let Some(hook) = hook.borrow_mut().as_mut() {
                hook();
            }
        });
    }

    /// A log caught in the instant its last connection closes, which nothing holds any more and
    /// which a moment later is folded into the file and gone, is not taken for one left behind:
    /// the gate looks again before it answers, and reads what the fold left. A log left behind is
    /// still refused after the wait, with every file as it was.
    #[test]
    fn tells_a_log_being_folded_from_one_left_behind() {
        let folder = std::env::temp_dir().join(format!("devhq-settling-{}", Uuid::new_v4()));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("inventory.sqlite3");
        start(&path).unwrap();
        resource(&path, "Host");
        kill(writer(&path, "log"), &path);
        let files = || {
            let mut found: Vec<_> = std::fs::read_dir(&folder)
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    (entry.file_name(), std::fs::read(entry.path()).unwrap())
                })
                .collect();
            found.sort();
            found
        };
        let names = |db: &mut Connection| -> Result<Vec<String>, ApiError> {
            let mut statement = db
                .prepare("SELECT name FROM resources ORDER BY name")
                .map_err(ApiError::database)?;
            let rows = statement
                .query_map([], |row| row.get(0))
                .map_err(ApiError::database)?
                .collect::<Result<Vec<String>, _>>()
                .map_err(ApiError::database);
            rows
        };

        // Left behind: refused once the files stayed as they were.
        let before = files();
        let (found, read) = examine(&path, names);
        assert_eq!(found, Found::NeedsRecovery(Recovery::Log));
        assert!(read.is_none());
        assert_eq!(files(), before);

        // Caught as it is folded: while the gate waits, a connection finishes with the log and,
        // as the last one to close does, folds it into the file and deletes it.
        let closing = path.clone();
        let mut done = false;
        SETTLING.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                if !std::mem::replace(&mut done, true) {
                    let db = Connection::open(&closing).unwrap();
                    db.query_row("SELECT count(*) FROM resources", [], |_| Ok(()))
                        .unwrap();
                }
            }));
        });
        let (found, read) = examine(&path, names);
        SETTLING.with(|hook| *hook.borrow_mut() = None);
        assert_eq!(found, Found::Current);
        assert_eq!(read.unwrap().unwrap(), ["Host", "Live box"]);
        assert!(!beside(&path, "-wal").exists());
        std::fs::remove_dir_all(&folder).unwrap();
    }

    /// What a writer killed in the middle left, as the window finds it when it starts: an
    /// inventory in this build's schema is recovered, keeping what was committed; any other is
    /// found out on a recovered copy and refused, its files left exactly as they were.
    #[test]
    fn the_window_recovers_only_an_inventory_in_this_schema() {
        let folder = std::env::temp_dir().join(format!("devhq-starting-{}", Uuid::new_v4()));
        let path = folder.join("inventory.sqlite3");
        let files = || {
            let mut found: Vec<_> = std::fs::read_dir(&folder)
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    (entry.file_name(), std::fs::read(entry.path()).unwrap())
                })
                .collect();
            found.sort();
            found
        };

        // Current, with a committed change still in a log no connection holds.
        std::fs::create_dir(&folder).unwrap();
        start(&path).unwrap();
        resource(&path, "Host");
        kill(writer(&path, "log"), &path);
        assert_eq!(
            examine(&path, |_| Ok(())).0,
            Found::NeedsRecovery(Recovery::Log)
        );
        start(&path).unwrap();
        let names: Vec<_> = list(&path)
            .unwrap()
            .resources
            .into_iter()
            .map(|resource| resource.name)
            .collect();
        assert_eq!(names, ["Host", "Live box"], "what was committed is kept");
        assert_eq!(examine(&path, |_| Ok(())).0, Found::Current);
        std::fs::remove_dir_all(&folder).unwrap();

        // Older and newer, each with a write that did not finish.
        for (version, code) in [(6, "schema_outdated"), (SCHEMA_VERSION + 1, "schema_newer")] {
            std::fs::create_dir(&folder).unwrap();
            database_at(&path, version.min(SCHEMA_VERSION))
                .pragma_update(None, "user_version", version)
                .unwrap();
            kill(writer(&path, "journal"), &path);
            let before = files();
            let refused = start(&path).unwrap_err();
            assert_eq!(refused.code, code, "v{version}: {}", refused.message);
            assert!(
                refused.message.contains("also need recovering"),
                "{}",
                refused.message
            );
            assert_eq!(files(), before, "v{version}: its files were changed");
            std::fs::remove_dir_all(&folder).unwrap();
        }
    }

    /// Leaves at `path` what a process killed in the middle of `sql` leaves of the inventory there.
    /// With a rollback journal, `sql` runs in a transaction that never commits, on a cache so small
    /// that it already wrote to the file: the file holds part of it, and the journal what it
    /// replaced. With a write-ahead log, `sql` commits while another connection holds the log, so
    /// that it is only in the log, which is left without its index. Made of copies of the files
    /// taken while the connections hold them, as a crash leaves them.
    fn crashed(path: &Path, sql: &str) {
        let held = Connection::open(path).unwrap();
        let mode: String = held
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        let suffix = if mode == "wal" {
            held.query_row("SELECT count(*) FROM resources", [], |_| Ok(()))
                .unwrap();
            Connection::open(path).unwrap().execute_batch(sql).unwrap();
            "-wal"
        } else {
            held.pragma_update(None, "cache_size", 2).unwrap();
            held.execute_batch(&format!("BEGIN; {sql}")).unwrap();
            "-journal"
        };
        let file = std::fs::read(path).unwrap();
        let left = std::fs::read(beside(path, suffix)).unwrap();
        assert!(left.len() as u64 > LOG_HEADER, "{suffix} holds the write");
        // Rolls back or folds the log, and removes what it used.
        drop(held);
        for gone in ["-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(beside(path, gone));
        }
        std::fs::write(path, file).unwrap();
        std::fs::write(beside(path, suffix), left).unwrap();
        let found = examine(path, |_| Ok(())).0;
        assert!(matches!(found, Found::NeedsRecovery(_)), "{found:?}");
    }

    /// Adds 200 resources with long names to the inventory at `path`: more pages than `crashed`'s
    /// cache holds, so that rewriting them all writes to the file before it commits.
    fn filled(path: &Path) {
        for number in 0..200 {
            resource(path, &format!("Resource {number:03} {}", "x".repeat(60)));
        }
    }

    /// What SQLite's integrity check finds in the inventory at `path`: "ok" when nothing.
    fn integrity(path: &Path) -> String {
        Connection::open(path)
            .unwrap()
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap()
    }

    fn resource_names(path: &Path) -> Vec<String> {
        let mut names: Vec<_> = list(path)
            .unwrap()
            .resources
            .into_iter()
            .map(|resource| resource.name)
            .collect();
        names.sort();
        names
    }

    /// A journal or a log that an inventory left, without the inventory: no new one is created
    /// beside them, which SQLite would play the journal back into, damaging it, or read the log
    /// as part of; and they are kept as they are, so that with the inventory put back the window
    /// recovers it with what it holds.
    #[test]
    fn never_creates_an_inventory_beside_another_ones_files() {
        for logged in [false, true] {
            let case = if logged { "log" } else { "journal" };
            let folder = std::env::temp_dir().join(format!("devhq-left-{}", Uuid::new_v4()));
            std::fs::create_dir(&folder).unwrap();
            let path = folder.join("inventory.sqlite3");
            if logged {
                start(&path).unwrap();
            } else {
                drop(database_at(&path, SCHEMA_VERSION));
            }
            filled(&path);
            let before = resource_names(&path);
            crashed(&path, "UPDATE resources SET name = 'Renamed ' || name");
            let file = std::fs::read(&path).unwrap();
            std::fs::remove_file(&path).unwrap();
            let left = files_of(&path);
            let refused = start(&path).unwrap_err();
            assert_eq!(
                refused.code, "leftover_files",
                "{case}: {}",
                refused.message
            );
            // Nothing created, played back, folded or removed, and no name the build used.
            assert_eq!(files_of(&path), left, "{case}");
            assert!(!path.exists(), "{case}");
            // Put back, the inventory they belong to is recovered with what was committed.
            std::fs::write(&path, file).unwrap();
            start(&path).unwrap();
            assert_eq!(integrity(&path), "ok", "{case}");
            let expected: Vec<_> = if logged {
                before
                    .iter()
                    .map(|name| format!("Renamed {name}"))
                    .collect()
            } else {
                before
            };
            assert_eq!(resource_names(&path), expected, "{case}");
            std::fs::remove_dir_all(&folder).unwrap();
        }
        // An index without its log, or anything else with one of those names, is refused too.
        let path = nothing();
        std::fs::write(beside(&path, "-shm"), b"").unwrap();
        assert_eq!(start(&path).unwrap_err().code, "leftover_files");
        assert!(!path.exists());
        assert_eq!(std::fs::read(beside(&path, "-shm")).unwrap(), b"");
        std::fs::remove_file(beside(&path, "-shm")).unwrap();
    }

    /// The window recovers only the files it found out about on a copy: files put in their place
    /// meanwhile, before the connection that recovers them opens or after (a log, which SQLite
    /// opens only at the first read), are looked at again from the start, and a newer inventory
    /// among them is refused with every file as it was put there.
    #[test]
    fn recovers_only_the_files_it_found_out_about() {
        let root = std::env::temp_dir().join(format!("devhq-swapped-{}", Uuid::new_v4()));
        let folder = root.join("inventory");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("inventory.sqlite3");
        let files = |folder: &Path| {
            let mut found: Vec<_> = std::fs::read_dir(folder)
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    (entry.file_name(), std::fs::read(entry.path()).unwrap())
                })
                .collect();
            found.sort();
            found
        };
        // What they are, readable when an assertion fails: names, sizes and digests.
        let seen = |files: &[(std::ffi::OsString, Vec<u8>)]| {
            files
                .iter()
                .map(|(name, bytes)| {
                    let digest: [u8; 32] = Sha256::digest(bytes).into();
                    (name.clone(), bytes.len(), digest)
                })
                .collect::<Vec<_>>()
        };
        // An inventory in this build's schema that needs recovering, made again for each case.
        let needing_recovery = || {
            for entry in std::fs::read_dir(&folder).unwrap() {
                std::fs::remove_file(entry.unwrap().path()).unwrap();
            }
            start(&path).unwrap();
            resource(&path, "Host");
            crashed(&path, "UPDATE resources SET name = 'Renamed'");
            assert_eq!(
                examine(&path, |_| Ok(())).0,
                Found::NeedsRecovery(Recovery::LogWithoutIndex)
            );
        };

        // Every file replaced by a newer inventory with a write that did not finish, once the copy
        // was found current: the audit's case.
        let newer = root.join("newer");
        std::fs::create_dir(&newer).unwrap();
        let other = newer.join("inventory.sqlite3");
        drop(database_at(&other, SCHEMA_VERSION));
        filled(&other);
        Connection::open(&other)
            .unwrap()
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        crashed(&other, "UPDATE resources SET name = 'Renamed ' || name");
        let put = files(&newer);
        needing_recovery();
        let mut moments = Vec::new();
        let refused = start_with(&path, &mut |moment| {
            if moments.is_empty() {
                for entry in std::fs::read_dir(&folder).unwrap() {
                    std::fs::remove_file(entry.unwrap().path()).unwrap();
                }
                for (name, bytes) in &put {
                    std::fs::write(folder.join(name), bytes).unwrap();
                }
            }
            moments.push(moment);
        })
        .unwrap_err();
        assert_eq!(moments, [Moment::Copied, Moment::Opened]);
        assert_eq!(refused.code, "schema_newer", "{}", refused.message);
        assert!(refused.message.contains("also need recovering"));
        assert_eq!(
            seen(&files(&folder)),
            seen(&put),
            "the newer inventory was changed"
        );

        // Only the log replaced, by one that makes it a newer inventory, once the connection that
        // would recover it is open: the database file itself is held by that connection.
        let newer_log = root.join("newer-log");
        std::fs::create_dir(&newer_log).unwrap();
        let other = newer_log.join("inventory.sqlite3");
        start(&other).unwrap();
        resource(&other, "Elsewhere");
        crashed(
            &other,
            &format!("PRAGMA user_version = {};", SCHEMA_VERSION + 1),
        );
        let log = std::fs::read(beside(&other, "-wal")).unwrap();
        needing_recovery();
        let crashed_files = files(&folder);
        let mut moments = Vec::new();
        let refused = start_with(&path, &mut |moment| {
            if moment == Moment::Opened && !moments.contains(&Moment::Opened) {
                // Opening it changed nothing: SQLite recovers at the first read.
                assert_eq!(seen(&files(&folder)), seen(&crashed_files));
                std::fs::write(beside(&path, "-wal"), &log).unwrap();
            }
            moments.push(moment);
        })
        .unwrap_err();
        assert_eq!(moments, [Moment::Copied, Moment::Opened]);
        assert_eq!(refused.code, "schema_newer", "{}", refused.message);
        let mut put = crashed_files;
        put.iter_mut()
            .find(|(name, _)| name.to_str().unwrap().ends_with("-wal"))
            .unwrap()
            .1 = log;
        assert_eq!(
            seen(&files(&folder)),
            seen(&put),
            "the log put in place was recovered"
        );

        // Left alone, it is recovered with what was committed.
        needing_recovery();
        let mut moments = Vec::new();
        start_with(&path, &mut |moment| moments.push(moment)).unwrap();
        assert_eq!(moments, [Moment::Copied, Moment::Opened]);
        assert_eq!(resource_names(&path), ["Renamed"]);
        assert_eq!(integrity(&path), "ok");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
