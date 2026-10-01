use crate::{http_check, records};
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
    /// Which automated results the snapshot leaves out. Manual records are always complete.
    pub automated_history: records::AutomatedHistory,
}

const SCHEMA_VERSION: i64 = 6;
const MIGRATIONS: [(i64, &str); 6] = [
    (1, include_str!("../migrations/001_inventory.sql")),
    (2, include_str!("../migrations/002_inventory_details.sql")),
    (3, include_str!("../migrations/003_operational_records.sql")),
    (4, include_str!("../migrations/004_http_checks.sql")),
    (5, include_str!("../migrations/005_http_monitoring.sql")),
    (6, include_str!("../migrations/006_history_window.sql")),
];

pub(crate) fn open(path: &Path) -> Result<Connection, ApiError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ApiError::database)?;
    }
    let mut db = Connection::open(path).map_err(ApiError::database)?;
    db.busy_timeout(Duration::from_secs(5))
        .map_err(ApiError::database)?;
    // Upgrades rebuild tables, which SQLite only allows safely with enforcement off.
    // migrate() verifies every relationship before committing; from then on it is on.
    db.pragma_update(None, "foreign_keys", "OFF")
        .map_err(ApiError::database)?;
    migrate(&mut db)?;
    db.pragma_update(None, "foreign_keys", "ON")
        .map_err(ApiError::database)?;
    Ok(db)
}

fn migrate(db: &mut Connection) -> Result<(), ApiError> {
    // Serialize first launch: two WebView requests may reach this path together.
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    let version: i64 = tx
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(ApiError::database)?;
    if version > SCHEMA_VERSION {
        return Err(ApiError::database(format!(
            "unsupported inventory schema version {version}"
        )));
    }
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

/// What a person typed, without surrounding whitespace, and no longer than `max` characters.
pub(crate) fn bounded_text(value: &str, max: usize, label: &str) -> Result<String, ApiError> {
    let text = value.trim();
    if text.chars().count() > max {
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

fn required_name(value: &str, label: &str) -> Result<String, ApiError> {
    required_text(value, MAX_NAME, &format!("{label} name"))
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
    tx.commit().map_err(ApiError::database)?;
    Ok(Inventory {
        applications,
        resources,
        observations,
        incidents,
        events,
        http_checks,
        automated_history,
    })
}

pub fn save_resource(path: &Path, input: ResourceInput) -> Result<Resource, ApiError> {
    let name = required_name(&input.name, "Resource")?;
    allowed(&input.kind, &["server", "workstation"], "resource kind")?;
    let description = bounded_text(&input.description, MAX_DESCRIPTION, "Resource description")?;
    let operating_system = bounded_text(&input.operating_system, MAX_NAME, "Operating system")?;
    let db = open(path)?;
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    if id.is_empty() {
        return Err(ApiError::invalid("Resource ID is required."));
    }
    if db.execute(
        "INSERT INTO resources (id, name, kind, description, operating_system) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, kind=excluded.kind, description=excluded.description, operating_system=excluded.operating_system",
        params![id, name, input.kind, description, operating_system],
    ).map_err(ApiError::database)? == 0 { return Err(ApiError::database("Resource was not saved.")); }
    Ok(Resource {
        id,
        name,
        kind: input.kind,
        description,
        operating_system,
    })
}

pub fn save_application(path: &Path, input: ApplicationInput) -> Result<Application, ApiError> {
    let name = required_name(&input.name, "Application")?;
    allowed(
        &input.lifecycle,
        &["development", "testing", "production", "archived"],
        "lifecycle",
    )?;
    let description = bounded_text(
        &input.description,
        MAX_DESCRIPTION,
        "Application description",
    )?;
    // A blank version is no version.
    let version = match &input.version {
        Some(version) => Some(bounded_text(version, MAX_VERSION, "Version")?),
        None => None,
    }
    .filter(|version| !version.is_empty());
    let mut components = Vec::with_capacity(input.components.len());
    for component in input.components {
        let name = required_name(&component.name, "Component")?;
        allowed(
            &component.kind,
            &["backend", "database", "docker", "frontend"],
            "component kind",
        )?;
        let id = component.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        if id.is_empty() {
            return Err(ApiError::invalid("Component ID is required."));
        }
        if components.iter().any(|item: &Component| item.id == id) {
            return Err(ApiError::invalid(format!(
                "Component {id} appears twice. Each component can be listed once."
            )));
        }
        components.push(Component {
            id,
            name,
            kind: component.kind,
            resource_id: component.resource_id,
        });
    }
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    if id.is_empty() {
        return Err(ApiError::invalid("Application ID is required."));
    }
    let mut db = open(path)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    for component in &components {
        if let Some(resource_id) = &component.resource_id {
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
        // A component belongs to one application; reusing its ID elsewhere would move it.
        let owner: Option<String> = tx
            .query_row(
                "SELECT application_id FROM components WHERE id = ?1",
                [&component.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(ApiError::database)?;
        if owner.is_some_and(|owner| owner != id) {
            return Err(ApiError::invalid(format!(
                "Component {} belongs to another application.",
                component.id
            )));
        }
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
    tx.commit().map_err(ApiError::database)?;
    Ok(Application {
        id,
        name,
        description,
        lifecycle: input.lifecycle,
        version,
        components,
    })
}

fn count(tx: &Transaction, sql: &str, id: &str) -> Result<i64, ApiError> {
    tx.query_row(sql, [id], |row| row.get(0))
        .map_err(ApiError::database)
}

fn plural(count: i64, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

/// Records that reference a subject. History is never deleted, so they keep it in place.
fn history_of(tx: &Transaction, column: &str, id: &str) -> Result<Vec<String>, ApiError> {
    let mut found = Vec::new();
    for (table, noun) in [
        ("observations", "observation"),
        ("incidents", "incident"),
        ("events", "event"),
    ] {
        let total = count(
            tx,
            &format!("SELECT count(*) FROM {table} WHERE {column} = ?1"),
            id,
        )?;
        if total > 0 {
            found.push(plural(total, noun));
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
    let name: String = tx
        .query_row("SELECT name FROM applications WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(ApiError::database)?
        .ok_or_else(|| ApiError::invalid(format!("The application {id} does not exist.")))?;
    let history = history_of(&tx, "application_id", id)?;
    if !history.is_empty() {
        return Err(ApiError::invalid(format!(
            "{name} has {}. DevHQ keeps history, so it cannot be deleted; set its lifecycle to Archived instead.",
            listed(&history)
        )));
    }
    // Its components and its HTTP check go with it (ON DELETE CASCADE).
    tx.execute("DELETE FROM applications WHERE id = ?1", [id])
        .map_err(ApiError::database)?;
    tx.commit().map_err(ApiError::database)
}

/// Deletes a resource that no component runs on and that has no records, with its HTTP check.
pub fn delete_resource(path: &Path, id: &str) -> Result<(), ApiError> {
    let mut db = open(path)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    let name: String = tx
        .query_row("SELECT name FROM resources WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(ApiError::database)?
        .ok_or_else(|| ApiError::invalid(format!("The resource {id} does not exist.")))?;
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
    let history = history_of(&tx, "resource_id", id)?;
    if !history.is_empty() {
        return Err(ApiError::invalid(format!(
            "{name} has {}. DevHQ keeps history, so it cannot be deleted.",
            listed(&history)
        )));
    }
    // Its HTTP check goes with it (ON DELETE CASCADE).
    tx.execute("DELETE FROM resources WHERE id = ?1", [id])
        .map_err(ApiError::database)?;
    tx.commit().map_err(ApiError::database)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "Example has 1 incident and 2 events. DevHQ keeps history, so it cannot be deleted; set its lifecycle to Archived instead."
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
            "Host has 1 observation and 1 event. DevHQ keeps history, so it cannot be deleted."
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
            },
        )
        .unwrap_err();
        assert_eq!(late.code, "invalid_input");
        let snapshot = list(&path).unwrap();
        assert!(snapshot.observations.is_empty() && snapshot.resources.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn concurrent_first_open_migrates_once() {
        let path = path();
        let first = path.clone();
        let second = path.clone();
        let one = std::thread::spawn(move || list(&first));
        let two = std::thread::spawn(move || list(&second));
        assert!(one.join().unwrap().is_ok());
        assert!(two.join().unwrap().is_ok());
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
                "components_application_id",
                "components_resource_id",
                "events_application_id",
                "events_incident_id",
                "events_manual",
                "events_observation_id",
                "events_resource_id",
                "incidents_application_id",
                "incidents_resource_id",
                "observations_application_id",
                "observations_manual",
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
    fn dump(db: &Connection, table: &str) -> Vec<String> {
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
        let unchanged = [
            "resources",
            "applications",
            "components",
            "observations",
            "incidents",
            "events",
        ];
        let before: Vec<_> = unchanged.iter().map(|table| dump(&db, table)).collect();
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

        let upgraded = list(&path).unwrap();
        assert_eq!(upgraded.observations.len(), 2);
        assert_eq!(upgraded.events.len(), 3);
        assert_eq!(upgraded.http_checks[0].monitor, "active");
        assert!(upgraded.automated_history.truncated.is_empty());

        let db = Connection::open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 6);
        for (table, rows) in tables.iter().zip(&before) {
            assert_eq!(&dump(&db, table), rows, "{table}");
        }
        // Partial: they hold only the manual records.
        assert_eq!(
            strings(
                &db,
                "SELECT name || ': ' || substr(sql, instr(sql, 'WHERE')) FROM sqlite_master WHERE name LIKE '%_manual' ORDER BY name"
            ),
            [
                "events_manual: WHERE source = 'manual'",
                "observations_manual: WHERE source = 'manual'",
            ]
        );
        drop(db);
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
