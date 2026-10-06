//! The agent gate: `devhq --mcp` serves DevHQ's inventory to an agent (Claude Code, Codex or any
//! MCP client) over the Model Context Protocol, so that it can do what the person does at the
//! window when the person asks it to. The client starts this process for a session and ends it
//! with the session: nothing listens on a port, nothing runs in the background, and no window or
//! WebView is created.
//!
//! The protocol is JSON-RPC 2.0, one message per line on stdin and stdout (MCP's stdio
//! transport); stderr is free for diagnostics. Every tool goes through the same use cases as the
//! window's commands, with the same validation, limits and transactions.

use crate::attention::{self, AttentionItem};
use crate::http_check::HttpCheck;
use crate::inventory::{self, ApiError, Inventory};
use crate::records::{self, Subject};
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
            "status" => self.status(),
            "attention" => self.attention(),
            "list_systems" => self.list_systems(),
            "get_system" => self.get_system(arguments),
            "history" => self.history(arguments),
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

    /// The inventory, but only one that exists: the gate never creates an empty inventory where
    /// the person's DevHQ would not find it.
    fn inventory(&self) -> Result<Inventory, ApiError> {
        if !self.database.is_file() {
            return Err(ApiError::invalid(format!(
                "There is no DevHQ inventory at {}. Open DevHQ once to create it, or start the gate with --database <path>.",
                self.database.display()
            )));
        }
        inventory::list(&self.database)
    }

    fn status(&self) -> Result<Value, ApiError> {
        let location = inventory::DataLocation::of(&self.database);
        let exists = self.database.is_file();
        let counts = if exists {
            let inventory = self.inventory()?;
            json!({ "applications": inventory.applications.len(), "resources": inventory.resources.len() })
        } else {
            Value::Null
        };
        let mut notes = vec![
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

const INSTRUCTIONS: &str = "DevHQ is the person's local operations inventory: applications, the resources they run on, and the evidence about them (observations, incidents, events, HTTP checks). Start with `attention` to answer «what needs my attention?». Never present a system without current evidence as healthy: unobserved and expired evidence are gaps, not health. Every record says who produced it (`source`: manual, http or agent) and when.";

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

/// What the window shows about a system at a glance: identity, evidence, active incidents and
/// its HTTP check, plus what it is related to.
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
            })), "components": components })
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
            })), "usedBy": used_by })
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
        "note": "Manual and agent records are always complete. Automated HTTP results are the latest ones per system; for each system in `cut`, older ones are kept but only loaded through `history`.",
    })
}

fn text<'a>(arguments: &'a Map<String, Value>, key: &str) -> Result<&'a str, ApiError> {
    arguments.get(key).and_then(Value::as_str).ok_or_else(|| {
        ApiError::invalid(format!("The argument {key} is required and must be text."))
    })
}

/// The system an agent names by type and either ID or name (exact, ignoring case).
fn find_system(inventory: &Inventory, arguments: &Map<String, Value>) -> Result<Subject, ApiError> {
    let kind = text(arguments, "type")?;
    let id = arguments.get("id").and_then(Value::as_str);
    let name = arguments.get("name").and_then(Value::as_str);
    let candidates: Vec<(String, &str)> = match kind {
        "application" => inventory
            .applications
            .iter()
            .map(|item| (item.id.clone(), item.name.as_str()))
            .collect(),
        "resource" => inventory
            .resources
            .iter()
            .map(|item| (item.id.clone(), item.name.as_str()))
            .collect(),
        _ => {
            return Err(ApiError::invalid(
                "type must be \"application\" or \"resource\".",
            ))
        }
    };
    let found: Vec<&String> = match (id, name) {
        (Some(id), _) => candidates
            .iter()
            .filter(|(item, _)| item == id)
            .map(|(item, _)| item)
            .collect(),
        (None, Some(name)) => candidates
            .iter()
            .filter(|(_, item)| item.to_lowercase() == name.trim().to_lowercase())
            .map(|(item, _)| item)
            .collect(),
        (None, None) => return Err(ApiError::invalid("Name the system by id or by name.")),
    };
    let id = match found.as_slice() {
        [id] => (*id).clone(),
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
    Ok(match kind {
        "application" => Subject::Application { id },
        _ => Subject::Resource { id },
    })
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
    vec![
        read_only(
            "status",
            "DevHQ status",
            "Which inventory file the gate opened, where the system really keeps it, its schema version and how many systems it holds, and what the gate cannot know.",
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
            "Every application and resource with its evidence state and latest observation, active incidents, HTTP check and relations (components and what they run on; what uses each resource).",
            json!({}),
            &[],
        ),
        read_only(
            "get_system",
            "System detail",
            "One application or resource with every loaded observation, incident and event about it, newest first, and from when its automated results are complete.",
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
    fn path() -> PathBuf {
        let folder = std::env::temp_dir().join(format!("devhq-gate-test-{}", uuid::Uuid::new_v4()));
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
        let tools: Vec<_> = replies[2]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            tools,
            [
                "status",
                "attention",
                "list_systems",
                "get_system",
                "history"
            ]
        );
        // Reading never changes anything, and says so to the client.
        assert!(replies[2]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tool| tool["annotations"]["readOnlyHint"] == true
                && tool["inputSchema"]["type"] == "object"));
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
