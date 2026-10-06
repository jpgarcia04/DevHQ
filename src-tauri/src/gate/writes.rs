//! What an agent changes through the gate, when the person lets agents write. Every tool goes
//! through the window's own use cases, with their validation, limits and transactions:
//! - Records (observations, incidents, events) carry the agent as their source and name.
//! - Configuration (applications, resources, components, HTTP checks) has no source, so each
//!   change writes its entry in `agent_log` in the same transaction, after checking there, with
//!   SQLite's write lock held, that the person still lets agents write.
//! - Edits are partial: what an agent leaves out is kept, so that changing one field can never
//!   erase another by omission. A component list, when given, is the complete list, as in the
//!   window's form.

use super::{pick, system_type, text, Session};
use crate::access;
use crate::agent_log;
use crate::backup;
use crate::http_check::{self, HttpCheck, HttpCheckInput};
use crate::inventory::{
    self, ApiError, Application, ApplicationInput, ComponentInput, Resource, ResourceInput,
};
use crate::records::{
    self, Actor, EventInput, IncidentInput, IncidentStatusInput, Lifeline, ObservationInput,
    Subject,
};
use rusqlite::{Transaction, TransactionBehavior};
use serde_json::{json, Map, Value};

/// The name an agent gets when its client declared none.
const UNNAMED: &str = "unnamed MCP client";

fn optional_text<'a>(
    arguments: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, ApiError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(ApiError::invalid(format!(
            "The argument {key} must be text."
        ))),
    }
}

fn optional_integer(arguments: &Map<String, Value>, key: &str) -> Result<Option<i64>, ApiError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_i64().map(Some).ok_or_else(|| {
            ApiError::invalid(format!("The argument {key} must be a whole number."))
        }),
    }
}

/// Text that can be given, cleared with null, or left out to keep what is stored.
enum Field<'a> {
    Keep,
    Clear,
    Set(&'a str),
}

fn field<'a>(arguments: &'a Map<String, Value>, key: &str) -> Result<Field<'a>, ApiError> {
    match arguments.get(key) {
        None => Ok(Field::Keep),
        Some(Value::Null) => Ok(Field::Clear),
        Some(Value::String(value)) => Ok(Field::Set(value)),
        Some(_) => Err(ApiError::invalid(format!(
            "The argument {key} must be text or null."
        ))),
    }
}

/// A value for a summary: «name» in quotes, «none» for nothing.
fn quoted(value: Option<&str>) -> String {
    match value {
        Some(value) if !value.is_empty() => format!("«{value}»"),
        _ => "none".to_owned(),
    }
}

/// Every application or resource of a kind, as (ID, name), read inside the transaction.
fn candidates(tx: &Transaction, kind: &str) -> Result<Vec<(String, String)>, ApiError> {
    let table = if kind == "application" {
        "applications"
    } else {
        "resources"
    };
    let mut statement = tx
        .prepare(&format!("SELECT id, name FROM {table} ORDER BY rowid"))
        .map_err(ApiError::database)?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(ApiError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApiError::database);
    rows
}

/// The system the arguments name (by `type`, or the given kind, and `id` or `name`), inside the
/// transaction: its subject and current name.
fn subject_in(
    tx: &Transaction,
    kind: Option<&'static str>,
    arguments: &Map<String, Value>,
) -> Result<(Subject, String), ApiError> {
    let kind = match kind {
        Some(kind) => kind,
        None => system_type(arguments)?,
    };
    pick(kind, &candidates(tx, kind)?, arguments)
}

/// A resource named by ID or, failing that, by exact name: its ID. An unknown one is passed on
/// as an ID, and the use case refuses it with its own message.
fn resource_id(tx: &Transaction, named: &str) -> Result<String, ApiError> {
    let resources = candidates(tx, "resource")?;
    if resources.iter().any(|(id, _)| id == named) {
        return Ok(named.to_owned());
    }
    let matching: Vec<_> = resources
        .iter()
        .filter(|(_, name)| name.to_lowercase() == named.trim().to_lowercase())
        .collect();
    match matching.as_slice() {
        [(id, _)] => Ok(id.clone()),
        [] => Ok(named.to_owned()),
        _ => Err(ApiError::invalid(format!(
            "Several resources are named {named}; use the resource's id."
        ))),
    }
}

fn component_summary(application: &Application, resources: &[(String, String)]) -> Vec<String> {
    application
        .components
        .iter()
        .map(|component| {
            let runs_on = component.resource_id.as_ref().map(|id| {
                resources
                    .iter()
                    .find(|(resource, _)| resource == id)
                    .map_or(id.as_str(), |(_, name)| name.as_str())
            });
            match runs_on {
                Some(resource) => format!("{} ({}, on {resource})", component.name, component.kind),
                None => format!("{} ({})", component.name, component.kind),
            }
        })
        .collect()
}

/// What changed from `before` to `after`, field by field.
fn application_changes(
    before: &Application,
    after: &Application,
    resources: &[(String, String)],
) -> Vec<String> {
    let mut changes = Vec::new();
    if before.name != after.name {
        changes.push(format!(
            "name {} → {}",
            quoted(Some(&before.name)),
            quoted(Some(&after.name))
        ));
    }
    if before.lifecycle != after.lifecycle {
        changes.push(format!(
            "lifecycle {} → {}",
            before.lifecycle, after.lifecycle
        ));
    }
    if before.version != after.version {
        changes.push(format!(
            "version {} → {}",
            quoted(before.version.as_deref()),
            quoted(after.version.as_deref())
        ));
    }
    if before.description != after.description {
        changes.push("description changed".to_owned());
    }
    let (was, is) = (
        component_summary(before, resources),
        component_summary(after, resources),
    );
    if was != is {
        changes.push(format!(
            "components {} → {}",
            if was.is_empty() {
                "none".to_owned()
            } else {
                was.join(", ")
            },
            if is.is_empty() {
                "none".to_owned()
            } else {
                is.join(", ")
            },
        ));
    }
    changes
}

fn resource_changes(before: &Resource, after: &Resource) -> Vec<String> {
    let mut changes = Vec::new();
    if before.name != after.name {
        changes.push(format!(
            "name {} → {}",
            quoted(Some(&before.name)),
            quoted(Some(&after.name))
        ));
    }
    if before.kind != after.kind {
        changes.push(format!("kind {} → {}", before.kind, after.kind));
    }
    if before.operating_system != after.operating_system {
        changes.push(format!(
            "operating system {} → {}",
            quoted(Some(&before.operating_system)),
            quoted(Some(&after.operating_system))
        ));
    }
    if before.description != after.description {
        changes.push("description changed".to_owned());
    }
    changes
}

fn check_summary(check: &HttpCheck) -> String {
    let supervision = match (check.monitor.as_str(), check.monitor_interval_minutes) {
        ("active", Some(minutes)) => format!("supervised every {minutes} min while DevHQ is open"),
        ("paused", Some(minutes)) => format!("supervision paused (every {minutes} min)"),
        _ => "run only when asked".to_owned(),
    };
    format!(
        "GET {} expecting {} within {} s, valid {} min, {supervision}",
        check.url, check.expected_status, check.timeout_seconds, check.valid_for_minutes
    )
}

impl Session {
    /// The agent, by the name its client declared in `initialize`.
    fn actor(&self) -> Result<Actor, ApiError> {
        Actor::agent(
            self.client
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(UNNAMED),
        )
    }

    /// Makes a configuration change and its log entry in one write transaction, only while the
    /// person lets agents write: checked inside it, under SQLite's write lock, so that closing
    /// the gate in Settings can never race a change.
    fn change(
        &self,
        apply: impl FnOnce(&Transaction, &str) -> Result<Value, ApiError>,
    ) -> Result<Value, ApiError> {
        self.existing()?;
        let actor = self.actor()?;
        let mut db = inventory::open(&self.database)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(ApiError::database)?;
        super::permitted(&access::mode(&tx)?, "write")?;
        let value = apply(&tx, actor.name().unwrap_or(UNNAMED))?;
        tx.commit().map_err(ApiError::database)?;
        Ok(value)
    }

    pub(super) fn save_application(
        &self,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ApiError> {
        self.change(|tx, agent| {
            let existing = match optional_text(arguments, "id")? {
                Some(id) => Some(inventory::application_in(tx, id)?.ok_or_else(|| {
                    ApiError::invalid(format!(
                        "No application has the id {id}. list_systems lists them; leave id out to add one."
                    ))
                })?),
                None => None,
            };
            let name = match optional_text(arguments, "name")? {
                Some(name) => name.to_owned(),
                None => existing
                    .as_ref()
                    .map(|item| item.name.clone())
                    .ok_or_else(|| ApiError::invalid("name is required to add an application."))?,
            };
            let lifecycle = match optional_text(arguments, "lifecycle")? {
                Some(lifecycle) => lifecycle.to_owned(),
                None => existing.as_ref().map(|item| item.lifecycle.clone()).ok_or_else(|| {
                    ApiError::invalid(
                        "lifecycle is required to add an application: development, testing, production or archived.",
                    )
                })?,
            };
            let description = match optional_text(arguments, "description")? {
                Some(description) => description.to_owned(),
                None => existing.as_ref().map(|item| item.description.clone()).unwrap_or_default(),
            };
            let version = match field(arguments, "version")? {
                Field::Keep => existing.as_ref().and_then(|item| item.version.clone()),
                Field::Clear => None,
                Field::Set(version) => Some(version.to_owned()),
            };
            let components = match arguments.get("components") {
                None => existing
                    .as_ref()
                    .map(|item| {
                        item.components
                            .iter()
                            .map(|component| ComponentInput {
                                id: Some(component.id.clone()),
                                name: component.name.clone(),
                                kind: component.kind.clone(),
                                resource_id: component.resource_id.clone(),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|item| {
                        let item = item.as_object().ok_or_else(|| {
                            ApiError::invalid("Each component must be an object with name and kind.")
                        })?;
                        Ok(ComponentInput {
                            id: optional_text(item, "id")?.map(str::to_owned),
                            name: text(item, "name")?.to_owned(),
                            kind: text(item, "kind")?.to_owned(),
                            resource_id: optional_text(item, "runsOn")?
                                .map(|named| resource_id(tx, named))
                                .transpose()?,
                        })
                    })
                    .collect::<Result<Vec<_>, ApiError>>()?,
                Some(_) => return Err(ApiError::invalid("components must be a list.")),
            };
            let saved = inventory::save_application_in(
                tx,
                ApplicationInput {
                    id: existing.as_ref().map(|item| item.id.clone()),
                    name,
                    description,
                    lifecycle,
                    version,
                    components,
                },
            )?;
            let resources = candidates(tx, "resource")?;
            let (action, summary) = match &existing {
                None => {
                    let parts = component_summary(&saved, &resources);
                    (
                        "created",
                        format!(
                            "Added application «{}» ({}{}){}",
                            saved.name,
                            saved.lifecycle,
                            saved.version.as_ref().map(|version| format!(", version {version}")).unwrap_or_default(),
                            if parts.is_empty() { String::new() } else { format!(" with {}", parts.join(", ")) }
                        ),
                    )
                }
                Some(before) => {
                    let changes = application_changes(before, &saved, &resources);
                    (
                        "updated",
                        if changes.is_empty() {
                            format!("Saved application «{}» without changes", saved.name)
                        } else {
                            format!("Updated application «{}»: {}", saved.name, changes.join("; "))
                        },
                    )
                }
            };
            let subject = Subject::Application { id: saved.id.clone() };
            let change = agent_log::record(tx, agent, action, &subject, &saved.name, &summary)?;
            Ok(json!({ "application": saved, "change": change }))
        })
    }

    pub(super) fn save_resource(&self, arguments: &Map<String, Value>) -> Result<Value, ApiError> {
        self.change(|tx, agent| {
            let existing = match optional_text(arguments, "id")? {
                Some(id) => Some(inventory::resource_in(tx, id)?.ok_or_else(|| {
                    ApiError::invalid(format!(
                        "No resource has the id {id}. list_systems lists them; leave id out to add one."
                    ))
                })?),
                None => None,
            };
            let keep = |key: &str, kept: Option<String>, required: Option<&str>| -> Result<String, ApiError> {
                match optional_text(arguments, key)? {
                    Some(value) => Ok(value.to_owned()),
                    None => match (kept, required) {
                        (Some(kept), _) => Ok(kept),
                        (None, Some(message)) => Err(ApiError::invalid(message)),
                        (None, None) => Ok(String::new()),
                    },
                }
            };
            let saved = inventory::save_resource_in(
                tx,
                ResourceInput {
                    id: existing.as_ref().map(|item| item.id.clone()),
                    name: keep(
                        "name",
                        existing.as_ref().map(|item| item.name.clone()),
                        Some("name is required to add a resource."),
                    )?,
                    kind: keep(
                        "kind",
                        existing.as_ref().map(|item| item.kind.clone()),
                        Some("kind is required to add a resource: server or workstation."),
                    )?,
                    description: keep("description", existing.as_ref().map(|item| item.description.clone()), None)?,
                    operating_system: keep(
                        "operatingSystem",
                        existing.as_ref().map(|item| item.operating_system.clone()),
                        None,
                    )?,
                },
            )?;
            let (action, summary) = match &existing {
                None => (
                    "created",
                    format!(
                        "Added resource «{}» ({}{})",
                        saved.name,
                        saved.kind,
                        if saved.operating_system.is_empty() { String::new() } else { format!(", {}", saved.operating_system) }
                    ),
                ),
                Some(before) => {
                    let changes = resource_changes(before, &saved);
                    (
                        "updated",
                        if changes.is_empty() {
                            format!("Saved resource «{}» without changes", saved.name)
                        } else {
                            format!("Updated resource «{}»: {}", saved.name, changes.join("; "))
                        },
                    )
                }
            };
            let subject = Subject::Resource { id: saved.id.clone() };
            let change = agent_log::record(tx, agent, action, &subject, &saved.name, &summary)?;
            Ok(json!({ "resource": saved, "change": change }))
        })
    }

    pub(super) fn delete(
        &self,
        kind: &'static str,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ApiError> {
        self.change(|tx, agent| {
            let (subject, _) = subject_in(tx, Some(kind), arguments)?;
            let id = match &subject {
                Subject::Application { id } | Subject::Resource { id } => id.clone(),
            };
            let name = if kind == "application" {
                inventory::delete_application_in(tx, &id)?
            } else {
                inventory::delete_resource_in(tx, &id)?
            };
            let change = agent_log::record(
                tx,
                agent,
                "deleted",
                &subject,
                &name,
                &format!("Deleted {kind} «{name}», which had no history"),
            )?;
            Ok(json!({ "deleted": { "type": kind, "id": id, "name": name }, "change": change }))
        })
    }

    pub(super) fn save_check(&self, arguments: &Map<String, Value>) -> Result<Value, ApiError> {
        self.change(|tx, agent| {
            let (subject, name) = subject_in(tx, None, arguments)?;
            let existing = http_check::find(tx, &subject)?;
            let url = match optional_text(arguments, "url")? {
                Some(url) => url.to_owned(),
                None => existing
                    .as_ref()
                    .map(|check| check.url.clone())
                    .ok_or_else(|| ApiError::invalid("url is required to set up a check."))?,
            };
            // The window's defaults for a new check.
            let integer = |key: &str, kept: Option<i64>, default: i64| -> Result<i64, ApiError> {
                Ok(optional_integer(arguments, key)?
                    .or(kept)
                    .unwrap_or(default))
            };
            let interval = match arguments.get("supervisionIntervalMinutes") {
                None => existing
                    .as_ref()
                    .and_then(|check| check.monitor_interval_minutes),
                Some(Value::Null) => None,
                Some(_) => optional_integer(arguments, "supervisionIntervalMinutes")?,
            };
            let saved = http_check::save_http_check_in(
                tx,
                HttpCheckInput {
                    subject: subject.clone(),
                    url,
                    expected_status: integer(
                        "expectedStatus",
                        existing.as_ref().map(|check| check.expected_status),
                        200,
                    )?,
                    timeout_seconds: integer(
                        "timeoutSeconds",
                        existing.as_ref().map(|check| check.timeout_seconds),
                        5,
                    )?,
                    valid_for_minutes: integer(
                        "validForMinutes",
                        existing.as_ref().map(|check| check.valid_for_minutes),
                        60,
                    )?,
                    monitor_interval_minutes: interval,
                },
            )?;
            let summary = format!(
                "{} the HTTP check of «{name}»: {}",
                if existing.is_some() {
                    "Changed"
                } else {
                    "Set up"
                },
                check_summary(&saved)
            );
            let change = agent_log::record(tx, agent, "check_saved", &subject, &name, &summary)?;
            Ok(json!({ "httpCheck": saved, "change": change }))
        })
    }

    /// A check left supervised while a DevHQ window is open on the inventory is that window's to
    /// run: it notices the change within about 2 s and runs the check at once if it never ran or
    /// is overdue. The answer says so, so that an agent asked to «set it up and run it» does not
    /// record a second observation a moment later.
    pub(super) fn window_note(&self, mut value: Value) -> Value {
        let supervised = value.pointer("/httpCheck/monitor") == Some(&json!("active"));
        if supervised && crate::window_lock::window_open(&self.database) == Some(true) {
            if let Value::Object(object) = &mut value {
                object.insert(
                    "note".into(),
                    json!("A DevHQ window is open on this inventory. Within about 2 s it supervises this check, and runs it at once if it never ran or its last run is older than the interval. run_check now would record a second observation."),
                );
            }
        }
        value
    }

    pub(super) fn set_check_paused(
        &self,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ApiError> {
        let paused = arguments
            .get("paused")
            .and_then(Value::as_bool)
            .ok_or_else(|| ApiError::invalid("The argument paused is required: true or false."))?;
        self.change(|tx, agent| {
            let (subject, name) = subject_in(tx, None, arguments)?;
            let saved = http_check::set_http_check_paused_in(tx, &subject, paused)?;
            let (action, summary) = if paused {
                (
                    "check_paused",
                    format!("Paused the supervision of «{name}»"),
                )
            } else {
                (
                    "check_resumed",
                    format!(
                        "Resumed the supervision of «{name}»: {}",
                        check_summary(&saved)
                    ),
                )
            };
            let change = agent_log::record(tx, agent, action, &subject, &name, &summary)?;
            Ok(json!({ "httpCheck": saved, "change": change }))
        })
    }

    pub(super) fn remove_check(&self, arguments: &Map<String, Value>) -> Result<Value, ApiError> {
        self.change(|tx, agent| {
            let (subject, name) = subject_in(tx, None, arguments)?;
            let removed = http_check::find(tx, &subject)?;
            http_check::remove_http_check_in(tx, &subject)?;
            let summary = format!(
                "Removed the HTTP check of «{name}»{}; the evidence it recorded stays",
                removed
                    .map(|check| format!(" ({})", check.url))
                    .unwrap_or_default()
            );
            let change = agent_log::record(tx, agent, "check_removed", &subject, &name, &summary)?;
            Ok(json!({ "removed": true, "change": change }))
        })
    }

    /// Runs the check now, as Run check does, and records its observation as DevHQ's own
    /// evidence (`source` http); the log says which agent asked for it.
    pub(super) fn run_check(&self, arguments: &Map<String, Value>) -> Result<Value, ApiError> {
        let inventory = self.inventory()?;
        let subject = super::find_system(&inventory, arguments)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(ApiError::check)?;
        let observation = runtime.block_on(http_check::run_http_check(
            &self.database,
            subject.clone(),
            &Lifeline::default(),
        ))?;
        let change = self.change(|tx, agent| {
            let (_, name) = subject_in(tx, None, arguments)?;
            let summary = format!("Ran the HTTP check of «{name}»: {}", observation.summary);
            Ok(json!(agent_log::record(
                tx,
                agent,
                "check_run",
                &subject,
                &name,
                &summary
            )?))
        })?;
        Ok(json!({ "observation": observation, "change": change }))
    }

    pub(super) fn record_observation(
        &self,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ApiError> {
        let subject = super::find_system(&self.inventory()?, arguments)?;
        let valid_for_minutes =
            optional_integer(arguments, "validForMinutes")?.ok_or_else(|| {
                ApiError::invalid(
                    "validForMinutes is required: how long the result vouches, 1 to 43200.",
                )
            })?;
        let observation = records::record_observation_by(
            &self.database,
            ObservationInput {
                subject,
                result: text(arguments, "result")?.to_owned(),
                summary: text(arguments, "summary")?.to_owned(),
                observed_at: optional_text(arguments, "observedAt")?.map(str::to_owned),
                valid_for_minutes,
            },
            &self.actor()?,
        )?;
        Ok(json!({ "observation": observation }))
    }

    pub(super) fn open_incident(&self, arguments: &Map<String, Value>) -> Result<Value, ApiError> {
        let subject = super::find_system(&self.inventory()?, arguments)?;
        let incident = records::open_incident_by(
            &self.database,
            IncidentInput {
                subject,
                title: text(arguments, "title")?.to_owned(),
                description: optional_text(arguments, "description")?
                    .unwrap_or_default()
                    .to_owned(),
                severity: text(arguments, "severity")?.to_owned(),
            },
            &self.actor()?,
        )?;
        Ok(json!({ "incident": incident }))
    }

    pub(super) fn update_incident(
        &self,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ApiError> {
        let incident = records::update_incident_status_by(
            &self.database,
            IncidentStatusInput {
                id: text(arguments, "incidentId")?.to_owned(),
                status: text(arguments, "status")?.to_owned(),
                note: optional_text(arguments, "note")?
                    .unwrap_or_default()
                    .to_owned(),
            },
            &self.actor()?,
        )?;
        Ok(json!({ "incident": incident }))
    }

    pub(super) fn record_event(&self, arguments: &Map<String, Value>) -> Result<Value, ApiError> {
        let subject = super::find_system(&self.inventory()?, arguments)?;
        let event = records::record_event_by(
            &self.database,
            EventInput {
                subject,
                kind: text(arguments, "kind")?.to_owned(),
                outcome: text(arguments, "outcome")?.to_owned(),
                title: text(arguments, "title")?.to_owned(),
                detail: optional_text(arguments, "detail")?
                    .unwrap_or_default()
                    .to_owned(),
                occurred_at: optional_text(arguments, "occurredAt")?.map(str::to_owned),
            },
            &self.actor()?,
        )?;
        Ok(json!({ "event": event }))
    }

    pub(super) fn backup(&self) -> Result<Value, ApiError> {
        self.existing()?;
        Ok(json!({ "backup": backup::back_up(&self.database)? }))
    }
}

fn tool(
    name: &str,
    title: &str,
    description: &str,
    properties: Value,
    required: &[&str],
    hints: Value,
) -> Value {
    let mut annotations = json!({ "title": title, "readOnlyHint": false, "openWorldHint": false });
    if let (Value::Object(annotations), Value::Object(hints)) = (&mut annotations, hints) {
        annotations.extend(hints);
    }
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required, "additionalProperties": false },
        "annotations": annotations,
    })
}

fn with_system(extra: Value) -> Value {
    super::system_arguments(extra)
}

fn named(kind: &str) -> Value {
    json!({
        "id": { "type": "string", "description": format!("The {kind}'s id, from list_systems.") },
        "name": { "type": "string", "description": "Or its exact name, ignoring case." },
    })
}

/// The tools that change something. Saving and deleting overwrite or remove configuration, so
/// they are marked destructive; adding a record never changes another.
pub(super) fn tools() -> Vec<Value> {
    let additive = json!({ "destructiveHint": false, "idempotentHint": false });
    let overwriting = json!({ "destructiveHint": true, "idempotentHint": false });
    vec![
        tool(
            "save_application",
            "Add or edit an application",
            "Adds an application (name and lifecycle required), or edits the one with the given id. An edit keeps what it leaves out. `components`, when given, is the complete list: include the existing ones with their ids (from get_system) to keep them; `runsOn` names a resource by id or exact name, or null. `version: null` clears the version. Logged as the agent's change.",
            json!({
                "id": { "type": "string", "description": "The application to edit; leave out to add one." },
                "name": { "type": "string" },
                "lifecycle": { "type": "string", "enum": ["development", "testing", "production", "archived"] },
                "description": { "type": "string" },
                "version": { "type": ["string", "null"] },
                "components": { "type": "array", "items": { "type": "object", "properties": {
                    "id": { "type": "string", "description": "An existing component's id, to keep it." },
                    "name": { "type": "string" },
                    "kind": { "type": "string", "enum": ["backend", "database", "docker", "frontend"] },
                    "runsOn": { "type": ["string", "null"], "description": "A resource's id or exact name." },
                }, "required": ["name", "kind"], "additionalProperties": false } },
            }),
            &[],
            overwriting.clone(),
        ),
        tool(
            "save_resource",
            "Add or edit a resource",
            "Adds a resource (name and kind required), or edits the one with the given id, keeping what the edit leaves out. Logged as the agent's change.",
            json!({
                "id": { "type": "string", "description": "The resource to edit; leave out to add one." },
                "name": { "type": "string" },
                "kind": { "type": "string", "enum": ["server", "workstation"] },
                "description": { "type": "string" },
                "operatingSystem": { "type": "string" },
            }),
            &[],
            overwriting.clone(),
        ),
        tool(
            "delete_application",
            "Delete an application without history",
            "Deletes an application that has no observations, incidents or events, with its components and HTTP check. One with history is refused: DevHQ never erases history; set its lifecycle to archived instead. Logged as the agent's change.",
            named("application"),
            &[],
            json!({ "destructiveHint": true, "idempotentHint": false }),
        ),
        tool(
            "delete_resource",
            "Delete a resource without history",
            "Deletes a resource that no component runs on and that has no observations, incidents or events, with its HTTP check. Anything else is refused and explained. Logged as the agent's change.",
            named("resource"),
            &[],
            json!({ "destructiveHint": true, "idempotentHint": false }),
        ),
        tool(
            "record_observation",
            "Record an observation",
            "Records what the agent observed about a system, tagged as the agent's: a result, what it saw, and how long that vouches for the system (validForMinutes, 1 to 43200). observedAt (RFC 3339) defaults to now and cannot be in the future. Observations are immutable; record another to correct one.",
            with_system(json!({
                "result": { "type": "string", "enum": ["healthy", "degraded", "unhealthy"] },
                "summary": { "type": "string", "description": "What was observed, up to 500 characters." },
                "validForMinutes": { "type": "integer", "minimum": 1, "maximum": 43200 },
                "observedAt": { "type": "string", "description": "RFC 3339, with a time zone." },
            })),
            &["type", "result", "summary", "validForMinutes"],
            additive.clone(),
        ),
        tool(
            "open_incident",
            "Open an incident",
            "Opens an incident about a system, tagged as the agent's.",
            with_system(json!({
                "title": { "type": "string" },
                "severity": { "type": "string", "enum": ["info", "warning", "critical"] },
                "description": { "type": "string" },
            })),
            &["type", "title", "severity"],
            additive.clone(),
        ),
        tool(
            "update_incident",
            "Change an incident's status",
            "Moves an incident to open, investigating or resolved, with an optional note. The change is the agent's; the incident keeps who opened it.",
            json!({
                "incidentId": { "type": "string" },
                "status": { "type": "string", "enum": ["open", "investigating", "resolved"] },
                "note": { "type": "string" },
            }),
            &["incidentId", "status"],
            additive.clone(),
        ),
        tool(
            "record_event",
            "Log an event",
            "Logs a deployment, backup, maintenance or note for a system, tagged as the agent's. occurredAt (RFC 3339) defaults to now and cannot be in the future.",
            with_system(json!({
                "kind": { "type": "string", "enum": ["deployment", "backup", "maintenance", "note"] },
                "outcome": { "type": "string", "enum": ["success", "warning", "failure", "info"] },
                "title": { "type": "string" },
                "detail": { "type": "string" },
                "occurredAt": { "type": "string", "description": "RFC 3339, with a time zone." },
            })),
            &["type", "kind", "outcome", "title"],
            additive.clone(),
        ),
        tool(
            "save_check",
            "Set up or change an HTTP check",
            "Sets up a system's only HTTP check, a read-only GET (url required; expectedStatus 200, timeoutSeconds 5 and validForMinutes 60 by default), or changes it, keeping what the change leaves out. supervisionIntervalMinutes (5 to 60) has DevHQ run it on that schedule while its window is open; null runs it only when asked; validity must outlast the interval. With a DevHQ window open (status: windowOpen), that window runs a newly supervised check within about 2 s, and the answer's note says so. Logged as the agent's change.",
            with_system(json!({
                "url": { "type": "string" },
                "expectedStatus": { "type": "integer" },
                "timeoutSeconds": { "type": "integer", "minimum": 1, "maximum": 10 },
                "validForMinutes": { "type": "integer", "minimum": 1, "maximum": 43200 },
                "supervisionIntervalMinutes": { "type": ["integer", "null"], "minimum": 5, "maximum": 60 },
            })),
            &["type"],
            overwriting.clone(),
        ),
        tool(
            "set_check_paused",
            "Pause or resume supervision",
            "Pauses or resumes the supervision of a system's HTTP check; it keeps its interval. Logged as the agent's change.",
            with_system(json!({ "paused": { "type": "boolean" } })),
            &["type", "paused"],
            overwriting.clone(),
        ),
        tool(
            "remove_check",
            "Remove an HTTP check",
            "Removes a system's HTTP check. The evidence it recorded stays. Logged as the agent's change.",
            with_system(json!({})),
            &["type"],
            json!({ "destructiveHint": true, "idempotentHint": false }),
        ),
        tool(
            "run_check",
            "Run an HTTP check now",
            "Runs a system's HTTP check once, as Run check does, and returns the observation it recorded (DevHQ's own evidence, source http). The request leaves this computer. Not needed right after save_check or set_check_paused answered with a note that an open DevHQ window runs it.",
            with_system(json!({})),
            &["type"],
            json!({ "destructiveHint": false, "idempotentHint": false, "openWorldHint": true }),
        ),
        tool(
            "backup",
            "Back up the inventory",
            "Copies the inventory into its backups folder and verifies the copy, as Back up now does. Changes nothing in the inventory.",
            json!({}),
            &[],
            json!({ "destructiveHint": false, "idempotentHint": false }),
        ),
    ]
}
