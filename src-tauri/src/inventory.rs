use crate::records;
use rusqlite::{params, Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Serialize)]
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
}

pub(crate) fn open(path: &Path) -> Result<Connection, ApiError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ApiError::database)?;
    }
    let mut db = Connection::open(path).map_err(ApiError::database)?;
    db.pragma_update(None, "foreign_keys", "ON")
        .map_err(ApiError::database)?;
    db.busy_timeout(Duration::from_secs(5))
        .map_err(ApiError::database)?;
    migrate(&mut db)?;
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
    if version > 3 {
        return Err(ApiError::database(format!(
            "unsupported inventory schema version {version}"
        )));
    }
    for (number, sql) in [
        (1, include_str!("../migrations/001_inventory.sql")),
        (2, include_str!("../migrations/002_inventory_details.sql")),
        (3, include_str!("../migrations/003_operational_records.sql")),
    ] {
        if version < number {
            tx.execute_batch(sql).map_err(ApiError::database)?;
            tx.pragma_update(None, "user_version", number)
                .map_err(ApiError::database)?;
        }
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
    tx.commit().map_err(ApiError::database)?;
    Ok(Inventory {
        applications,
        resources,
        observations,
        incidents,
        events,
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
        assert_eq!(version, 3);
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
        assert_eq!(version, 3);
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

    #[test]
    fn refuses_a_newer_schema_instead_of_guessing() {
        let path = path();
        let db = Connection::open(&path).unwrap();
        db.pragma_update(None, "user_version", 4).unwrap();
        drop(db);
        let error = list(&path).unwrap_err();
        assert_eq!(error.code, "database_error");
        assert!(error
            .message
            .contains("unsupported inventory schema version 4"));
        std::fs::remove_file(path).unwrap();
    }
}
