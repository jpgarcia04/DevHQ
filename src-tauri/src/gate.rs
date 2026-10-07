//! The agent gate: `devhq --mcp` serves DevHQ's inventory to an agent (Claude Code, Codex or any
//! MCP client) over the Model Context Protocol, so that it can do what the person does at the
//! window when the person asks it to. The client starts this process for a session and ends it
//! with the session: nothing listens on a port, nothing runs in the background, and no window or
//! WebView is created.
//!
//! The protocol is JSON-RPC 2.0, one message per line on stdin and stdout (MCP's stdio
//! transport); stderr is free for diagnostics. Every tool goes through the same use cases as the
//! window's commands, with the same validation, limits and transactions.

mod writes;

use crate::access;
use crate::attention::{self, AttentionItem};
use crate::http_check::HttpCheck;
use crate::inventory::{self, ApiError, Inventory};
use crate::records::{self, Subject};
use crate::references::Reference;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// Tauri's identifier for DevHQ, which names its data directory. A test holds it to
/// `tauri.conf.json`.
pub const IDENTIFIER: &str = "com.devhq.desktop";

/// Revisions of the protocol this server speaks, newest first. A client asking for one of them
/// gets it; any other gets the newest, and decides whether to go on.
const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// What the person's DevHQ opens: `inventory.sqlite3` in the data directory Tauri gives it
/// (`app_data_dir`, which is the system's data directory followed by the identifier).
pub fn default_database() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join(IDENTIFIER).join("inventory.sqlite3"))
}

/// `devhq --mcp [--database <path>]`: serves stdin and stdout until the client closes stdin.
/// Returns the process exit code.
pub fn main(args: impl IntoIterator<Item = String>) -> i32 {
    let mut database = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mcp" => {}
            "--database" => match args.next() {
                Some(path) => database = Some(PathBuf::from(path)),
                None => return usage("--database needs a path"),
            },
            other => return usage(&format!("unknown argument {other}")),
        }
    }
    let Some(database) = database.or_else(default_database) else {
        return usage("the system has no data directory; pass --database");
    };
    let stdin = std::io::stdin();
    match serve(&database, stdin.lock(), std::io::stdout().lock()) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("devhq --mcp: {error}");
            1
        }
    }
}

fn usage(problem: &str) -> i32 {
    eprintln!("devhq --mcp: {problem}\nusage: devhq --mcp [--database <path>]");
    2
}

/// Answers each message read from `input` on `output` until `input` ends.
pub fn serve(database: &Path, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
    let mut session = Session {
        database: database.to_owned(),
        client: None,
    };
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = session.handle(&line) {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

struct Session {
    database: PathBuf,
    /// The name the client declared in `initialize`. Unverified: it is only ever shown as such.
    client: Option<String>,
}

// JSON-RPC error codes.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

fn error_reply(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

impl Session {
    /// The reply to one message, or None for a notification or a response, which get none.
    fn handle(&mut self, line: &str) -> Option<Value> {
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(error) => return Some(error_reply(Value::Null, PARSE_ERROR, error.to_string())),
        };
        let Some(object) = message.as_object() else {
            return Some(error_reply(
                Value::Null,
                INVALID_REQUEST,
                "Expected one JSON-RPC message per line (batches are not part of MCP).",
            ));
        };
        let id = object.get("id").cloned();
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            // A response to a request of ours: this server never sends any.
            return id.map(|id| error_reply(id, INVALID_REQUEST, "A request needs a method."));
        };
        // Notifications (no id) are never answered; `notifications/initialized` and
        // `notifications/cancelled` need nothing from a server that answers each request at once.
        let id = id?;
        let params = object.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => Ok(self.initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => self.call(&params),
            other => Err((METHOD_NOT_FOUND, format!("Unknown method {other}."))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => error_reply(id, code, message),
        })
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str);
        let version = PROTOCOL_VERSIONS
            .into_iter()
            .find(|version| Some(*version) == asked)
            .unwrap_or(PROTOCOL_VERSIONS[0]);
        self.client = params
            .pointer("/clientInfo/name")
            .and_then(Value::as_str)
            .map(str::to_owned);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "devhq", "title": "DevHQ", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        })
    }

    fn call(&self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params.get("name").and_then(Value::as_str).ok_or((
            INVALID_PARAMS,
            "tools/call needs the tool's name.".to_owned(),
        ))?;
        let empty = Map::new();
        let arguments = match params.get("arguments") {
            None | Some(Value::Null) => &empty,
            Some(Value::Object(arguments)) => arguments,
            Some(_) => return Err((INVALID_PARAMS, "Tool arguments must be an object.".into())),
        };
        let outcome = match name {
            // Always answered, so that an agent can tell the person how to open the gate.
            "status" => self.status(),
            "attention" => self.allow("read").and_then(|()| self.attention()),
            "list_systems" => self.allow("read").and_then(|()| self.list_systems()),
            "get_system" => self.allow("read").and_then(|()| self.get_system(arguments)),
            "history" => self.allow("read").and_then(|()| self.history(arguments)),
            // Configuration changes check the access inside their own transaction; records,
            // checks run and backups check it first.
            "save_application" => self.save_application(arguments),
            "save_resource" => self.save_resource(arguments),
            "delete_application" => self.delete("application", arguments),
            "delete_resource" => self.delete("resource", arguments),
            "save_check" => self
                .save_check(arguments)
                .map(|value| self.window_note(value)),
            "set_check_paused" => self
                .set_check_paused(arguments)
                .map(|value| self.window_note(value)),
            "remove_check" => self.remove_check(arguments),
            "run_check" => self.allow("write").and_then(|()| self.run_check(arguments)),
            "record_observation" => self
                .allow("write")
                .and_then(|()| self.record_observation(arguments)),
            "open_incident" => self
                .allow("write")
                .and_then(|()| self.open_incident(arguments)),
            "update_incident" => self
                .allow("write")
                .and_then(|()| self.update_incident(arguments)),
            "record_event" => self
                .allow("write")
                .and_then(|()| self.record_event(arguments)),
            "backup" => self.allow("write").and_then(|()| self.backup()),
            other => return Err((INVALID_PARAMS, format!("Unknown tool {other}."))),
        };
        // A tool that fails tells the agent why, as the window would tell the person; it is a
        // result, not a protocol error, so the agent can read it and correct itself.
        Ok(match outcome {
            Ok(value) => json!({
                "content": [{ "type": "text", "text": value.to_string() }],
                "structuredContent": value,
                "isError": false,
            }),
            Err(error) => json!({
                "content": [{ "type": "text", "text": format!("{} ({})", error.message, error.code) }],
                "isError": true,
            }),
        })
    }

    /// The gate never creates an empty inventory where the person's DevHQ would not find it.
    fn existing(&self) -> Result<(), ApiError> {
        if self.database.is_file() {
            Ok(())
        } else {
            Err(ApiError::invalid(format!(
                "There is no DevHQ inventory at {}. Open DevHQ once to create it, or start the gate with --database <path>.",
                self.database.display()
            )))
        }
    }

    /// Whether the person lets agents `read` or `write` through the gate, checked on every call
    /// so that a change in Settings applies at once.
    fn allow(&self, need: &str) -> Result<(), ApiError> {
        self.existing()?;
        permitted(&access::mode(&inventory::open(&self.database)?)?, need)
    }

    fn inventory(&self) -> Result<Inventory, ApiError> {
        self.existing()?;
        inventory::list(&self.database)
    }

    fn status(&self) -> Result<Value, ApiError> {
        let location = inventory::DataLocation::of(&self.database);
        let exists = self.database.is_file();
        let access = if exists {
            Some(access::mode(&inventory::open(&self.database)?)?)
        } else {
            None
        };
        // What the inventory holds is for agents the person let in.
        let counts = if access.as_deref().is_some_and(|mode| mode != "off") {
            let inventory = self.inventory()?;
            json!({ "applications": inventory.applications.len(), "resources": inventory.resources.len() })
        } else {
            Value::Null
        };
        let window_open = crate::window_lock::window_open(&self.database);
        let mut notes = vec![
            match window_open {
                Some(true) => "A DevHQ window is open on this inventory. It notices what the gate commits within about 2 s: it shows it, and its supervision follows a check configured, paused, removed or run here.",
                Some(false) => "No DevHQ window is open on this inventory, so nothing supervises its HTTP checks: they run only when asked. What the gate writes is there when DevHQ opens.",
                None => "The system could not say whether a DevHQ window is open on this inventory.",
            }
            .to_owned(),
            "Supervision's live state (runs in flight, next run, DevHQ's own run errors) exists only in the DevHQ window's process; the gate does not know it.".to_owned(),
        ];
        if let Some(stored_at) = &location.stored_at {
            notes.push(format!(
                "The system keeps this file at {stored_at}, not where DevHQ asked for it. A DevHQ started another way (from the Start menu or File Explorer) may see a different inventory."
            ));
        }
        Ok(json!({
            "database": location,
            "exists": exists,
            "schemaVersion": exists.then_some(inventory::SCHEMA_VERSION),
            "access": access,
            "windowOpen": window_open,
            "counts": counts,
            "client": self.client,
            "notes": notes,
        }))
    }

    fn attention(&self) -> Result<Value, ApiError> {
        let inventory = self.inventory()?;
        let now = records::now_ms();
        let names = Names::of(&inventory);
        let subjects = subjects(&inventory);
        let attention = attention::assess(
            &subjects,
            &inventory.observations,
            &inventory.incidents,
            now,
        )?;
        let items: Vec<Value> = attention
            .items
            .iter()
            .map(|item| match item {
                AttentionItem::Incident { subject, incident } => json!({
                    "kind": "incident",
                    "system": names.system(subject),
                    "incident": incident,
                }),
                AttentionItem::Evidence {
                    subject,
                    state,
                    observation,
                } => json!({
                    "kind": "evidence",
                    "system": names.system(subject),
                    "state": state,
                    "observation": observation,
                }),
            })
            .collect();
        let mut never_observed = Vec::new();
        let mut not_supervised = Vec::new();
        for subject in &subjects {
            if attention::evidence(&inventory.observations, subject, now)?.0 == "unobserved" {
                never_observed.push(names.system(subject));
            }
            if check_of(&inventory, subject).is_none_or(|check| check.monitor != "active") {
                not_supervised.push(names.system(subject));
            }
        }
        let meaning = match attention.status {
            "empty" => "There are no applications or resources yet. That is not healthy: there is nothing to know about.",
            "attention" => "The items need attention, most urgent first.",
            "unobserved" => "Nothing observed needs attention, but some systems were never observed. That is a gap, not health.",
            _ => "Every system has current evidence and nothing needs attention.",
        };
        Ok(json!({
            "now": records::format_time(now),
            "status": attention.status,
            "meaning": meaning,
            "items": items,
            "coverage": attention.coverage,
            "neverObserved": never_observed,
            "notSupervised": not_supervised,
            "history": history_note(&inventory),
        }))
    }

    fn list_systems(&self) -> Result<Value, ApiError> {
        let inventory = self.inventory()?;
        let now = records::now_ms();
        let names = Names::of(&inventory);
        let applications = inventory
            .applications
            .iter()
            .map(|application| {
                summary(
                    &inventory,
                    &names,
                    &Subject::Application {
                        id: application.id.clone(),
                    },
                    now,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let resources = inventory
            .resources
            .iter()
            .map(|resource| {
                summary(
                    &inventory,
                    &names,
                    &Subject::Resource {
                        id: resource.id.clone(),
                    },
                    now,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(json!({
            "now": records::format_time(now),
            "applications": applications,
            "resources": resources,
            "history": history_note(&inventory),
        }))
    }

    fn get_system(&self, arguments: &Map<String, Value>) -> Result<Value, ApiError> {
        let inventory = self.inventory()?;
        let now = records::now_ms();
        let names = Names::of(&inventory);
        let subject = find_system(&inventory, arguments)?;
        let mut system = summary(&inventory, &names, &subject, now)?;
        let mine = |record_subject: &Subject| record_subject == &subject;
        let detail = json!({
            "observations": inventory.observations.iter().filter(|item| mine(&item.subject)).collect::<Vec<_>>(),
            "incidents": inventory.incidents.iter().filter(|item| mine(&item.subject)).collect::<Vec<_>>(),
            "events": inventory.events.iter().filter(|item| mine(&item.subject)).collect::<Vec<_>>(),
            "changes": inventory.changes.iter().filter(|item| mine(&item.subject)).collect::<Vec<_>>(),
            "automatedCompleteSince": inventory
                .automated_history
                .truncated
                .iter()
                .find(|item| item.subject == subject)
                .map(|item| &item.since),
        });
        if let (Value::Object(system), Value::Object(detail)) = (&mut system, detail) {
            system.extend(detail);
        }
        Ok(
            json!({ "now": records::format_time(now), "system": system, "history": history_note(&inventory) }),
        )
    }

    fn history(&self, arguments: &Map<String, Value>) -> Result<Value, ApiError> {
        let inventory = self.inventory()?;
        let subject = find_system(&inventory, arguments)?;
        let before = text(arguments, "before")?;
        match arguments.get("records").and_then(Value::as_str) {
            Some("observations") => {
                let page = records::older_observations(&self.database, &subject, before)?;
                Ok(json!({ "observations": page.observations, "more": page.more }))
            }
            Some("events") => {
                let page =
                    records::older_events(&self.database, std::slice::from_ref(&subject), before)?;
                Ok(json!({ "events": page.events, "more": page.more }))
            }
            _ => Err(ApiError::invalid(
                "records must be \"observations\" or \"events\".",
            )),
        }
    }
}

/// Whether the access mode in force allows what a tool needs: `read` or `write`.
fn permitted(mode: &str, need: &str) -> Result<(), ApiError> {
    let enough = match need {
        "read" => mode != "off",
        _ => mode == "write",
    };
    match (enough, mode) {
        (true, _) => Ok(()),
        (false, "off") => Err(ApiError::denied(
            "The person has not opened the agent gate. They can choose Read only or Read and write in DevHQ, under Settings → Agent gate.",
        )),
        (false, _) => Err(ApiError::denied(
            "The person opened the agent gate for reading only. To let agents change the inventory, they can choose Read and write in DevHQ, under Settings → Agent gate.",
        )),
    }
}

const INSTRUCTIONS: &str = "DevHQ is the person's local operations inventory: applications, the resources they run on, where each one is (its references: repository, local copies, documentation, logs, dashboard), and the evidence about them (observations, incidents, events, HTTP checks). Working in a folder, an agent can find its system by a local copy's path or by its repository's URL in `list_systems`. Start with `attention` to answer «what needs my attention?». Never present a system without current evidence as healthy: unobserved and expired evidence are gaps, not health. Every record says who produced it (`source`: manual, http or agent) and when.";

/// Every application, then every resource, in the inventory's order.
fn subjects(inventory: &Inventory) -> Vec<Subject> {
    inventory
        .applications
        .iter()
        .map(|item| Subject::Application {
            id: item.id.clone(),
        })
        .chain(inventory.resources.iter().map(|item| Subject::Resource {
            id: item.id.clone(),
        }))
        .collect()
}

fn check_of<'a>(inventory: &'a Inventory, subject: &Subject) -> Option<&'a HttpCheck> {
    inventory
        .http_checks
        .iter()
        .find(|check| &check.subject == subject)
}

/// Names by subject, so that every reference an agent reads says what it is.
struct Names(HashMap<Subject, String>);

impl Names {
    fn of(inventory: &Inventory) -> Self {
        Self(
            inventory
                .applications
                .iter()
                .map(|item| {
                    (
                        Subject::Application {
                            id: item.id.clone(),
                        },
                        item.name.clone(),
                    )
                })
                .chain(inventory.resources.iter().map(|item| {
                    (
                        Subject::Resource {
                            id: item.id.clone(),
                        },
                        item.name.clone(),
                    )
                }))
                .collect(),
        )
    }

    fn system(&self, subject: &Subject) -> Value {
        let (kind, id) = match subject {
            Subject::Application { id } => ("application", id),
            Subject::Resource { id } => ("resource", id),
        };
        json!({ "type": kind, "id": id, "name": self.0.get(subject) })
    }
}

/// Where a system is, as an agent reads it: each path's machine by name, and for a repository
/// the other systems that declare the same one, derived at this read as the window derives it.
fn reference_list(
    inventory: &Inventory,
    names: &Names,
    subject: &Subject,
    references: &[Reference],
) -> Vec<Value> {
    references
        .iter()
        .map(|reference| {
            let mut value = json!({
                "id": reference.id,
                "kind": reference.kind,
                "target": reference.target,
                "label": reference.label,
                "machine": reference.machine_id.as_ref().map(|id| names.system(&Subject::Resource { id: id.clone() })),
            });
            if reference.kind == "repository" {
                let shared: Vec<Value> = inventory
                    .shared_repositories
                    .iter()
                    .filter(|repository| repository.target == reference.target)
                    .flat_map(|repository| &repository.systems)
                    .filter(|system| *system != subject)
                    .map(|system| names.system(system))
                    .collect();
                value["sameRepositoryAs"] = json!(shared);
            }
            value
        })
        .collect()
}

/// What the window shows about a system at a glance: identity, evidence, active incidents and
/// its HTTP check, plus what it is related to and where it is.
fn summary(
    inventory: &Inventory,
    names: &Names,
    subject: &Subject,
    now: i64,
) -> Result<Value, ApiError> {
    let (state, latest) = attention::evidence(&inventory.observations, subject, now)?;
    let active_incidents = inventory
        .incidents
        .iter()
        .filter(|item| &item.subject == subject && item.status != "resolved")
        .count();
    let mut value = match subject {
        Subject::Application { id } => {
            let application = inventory.applications.iter().find(|item| &item.id == id);
            let components: Vec<Value> = application
                .map(|item| item.components.iter())
                .into_iter()
                .flatten()
                .map(|component| {
                    json!({
                        "id": component.id,
                        "name": component.name,
                        "kind": component.kind,
                        "runsOn": component.resource_id.as_ref().map(|id| names.system(&Subject::Resource { id: id.clone() })),
                    })
                })
                .collect();
            json!({ "system": names.system(subject), "application": application.map(|item| json!({
                "description": item.description, "lifecycle": item.lifecycle, "version": item.version,
            })), "components": components, "references": application.map(|item| reference_list(inventory, names, subject, &item.references)) })
        }
        Subject::Resource { id } => {
            let resource = inventory.resources.iter().find(|item| &item.id == id);
            let used_by: Vec<Value> = inventory
                .applications
                .iter()
                .filter(|item| {
                    item.components
                        .iter()
                        .any(|part| part.resource_id.as_ref() == Some(id))
                })
                .map(|item| {
                    names.system(&Subject::Application {
                        id: item.id.clone(),
                    })
                })
                .collect();
            json!({ "system": names.system(subject), "resource": resource.map(|item| json!({
                "description": item.description, "kind": item.kind, "operatingSystem": item.operating_system,
            })), "usedBy": used_by, "references": resource.map(|item| reference_list(inventory, names, subject, &item.references)) })
        }
    };
    if let Value::Object(object) = &mut value {
        object.insert(
            "evidence".into(),
            json!({ "state": state, "latest": latest }),
        );
        object.insert("activeIncidents".into(), json!(active_incidents));
        object.insert("httpCheck".into(), json!(check_of(inventory, subject)));
    }
    Ok(value)
}

/// What the snapshot leaves out, so that an agent never takes a cut history for a short one.
fn history_note(inventory: &Inventory) -> Value {
    json!({
        "automatedResultsPerSystem": inventory.automated_history.limit,
        "cut": inventory.automated_history.truncated,
        "changesCompleteSince": inventory.changes_since,
        "note": "Manual and agent records are always complete. Automated HTTP results are the latest ones per system; for each system in `cut`, older ones are kept but only loaded through `history`. Configuration changes (`changes`), by the person or by an agent, are complete from `changesCompleteSince` on: before it, only agents' changes were logged (null: always complete).",
    })
}

fn text<'a>(arguments: &'a Map<String, Value>, key: &str) -> Result<&'a str, ApiError> {
    arguments.get(key).and_then(Value::as_str).ok_or_else(|| {
        ApiError::invalid(format!("The argument {key} is required and must be text."))
    })
}

/// The system an agent names by type and either ID or name (exact, ignoring case).
fn find_system(inventory: &Inventory, arguments: &Map<String, Value>) -> Result<Subject, ApiError> {
    let kind = system_type(arguments)?;
    let candidates: Vec<(String, String)> = match kind {
        "application" => inventory
            .applications
            .iter()
            .map(|item| (item.id.clone(), item.name.clone()))
            .collect(),
        _ => inventory
            .resources
            .iter()
            .map(|item| (item.id.clone(), item.name.clone()))
            .collect(),
    };
    pick(kind, &candidates, arguments).map(|(subject, _)| subject)
}

fn system_type(arguments: &Map<String, Value>) -> Result<&'static str, ApiError> {
    match text(arguments, "type")? {
        "application" => Ok("application"),
        "resource" => Ok("resource"),
        _ => Err(ApiError::invalid(
            "type must be \"application\" or \"resource\".",
        )),
    }
}

/// Among `candidates` (ID, name) of one kind, the one the arguments name by `id`, or else by
/// `name` (exact, ignoring case and surrounding spaces): its subject and name.
fn pick(
    kind: &str,
    candidates: &[(String, String)],
    arguments: &Map<String, Value>,
) -> Result<(Subject, String), ApiError> {
    let id = arguments.get("id").and_then(Value::as_str);
    let name = arguments.get("name").and_then(Value::as_str);
    let found: Vec<&(String, String)> = match (id, name) {
        (Some(id), _) => candidates.iter().filter(|(item, _)| item == id).collect(),
        (None, Some(name)) => candidates
            .iter()
            .filter(|(_, item)| item.to_lowercase() == name.trim().to_lowercase())
            .collect(),
        (None, None) => return Err(ApiError::invalid("Name the system by id or by name.")),
    };
    let (id, name) = match found.as_slice() {
        [(id, name)] => (id.clone(), name.clone()),
        [] => {
            return Err(ApiError::invalid(format!(
                "No {kind} matches. `list_systems` lists them."
            )))
        }
        _ => {
            return Err(ApiError::invalid(format!(
                "Several {kind}s have that name; use its id."
            )))
        }
    };
    let subject = match kind {
        "application" => Subject::Application { id },
        _ => Subject::Resource { id },
    };
    Ok((subject, name))
}

fn system_arguments(extra: Value) -> Value {
    let mut properties = json!({
        "type": { "type": "string", "enum": ["application", "resource"] },
        "id": { "type": "string", "description": "The system's id, from list_systems." },
        "name": { "type": "string", "description": "Or its exact name, ignoring case." },
    });
    if let (Value::Object(properties), Value::Object(extra)) = (&mut properties, extra) {
        properties.extend(extra);
    }
    properties
}

fn read_only(
    name: &str,
    title: &str,
    description: &str,
    properties: Value,
    required: &[&str],
) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required, "additionalProperties": false },
        "annotations": { "title": title, "readOnlyHint": true, "openWorldHint": false },
    })
}

fn tools() -> Vec<Value> {
    let mut tools = read_tools();
    tools.extend(writes::tools());
    tools
}

fn read_tools() -> Vec<Value> {
    vec![
        read_only(
            "status",
            "DevHQ status",
"Which inventory file the gate opened, where the system really keeps it, its schema version, whether the person lets agents read or write through the gate (`access`: off, read or write), whether a DevHQ window is open on it (`windowOpen`; null when the system cannot say), how many systems it holds, and what the gate cannot know. Always answered, even with the gate off.",
            json!({}),
            &[],
        ),
        read_only(
            "attention",
            "What needs attention",
            "«What needs my attention?», ranked as DevHQ's Overview ranks it: active incidents and problem evidence, most urgent first, plus coverage (current, expired, never observed) and the systems nothing supervises. An empty list is only reassuring when status is `covered`.",
            json!({}),
            &[],
        ),
        read_only(
            "list_systems",
            "List applications and resources",
            "Every application and resource with its evidence state and latest observation, active incidents, HTTP check, relations (components and what they run on; what uses each resource) and references: where its repository, its local copies (each path with the machine it is on), its documentation, its logs and its dashboard are, and which other systems declare the same repository (`sameRepositoryAs`, by exact URL). References are declared inventory: DevHQ never opens or checks them.",
            json!({}),
            &[],
        ),
        read_only(
            "get_system",
            "System detail",
            "One application or resource with its references (as list_systems gives them), every loaded observation, incident and event about it, every change to its configuration (who made it, and each field before and after), newest first, and from when its automated results are complete.",
            system_arguments(json!({})),
            &["type"],
        ),
        read_only(
            "history",
            "Older automated results",
            "Automated HTTP results (observations, or their events) older than one already shown, 200 at a time, for a system whose automated history get_system says is cut.",
            system_arguments(json!({
                "records": { "type": "string", "enum": ["observations", "events"] },
                "before": { "type": "string", "description": "The id of the oldest automated observation or event already shown." },
            })),
            &["type", "records", "before"],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_check::{save_http_check, HttpCheckInput};
    use crate::inventory::{
        save_application, save_resource, ApplicationInput, ComponentInput, ResourceInput,
    };
    use crate::records::{open_incident, record_observation, IncidentInput, ObservationInput};

    /// An inventory path in a folder of its own.
    /// A fresh inventory path in the temporary folder as the file system names it, so that only
    /// a link the test makes, never a short 8.3 name in `TMP` (as on a Windows runner), can put
    /// the file somewhere else than asked.
    fn path() -> PathBuf {
        let folder = crate::inventory::tests::resolved_temp_dir()
            .join(format!("devhq-gate-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        folder.join("inventory.sqlite3")
    }

    fn remove(path: &Path) {
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    /// Shop runs its API on Web host; Spare box runs nothing and was never observed. Web host
    /// is unhealthy and supervised, Shop has a critical incident and healthy evidence.
    fn seed(path: &Path) -> (String, String) {
        let host = save_resource(
            path,
            ResourceInput {
                id: None,
                name: "Web host".into(),
                kind: "server".into(),
                description: String::new(),
                operating_system: "Debian 13".into(),
                references: vec![],
            },
        )
        .unwrap();
        save_resource(
            path,
            ResourceInput {
                id: None,
                name: "Spare box".into(),
                kind: "workstation".into(),
                description: String::new(),
                operating_system: String::new(),
                references: vec![],
            },
        )
        .unwrap();
        let shop = save_application(
            path,
            ApplicationInput {
                id: None,
                name: "Shop".into(),
                description: "Store front".into(),
                lifecycle: "production".into(),
                version: Some("2.4.1".into()),
                components: vec![ComponentInput {
                    id: None,
                    name: "API".into(),
                    kind: "backend".into(),
                    resource_id: Some(host.id.clone()),
                }],
                references: vec![],
            },
        )
        .unwrap();
        let observe = |subject: Subject, result: &str| {
            record_observation(
                path,
                ObservationInput {
                    subject,
                    result: result.into(),
                    summary: format!("Looked {result}"),
                    observed_at: None,
                    valid_for_minutes: 60,
                },
            )
            .unwrap();
        };
        observe(
            Subject::Application {
                id: shop.id.clone(),
            },
            "healthy",
        );
        observe(
            Subject::Resource {
                id: host.id.clone(),
            },
            "unhealthy",
        );
        open_incident(
            path,
            IncidentInput {
                subject: Subject::Application {
                    id: shop.id.clone(),
                },
                title: "Checkout fails".into(),
                description: String::new(),
                severity: "critical".into(),
            },
        )
        .unwrap();
        save_http_check(
            path,
            HttpCheckInput {
                subject: Subject::Resource {
                    id: host.id.clone(),
                },
                url: "https://host.example/health".into(),
                expected_status: 200,
                timeout_seconds: 5,
                valid_for_minutes: 30,
                monitor_interval_minutes: Some(15),
            },
        )
        .unwrap();
        crate::access::set_agent_access(path, "read").unwrap();
        (shop.id, host.id)
    }

    /// Sends each message, one per line, and returns every reply.
    fn exchange(database: &Path, messages: &[Value]) -> Vec<Value> {
        let input: String = messages
            .iter()
            .map(|message| format!("{message}\n"))
            .collect();
        let mut output = Vec::new();
        serve(database, input.as_bytes(), &mut output).unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn call(database: &Path, tool: &str, arguments: Value) -> Value {
        let replies = exchange(
            database,
            &[
                json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": tool, "arguments": arguments } }),
            ],
        );
        replies[0]["result"].clone()
    }

    /// A tool's answer, which must not be an error, as structured content. Its text content
    /// carries the same JSON for clients that read only text.
    fn answer(database: &Path, tool: &str, arguments: Value) -> Value {
        let result = call(database, tool, arguments);
        assert_eq!(result["isError"], false, "{result:#}");
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, result["structuredContent"]);
        result["structuredContent"].clone()
    }

    fn refusal(database: &Path, tool: &str, arguments: Value) -> String {
        let result = call(database, tool, arguments);
        assert_eq!(result["isError"], true, "{result:#}");
        result["content"][0]["text"].as_str().unwrap().to_owned()
    }

    #[test]
    fn speaks_mcp_over_one_json_message_per_line() {
        let path = path();
        let replies = exchange(
            &path,
            &[
                json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                    "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "claude-code", "version": "2" },
                } }),
                // Notifications get no reply.
                json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
                json!({ "jsonrpc": "2.0", "id": "two", "method": "ping" }),
                json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
                json!({ "jsonrpc": "2.0", "id": 4, "method": "resources/list" }),
                json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": { "name": "drop_everything" } }),
                json!({ "jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": { "name": "status", "arguments": [] } }),
                json!([{ "jsonrpc": "2.0", "id": 7, "method": "ping" }]),
            ],
        );
        assert_eq!(replies.len(), 7);
        assert_eq!(replies[0]["id"], 1);
        assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(replies[0]["result"]["serverInfo"]["name"], "devhq");
        assert_eq!(replies[0]["result"]["capabilities"], json!({ "tools": {} }));
        assert_eq!(
            replies[1],
            json!({ "jsonrpc": "2.0", "id": "two", "result": {} })
        );
        let tools = replies[2]["result"]["tools"].as_array().unwrap();
        let names = |read_only: bool| -> Vec<&str> {
            tools
                .iter()
                .filter(|tool| tool["annotations"]["readOnlyHint"] == read_only)
                .map(|tool| tool["name"].as_str().unwrap())
                .collect()
        };
        assert_eq!(
            names(true),
            [
                "status",
                "attention",
                "list_systems",
                "get_system",
                "history"
            ]
        );
        assert_eq!(
            names(false),
            [
                "save_application",
                "save_resource",
                "delete_application",
                "delete_resource",
                "record_observation",
                "open_incident",
                "update_incident",
                "record_event",
                "save_check",
                "set_check_paused",
                "remove_check",
                "run_check",
                "backup",
            ]
        );
        assert!(tools
            .iter()
            .all(|tool| tool["inputSchema"]["type"] == "object"));
        // Only what removes configuration is destructive, and only a check run leaves the computer.
        let hinted = |hint: &str| -> Vec<&str> {
            tools
                .iter()
                .filter(|tool| tool["annotations"][hint] == true)
                .map(|tool| tool["name"].as_str().unwrap())
                .collect()
        };
        assert_eq!(
            hinted("destructiveHint"),
            [
                "save_application",
                "save_resource",
                "delete_application",
                "delete_resource",
                "save_check",
                "set_check_paused",
                "remove_check",
            ]
        );
        assert_eq!(hinted("openWorldHint"), ["run_check"]);
        assert_eq!(replies[3]["error"]["code"], METHOD_NOT_FOUND);
        assert_eq!(replies[4]["error"]["code"], INVALID_PARAMS);
        assert_eq!(replies[5]["error"]["code"], INVALID_PARAMS);
        assert_eq!(replies[6]["error"]["code"], INVALID_REQUEST);
        assert_eq!(replies[6]["id"], Value::Null);

        // A revision it does not speak gets its newest; a line that is not JSON, a parse error.
        let mut output = Vec::new();
        serve(
            &path,
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"1999-01-01\"}}\nnot json\n\n".as_bytes(),
            &mut output,
        )
        .unwrap();
        let replies: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            replies[0]["result"]["protocolVersion"],
            PROTOCOL_VERSIONS[0]
        );
        assert_eq!(replies[1]["error"]["code"], PARSE_ERROR);
        assert_eq!(replies.len(), 2);
        remove(&path);
    }

    #[test]
    fn never_creates_an_inventory_where_devhq_would_not_find_it() {
        let path = path();
        let status = answer(&path, "status", json!({}));
        assert_eq!(status["exists"], false);
        assert_eq!(status["counts"], Value::Null);
        for tool in ["attention", "list_systems"] {
            assert!(refusal(&path, tool, json!({})).contains("There is no DevHQ inventory at"));
        }
        assert!(!path.exists());
        remove(&path);
    }

    #[test]
    fn answers_what_needs_attention_as_the_window_does() {
        let path = path();
        let (shop, host) = seed(&path);
        // A check that only runs when asked supervises nothing.
        let spare = answer(
            &path,
            "get_system",
            json!({ "type": "resource", "name": "Spare box" }),
        );
        save_http_check(
            &path,
            HttpCheckInput {
                subject: Subject::Resource {
                    id: spare["system"]["system"]["id"].as_str().unwrap().into(),
                },
                url: "https://spare.example/".into(),
                expected_status: 200,
                timeout_seconds: 5,
                valid_for_minutes: 30,
                monitor_interval_minutes: None,
            },
        )
        .unwrap();
        let attention = answer(&path, "attention", json!({}));
        assert_eq!(attention["status"], "attention");
        let items = attention["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["kind"], "incident");
        assert_eq!(
            items[0]["system"],
            json!({ "type": "application", "id": shop, "name": "Shop" })
        );
        assert_eq!(items[0]["incident"]["title"], "Checkout fails");
        assert_eq!(items[1]["kind"], "evidence");
        assert_eq!(items[1]["state"], "unhealthy");
        assert_eq!(items[1]["system"]["name"], "Web host");
        assert_eq!(items[1]["observation"]["source"], "manual");
        assert_eq!(
            attention["coverage"],
            json!({ "current": 2, "stale": 0, "unobserved": 1 })
        );
        // A gap is named, never passed off as health.
        assert_eq!(attention["neverObserved"][0]["name"], "Spare box");
        let unsupervised: Vec<_> = attention["notSupervised"]
            .as_array()
            .unwrap()
            .iter()
            .map(|system| system["name"].as_str().unwrap())
            .collect();
        assert_eq!(unsupervised, ["Shop", "Spare box"]);
        assert!(!unsupervised.contains(&"Web host"), "{host} is supervised");
        assert!(attention["now"].is_string());
        assert_eq!(attention["history"]["cut"], json!([]));
        remove(&path);
    }

    #[test]
    fn lists_and_details_systems_with_their_relations_and_records() {
        let path = path();
        let (shop, host) = seed(&path);
        let listed = answer(&path, "list_systems", json!({}));
        let application = &listed["applications"][0];
        assert_eq!(application["system"]["name"], "Shop");
        assert_eq!(application["application"]["version"], "2.4.1");
        assert_eq!(application["components"][0]["runsOn"]["name"], "Web host");
        assert_eq!(application["evidence"]["state"], "healthy");
        assert_eq!(application["activeIncidents"], 1);
        assert_eq!(application["httpCheck"], Value::Null);
        let resources = listed["resources"].as_array().unwrap();
        let spare = resources
            .iter()
            .find(|item| item["system"]["name"] == "Spare box")
            .unwrap();
        assert_eq!(
            spare["evidence"],
            json!({ "state": "unobserved", "latest": null })
        );
        let web = resources
            .iter()
            .find(|item| item["system"]["id"] == host.as_str())
            .unwrap();
        assert_eq!(web["usedBy"][0]["id"], shop.as_str());
        assert_eq!(web["httpCheck"]["monitor"], "active");

        // By name, ignoring case and surrounding spaces, or by id: the same system.
        let by_name = answer(
            &path,
            "get_system",
            json!({ "type": "resource", "name": " web HOST " }),
        );
        let by_id = answer(
            &path,
            "get_system",
            json!({ "type": "resource", "id": host }),
        );
        assert_eq!(by_name["system"], by_id["system"]);
        let system = &by_id["system"];
        assert_eq!(system["observations"].as_array().unwrap().len(), 1);
        assert_eq!(system["events"].as_array().unwrap().len(), 1);
        assert_eq!(system["incidents"], json!([]));
        assert_eq!(system["automatedCompleteSince"], Value::Null);

        assert!(refusal(
            &path,
            "get_system",
            json!({ "type": "resource", "name": "Nope" })
        )
        .contains("No resource matches"));
        assert!(
            refusal(&path, "get_system", json!({ "type": "server", "id": host }))
                .contains("type must be")
        );
        assert!(refusal(&path, "get_system", json!({ "type": "resource" }))
            .contains("by id or by name"));
        // Two applications with one name: only the id tells them apart.
        save_application(
            &path,
            ApplicationInput {
                id: None,
                name: "shop".into(),
                description: String::new(),
                lifecycle: "development".into(),
                version: None,
                components: vec![],
                references: vec![],
            },
        )
        .unwrap();
        assert!(refusal(
            &path,
            "get_system",
            json!({ "type": "application", "name": "Shop" })
        )
        .contains("use its id"));
        remove(&path);
    }

    #[test]
    fn pages_through_older_automated_results() {
        let path = path();
        let (_, host) = seed(&path);
        // As supervision writes them, five minutes apart, a few more than a snapshot carries.
        let limit = records::AUTOMATED_HISTORY_LIMIT;
        let first = records::now_ms() - (limit + 10) * 5 * 60_000;
        {
            let db = inventory::open(&path).unwrap();
            for index in 0..limit + 3 {
                let at = first + index * 5 * 60_000;
                let id = uuid::Uuid::new_v4().to_string();
                db.execute(
                    "INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at,
                                               http_url, http_expected_status, http_status, http_latency_ms, http_error, http_redirects)
                     VALUES (?1, ?2, 'healthy', 'HTTP 200 as expected', 'http', ?3, ?4, ?3, 'https://host.example/health', 200, 200, 5, NULL, '[]')",
                    rusqlite::params![id, host, at, at + 30 * 60_000],
                )
                .unwrap();
                db.execute(
                    "INSERT INTO events (id, resource_id, kind, outcome, title, detail, source, observation_id, occurred_at, recorded_at)
                     VALUES (?1, ?2, 'observation', 'success', 'HTTP check: Healthy', '', 'http', ?3, ?4, ?4)",
                    rusqlite::params![uuid::Uuid::new_v4().to_string(), host, id, at],
                )
                .unwrap();
            }
        }
        let detail = answer(
            &path,
            "get_system",
            json!({ "type": "resource", "id": host }),
        );
        let since = detail["system"]["automatedCompleteSince"].as_str().unwrap();
        assert_eq!(detail["history"]["cut"][0]["since"], since);
        let oldest = detail["system"]["observations"]
            .as_array()
            .unwrap()
            .iter()
            .rfind(|item| item["source"] == "http")
            .unwrap()["id"]
            .clone();
        let older = answer(
            &path,
            "history",
            json!({ "type": "resource", "id": host, "records": "observations", "before": oldest }),
        );
        assert_eq!(older["observations"].as_array().unwrap().len(), 3);
        assert_eq!(older["more"], false);
        let oldest_event = detail["system"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .rfind(|item| item["source"] == "http")
            .unwrap()["id"]
            .clone();
        let older = answer(
            &path,
            "history",
            json!({ "type": "resource", "id": host, "records": "events", "before": oldest_event }),
        );
        assert_eq!(older["events"].as_array().unwrap().len(), 3);
        assert!(refusal(
            &path,
            "history",
            json!({ "type": "resource", "id": host, "records": "notes", "before": "x" })
        )
        .contains("records must be"));
        remove(&path);
    }

    #[test]
    fn answers_only_as_far_as_the_person_opened_the_gate() {
        let path = path();
        seed(&path);
        crate::access::set_agent_access(&path, "off").unwrap();
        // Off: only status answers, and says how to open the gate, without what it holds.
        let status = answer(&path, "status", json!({}));
        assert_eq!(status["access"], "off");
        assert_eq!(status["counts"], Value::Null);
        for tool in ["attention", "list_systems"] {
            let refused = refusal(&path, tool, json!({}));
            assert!(refused.contains("Settings → Agent gate"), "{refused}");
            assert!(refused.contains("access_denied"), "{refused}");
        }
        assert!(refusal(
            &path,
            "get_system",
            json!({ "type": "resource", "name": "Web host" })
        )
        .contains("not opened the agent gate"));
        // A change in Settings applies to the very next call of a session already running.
        let mut session = Session {
            database: path.clone(),
            client: None,
        };
        let attention =
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "attention" } })
                .to_string();
        assert_eq!(
            session.handle(&attention).unwrap()["result"]["isError"],
            true
        );
        crate::access::set_agent_access(&path, "read").unwrap();
        let opened = session.handle(&attention).unwrap();
        assert_eq!(opened["result"]["isError"], false);
        assert_eq!(opened["result"]["structuredContent"]["status"], "attention");
        assert_eq!(answer(&path, "status", json!({}))["access"], "read");
        remove(&path);
    }

    #[test]
    fn says_where_the_system_really_keeps_the_inventory() {
        let path = path();
        seed(&path);
        let status = answer(&path, "status", json!({}));
        assert_eq!(status["exists"], true);
        assert_eq!(
            status["counts"],
            json!({ "applications": 1, "resources": 2 })
        );
        assert_eq!(status["schemaVersion"], inventory::SCHEMA_VERSION);
        assert_eq!(status["database"]["storedAt"], Value::Null);
        // Reached through a link, as through Windows' AppData redirection, the file is
        // somewhere else than asked, and the gate says so.
        #[cfg(unix)]
        {
            let linked = path.parent().unwrap().join("linked");
            std::fs::create_dir(&linked).unwrap();
            std::os::unix::fs::symlink(&path, linked.join("inventory.sqlite3")).unwrap();
            let status = answer(&linked.join("inventory.sqlite3"), "status", json!({}));
            assert!(status["database"]["storedAt"].is_string(), "{status:#}");
            assert!(status["notes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|note| note.as_str().unwrap().contains("not where DevHQ asked")));
        }
        remove(&path);
    }

    #[test]
    fn says_whether_a_devhq_window_is_open_on_the_inventory() {
        let path = path();
        seed(&path);
        crate::access::set_agent_access(&path, "off").unwrap();
        let notes = |status: &Value| -> String {
            status["notes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|note| note.as_str().unwrap())
                .collect()
        };
        // Said even with the gate off: it tells nothing about what the inventory holds.
        let closed = answer(&path, "status", json!({}));
        assert_eq!(closed["windowOpen"], false);
        assert!(notes(&closed).contains("nothing supervises"), "{closed:#}");
        let window = crate::window_lock::WindowLock::hold(&path).unwrap();
        let open = answer(&path, "status", json!({}));
        assert_eq!(open["windowOpen"], true);
        assert!(notes(&open).contains("within about 2 s"), "{open:#}");
        // A check supervised now is the open window's to run, and the answer says so; one that
        // only runs when asked is not.
        crate::access::set_agent_access(&path, "write").unwrap();
        let mut agent = session(&path, "claude-code");
        let spare =
            json!({ "type": "resource", "name": "Spare box", "url": "https://spare.example/" });
        let on_demand = use_tool(&mut agent, "save_check", spare.clone()).unwrap();
        assert_eq!(on_demand["note"], Value::Null);
        let mut supervised = spare.clone();
        supervised["supervisionIntervalMinutes"] = json!(15);
        let noted = use_tool(&mut agent, "save_check", supervised.clone()).unwrap();
        assert!(
            noted["note"]
                .as_str()
                .unwrap()
                .contains("run_check now would record a second"),
            "{noted:#}"
        );
        let paused = json!({ "type": "resource", "name": "Spare box", "paused": true });
        assert_eq!(
            use_tool(&mut agent, "set_check_paused", paused).unwrap()["note"],
            Value::Null
        );
        let resumed = json!({ "type": "resource", "name": "Spare box", "paused": false });
        assert!(use_tool(&mut agent, "set_check_paused", resumed).unwrap()["note"].is_string());
        drop(window);
        // Without a window, nothing runs it: no note.
        assert_eq!(
            use_tool(&mut agent, "save_check", supervised).unwrap()["note"],
            Value::Null
        );
        assert_eq!(answer(&path, "status", json!({}))["windowOpen"], false);
        remove(&path);
    }

    /// A session whose client named itself `client`, as after `initialize`.
    fn session(path: &Path, client: &str) -> Session {
        Session {
            database: path.to_owned(),
            client: Some(client.to_owned()),
        }
    }

    /// A tool's result in the session: its structured content, or the text of its refusal.
    fn use_tool(session: &mut Session, tool: &str, arguments: Value) -> Result<Value, String> {
        let reply = session
            .handle(
                &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": tool, "arguments": arguments } })
                    .to_string(),
            )
            .unwrap();
        let result = &reply["result"];
        if result["isError"] == true {
            Err(result["content"][0]["text"].as_str().unwrap().to_owned())
        } else {
            Ok(result["structuredContent"].clone())
        }
    }

    fn changes(path: &Path) -> Vec<(String, String, String)> {
        let db = inventory::open(path).unwrap();
        let mut statement = db
            .prepare(
                "SELECT agent, action, summary FROM change_log WHERE source = 'agent' ORDER BY rowid",
            )
            .unwrap();
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    }

    #[test]
    fn changes_nothing_unless_the_person_lets_agents_write() {
        let path = path();
        let (shop, _) = seed(&path);
        let before = inventory::list(&path).unwrap();
        let mut agent = session(&path, "claude-code");
        let attempts = [
            (
                "save_application",
                json!({ "name": "New", "lifecycle": "development" }),
            ),
            ("save_resource", json!({ "name": "New", "kind": "server" })),
            ("delete_application", json!({ "id": shop })),
            (
                "record_observation",
                json!({ "type": "application", "id": shop, "result": "healthy", "summary": "Fine", "validForMinutes": 60 }),
            ),
            (
                "open_incident",
                json!({ "type": "application", "id": shop, "title": "Down", "severity": "critical" }),
            ),
            (
                "record_event",
                json!({ "type": "application", "id": shop, "kind": "note", "outcome": "info", "title": "Noted" }),
            ),
            (
                "save_check",
                json!({ "type": "application", "id": shop, "url": "https://shop.example/" }),
            ),
            (
                "run_check",
                json!({ "type": "resource", "name": "Web host" }),
            ),
            (
                "remove_check",
                json!({ "type": "resource", "name": "Web host" }),
            ),
            ("backup", json!({})),
        ];
        for (tool, arguments) in &attempts {
            let refused = use_tool(&mut agent, tool, arguments.clone()).unwrap_err();
            assert!(refused.contains("reading only"), "{tool}: {refused}");
        }
        crate::access::set_agent_access(&path, "off").unwrap();
        let refused = use_tool(
            &mut agent,
            "save_resource",
            json!({ "name": "New", "kind": "server" }),
        )
        .unwrap_err();
        assert!(refused.contains("not opened the agent gate"), "{refused}");
        let after = inventory::list(&path).unwrap();
        assert_eq!(after.applications, before.applications);
        assert_eq!(after.resources, before.resources);
        assert_eq!(after.observations, before.observations);
        assert_eq!(after.events, before.events);
        assert_eq!(after.incidents, before.incidents);
        assert_eq!(after.http_checks, before.http_checks);
        assert!(changes(&path).is_empty());
        assert!(!path.parent().unwrap().join("backups").exists());
        remove(&path);
    }

    #[test]
    fn adds_and_edits_systems_keeping_what_an_edit_leaves_out_and_logs_each_change() {
        let path = path();
        seed(&path);
        crate::access::set_agent_access(&path, "write").unwrap();
        let mut agent = session(&path, "claude-code");
        let vps = use_tool(
            &mut agent,
            "save_resource",
            json!({ "name": "VPS", "kind": "server", "operatingSystem": "Ubuntu 24.04" }),
        )
        .unwrap();
        let vps_id = vps["resource"]["id"].as_str().unwrap().to_owned();
        assert_eq!(
            vps["change"]["summary"],
            "Added resource «VPS» (server, Ubuntu 24.04)"
        );
        assert_eq!(vps["change"]["agent"], "claude-code");
        // A component runs on a resource named by id or by its name, ignoring case.
        let added = use_tool(
            &mut agent,
            "save_application",
            json!({ "name": "Tienda", "lifecycle": "production", "description": "Online store", "version": "1.3.0",
                    "components": [{ "name": "API", "kind": "backend", "runsOn": "vps" }, { "name": "Web", "kind": "frontend" }] }),
        )
        .unwrap();
        let tienda = added["application"]["id"].as_str().unwrap().to_owned();
        assert_eq!(
            added["application"]["components"][0]["resourceId"],
            vps_id.as_str()
        );
        assert_eq!(
            added["change"]["summary"],
            "Added application «Tienda» (production, version 1.3.0) with API (backend, on VPS), Web (frontend)"
        );
        // An edit of one field keeps every other, components included.
        let edited = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": tienda, "version": "1.4.0" }),
        )
        .unwrap();
        assert_eq!(edited["application"]["description"], "Online store");
        assert_eq!(
            edited["application"]["components"],
            added["application"]["components"]
        );
        assert_eq!(edited["change"]["action"], "updated");
        assert_eq!(
            edited["change"]["summary"],
            "Updated application «Tienda»: version «1.3.0» → «1.4.0»"
        );
        let cleared = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": tienda, "version": null, "lifecycle": "testing" }),
        )
        .unwrap();
        assert_eq!(cleared["application"]["version"], Value::Null);
        assert_eq!(
            cleared["change"]["summary"],
            "Updated application «Tienda»: lifecycle production → testing; version «1.4.0» → none"
        );
        // A given component list is the complete list.
        let api = added["application"]["components"][0]["id"].clone();
        let trimmed = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": tienda, "components": [{ "id": api, "name": "API", "kind": "backend", "runsOn": vps_id }] }),
        )
        .unwrap();
        assert_eq!(
            trimmed["application"]["components"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            trimmed["change"]["summary"],
            "Updated application «Tienda»: components API (backend, on VPS), Web (frontend) → API (backend, on VPS)"
        );
        let renamed = use_tool(
            &mut agent,
            "save_resource",
            json!({ "id": vps_id, "name": "VPS Hetzner" }),
        )
        .unwrap();
        assert_eq!(renamed["resource"]["operatingSystem"], "Ubuntu 24.04");
        assert_eq!(
            renamed["change"]["summary"],
            "Updated resource «VPS Hetzner»: name «VPS» → «VPS Hetzner»"
        );

        // Refusals change nothing and log nothing.
        let logged = changes(&path).len();
        for (tool, arguments, why) in [
            (
                "save_application",
                json!({ "name": "Nameless lifecycle" }),
                "lifecycle is required",
            ),
            (
                "save_application",
                json!({ "lifecycle": "production" }),
                "name is required",
            ),
            (
                "save_application",
                json!({ "id": "ghost", "version": "2" }),
                "No application has the id ghost",
            ),
            (
                "save_application",
                json!({ "name": "Bad", "lifecycle": "production", "components": [{ "name": "X", "kind": "lambda" }] }),
                "Invalid component kind",
            ),
            (
                "save_application",
                json!({ "name": "Bad", "lifecycle": "production", "components": [{ "name": "X", "kind": "backend", "runsOn": "Nowhere" }] }),
                "Resource Nowhere does not exist",
            ),
            (
                "save_resource",
                json!({ "name": "Kindless" }),
                "kind is required",
            ),
        ] {
            let refused = use_tool(&mut agent, tool, arguments).unwrap_err();
            assert!(refused.contains(why), "{refused}");
        }
        assert_eq!(changes(&path).len(), logged);
        assert!(!inventory::list(&path)
            .unwrap()
            .applications
            .iter()
            .any(|item| item.name == "Bad"));

        // The agent reads its changes back with the system.
        let detail = use_tool(
            &mut agent,
            "get_system",
            json!({ "type": "application", "id": tienda }),
        )
        .unwrap();
        let actions: Vec<_> = detail["system"]["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|change| change["action"].as_str().unwrap())
            .collect();
        assert_eq!(actions, ["updated", "updated", "updated", "created"]);
        remove(&path);
    }

    #[test]
    fn agents_declare_where_systems_are_and_find_them_by_it() {
        let path = path();
        let (shop, host) = seed(&path);
        crate::access::set_agent_access(&path, "write").unwrap();
        let mut agent = session(&path, "claude-code");
        // A new resource can be the machine of its own paths, named by the name it is given.
        let laptop = use_tool(
            &mut agent,
            "save_resource",
            json!({ "name": "Laptop", "kind": "workstation", "references": [
                { "kind": "logs", "target": r"C:\ProgramData\Agent\logs", "machine": "laptop" },
            ] }),
        )
        .unwrap();
        let laptop_id = laptop["resource"]["id"].as_str().unwrap().to_owned();
        assert_eq!(
            laptop["resource"]["references"][0]["machineId"],
            laptop_id.as_str()
        );
        // Each path's machine by exact name, ignoring case, or by id; the list is complete.
        let saved = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": shop, "references": [
                { "kind": "repository", "target": "https://github.com/me/platform" },
                { "kind": "local_copy", "target": r"C:\src\platform", "machine": "LAPTOP", "label": "Work" },
                { "kind": "logs", "target": "/var/log/shop", "machine": host },
            ] }),
        )
        .unwrap();
        assert_eq!(
            saved["change"]["summary"],
            r"Updated application «Shop»: references none → repository https://github.com/me/platform, local copy «Work» C:\src\platform on Laptop, logs /var/log/shop on Web host"
        );
        // An edit that leaves them out keeps them, as it keeps the components.
        let versioned = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": shop, "version": "2.5.0" }),
        )
        .unwrap();
        assert_eq!(
            versioned["application"]["references"],
            saved["application"]["references"]
        );
        assert_eq!(
            versioned["application"]["components"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        // Another application in the same repository, as the parts of a monorepo are.
        let admin = use_tool(
            &mut agent,
            "save_application",
            json!({ "name": "Admin", "lifecycle": "development", "references": [
                { "kind": "repository", "target": "https://github.com/me/platform" },
            ] }),
        )
        .unwrap();
        let admin_id = admin["application"]["id"].as_str().unwrap().to_owned();

        let listed = answer(&path, "list_systems", json!({}));
        let system = |name: &str| -> Value {
            listed["applications"]
                .as_array()
                .unwrap()
                .iter()
                .chain(listed["resources"].as_array().unwrap())
                .find(|item| item["system"]["name"] == name)
                .unwrap()
                .clone()
        };
        let shown: Vec<Value> = system("Shop")["references"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| {
                let mut item = item.clone();
                item.as_object_mut().unwrap().remove("id");
                item
            })
            .collect();
        assert_eq!(
            shown,
            [
                json!({ "kind": "repository", "target": "https://github.com/me/platform", "label": null, "machine": null,
                        "sameRepositoryAs": [{ "type": "application", "id": admin_id, "name": "Admin" }] }),
                json!({ "kind": "local_copy", "target": r"C:\src\platform", "label": "Work",
                        "machine": { "type": "resource", "id": laptop_id, "name": "Laptop" } }),
                json!({ "kind": "logs", "target": "/var/log/shop", "label": null,
                        "machine": { "type": "resource", "id": host, "name": "Web host" } }),
            ]
        );
        assert_eq!(
            system("Admin")["references"][0]["sameRepositoryAs"][0]["name"],
            "Shop"
        );
        assert_eq!(system("Spare box")["references"], json!([]));
        // Working in a folder, an agent finds which system it is.
        let here: Vec<&str> = listed["applications"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| {
                item["references"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|reference| reference["target"] == r"C:\src\platform")
            })
            .map(|item| item["system"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(here, ["Shop"]);
        let detail = answer(
            &path,
            "get_system",
            json!({ "type": "resource", "name": "Laptop" }),
        );
        assert_eq!(
            detail["system"]["references"][0]["machine"]["name"],
            "Laptop"
        );

        // Refused as the window's save refuses them, and nothing changes.
        let logged = changes(&path).len();
        for (tool, arguments, why) in [
            (
                "save_application",
                json!({ "id": shop, "references": "https://github.com/me/platform" }),
                "references must be a list",
            ),
            (
                "save_application",
                json!({ "id": shop, "references": [{ "target": "https://x.example" }] }),
                "The argument kind is required",
            ),
            (
                "save_application",
                json!({ "id": shop, "references": [{ "kind": "local_copy", "target": "/srv/shop", "machine": "Web host" }] }),
                "Web host is a server, not a workstation",
            ),
            (
                "save_resource",
                json!({ "id": laptop_id, "kind": "server" }),
                r"Laptop holds local copies of other systems: Shop (local copy C:\src\platform)",
            ),
            (
                "delete_resource",
                json!({ "name": "Laptop" }),
                r"Laptop holds paths of other systems: Shop (local copy C:\src\platform)",
            ),
        ] {
            let refused = use_tool(&mut agent, tool, arguments).unwrap_err();
            assert!(refused.contains(why), "{refused}");
        }
        assert_eq!(changes(&path).len(), logged);
        remove(&path);
    }

    #[test]
    fn deletes_only_what_has_no_history_and_the_log_outlives_it() {
        let path = path();
        let (shop, _) = seed(&path);
        crate::access::set_agent_access(&path, "write").unwrap();
        let mut agent = session(&path, "codex");
        let refused =
            use_tool(&mut agent, "delete_application", json!({ "id": shop })).unwrap_err();
        assert!(refused.contains("DevHQ keeps history"), "{refused}");
        let refused =
            use_tool(&mut agent, "delete_resource", json!({ "name": "Web host" })).unwrap_err();
        assert!(refused.contains("is used by Shop (API)"), "{refused}");
        let deleted = use_tool(
            &mut agent,
            "delete_resource",
            json!({ "name": "spare box" }),
        )
        .unwrap();
        assert_eq!(deleted["deleted"]["name"], "Spare box");
        assert_eq!(
            deleted["change"]["summary"],
            "Deleted resource «Spare box», which had no history"
        );
        let snapshot = inventory::list(&path).unwrap();
        assert!(snapshot
            .resources
            .iter()
            .all(|item| item.name != "Spare box"));
        // The window's snapshot carries the change, naming the resource that is gone.
        let by_agents: Vec<_> = snapshot
            .changes
            .iter()
            .filter(|change| change.source == "agent")
            .collect();
        assert_eq!(by_agents.len(), 1);
        assert_eq!(by_agents[0].subject_name, "Spare box");
        assert_eq!(by_agents[0].action, "deleted");
        assert_eq!(
            changes(&path),
            [(
                "codex".to_owned(),
                "deleted".to_owned(),
                "Deleted resource «Spare box», which had no history".to_owned()
            )]
        );
        remove(&path);
    }

    #[test]
    fn records_carry_the_agent_and_check_changes_are_logged() {
        let path = path();
        let (shop, _) = seed(&path);
        crate::access::set_agent_access(&path, "write").unwrap();
        let mut agent = session(&path, "claude-code");
        let shop_ref = json!({ "type": "application", "name": "shop" });
        let with = |extra: Value| {
            let mut arguments = shop_ref.as_object().unwrap().clone();
            arguments.extend(extra.as_object().unwrap().clone());
            Value::Object(arguments)
        };
        let observed = use_tool(
            &mut agent,
            "record_observation",
            with(
                json!({ "result": "degraded", "summary": "Checkout slow", "validForMinutes": 30 }),
            ),
        )
        .unwrap();
        assert_eq!(observed["observation"]["source"], "agent");
        assert_eq!(observed["observation"]["agent"], "claude-code");
        let opened = use_tool(
            &mut agent,
            "open_incident",
            with(json!({ "title": "Payments fail", "severity": "warning" })),
        )
        .unwrap();
        assert_eq!(opened["incident"]["source"], "agent");
        let resolved = use_tool(
            &mut agent,
            "update_incident",
            json!({ "incidentId": opened["incident"]["id"], "status": "resolved", "note": "Provider fixed it" }),
        )
        .unwrap();
        assert_eq!(resolved["incident"]["status"], "resolved");
        let noted = use_tool(
            &mut agent,
            "record_event",
            with(json!({ "kind": "deployment", "outcome": "success", "title": "Deployed 2.5.0" })),
        )
        .unwrap();
        assert_eq!(noted["event"]["agent"], "claude-code");
        let refused = use_tool(
            &mut agent,
            "record_observation",
            with(json!({ "result": "fine", "summary": "?", "validForMinutes": 30 })),
        )
        .unwrap_err();
        assert!(refused.contains("Invalid observation result"), "{refused}");

        // A new check takes the window's defaults; a change keeps what it leaves out.
        let set = use_tool(
            &mut agent,
            "save_check",
            with(json!({ "url": "http://devhq-gate.invalid/health" })),
        )
        .unwrap();
        assert_eq!(
            (
                set["httpCheck"]["expectedStatus"].clone(),
                set["httpCheck"]["timeoutSeconds"].clone(),
                set["httpCheck"]["validForMinutes"].clone(),
                set["httpCheck"]["monitor"].clone()
            ),
            (json!(200), json!(5), json!(60), json!("off"))
        );
        let supervised = use_tool(
            &mut agent,
            "save_check",
            with(json!({ "supervisionIntervalMinutes": 15, "timeoutSeconds": 1 })),
        )
        .unwrap();
        assert_eq!(
            supervised["httpCheck"]["url"],
            "http://devhq-gate.invalid/health"
        );
        assert_eq!(supervised["httpCheck"]["monitor"], "active");
        let lapsing = use_tool(
            &mut agent,
            "save_check",
            with(json!({ "validForMinutes": 10 })),
        )
        .unwrap_err();
        assert!(lapsing.contains("must stay valid for longer"), "{lapsing}");
        use_tool(
            &mut agent,
            "set_check_paused",
            with(json!({ "paused": true })),
        )
        .unwrap();
        use_tool(
            &mut agent,
            "set_check_paused",
            with(json!({ "paused": false })),
        )
        .unwrap();
        // The run is DevHQ's own evidence; the log says the agent asked for it.
        let ran = use_tool(&mut agent, "run_check", shop_ref.clone()).unwrap();
        assert_eq!(ran["observation"]["source"], "http");
        assert_eq!(ran["observation"]["result"], "unhealthy");
        use_tool(&mut agent, "remove_check", shop_ref.clone()).unwrap();
        assert!(use_tool(&mut agent, "remove_check", shop_ref.clone()).is_err());

        let logged: Vec<_> = changes(&path)
            .into_iter()
            .map(|(agent, action, summary)| {
                assert_eq!(agent, "claude-code");
                (action, summary)
            })
            .collect();
        let actions: Vec<_> = logged.iter().map(|(action, _)| action.as_str()).collect();
        assert_eq!(
            actions,
            [
                "check_saved",
                "check_saved",
                "check_paused",
                "check_resumed",
                "check_run",
                "check_removed"
            ]
        );
        assert_eq!(
            logged[0].1,
            "Set up the HTTP check of «Shop»: GET http://devhq-gate.invalid/health expecting 200 within 5 s, valid 60 min, run only when asked"
        );
        assert_eq!(
            logged[1].1,
            "Changed the HTTP check of «Shop»: GET http://devhq-gate.invalid/health expecting 200 within 1 s, valid 60 min, supervised every 15 min while DevHQ is open"
        );
        assert!(
            logged[4].1.starts_with("Ran the HTTP check of «Shop»: "),
            "{}",
            logged[4].1
        );
        assert_eq!(logged[5].1, "Removed the HTTP check of «Shop» (http://devhq-gate.invalid/health); the evidence it recorded stays");
        // Records name the agent themselves; they are not logged again.
        let snapshot = inventory::list(&path).unwrap();
        assert!(snapshot
            .events
            .iter()
            .any(|event| event.title == "Incident resolved: Payments fail"
                && event.agent.as_deref() == Some("claude-code")));
        let _ = shop;
        remove(&path);
    }

    #[test]
    fn backs_up_as_back_up_now_does() {
        let path = path();
        seed(&path);
        crate::access::set_agent_access(&path, "write").unwrap();
        let mut agent = session(&path, "claude-code");
        let backup = use_tool(&mut agent, "backup", json!({})).unwrap();
        assert_eq!(backup["backup"]["quickCheck"], "ok");
        assert_eq!(backup["backup"]["counts"]["applications"], 1);
        remove(&path);
    }

    #[test]
    fn a_client_without_a_name_is_still_told_apart_from_the_person() {
        let path = path();
        seed(&path);
        crate::access::set_agent_access(&path, "write").unwrap();
        let mut agent = Session {
            database: path.clone(),
            client: None,
        };
        let saved = use_tool(
            &mut agent,
            "save_resource",
            json!({ "name": "Box", "kind": "workstation" }),
        )
        .unwrap();
        assert_eq!(saved["change"]["agent"], "unnamed MCP client");
        remove(&path);
    }

    #[test]
    fn opens_the_inventory_the_app_opens() {
        let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], IDENTIFIER);
        // Resolved by Tauri itself, as `run` does, for an app with DevHQ's identifier.
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().identifier = IDENTIFIER.into();
        let app = tauri::test::mock_builder().build(context).unwrap();
        use tauri::Manager;
        assert_eq!(
            default_database().unwrap(),
            app.path().app_data_dir().unwrap().join("inventory.sqlite3")
        );
    }
}
