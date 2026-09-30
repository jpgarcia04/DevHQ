use crate::{http_check, records};
use rusqlite::{params, Connection, TransactionBehavior};
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

#[derive(Debug, Deserialize)]
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
}

const SCHEMA_VERSION: i64 = 5;
const MIGRATIONS: [(i64, &str); 5] = [
    (1, include_str!("../migrations/001_inventory.sql")),
    (2, include_str!("../migrations/002_inventory_details.sql")),
    (3, include_str!("../migrations/003_operational_records.sql")),
    (4, include_str!("../migrations/004_http_checks.sql")),
    (5, include_str!("../migrations/005_http_monitoring.sql")),
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

pub(crate) fn required_text(value: &str, label: &str) -> Result<String, ApiError> {
    let text = value.trim();
    if text.is_empty() {
        Err(ApiError::invalid(format!("{label} is required.")))
    } else {
        Ok(text.to_owned())
    }
}

fn required_name(value: &str, label: &str) -> Result<String, ApiError> {
    required_text(value, &format!("{label} name"))
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
    let records::Records {
        observations,
        incidents,
        events,
    } = records::load(&tx)?;
    let http_checks = http_check::load(&tx)?;
    tx.commit().map_err(ApiError::database)?;
    Ok(Inventory {
        applications,
        resources,
        observations,
        incidents,
        events,
        http_checks,
    })
}

pub fn save_resource(path: &Path, input: ResourceInput) -> Result<Resource, ApiError> {
    let name = required_name(&input.name, "Resource")?;
    allowed(&input.kind, &["server", "workstation"], "resource kind")?;
    let db = open(path)?;
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    if id.is_empty() {
        return Err(ApiError::invalid("Resource ID is required."));
    }
    if db.execute(
        "INSERT INTO resources (id, name, kind, description, operating_system) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, kind=excluded.kind, description=excluded.description, operating_system=excluded.operating_system",
        params![id, name, input.kind, input.description, input.operating_system],
    ).map_err(ApiError::database)? == 0 { return Err(ApiError::database("Resource was not saved.")); }
    Ok(Resource {
        id,
        name,
        kind: input.kind,
        description: input.description,
        operating_system: input.operating_system,
    })
}

pub fn save_application(path: &Path, input: ApplicationInput) -> Result<Application, ApiError> {
    let name = required_name(&input.name, "Application")?;
    allowed(
        &input.lifecycle,
        &["development", "testing", "production", "archived"],
        "lifecycle",
    )?;
    for component in &input.components {
        required_name(&component.name, "Component")?;
        allowed(
            &component.kind,
            &["backend", "database", "docker", "frontend"],
            "component kind",
        )?;
    }
    let mut db = open(path)?;
    let tx = db.transaction().map_err(ApiError::database)?;
    for component in &input.components {
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
    }
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    if id.is_empty() {
        return Err(ApiError::invalid("Application ID is required."));
    }
    tx.execute(
        "INSERT INTO applications (id, name, lifecycle, description, version) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, lifecycle=excluded.lifecycle, description=excluded.description, version=excluded.version",
        params![id, name, input.lifecycle, input.description, input.version],
    ).map_err(ApiError::database)?;
    // The submitted component list is the complete set. Omitted components are removed atomically.
    tx.execute("DELETE FROM components WHERE application_id = ?1", [&id])
        .map_err(ApiError::database)?;
    let mut components = Vec::with_capacity(input.components.len());
    for component in input.components {
        let component_id = component.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        if component_id.is_empty() {
            return Err(ApiError::invalid("Component ID is required."));
        }
        let component_name = component.name.trim().to_owned();
        tx.execute("INSERT INTO components (id, application_id, name, kind, resource_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![component_id, id, component_name, component.kind, component.resource_id]).map_err(ApiError::database)?;
        components.push(Component {
            id: component_id,
            name: component_name,
            kind: component.kind,
            resource_id: component.resource_id,
        });
    }
    tx.commit().map_err(ApiError::database)?;
    Ok(Application {
        id,
        name,
        description: input.description,
        lifecycle: input.lifecycle,
        version: input.version,
        components,
    })
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
                "events_observation_id",
                "events_resource_id",
                "incidents_application_id",
                "incidents_resource_id",
                "observations_application_id",
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
        assert_eq!(version, 5);
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
