//! «Back up now»: a copy of the inventory made while DevHQ stays open.
//!
//! `VACUUM INTO` reads the last committed state, as every reader does under write-ahead logging,
//! so it waits for no writer and leaves out whatever is not committed. The copy is one file in
//! rollback-journal mode, complete by itself. It is written to a `.partial` file that the backup
//! creates itself, verified (`quick_check` and the schema version), forced to disk and only then
//! linked under its final name, which the file system refuses if any file has it: a backup never
//! replaces a file. A failure removes the `.partial`, so a file with a final name is a copy that
//! was complete and verified when it was made. Nothing deletes or rewrites a finished backup.
//! A backup is not evidence about a system, so the timeline records nothing.

use crate::inventory::{open, stored_at, ApiError, SCHEMA_VERSION};
use crate::records::{format_time, now_ms};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use time::OffsetDateTime;

/// Rows in a backup, by table.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupCounts {
    pub applications: i64,
    pub components: i64,
    pub resources: i64,
    pub observations: i64,
    pub incidents: i64,
    pub events: i64,
    pub http_checks: i64,
}

/// A finished backup, as it was verified once written.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub name: String,
    /// Where the file is, to copy it somewhere safer.
    pub path: String,
    /// Where the file system really keeps it, when that is not `path` (see `inventory::stored_at`).
    pub stored_at: Option<String>,
    /// When the copy was taken (UTC), as its name says.
    pub taken_at: String,
    pub size_bytes: u64,
    /// What `PRAGMA quick_check` said of the copy. Only an "ok" copy gets its final name.
    pub quick_check: String,
    pub schema_version: i64,
    pub counts: BackupCounts,
}

/// A file in the backups folder, as found there. DevHQ vouches for none of them: a backup is
/// verified once, when it is made, and anything can happen to the file afterwards.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub name: String,
    pub size_bytes: u64,
    /// When the file was last written, if the system says.
    pub modified_at: Option<String>,
    /// A copy that a backup was still writing, or that an interrupted one left behind.
    pub partial: bool,
    /// Where the file system really keeps it, when that is not in `BackupFolder::directory`.
    /// Asked file by file: a folder that Windows redirects for a packaged app can show files kept
    /// in two places at once (see `inventory::stored_at`).
    pub stored_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupFolder {
    pub directory: String,
    /// Newest first.
    pub files: Vec<BackupFile>,
}

/// The folder next to the inventory, in DevHQ's data directory, that holds its backups.
pub fn folder_of(database: &Path) -> PathBuf {
    database.with_file_name("backups")
}

/// `inventory-20261001T120320.450Z.sqlite3`: the UTC moment in ISO 8601's basic format, so that
/// names sort by time and hold no ':', which Windows refuses in a file name.
fn file_name(taken_at: i64) -> Option<String> {
    let time = OffsetDateTime::from_unix_timestamp_nanos(i128::from(taken_at) * 1_000_000).ok()?;
    Some(format!(
        "inventory-{:04}{:02}{:02}T{:02}{:02}{:02}.{:03}Z.sqlite3",
        time.year(),
        u8::from(time.month()),
        time.day(),
        time.hour(),
        time.minute(),
        time.second(),
        time.millisecond()
    ))
}

/// Backs up the inventory at `database` into its backups folder, now.
pub fn back_up(database: &Path) -> Result<Backup, ApiError> {
    write(database, now_ms(), |_| Ok(()))
}

/// `damage` runs on the `.partial` once the copy is written: tests use it to break the copy.
fn write(
    database: &Path,
    taken_at: i64,
    damage: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<Backup, ApiError> {
    let folder = folder_of(database);
    let name = file_name(taken_at).ok_or_else(|| {
        ApiError::backup("The system clock is out of range. Nothing was written.")
    })?;
    let partial = folder.join(format!("{name}.partial"));
    let reserved = std::fs::create_dir_all(&folder).and_then(|()| {
        // Created here and only here, so that a failure removes this backup's file and no other.
        File::options().write(true).create_new(true).open(&partial)
    });
    if let Err(error) = reserved {
        return Err(ApiError::backup(format!(
            "DevHQ could not start the backup in {}: {error}. Nothing was written.",
            folder.display()
        )));
    }
    finish(database, &folder, &name, &partial, taken_at, damage).map_err(|cause| {
        ApiError::backup(match std::fs::remove_file(&partial) {
            Ok(()) => format!("{cause} The incomplete copy was removed, so no backup was added."),
            Err(error) => format!(
                "{cause} The incomplete copy {} could not be removed ({error}); it is not a backup.",
                partial.display()
            ),
        })
    })
}

fn finish(
    database: &Path,
    folder: &Path,
    name: &str,
    partial: &Path,
    taken_at: i64,
    damage: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<Backup, String> {
    let target = partial
        .to_str()
        .ok_or("The backups folder's path is not valid Unicode, which SQLite needs.")?;
    open(database)
        .map_err(|error| error.message)?
        .execute("VACUUM INTO ?1", [target])
        .map_err(|error| format!("Copying the inventory failed: {error}."))?;
    damage(partial).map_err(|error| error.to_string())?;
    let (quick_check, schema_version, counts) = verify(partial)?;
    // On disk before it is named, so that a backup's name never points at data still in a cache.
    // Windows flushes only through a handle that can write.
    File::options()
        .write(true)
        .open(partial)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("Writing the copy to disk failed: {error}."))?;
    let size_bytes = std::fs::metadata(partial)
        .map_err(|error| format!("The copy cannot be measured: {error}."))?
        .len();
    // A second name for the same file, which the file system refuses if the name is taken;
    // removing the first name then completes the rename. A plain rename would replace a file.
    let finished = folder.join(name);
    std::fs::hard_link(partial, &finished).map_err(|error| match error.kind() {
        ErrorKind::AlreadyExists => {
            format!(
                "{name} already exists in the backups folder, and a backup never replaces a file."
            )
        }
        _ => format!("Naming the copy {name} failed: {error}."),
    })?;
    // Named, the backup is finished: nothing below can fail it. A `.partial` that could not be
    // removed is listed as incomplete.
    let _ = std::fs::remove_file(partial);
    // The names themselves reach the disk with the folder (Windows journals them as it goes).
    #[cfg(unix)]
    let _ = File::open(folder).and_then(|folder| folder.sync_all());
    Ok(Backup {
        name: name.to_owned(),
        path: finished.display().to_string(),
        stored_at: stored_at(&finished),
        taken_at: format_time(taken_at).unwrap_or_default(),
        size_bytes,
        quick_check,
        schema_version,
        counts,
    })
}

/// Reads the copy back on its own: its integrity, its schema version and what it holds.
fn verify(copy: &Path) -> Result<(String, i64, BackupCounts), String> {
    let unreadable = |error: rusqlite::Error| format!("The copy cannot be read back: {error}.");
    let db = Connection::open_with_flags(
        copy,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(unreadable)?;
    // Damage shows up as rows describing it, or, when SQLite cannot even walk a page, as an error.
    let findings = db
        .prepare("PRAGMA quick_check")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap_or_else(|error| vec![error.to_string()]);
    if findings != ["ok"] {
        return Err(format!(
            "The copy failed its integrity check: {}.",
            findings.join("; ")
        ));
    }
    let schema_version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(unreadable)?;
    if schema_version != SCHEMA_VERSION {
        return Err(format!(
            "The copy has schema version {schema_version} instead of {SCHEMA_VERSION}."
        ));
    }
    let count = |table: &str| {
        db.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(unreadable)
    };
    let counts = BackupCounts {
        applications: count("applications")?,
        components: count("components")?,
        resources: count("resources")?,
        observations: count("observations")?,
        incidents: count("incidents")?,
        events: count("events")?,
        http_checks: count("http_checks")?,
    };
    Ok(("ok".into(), schema_version, counts))
}

/// The files in the backups folder, newest first. A folder that does not exist yet is empty.
pub fn list(database: &Path) -> Result<BackupFolder, ApiError> {
    let folder = folder_of(database);
    let unreadable = |error: std::io::Error| {
        ApiError::backup(format!(
            "DevHQ could not read the backups folder {}: {error}.",
            folder.display()
        ))
    };
    let mut files = Vec::new();
    match std::fs::read_dir(&folder) {
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(unreadable(error)),
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(unreadable)?;
                let metadata = entry.metadata().map_err(unreadable)?;
                if !metadata.is_file() {
                    continue;
                }
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok());
                let name = entry.file_name().to_string_lossy().into_owned();
                files.push((
                    modified,
                    BackupFile {
                        partial: name.ends_with(".partial"),
                        name,
                        size_bytes: metadata.len(),
                        modified_at: modified.and_then(format_time),
                        stored_at: stored_at(&entry.path()),
                    },
                ));
            }
        }
    }
    files.sort_by(|(a_time, a), (b_time, b)| b_time.cmp(a_time).then_with(|| b.name.cmp(&a.name)));
    Ok(BackupFolder {
        directory: folder.display().to_string(),
        files: files.into_iter().map(|(_, file)| file).collect(),
    })
}

/// Makes backups one at a time, away from the thread that asks for them.
#[derive(Default)]
pub struct Backups {
    running: Arc<tokio::sync::Mutex<()>>,
}

impl Backups {
    pub async fn back_up(&self, database: PathBuf) -> Result<Backup, ApiError> {
        let running = Arc::clone(&self.running).try_lock_owned().map_err(|_| {
            ApiError::backup("A backup is already being made. Wait for it to finish.")
        })?;
        tokio::task::spawn_blocking(move || {
            let _running = running;
            back_up(&database)
        })
        .await
        .map_err(|error| ApiError::backup(format!("The backup stopped unexpectedly: {error}.")))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::tests::{dump, holding, linked_folder, remove_linked, resolved_temp_dir};
    use crate::inventory::{list as snapshot, save_resource, ResourceInput};
    use crate::records::{record_event, EventInput, Subject};
    use rusqlite::TransactionBehavior;

    /// An inventory path in a folder of its own, so that its backups folder is its own too.
    fn inventory() -> PathBuf {
        let folder =
            resolved_temp_dir().join(format!("devhq-backup-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        folder.join("inventory.sqlite3")
    }

    fn remove(database: &Path) {
        std::fs::remove_dir_all(database.parent().unwrap()).unwrap();
    }

    fn names(database: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(folder_of(database))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    const TABLES: [&str; 7] = [
        "resources",
        "applications",
        "components",
        "observations",
        "incidents",
        "events",
        "http_checks",
    ];

    /// One of every kind of record, as the use cases and supervision write them.
    fn seed(database: &Path) {
        open(database)
            .unwrap()
            .execute_batch(
                "INSERT INTO resources (id, name, kind, description, operating_system) VALUES ('host', 'Host', 'server', 'Rack 2', 'Linux');
                 INSERT INTO applications (id, name, lifecycle, description, version) VALUES ('shop', 'Shop', 'production', 'Store', '2.0');
                 INSERT INTO components (id, application_id, name, kind, resource_id) VALUES ('api', 'shop', 'API', 'backend', 'host');
                 INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at)
                   VALUES ('obs-manual', 'host', 'degraded', 'Slow', 'manual', 500, 900, 600);
                 INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at,
                                           http_url, http_expected_status, http_status, http_latency_ms, http_error, http_redirects)
                   VALUES ('obs-http', 'host', 'unhealthy', 'HTTP 500, expected 200', 'http', 1000, 2000, 1000,
                           'http://host/health', 200, 500, 9, NULL, '[{\"status\":301,\"location\":\"http://host/health\"}]');
                 INSERT INTO incidents (id, application_id, title, description, severity, status, source, opened_at, updated_at)
                   VALUES ('inc', 'shop', 'Down', 'Checkout', 'critical', 'open', 'manual', 700, 700);
                 INSERT INTO events (id, resource_id, kind, outcome, title, detail, source, observation_id, occurred_at, recorded_at)
                   VALUES ('evt-http', 'host', 'observation', 'failure', 'HTTP check: Unhealthy', 'HTTP 500', 'http', 'obs-http', 1000, 1000),
                          ('evt-manual', 'host', 'observation', 'warning', 'Observation: Attention required', 'Slow', 'manual', 'obs-manual', 500, 600);
                 INSERT INTO events (id, application_id, kind, outcome, title, source, incident_id, occurred_at, recorded_at)
                   VALUES ('evt-inc', 'shop', 'incident', 'failure', 'Incident opened: Down', 'manual', 'inc', 700, 700);
                 INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at, monitor, monitor_interval_minutes)
                   VALUES ('host', 'http://host/health', 200, 5, 15, 800, 'active', 5);",
            )
            .unwrap();
        // And through a use case, with the times and IDs it makes.
        record_event(
            database,
            EventInput {
                subject: Subject::Application { id: "shop".into() },
                kind: "backup".into(),
                outcome: "success".into(),
                title: "Copied the database by hand".into(),
                detail: String::new(),
                occurred_at: None,
            },
        )
        .unwrap();
    }

    fn readonly(path: &Path) -> Connection {
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
    }

    // 2026-01-01T09:30:00.123Z
    const TAKEN_AT: i64 = 1_767_259_800_123;

    #[test]
    fn a_backup_holds_every_row_of_the_inventory_and_nothing_else() {
        let database = inventory();
        // As while DevHQ is open: with a connection open, the latest changes stay in the log, and
        // a copy of the database file alone would miss them.
        let open_meanwhile = open(&database).unwrap();
        seed(&database);
        let log = database.with_file_name("inventory.sqlite3-wal");
        assert!(std::fs::metadata(&log).unwrap().len() > 0);
        let before: Vec<_> = TABLES
            .iter()
            .map(|table| dump(&open_meanwhile, table))
            .collect();

        let backup = write(&database, TAKEN_AT, |_| Ok(())).unwrap();
        let name = "inventory-20260101T093000.123Z.sqlite3";
        let file = folder_of(&database).join(name);
        assert_eq!(
            backup,
            Backup {
                name: name.into(),
                path: file.display().to_string(),
                stored_at: None,
                taken_at: "2026-01-01T09:30:00.123Z".into(),
                size_bytes: std::fs::metadata(&file).unwrap().len(),
                quick_check: "ok".into(),
                schema_version: SCHEMA_VERSION,
                counts: BackupCounts {
                    applications: 1,
                    components: 1,
                    resources: 1,
                    observations: 2,
                    incidents: 1,
                    events: 4,
                    http_checks: 1,
                },
            }
        );
        // Row by row, rowids included, as the schema upgrades compare them.
        let copy = readonly(&file);
        for (table, rows) in TABLES.iter().zip(&before) {
            assert_eq!(&dump(&copy, table), rows, "{table}");
        }
        let pragma = |name: &str| -> String {
            copy.query_row(&format!("PRAGMA {name}"), [], |row| {
                row.get::<_, rusqlite::types::Value>(0)
            })
            .map(|value| format!("{value:?}"))
            .unwrap()
        };
        assert_eq!(pragma("user_version"), format!("Integer({SCHEMA_VERSION})"));
        // One file, complete by itself: no log to carry along.
        assert_eq!(pragma("journal_mode"), "Text(\"delete\")");
        drop(copy);
        // Nothing else in the folder: no `.partial`, no journal.
        assert_eq!(names(&database), [name]);
        // Making a backup records nothing in the timeline.
        assert_eq!(dump(&open_meanwhile, "events"), before[5]);
        drop(open_meanwhile);

        // Restoring is putting it in place of the inventory: DevHQ reads it as it was.
        let restored = database.with_file_name("restored.sqlite3");
        std::fs::copy(&file, &restored).unwrap();
        let original = snapshot(&database).unwrap();
        let copied = snapshot(&restored).unwrap();
        assert_eq!(copied.applications, original.applications);
        assert_eq!(copied.resources, original.resources);
        assert_eq!(copied.observations, original.observations);
        assert_eq!(copied.incidents, original.incidents);
        assert_eq!(copied.events, original.events);
        assert_eq!(copied.http_checks, original.http_checks);
        remove(&database);
    }

    #[test]
    fn a_backup_leaves_out_what_is_not_committed_and_waits_for_no_writer() {
        let database = inventory();
        seed(&database);
        let (release, writer) = holding(&database, |path, hold| {
            let mut db = open(path).unwrap();
            let tx = db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            tx.execute(
                "INSERT INTO resources (id, name, kind) VALUES ('pending', 'Pending', 'server')",
                [],
            )
            .unwrap();
            hold();
            tx.commit().unwrap();
        });
        // The writer holds its transaction until this returns: waiting for it would have given
        // up after five seconds and failed.
        let backup = back_up(&database);
        release.send(()).unwrap();
        writer.join().unwrap();
        let backup = backup.unwrap();
        assert_eq!(backup.counts.resources, 1);
        let copy = readonly(Path::new(&backup.path));
        assert_eq!(
            dump(&copy, "resources").len(),
            1,
            "the uncommitted resource is not in the copy"
        );
        drop(copy);
        assert_eq!(snapshot(&database).unwrap().resources.len(), 2);
        remove(&database);
    }

    #[test]
    fn a_backup_never_replaces_a_file_nor_removes_one_it_did_not_make() {
        let database = inventory();
        seed(&database);
        let folder = folder_of(&database);
        std::fs::create_dir_all(&folder).unwrap();
        let name = "inventory-20260101T093000.123Z.sqlite3";
        // A finished backup with that name already, however it came to be.
        std::fs::write(folder.join(name), "an earlier backup").unwrap();
        let error = write(&database, TAKEN_AT, |_| Ok(())).unwrap_err();
        assert_eq!(error.code, "backup_error");
        assert!(
            error
                .message
                .contains("already exists in the backups folder")
                && error.message.contains("no backup was added"),
            "{}",
            error.message
        );
        assert_eq!(
            std::fs::read_to_string(folder.join(name)).unwrap(),
            "an earlier backup"
        );
        assert_eq!(names(&database), [name]);
        std::fs::remove_file(folder.join(name)).unwrap();

        // Another backup's `.partial`, which it may still be writing: left alone.
        let partial = format!("{name}.partial");
        std::fs::write(folder.join(&partial), "still being written").unwrap();
        let error = write(&database, TAKEN_AT, |_| Ok(())).unwrap_err();
        assert!(
            error.message.contains("Nothing was written"),
            "{}",
            error.message
        );
        assert_eq!(
            std::fs::read_to_string(folder.join(&partial)).unwrap(),
            "still being written"
        );
        assert_eq!(names(&database), [partial]);
        remove(&database);
    }

    #[test]
    fn a_copy_that_fails_verification_is_removed_and_never_named() {
        let database = inventory();
        seed(&database);
        // Overwrites the start of the second page, where a table's b-tree begins.
        let error = write(&database, TAKEN_AT, |partial| {
            use std::io::{Seek, SeekFrom, Write};
            let mut file = File::options().write(true).open(partial)?;
            file.seek(SeekFrom::Start(4096))?;
            file.write_all(&[0xFF; 64])
        })
        .unwrap_err();
        assert_eq!(error.code, "backup_error");
        assert!(
            error.message.contains("integrity check")
                && error.message.contains("The incomplete copy was removed"),
            "{}",
            error.message
        );
        assert!(names(&database).is_empty());

        // Sound, but not the schema this DevHQ wrote.
        let error = write(&database, TAKEN_AT, |partial| {
            Connection::open(partial)
                .and_then(|copy| copy.pragma_update(None, "user_version", SCHEMA_VERSION - 1))
                .map_err(std::io::Error::other)
        })
        .unwrap_err();
        assert!(
            error.message.starts_with(&format!(
                "The copy has schema version {} instead of {SCHEMA_VERSION}.",
                SCHEMA_VERSION - 1
            )),
            "{}",
            error.message
        );
        assert!(names(&database).is_empty());
        // The inventory itself is untouched.
        assert_eq!(snapshot(&database).unwrap().resources.len(), 1);
        remove(&database);
    }

    #[tokio::test]
    async fn backups_are_made_one_at_a_time() {
        let database = inventory();
        seed(&database);
        let backups = Backups::default();
        let running = Arc::clone(&backups.running).try_lock_owned().unwrap();
        let error = backups.back_up(database.clone()).await.unwrap_err();
        assert_eq!(error.code, "backup_error");
        assert!(
            error.message.contains("already being made"),
            "{}",
            error.message
        );
        assert!(!folder_of(&database).exists());
        drop(running);
        let backup = backups.back_up(database.clone()).await.unwrap();
        assert_eq!(names(&database), [backup.name]);
        remove(&database);
    }

    #[test]
    fn lists_the_folder_as_found_newest_first() {
        let database = inventory();
        seed(&database);
        let empty = list(&database).unwrap();
        assert_eq!(
            empty,
            BackupFolder {
                directory: folder_of(&database).display().to_string(),
                files: vec![],
            }
        );
        let older = write(&database, TAKEN_AT, |_| Ok(())).unwrap();
        // Written later, so newer to the file system.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let folder = folder_of(&database);
        std::fs::write(folder.join("inventory-left-behind.sqlite3.partial"), "half").unwrap();
        std::fs::create_dir(folder.join("a folder")).unwrap();
        save_resource(
            &database,
            ResourceInput {
                id: None,
                name: "Spare".into(),
                kind: "workstation".into(),
                description: String::new(),
                operating_system: String::new(),
                references: vec![],
            },
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let newer = write(&database, TAKEN_AT + 60_000, |_| Ok(())).unwrap();
        let listed = list(&database).unwrap();
        let summary: Vec<_> = listed
            .files
            .iter()
            .map(|file| (file.name.as_str(), file.size_bytes, file.partial))
            .collect();
        assert_eq!(
            summary,
            [
                (newer.name.as_str(), newer.size_bytes, false),
                ("inventory-left-behind.sqlite3.partial", 4, true),
                (older.name.as_str(), older.size_bytes, false),
            ]
        );
        assert!(listed.files.iter().all(|file| file.modified_at.is_some()));
        // Kept where they are listed.
        assert!(listed.files.iter().all(|file| file.stored_at.is_none()));
        remove(&database);
    }

    #[test]
    fn says_where_the_file_system_really_keeps_each_backup() {
        let (real, link) = linked_folder();
        let database = link.join("inventory.sqlite3");
        seed(&database);
        let backup = write(&database, TAKEN_AT, |_| Ok(())).unwrap();
        let kept = real.join("backups").join(&backup.name);
        assert_eq!(
            backup.path,
            folder_of(&database)
                .join(&backup.name)
                .display()
                .to_string()
        );
        assert_eq!(backup.stored_at, Some(kept.display().to_string()));
        let listed = list(&database).unwrap();
        assert_eq!(listed.directory, folder_of(&database).display().to_string());
        assert_eq!(
            listed
                .files
                .iter()
                .map(|file| file.stored_at.clone())
                .collect::<Vec<_>>(),
            [Some(kept.display().to_string())]
        );
        remove_linked((real, link));
    }
}
