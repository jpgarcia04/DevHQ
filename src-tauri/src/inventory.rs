use crate::records::Actor;
use crate::{change_log, http_check, records};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
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
    /// No backup was made, or one is already being made. The inventory itself is untouched.
    pub(crate) fn backup(message: impl Into<String>) -> Self {
        Self {
            code: "backup_error",
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
}

/// One consistent read of the local workspace: inventory plus its operational records.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub applications: Vec<Application>,
    pub resources: Vec<Resource>,
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

pub(crate) const SCHEMA_VERSION: i64 = 10;
const MIGRATIONS: [(i64, &str); 10] = [
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
];

/// Opens the inventory, upgrading its schema first if it is older than this build's.
///
/// The database uses write-ahead logging: a reader works on the last committed state and
/// never waits for a writer, and a writer never waits for readers. Supervision writes from
/// the async runtime while the window reads, and some commands run on the main thread, so
/// with a rollback journal a reader could stall the window behind a commit in progress.
/// Writers still take turns. The mode is stored in the file, so setting it again is a no-op;
/// when the last connection closes SQLite folds the log back into `inventory.sqlite3`.
pub(crate) fn open(path: &Path) -> Result<Connection, ApiError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ApiError::database)?;
    }
    let mut db = Connection::open(path).map_err(ApiError::database)?;
    db.busy_timeout(Duration::from_secs(5))
        .map_err(ApiError::database)?;
    // Switching an older file needs it to itself, and SQLite does not wait for that: when
    // another connection holds a lock (two requests at first launch), a later open switches
    // it. Until then, and where the file system cannot share memory between connections, the
    // rollback journal stays: just as correct, only readers and writers wait for each other.
    match db.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0)) {
        Err(rusqlite::Error::SqliteFailure(error, _))
            if error.code == rusqlite::ErrorCode::DatabaseBusy => {}
        result => {
            result.map_err(ApiError::database)?;
        }
    }
    // Only an upgrade takes the write lock; an up-to-date database is just read.
    if schema_version(&db)? < SCHEMA_VERSION {
        // Upgrades rebuild tables, which SQLite only allows safely with enforcement off.
        // migrate() verifies every relationship before committing; from then on it is on.
        db.pragma_update(None, "foreign_keys", "OFF")
            .map_err(ApiError::database)?;
        migrate(&mut db)?;
    }
    db.pragma_update(None, "foreign_keys", "ON")
        .map_err(ApiError::database)?;
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

fn migrate(db: &mut Connection) -> Result<(), ApiError> {
    // Serialize first launch: two WebView requests may reach this path together. Whoever
    // gets the lock second finds the schema already upgraded.
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    let version = schema_version(&tx)?;
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    for (number, sql) in MIGRATIONS {
        if version < number {
            tx.execute_batch(sql).map_err(ApiError::database)?;
            tx.pragma_update(None, "user_version", number)
                .map_err(ApiError::database)?;
        }
    }
    let broken = {
        let mut statement = tx
            .prepare("PRAGMA foreign_key_check")
            .map_err(ApiError::database)?;
        let rows = statement
            .query_map([], |_| Ok(()))
            .map_err(ApiError::database)?
            .count();
        rows
    };
    if broken > 0 {
        // Dropping the transaction rolls the whole upgrade back.
        return Err(ApiError::database(format!(
            "upgrading to schema version {SCHEMA_VERSION} would leave {broken} broken relationships; the database was left unchanged"
        )));
    }
    tx.commit().map_err(ApiError::database)?;
    Ok(())
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
    let mut db = open(path)?;
    let tx = db.transaction().map_err(ApiError::database)?;
    let resources = {
        let mut statement = tx.prepare("SELECT id, name, kind, description, operating_system FROM resources ORDER BY name COLLATE NOCASE, id").map_err(ApiError::database)?;
        let result = statement
            .query_map([], |row| {
                Ok(Resource {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    kind: row.get(2)?,
                    description: row.get(3)?,
                    operating_system: row.get(4)?,
                })
            })
            .map_err(ApiError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(ApiError::database)?;
        result
    };
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
        }
    }
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
    } = records::load(&tx, &subjects)?;
    let http_checks = http_check::load(&tx)?;
    let changes = change_log::load(&tx)?;
    let changes_since = change_log::complete_since(&tx)?;
    tx.commit().map_err(ApiError::database)?;
    Ok(Inventory {
        applications,
        resources,
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
    let mut db = open(path)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    let (resource, _) = save_resource_in(&tx, input, &Actor::Person)?;
    tx.commit().map_err(ApiError::database)?;
    Ok(resource)
}

/// Saves a resource inside the caller's write transaction, which the caller commits, and logs
/// the change there as `by`'s: None when the save changed nothing.
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
    tx.execute(
        "INSERT INTO resources (id, name, kind, description, operating_system) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, kind=excluded.kind, description=excluded.description, operating_system=excluded.operating_system",
        params![id, name, input.kind, description, operating_system],
    )
    .map_err(ApiError::database)?;
    let resource = Resource {
        id,
        name,
        kind: input.kind,
        description,
        operating_system,
    };
    let change = change_log::resource_saved(tx, by, before.as_ref(), &resource)?;
    Ok((resource, change))
}

pub fn save_application(path: &Path, input: ApplicationInput) -> Result<Application, ApiError> {
    let mut db = open(path)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    let (application, _) = save_application_in(&tx, input, &Actor::Person)?;
    tx.commit().map_err(ApiError::database)?;
    Ok(application)
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
    let application = Application {
        id,
        name,
        description,
        lifecycle: input.lifecycle,
        version,
        components,
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
    Ok(Some(application))
}

/// The resource as stored, read inside the caller's transaction; None if there is none.
pub(crate) fn resource_in(tx: &Transaction, id: &str) -> Result<Option<Resource>, ApiError> {
    tx.query_row(
        "SELECT id, name, kind, description, operating_system FROM resources WHERE id = ?1",
        [id],
        |row| {
            Ok(Resource {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                description: row.get(3)?,
                operating_system: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(ApiError::database)
}

/// Which kinds of record reference a subject. History is never deleted, so any keeps it.
fn history_of(tx: &Transaction, column: &str, id: &str) -> Result<Vec<String>, ApiError> {
    let mut found = Vec::new();
    for table in ["observations", "incidents", "events"] {
        // EXISTS stops at the first one: a long supervised history costs no more than one.
        let exists: bool = tx
            .query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {column} = ?1)"),
                [id],
                |row| row.get(0),
            )
            .map_err(ApiError::database)?;
        if exists {
            found.push(table.to_owned());
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

/// Deletes an application that has no records, with its components and HTTP check. One with
/// observations, incidents or events is kept: DevHQ never erases history. Archive it instead.
pub fn delete_application(path: &Path, id: &str) -> Result<(), ApiError> {
    let mut db = open(path)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    delete_application_in(&tx, id, &Actor::Person)?;
    tx.commit().map_err(ApiError::database)
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
    // Its components and its HTTP check go with it (ON DELETE CASCADE).
    tx.execute("DELETE FROM applications WHERE id = ?1", [id])
        .map_err(ApiError::database)?;
    let change = change_log::application_deleted(tx, by, &before)?;
    Ok((name, change))
}

/// Deletes a resource that no component runs on and that has no records, with its HTTP check.
pub fn delete_resource(path: &Path, id: &str) -> Result<(), ApiError> {
    let mut db = open(path)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    delete_resource_in(&tx, id, &Actor::Person)?;
    tx.commit().map_err(ApiError::database)
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
    let history = history_of(tx, "resource_id", id)?;
    if !history.is_empty() {
        return Err(ApiError::invalid(format!(
            "{name} has recorded {}. DevHQ keeps history, so it cannot be deleted.",
            listed(&history)
        )));
    }
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

    fn path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("devhq-test-{}.sqlite3", Uuid::new_v4()))
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
        list(&asked).unwrap();
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

    #[test]
    fn concurrent_first_open_migrates_once() {
        let path = path();
        let first = path.clone();
        let second = path.clone();
        let one = std::thread::spawn(move || list(&first));
        let two = std::thread::spawn(move || list(&second));
        let (one, two) = (one.join().unwrap(), two.join().unwrap());
        assert!(
            one.is_ok() && two.is_ok(),
            "{:?} {:?}",
            one.err(),
            two.err()
        );
        std::fs::remove_file(path).unwrap();
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
        let path = path();
        // As earlier releases left it: a rollback journal.
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
        // Switching needs the file to itself and SQLite does not wait for it: while another
        // connection writes, DevHQ still opens it, in the mode it has.
        let (release, writer) = holding(&path, |path, hold| {
            let mut db = Connection::open(path).unwrap();
            let tx = db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            hold();
            tx.commit().unwrap();
        });
        let contended = list(&path);
        release.send(()).unwrap();
        writer.join().unwrap();
        assert!(contended.is_ok(), "{:?}", contended.err());
        assert_eq!(mode(&Connection::open(&path).unwrap()), "delete");
        // The next open has it to itself and switches it.
        resource(&path, "Host");
        resource(&path, "Spare");
        // The mode is stored in the file, for every later connection.
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
        let path = path();
        let db = Connection::open(&path).unwrap();
        db.execute_batch(include_str!("../migrations/001_inventory.sql"))
            .unwrap();
        db.pragma_update(None, "user_version", 1).unwrap();
        db.execute("INSERT INTO applications (id, name, lifecycle) VALUES ('legacy', 'Old app', 'testing')", []).unwrap();
        drop(db);
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
        let path = path();
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
        let path = path();
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
                "change_log_resource_id",
                "components_application_id",
                "components_resource_id",
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
        let path = path();
        let db = v3_database(&path);
        // Written with enforcement off, as another tool could have done.
        db.execute_batch(
            "INSERT INTO resources (id, name, kind) VALUES ('host', 'Host', 'server');
             INSERT INTO events (id, resource_id, kind, outcome, title, source, observation_id, occurred_at, recorded_at)
               VALUES ('orphan', 'host', 'observation', 'success', 'Observed', 'manual', 'missing', 1, 1);",
        )
        .unwrap();
        drop(db);
        let error = list(&path).unwrap_err();
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
    fn database_at(path: &Path, version: i64) -> Connection {
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
        let path = path();
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
        let path = path();
        let db = v4_database(&path);
        // A check for a resource that no longer exists, written with enforcement off.
        db.execute(
            "INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at)
             VALUES ('gone', 'http://x/', 200, 5, 60, 0)",
            [],
        )
        .unwrap();
        drop(db);
        let error = list(&path).unwrap_err();
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
        let path = path();
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
        let path = path();
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
        let path = path();
        let db = database_at(&path, 6);
        // An event of an incident that no longer exists, written with enforcement off.
        db.execute_batch(
            "INSERT INTO applications (id, name, lifecycle) VALUES ('shop', 'Shop', 'production');
             INSERT INTO events (id, application_id, kind, outcome, title, source, incident_id, occurred_at, recorded_at)
               VALUES ('orphan', 'shop', 'incident', 'failure', 'Incident opened: Gone', 'manual', 'gone', 1, 1);",
        )
        .unwrap();
        drop(db);
        let error = list(&path).unwrap_err();
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
        let path = path();
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
        let path = path();
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

    #[test]
    fn upgrades_v9_agent_log_into_the_change_log_keeping_every_row() {
        let path = path();
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

        let started = records::now_ms();
        let upgraded = list(&path).unwrap();
        let finished = records::now_ms();
        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 10);
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
            ["change_log_application_id", "change_log_resource_id"]
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
        let path = path();
        drop(v9_with_agent_log(&path, false));
        let upgraded = list(&path).unwrap();
        assert_eq!(upgraded.changes_since, None);
        assert_eq!(upgraded.changes.len(), 2);
        let db = Connection::open(&path).unwrap();
        assert!(dump(&db, "change_log_start").is_empty());
        drop(db);
        std::fs::remove_file(&path).unwrap();
        // A new inventory starts with every migration at once, and its log is complete.
        let fresh = list(&path).unwrap();
        assert_eq!(fresh.changes_since, None);
        assert!(fresh.changes.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn refuses_a_newer_schema_instead_of_guessing() {
        let path = path();
        let db = Connection::open(&path).unwrap();
        db.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        drop(db);
        let error = list(&path).unwrap_err();
        assert_eq!(error.code, "database_error");
        assert!(error.message.contains(&format!(
            "unsupported inventory schema version {}",
            SCHEMA_VERSION + 1
        )));
        std::fs::remove_file(path).unwrap();
    }
}
