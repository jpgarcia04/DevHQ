//! The agent gate: `devhq --mcp` serves DevHQ's inventory to an agent (Claude Code, Codex or any
//! MCP client) over the Model Context Protocol, so that it can do what the person does at the
//! window when the person asks it to. The client starts this process for a session and ends it
//! with the session: nothing listens on a port, nothing runs in the background, and no window or
//! WebView is created.
//!
//! The protocol is JSON-RPC 2.0, one message per line on stdin and stdout (MCP's stdio
//! transport); stderr is free for diagnostics. Every tool goes through the same use cases as the
//! window's commands, with the same validation, limits and transactions.

mod arguments;
mod writes;

use crate::access;
use crate::attention::{self, AttentionItem};
use crate::change_log;
use crate::http_check::HttpCheck;
use crate::identity;
use crate::inventory::{self, ApiError, Inventory};
use crate::local_copies::readings::{self, LocalCopy};
use crate::local_copies::Reader;
use crate::records::{self, Subject};
use crate::references::Reference;
use crate::workstation::{Computer, ThisWorkstation};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// Revisions of the protocol this server speaks, newest first. A client asking for one of them
/// gets it; any other gets the newest, and decides whether to go on.
const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

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
    // What this executable's window opens: the installed DevHQ's inventory, or development data
    // (see `identity`).
    let Some(database) = database.or_else(identity::database) else {
        return usage("the system has no data directory; pass --database");
    };
    let Some(local) = identity::local_data() else {
        return usage("the system has no local data directory");
    };
    let stdin = std::io::stdin();
    match serve(
        &database,
        Computer::this_one(local),
        stdin.lock(),
        std::io::stdout().lock(),
    ) {
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

/// Answers each message read from `input` on `output` until `input` ends, on `computer`.
pub(crate) fn serve(
    database: &Path,
    computer: Computer,
    input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    let mut session = Session {
        database: database.to_owned(),
        computer,
        reader: None,
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
    /// This computer, as the window's DevHQ tells it apart: which workstation it is.
    computer: Computer,
    /// How copies are read; this executable unless a test says otherwise.
    reader: Option<Reader>,
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
        // Only the tool's arguments are checked: the rest of `params` (`_meta`, and whatever a
        // later revision of the protocol adds) belongs to the protocol, which lets it grow.
        let arguments = match params.get("arguments") {
            None | Some(Value::Null) => &empty,
            Some(Value::Object(arguments)) => arguments,
            Some(_) => return Err((INVALID_PARAMS, "Tool arguments must be an object.".into())),
        };
        let Some(schema) = tools()
            .into_iter()
            .find(|tool| tool["name"] == name)
            .map(|tool| tool["inputSchema"].clone())
        else {
            return Err((INVALID_PARAMS, format!("Unknown tool {name}.")));
        };
        // Before anything opens: a call that does not match what the tool declares does nothing.
        let mut problems = arguments::problems(&schema, arguments);
        if problems.is_empty() {
            problems = arguments::rules(name, arguments);
        }
        let outcome = if problems.is_empty() {
            self.run(name, arguments)
        } else {
            Err(ApiError::invalid(format!(
                "{name} was not run: {}",
                problems.join(" ")
            )))
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

    /// Runs a tool whose arguments match its schema and rules.
    fn run(&self, name: &str, arguments: &Map<String, Value>) -> Result<Value, ApiError> {
        match name {
            // Always answered, so that an agent can tell the person how to open the gate.
            "status" => self.status(),
            "attention" => self.allow("read").and_then(|()| self.attention()),
            "list_systems" => self.allow("read").and_then(|()| self.list_systems()),
            "get_system" => self.allow("read").and_then(|()| self.get_system(arguments)),
            "history" => self.allow("read").and_then(|()| self.history(arguments)),
            // Configuration changes check the access inside their own transaction (adding and
            // editing applications and resources needs `maintain`, the rest `write`); records,
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
            "read_copies" => self
                .allow("write")
                .and_then(|()| self.read_copies(arguments)),
            // `tools` declares it, but nothing here runs it: a test calls every tool it declares.
            other => Err(ApiError::invalid(format!(
                "DevHQ declares the tool {other} but cannot run it."
            ))),
        }
    }

    /// What `read` makes of the inventory, read as it is on a connection that cannot change any of
    /// its files: never created, upgraded, converted or recovered, and refused unless it is in this
    /// build's schema (see `inventory::examine`).
    fn read<T>(
        &self,
        read: impl FnMut(&mut rusqlite::Connection) -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        inventory::read_as_found(&self.database, read)
    }

    /// Whether the person lets agents `read` or `write` through the gate, checked on every call
    /// so that a change in Settings applies at once.
    fn allow(&self, need: &str) -> Result<(), ApiError> {
        access::permitted(&self.read(|db| access::mode(db))?, need)
    }

    fn inventory(&self) -> Result<Inventory, ApiError> {
        self.read(inventory::list_from)
    }

    /// Always answered, whatever the file is, and without changing any of its files: what the gate
    /// found there, and, only for an inventory it serves, the access mode and, if the person lets
    /// agents in, what it holds. Nothing it could not read is presented as known.
    fn status(&self) -> Result<Value, ApiError> {
        let location = inventory::DataLocation::of(&self.database);
        let exists = self.database.is_file();
        let (found, served) = inventory::examine(&self.database, |db| self.served(db));
        let mut notes = Vec::new();
        let (access, counts, this_workstation) = match served {
            Some(Ok(served)) => served,
            Some(Err(error)) => {
                notes.push(format!(
                    "DevHQ could not read the inventory: {} ({}). What it holds and its access mode are unknown.",
                    error.message, error.code
                ));
                (None, Value::Null, Value::Null)
            }
            None => (None, Value::Null, Value::Null),
        };
        let window_open = crate::window_lock::window_open(&self.database);
        notes.extend([
            match window_open {
                Some(true) => "A DevHQ window is open on this inventory. It notices what the gate commits within about 2 s: it shows it, and its supervision follows a check configured, paused, removed or run here.",
                Some(false) => "No DevHQ window is open on this inventory, so nothing supervises its HTTP checks: they run only when asked. What the gate writes is there when DevHQ opens.",
                None => "The system could not say whether a DevHQ window is open on this inventory.",
            }
            .to_owned(),
            "Supervision's live state (runs in flight, next run, DevHQ's own run errors) exists only in the DevHQ window's process; the gate does not know it.".to_owned(),
        ]);
        if let Some(stored_at) = &location.stored_at {
            notes.push(format!(
                "The system keeps this file at {stored_at}, not where DevHQ asked for it. A DevHQ started another way (from the Start menu or File Explorer) may see a different inventory."
            ));
        }
        if identity::development() {
            notes.push(format!(
                "This DevHQ executable was not packed by DevHQ's installer, so without --database it opens development data ({}), apart from the installed DevHQ's inventory.",
                identity::DEVELOPMENT
            ));
        }
        Ok(json!({
            "database": location,
            "exists": exists,
            "schemaVersion": found.stored(),
            "schema": {
                "stored": found.stored(),
                "supported": inventory::SCHEMA_VERSION,
                "state": found.state(),
                "structure": found.structure(),
                // The gate never runs SQLite's integrity check: a schema it read is never
                // presented as sound.
                "integrity": found.stored().map(|_| "not_checked"),
                "meaning": found.meaning(),
            },
            "access": access,
            "windowOpen": window_open,
            "counts": counts,
            "thisWorkstation": this_workstation,
            "client": self.client,
            "notes": notes,
        }))
    }

    /// The access mode of an inventory the gate serves and, for agents the person let in, how
    /// many systems it holds and which of its workstations this computer is.
    fn served(
        &self,
        db: &mut rusqlite::Connection,
    ) -> Result<(Option<String>, Value, Value), ApiError> {
        let mode = access::mode(db)?;
        if mode == "off" {
            return Ok((Some(mode), Value::Null, Value::Null));
        }
        let inventory = inventory::list_from(db)?;
        Ok((
            Some(mode),
            json!({ "applications": inventory.applications.len(), "resources": inventory.resources.len() }),
            json!(self.this_workstation(&inventory)),
        ))
    }

    /// Which workstation this computer is, by the association the window keeps.
    fn this_workstation(&self, inventory: &Inventory) -> ThisWorkstation {
        self.computer.resolve(&inventory.resources)
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
        let this = self.this_workstation(&inventory);
        let local_copies = local_copies_overview(&inventory, &names, this.resource_id.as_deref());
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
            "localCopies": local_copies,
            "history": history_note(&inventory),
        }))
    }

    /// The inventory and, read in the same transaction, the last change logged about each system:
    /// what an agent's edit is based on (see `writes`). One transaction, so that both are of one
    /// state of the inventory: a change committed between two reads would give a `latestChange`
    /// newer than what the agent is shown, and an edit based on it would pass `basedOn` without its
    /// agent having seen that change.
    fn inventory_with_latest(&self) -> Result<(Inventory, HashMap<Subject, String>), ApiError> {
        self.read(|db| {
            let tx = inventory::begin(db, rusqlite::TransactionBehavior::Deferred)?;
            let inventory = inventory::list_in(&tx)?;
            #[cfg(test)]
            tests::between_reads();
            let latest = change_log::latest(&tx, &subjects(&inventory))?;
            tx.commit().map_err(ApiError::database)?;
            Ok((inventory, latest))
        })
    }

    fn list_systems(&self) -> Result<Value, ApiError> {
        let (inventory, latest) = self.inventory_with_latest()?;
        let now = records::now_ms();
        let names = Names::of(&inventory);
        let this = self.this_workstation(&inventory);
        let this = this.resource_id.as_deref();
        let applications = inventory
            .applications
            .iter()
            .map(|application| {
                let subject = Subject::Application {
                    id: application.id.clone(),
                };
                summary(&inventory, &names, &subject, now, this)
                    .map(|value| with_latest(value, &latest, &subject))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let resources = inventory
            .resources
            .iter()
            .map(|resource| {
                let subject = Subject::Resource {
                    id: resource.id.clone(),
                };
                summary(&inventory, &names, &subject, now, this)
                    .map(|value| with_latest(value, &latest, &subject))
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
        let (inventory, latest) = self.inventory_with_latest()?;
        let now = records::now_ms();
        let names = Names::of(&inventory);
        let subject = find_system(&inventory, arguments)?;
        let this = self.this_workstation(&inventory);
        let mut system = with_latest(
            summary(
                &inventory,
                &names,
                &subject,
                now,
                this.resource_id.as_deref(),
            )?,
            &latest,
            &subject,
        );
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
        self.read(|db| {
            let inventory = inventory::list_from(db)?;
            let subject = find_system(&inventory, arguments)?;
            match arguments.get("records").and_then(Value::as_str) {
                Some("observations") => {
                    let before = text(arguments, "before")?;
                    let page = records::older_observations_in(db, &subject, before)?;
                    Ok(json!({ "observations": page.observations, "more": page.more }))
                }
                Some("events") => {
                    let before = text(arguments, "before")?;
                    let page =
                        records::older_events_in(db, std::slice::from_ref(&subject), before)?;
                    Ok(json!({ "events": page.events, "more": page.more }))
                }
                // Readings start from the newest when no reading is named.
                Some("readings") => {
                    let before = match arguments.get("before") {
                        None | Some(Value::Null) => None,
                        Some(_) => Some(text(arguments, "before")?),
                    };
                    let page = readings::older_readings_in(db, &subject, before)?;
                    Ok(json!({ "readings": page.readings, "more": page.more }))
                }
                _ => Err(ApiError::invalid(
                    "records must be \"observations\", \"events\" or \"readings\".",
                )),
            }
        })
    }
}

const INSTRUCTIONS: &str = "DevHQ is the person's local operations inventory: applications, the resources they run on, where each one is (its references: repository, local copies, documentation, logs, dashboard), and the evidence about them (observations, incidents, events, HTTP checks). Working in a folder, an agent can find its system by a local copy's path or by its repository's URL in `list_systems`. A local copy's readings say what it held when DevHQ read it, never what it holds now. Start with `attention` to answer «what needs my attention?». Never present a system without current evidence as healthy: unobserved and expired evidence are gaps, not health. Every record says who produced it (`source`: manual, http or agent) and when. Applications, resources, components and references are declared inventory, by the person or an agent (each change in `changes`, with who made it), never evidence that DevHQ observed. To edit a system, base the edit on its `latestChange` (`basedOn`): DevHQ refuses it if the system changed after you read it, and never removes a component or reference you leave out. A component's `runsOn` and a path's `machine` name a resource by its id, never by its name. Every call is checked against its tool's input schema and the rules its description states: an argument the tool does not declare, misspelled or of the wrong type, is refused (invalid_input) and nothing is done.";

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

/// Where a system is, as an agent reads it: each path's machine by name, for a repository the
/// other systems that declare the same one, and for a local copy whether it is on this computer,
/// its newest reading and what DevHQ derives from it, at this read as the window derives it.
fn reference_list(
    inventory: &Inventory,
    names: &Names,
    subject: &Subject,
    references: &[Reference],
    this: Option<&str>,
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
            if let Some(copy) = inventory
                .local_copies
                .iter()
                .find(|copy| copy.reference_id == reference.id)
            {
                value["onThisWorkstation"] = json!(this == Some(copy.machine_id.as_str()));
                value["latestReading"] = json!(copy.latest);
                value["movedSinceRead"] = json!(copy.moved_since_read);
                value["repository"] = json!(copy.repository);
                value["localWork"] = json!(copy.local_work);
            }
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
    this: Option<&str>,
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
            })), "components": components, "references": application.map(|item| reference_list(inventory, names, subject, &item.references, this)) })
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
            })), "usedBy": used_by, "references": resource.map(|item| reference_list(inventory, names, subject, &item.references, this)) })
        }
    };
    if let Value::Object(object) = &mut value {
        object.insert(
            "evidence".into(),
            json!({ "state": state, "latest": latest }),
        );
        object.insert("activeIncidents".into(), json!(active_incidents));
        object.insert("httpCheck".into(), json!(check_of(inventory, subject)));
        object.insert("localActivity".into(), local_activity(inventory, subject));
    }
    Ok(value)
}

/// A system's summary with the last change logged about it (`latestChange`, null when there is
/// none): what an edit of it is based on (`basedOn`).
fn with_latest(mut value: Value, latest: &HashMap<Subject, String>, subject: &Subject) -> Value {
    if let Value::Object(object) = &mut value {
        object.insert("latestChange".into(), json!(latest.get(subject)));
    }
    value
}

/// The newest commit any reading of the system's copies found, and how many of its copies were
/// never read: a lower bound, never «inactive» while a copy is unread.
fn local_activity(inventory: &Inventory, subject: &Subject) -> Value {
    let copies: Vec<&LocalCopy> = inventory
        .local_copies
        .iter()
        .filter(|copy| &copy.subject == subject)
        .collect();
    let last_commit_at = copies
        .iter()
        .filter_map(|copy| copy.latest.as_ref()?.last_commit_at.clone())
        .max();
    json!({
        "lastCommitAt": last_commit_at,
        "copies": copies.len(),
        "copiesNeverRead": copies.iter().filter(|copy| copy.latest.is_none()).count(),
    })
}

/// What the local copies say beside «what needs my attention?», and never in its ranking: they are
/// not health. Copies of archived applications are left out.
fn local_copies_overview(inventory: &Inventory, names: &Names, this: Option<&str>) -> Value {
    let archived = |subject: &Subject| match subject {
        Subject::Application { id } => inventory
            .applications
            .iter()
            .any(|item| &item.id == id && item.lifecycle == "archived"),
        Subject::Resource { .. } => false,
    };
    let current: Vec<&LocalCopy> = inventory
        .local_copies
        .iter()
        .filter(|copy| !archived(&copy.subject))
        .collect();
    let described = |copy: &&LocalCopy| {
        json!({
            "system": names.system(&copy.subject),
            "path": copy.path,
            "machine": names.system(&Subject::Resource { id: copy.machine_id.clone() }),
            "reading": copy.latest,
        })
    };
    json!({
        "work": current.iter().filter(|copy| copy.local_work).map(described).collect::<Vec<_>>(),
        "notFound": current
            .iter()
            .filter(|copy| copy.latest.as_ref().is_some_and(|reading| reading.outcome != "read"))
            .map(described)
            .collect::<Vec<_>>(),
        "neverReadOnThisWorkstation": current
            .iter()
            .filter(|copy| copy.latest.is_none() && this == Some(copy.machine_id.as_str()))
            .map(described)
            .collect::<Vec<_>>(),
        "note": "Not health, and not ranked: what each copy held when it was read (`reading.observedAt`), which may no longer be so. `work` held commits on no remote, stashes, changes, new entries, files DevHQ could not verify or an operation left halfway; `notFound` were not where declared, or not readable. Only copies on this workstation can be read (`read_copies`), and only when asked.",
    })
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

/// Among `candidates` (ID, name) of one kind, the one the arguments name by `id` or by `name`
/// (exact, ignoring case and surrounding spaces): its subject and name. Given both, they must name
/// the same one: an agent that sends one system's id with another's name is refused, never taken
/// at its id.
fn pick(
    kind: &str,
    candidates: &[(String, String)],
    arguments: &Map<String, Value>,
) -> Result<(Subject, String), ApiError> {
    let id = arguments.get("id").and_then(Value::as_str);
    let name = arguments.get("name").and_then(Value::as_str);
    let named = |item: &str, name: &str| item.to_lowercase() == name.trim().to_lowercase();
    let found: Vec<&(String, String)> = match (id, name) {
        (Some(id), _) => candidates.iter().filter(|(item, _)| item == id).collect(),
        (None, Some(name)) => candidates
            .iter()
            .filter(|(_, item)| named(item, name))
            .collect(),
        (None, None) => return Err(ApiError::invalid("Name the system by id or by name.")),
    };
    if let (Some(id), Some(name), [(_, actual)]) = (id, name, found.as_slice()) {
        if !named(actual, name) {
            return Err(ApiError::invalid(format!(
                "The {kind} with the id {id} is named «{actual}», not «{name}». Name it by its id, by its name, or by both when they are the same {kind}'s."
            )));
        }
    }
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
        "id": { "type": "string", "description": "The system's id, from list_systems. Give id, name, or both." },
        "name": { "type": "string", "description": "Or its exact name, ignoring case. Given with id, it must be that system's name, or nothing is done." },
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
"Which inventory file the gate opened, where the system really keeps it, what the file is (`schema`: the version it declares, the one this DevHQ supports, its state: current, mismatch, outdated, newer, not_an_inventory, needs_recovery, unreadable or missing; whether it has the structure of this DevHQ's schema, compared table by table and column by column: matches, differs or not_checked for another version; its integrity, never checked by the gate: not_checked; `schemaVersion` is the declared one, null when unknown), how far the person lets agents go through the gate (`access`: off; read; maintain, which also adds and edits applications and resources; or write, everything), which holds for every agent session until the person changes it, whether a DevHQ window is open on it (`windowOpen`; null when the system cannot say), how many systems it holds, which of its workstations this computer is (`thisWorkstation`, or why DevHQ does not know: only the window chooses it), and what the gate cannot know. Access, counts and workstation are null unless the inventory is current, which is the only kind the gate serves. An inventory left needing recovery (a write that did not finish, or a log no connection holds) is reported, never recovered: DevHQ's window recovers it when it starts, only if it is in this DevHQ's schema. Always answered, even with the gate off, and never writes to the inventory or changes what it holds; the one file it can create is an empty log (`-wal`), if the last connection to the inventory closes just as the gate opens it.",
            json!({}),
            &[],
        ),
        read_only(
            "attention",
            "What needs attention",
            "«What needs my attention?», ranked as DevHQ's Overview ranks it: active incidents and problem evidence, most urgent first, plus coverage (current, expired, never observed) and the systems nothing supervises. An empty list is only reassuring when status is `covered`. Beside it, unranked and not health, `localCopies`: the copies whose last reading held work that existed only there, the ones not found, and the ones on this workstation never read.",
            json!({}),
            &[],
        ),
        read_only(
            "list_systems",
            "List applications and resources",
            "Every application and resource with its evidence state and latest observation, active incidents, HTTP check, relations (components and what they run on; what uses each resource) and references: where its repository, its local copies (each path with the machine it is on), its documentation, its logs and its dashboard are, and which other systems declare the same repository (`sameRepositoryAs`, by exact URL). References are declared inventory: DevHQ never opens or checks them. Each local copy also says whether it is on this workstation and its newest reading (`latestReading`, null if never read): what the copy held when DevHQ read it, at `observedAt`, which may no longer be so; `localActivity` gives the newest commit those readings found. Each system's `latestChange` (the last entry of its change log, null when it has none) is what an edit of it is based on (`basedOn`).",
            json!({}),
            &[],
        ),
        read_only(
            "get_system",
            "System detail",
            "One application or resource with its references (as list_systems gives them), every loaded observation, incident and event about it, every change to its configuration (who made it, and each field before and after), newest first, its `latestChange` (what an edit of it is based on), and from when its automated results are complete.",
            system_arguments(json!({})),
            &["type"],
        ),
        read_only(
            "history",
            "Older results and readings",
            "Automated HTTP results (observations, or their events) older than one already shown, 200 at a time, for a system whose automated history get_system says is cut; or the readings of a system's local copies, newest first, 200 at a time (before optional: the first page starts from the newest).",
            system_arguments(json!({
                "records": { "type": "string", "enum": ["observations", "events", "readings"] },
                "before": { "type": ["string", "null"], "description": "The id of the oldest observation, event or reading already shown. Required for observations and events; for readings, left out or null, the first page starts from the newest." },
            })),
            &["type", "records"],
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
        let path = folder.join("inventory.sqlite3");
        inventory::start(&path).unwrap();
        path
    }

    /// Where a test inventory would be, in a folder of its own that holds nothing yet.
    fn nothing() -> PathBuf {
        let path = path();
        std::fs::remove_file(&path).unwrap();
        path
    }

    fn remove(path: &Path) {
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    /// The computer the tests run on: its local folder beside the inventory, and a fingerprint of
    /// its own, so that no test reads or writes this machine's real association.
    fn computer(path: &Path) -> Computer {
        Computer {
            local: path.parent().unwrap().join("local"),
            fingerprint: Some("gate-test-computer".into()),
        }
    }

    /// Shop runs its API on Web host; Spare box runs nothing and was never observed. Web host
    /// is unhealthy and supervised, Shop has a critical incident and healthy evidence.
    fn seed(path: &Path) -> (String, String) {
        // As the window creates it, when there is none yet.
        if !path.exists() {
            inventory::start(path).unwrap();
        }
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
        serve(database, computer(database), input.as_bytes(), &mut output).unwrap();
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
                "read_copies",
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
            computer(&path),
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
        let path = nothing();
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
                .contains("`type` must be one of \"application\", \"resource\"")
        );
        assert!(refusal(&path, "get_system", json!({ "type": "resource" }))
            .contains("Name the system by `id` or by `name`"));
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
        .contains("`records` must be one of"));
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
            computer: computer(&path),
            reader: None,
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
            computer: computer(path),
            reader: None,
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

    /// What an agent's next edit of a system is based on: its latestChange, as get_system gives
    /// it to the agent.
    fn based_on(agent: &mut Session, kind: &str, id: &str) -> Value {
        use_tool(agent, "get_system", json!({ "type": kind, "id": id })).unwrap()["system"]
            ["latestChange"]
            .clone()
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
            ("read_copies", json!({})),
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
        // A component runs on a resource named by its id.
        let added = use_tool(
            &mut agent,
            "save_application",
            json!({ "name": "Tienda", "lifecycle": "production", "description": "Online store", "version": "1.3.0",
                    "components": [{ "name": "API", "kind": "backend", "runsOn": vps_id }, { "name": "Web", "kind": "frontend" }] }),
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
        // An edit of one field keeps every other, components included. It is based on the
        // application's latest change, which the answer to each save gives.
        let edited = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": tienda, "basedOn": added["change"]["id"], "version": "1.4.0" }),
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
            json!({ "id": tienda, "basedOn": edited["change"]["id"], "version": null, "lifecycle": "testing" }),
        )
        .unwrap();
        assert_eq!(cleared["application"]["version"], Value::Null);
        assert_eq!(
            cleared["change"]["summary"],
            "Updated application «Tienda»: lifecycle production → testing; version «1.4.0» → none"
        );
        // A component list keeps what it leaves out: naming one by its id changes only that one,
        // and here nothing, so there is no change to log.
        let api = added["application"]["components"][0]["id"].clone();
        let web = added["application"]["components"][1]["id"].clone();
        let kept = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": tienda, "basedOn": cleared["change"]["id"], "components": [{ "id": api, "runsOn": vps_id }] }),
        )
        .unwrap();
        assert_eq!(
            kept["application"]["components"],
            added["application"]["components"]
        );
        assert_eq!(kept["change"], Value::Null);
        // Removing one takes naming it.
        let trimmed = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": tienda, "basedOn": cleared["change"]["id"], "removeComponents": [web] }),
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
            json!({ "id": vps_id, "basedOn": vps["change"]["id"], "name": "VPS Hetzner" }),
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
                json!({ "id": "ghost", "basedOn": null, "version": "2" }),
                "No application has the id ghost",
            ),
            (
                "save_application",
                json!({ "name": "Bad", "lifecycle": "production", "components": [{ "name": "X", "kind": "lambda" }] }),
                "`components[0].kind` must be one of",
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
        // Each path's machine by its id.
        let read = based_on(&mut agent, "application", &shop);
        let saved = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": shop, "basedOn": read, "references": [
                { "kind": "repository", "target": "https://github.com/me/platform" },
                { "kind": "local_copy", "target": r"C:\src\platform", "machine": laptop_id, "label": "Work" },
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
            json!({ "id": shop, "basedOn": saved["change"]["id"], "version": "2.5.0" }),
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
                        "machine": { "type": "resource", "id": laptop_id, "name": "Laptop" },
                        // Never read, and this computer is no workstation of this inventory.
                        "onThisWorkstation": false, "latestReading": null, "movedSinceRead": false,
                        "repository": null, "localWork": false }),
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
        let shop_now = based_on(&mut agent, "application", &shop);
        let laptop_now = based_on(&mut agent, "resource", &laptop_id);
        for (tool, arguments, why) in [
            (
                "save_application",
                json!({ "id": shop, "basedOn": shop_now, "references": "https://github.com/me/platform" }),
                "`references` must be a list, not a string.",
            ),
            (
                "save_application",
                json!({ "id": shop, "basedOn": shop_now, "references": [{ "target": "https://x.example" }] }),
                "`references[0].kind` is required for a new reference",
            ),
            (
                "save_application",
                json!({ "id": shop, "basedOn": shop_now, "references": [{ "kind": "local_copy", "target": "/srv/shop", "machine": host }] }),
                "Web host is a server, not a workstation",
            ),
            (
                "save_resource",
                json!({ "id": laptop_id, "basedOn": laptop_now, "kind": "server" }),
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
        assert!(refused.contains("`result` must be one of"), "{refused}");

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
    fn reads_local_copies_when_asked_and_keeps_which_agent_asked() {
        let path = path();
        seed(&path);
        let folder = path.parent().unwrap().to_owned();
        let copy = folder.join("shop-copy");
        std::fs::create_dir(&copy).unwrap();
        crate::local_copies::tests::git(&copy, &["init", "-q", "-b", "main"]);
        std::fs::write(copy.join("a.txt"), "a\n").unwrap();
        crate::local_copies::tests::git(&copy, &["add", "-A"]);
        crate::local_copies::tests::git(&copy, &["commit", "-q", "-m", "a"]);
        std::fs::write(copy.join("a.txt"), "changed\n").unwrap();
        crate::access::set_agent_access(&path, "write").unwrap();
        let mut agent = session(&path, "claude-code");
        agent.reader = Some(crate::local_copies::tests::test_reader("confined", &[]));
        // An agent declares where the copy is; which workstation this computer is, it cannot say.
        let laptop = use_tool(
            &mut agent,
            "save_resource",
            json!({ "name": "Laptop", "kind": "workstation" }),
        )
        .unwrap()["resource"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let shop = inventory::list(&path).unwrap().applications[0].id.clone();
        let read = based_on(&mut agent, "application", &shop);
        use_tool(
            &mut agent,
            "save_application",
            json!({ "name": "Shop", "id": shop, "basedOn": read,
                    "references": [{ "kind": "local_copy", "target": copy.to_str().unwrap(), "machine": laptop }] }),
        )
        .unwrap();
        let refused = use_tool(&mut agent, "read_copies", json!({})).unwrap_err();
        assert!(
            refused.contains("does not know which workstation"),
            "{refused}"
        );
        let status = use_tool(&mut agent, "status", json!({})).unwrap();
        assert_eq!(status["thisWorkstation"]["problem"], "not_chosen");
        // The person chooses it in the window.
        computer(&path)
            .choose(&inventory::list(&path).unwrap().resources, Some(&laptop))
            .unwrap();
        let status = use_tool(&mut agent, "status", json!({})).unwrap();
        assert_eq!(status["thisWorkstation"]["resourceId"], laptop.as_str());

        let logged = changes(&path).len();
        let read = use_tool(&mut agent, "read_copies", json!({})).unwrap();
        assert_eq!(read["failures"], json!([]));
        let reading = &read["readings"][0];
        assert_eq!(reading["agent"], "claude-code");
        assert_eq!(reading["source"], "read");
        assert_eq!(
            (reading["outcome"].as_str(), reading["changed"].as_i64()),
            (Some("read"), Some(1))
        );
        // A reading is evidence, not a change to the configuration.
        assert_eq!(changes(&path).len(), logged);
        // By system, or by reference, as well.
        use_tool(
            &mut agent,
            "read_copies",
            json!({ "type": "application", "name": "Shop" }),
        )
        .unwrap();
        let reference = reading["referenceId"].as_str().unwrap().to_owned();
        use_tool(
            &mut agent,
            "read_copies",
            json!({ "referenceId": reference }),
        )
        .unwrap();

        let listed = use_tool(&mut agent, "list_systems", json!({})).unwrap();
        let shop = &listed["applications"][0];
        let copy_reference = shop["references"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["kind"] == "local_copy")
            .unwrap();
        assert_eq!(copy_reference["onThisWorkstation"], true);
        assert_eq!(copy_reference["localWork"], true);
        assert_eq!(copy_reference["latestReading"]["agent"], "claude-code");
        assert!(shop["localActivity"]["lastCommitAt"].is_string());
        let attention = use_tool(&mut agent, "attention", json!({})).unwrap();
        assert_eq!(
            attention["localCopies"]["work"][0]["system"]["name"],
            "Shop"
        );
        // Not health: the ranked list and the status are what they were.
        assert_eq!(attention["status"], "attention");
        let history = use_tool(
            &mut agent,
            "history",
            json!({ "type": "application", "name": "Shop", "records": "readings" }),
        )
        .unwrap();
        assert_eq!(history["readings"].as_array().unwrap().len(), 3);
        assert_eq!(history["more"], false);
        // Read only shows readings and never causes one.
        crate::access::set_agent_access(&path, "read").unwrap();
        assert!(use_tool(&mut agent, "read_copies", json!({}))
            .unwrap_err()
            .contains("reading only"));
        remove(&path);
    }

    #[test]
    fn a_client_without_a_name_is_still_told_apart_from_the_person() {
        let path = path();
        seed(&path);
        crate::access::set_agent_access(&path, "write").unwrap();
        let mut agent = Session {
            database: path.clone(),
            computer: computer(&path),
            reader: None,
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

    /// The tests run an executable the installer never packed, as `tauri dev` and `target/` do.
    #[test]
    fn says_when_it_keeps_development_data() {
        let path = path();
        seed(&path);
        let status = answer(&path, "status", json!({}));
        assert!(
            status["notes"].as_array().unwrap().iter().any(|note| note
                .as_str()
                .unwrap()
                .contains("development data (com.devhq.desktop.dev)")),
            "{status:#}"
        );
        remove(&path);
    }

    /// Every row of every table, as text: what a refused call must leave exactly as it was.
    fn rows(path: &Path) -> Vec<(String, Vec<String>)> {
        let db = rusqlite::Connection::open(path).unwrap();
        let tables: Vec<String> = db
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        tables
            .into_iter()
            .map(|table| {
                let dumped = crate::inventory::tests::dump(&db, &table);
                (table, dumped)
            })
            .collect()
    }

    /// Arguments each tool accepts, on the inventory `seed` makes: one call per tool it declares.
    fn valid_calls() -> Vec<(&'static str, Value)> {
        vec![
            ("status", json!({})),
            ("attention", json!({})),
            ("list_systems", json!({})),
            (
                "get_system",
                json!({ "type": "resource", "name": "Web host" }),
            ),
            (
                "history",
                json!({ "type": "resource", "name": "Web host", "records": "readings" }),
            ),
            (
                "save_application",
                json!({ "name": "New", "lifecycle": "development" }),
            ),
            ("save_resource", json!({ "name": "New", "kind": "server" })),
            ("delete_application", json!({ "name": "Shop" })),
            ("delete_resource", json!({ "name": "Spare box" })),
            (
                "record_observation",
                json!({ "type": "application", "name": "Shop", "result": "healthy", "summary": "Fine", "validForMinutes": 60 }),
            ),
            (
                "open_incident",
                json!({ "type": "application", "name": "Shop", "title": "Down", "severity": "critical" }),
            ),
            (
                "update_incident",
                json!({ "incidentId": "none", "status": "resolved" }),
            ),
            (
                "record_event",
                json!({ "type": "application", "name": "Shop", "kind": "note", "outcome": "info", "title": "Noted" }),
            ),
            (
                "save_check",
                json!({ "type": "application", "name": "Shop", "url": "https://shop.example/" }),
            ),
            (
                "set_check_paused",
                json!({ "type": "resource", "name": "Web host", "paused": true }),
            ),
            (
                "remove_check",
                json!({ "type": "resource", "name": "Web host" }),
            ),
            (
                "run_check",
                json!({ "type": "resource", "name": "Web host" }),
            ),
            ("backup", json!({})),
            ("read_copies", json!({})),
        ]
    }

    #[test]
    fn every_tool_refuses_an_argument_it_does_not_declare_before_opening_anything() {
        let calls = valid_calls();
        let declared: Vec<String> = tools()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect();
        let covered: Vec<String> = calls.iter().map(|(tool, _)| (*tool).to_owned()).collect();
        assert_eq!(covered, declared, "one call per tool the gate declares");

        // No inventory, not even its folder: a refused call never gets as far as looking for one.
        let base = path();
        let nowhere = base
            .parent()
            .unwrap()
            .join("missing")
            .join("inventory.sqlite3");
        let mut agent = session(&nowhere, "claude-code");
        for (tool, arguments) in &calls {
            let mut stray = arguments.clone();
            stray["unexpected"] = json!(true);
            let refused = use_tool(&mut agent, tool, stray).unwrap_err();
            assert!(
                refused.starts_with(&format!(
                    "{tool} was not run: `unexpected` is not an argument of this tool;"
                )) && refused.ends_with("(invalid_input)"),
                "{tool}: {refused}"
            );
            // With what it declares, it is run: it looks for the inventory, or answers status.
            let outcome = use_tool(&mut agent, tool, arguments.clone());
            match outcome {
                Ok(status) => {
                    assert_eq!((*tool, status["exists"].clone()), ("status", json!(false)))
                }
                Err(refused) => assert!(
                    refused.contains("There is no DevHQ inventory"),
                    "{tool}: {refused}"
                ),
            }
        }
        assert!(!nowhere.parent().unwrap().exists());

        // With the gate open for writing, a refused call changes nothing and logs nothing.
        let path = path();
        seed(&path);
        crate::access::set_agent_access(&path, "write").unwrap();
        let before = rows(&path);
        let mut agent = session(&path, "claude-code");
        for (tool, arguments) in &calls {
            let mut stray = arguments.clone();
            stray["Name"] = json!("Other");
            let refused = use_tool(&mut agent, tool, stray).unwrap_err();
            assert!(
                refused.contains("`Name` is not an argument"),
                "{tool}: {refused}"
            );
        }
        assert_eq!(rows(&path), before);
        assert!(!path.parent().unwrap().join("backups").exists());
        remove(&path);
        remove(&base);
    }

    /// The calls the diagnosis of the gate reproduced, each of which used to be taken in silence.
    #[test]
    fn refuses_what_it_used_to_ignore_in_silence() {
        let path = path();
        let (shop, host) = seed(&path);
        crate::access::set_agent_access(&path, "write").unwrap();
        let mut agent = session(&path, "claude-code");
        let shop_now = |path: &Path| -> Value {
            let listed = inventory::list(path).unwrap();
            json!(listed.applications.iter().find(|item| item.id == shop))
        };
        let kept = shop_now(&path);
        // A resource without history, which nothing runs on: a delete that reached it would go.
        let spare = inventory::list(&path)
            .unwrap()
            .resources
            .into_iter()
            .find(|item| item.name == "Spare box")
            .unwrap()
            .id;
        let before = rows(&path);
        for (tool, arguments, why) in [
            // A misspelled field used to answer «nothing changed».
            (
                "save_application",
                json!({ "id": shop, "descripton": "Changed" }),
                "`descripton` is not an argument of this tool; it takes ",
            ),
            (
                "save_application",
                json!({ "id": shop, "Description": "Changed" }),
                "Did you mean `description`?",
            ),
            (
                "save_application",
                json!({ "id": shop, "description": null }),
                "`description` must be a string, not null.",
            ),
            // A component used to be saved without the resource it runs on.
            (
                "save_application",
                json!({ "id": shop, "components": [{ "name": "API", "kind": "backend", "runOn": "Web host" }] }),
                "`components[0].runOn` is not a field of `components[0]`; it takes ",
            ),
            (
                "save_application",
                json!({ "id": shop, "components": [{ "name": "API", "kind": "backend", "runOn": "Web host" }] }),
                "Did you mean `runsOn`?",
            ),
            // A Host used to be deleted by its id, whatever the type and name said.
            (
                "delete_resource",
                json!({ "type": "application", "name": "Nope", "id": spare }),
                "`type` is not an argument of this tool; it takes ",
            ),
            (
                "delete_resource",
                json!({ "name": "Web host", "id": spare }),
                "is named «Spare box», not «Web host»",
            ),
            (
                "delete_resource",
                json!({ "name": "Nope", "id": spare }),
                "is named «Spare box», not «Nope»",
            ),
            (
                "delete_application",
                json!({ "name": "Web host", "id": shop }),
                "is named «Shop», not «Web host»",
            ),
            // The id used to win over a name of another system, and a numeric id to be ignored.
            (
                "get_system",
                json!({ "type": "application", "id": shop, "name": "Web host" }),
                "is named «Shop», not «Web host»",
            ),
            (
                "get_system",
                json!({ "type": "resource", "id": 1, "name": "Web host" }),
                "`id` must be a string, not 1.",
            ),
            (
                "save_check",
                json!({ "type": "resource", "id": host, "name": "Spare box", "url": "https://x.example/" }),
                "is named «Web host», not «Spare box»",
            ),
            (
                "status",
                json!({ "verbose": true }),
                "`verbose` is not an argument",
            ),
            (
                "list_systems",
                json!({ "filter": "Shop" }),
                "`filter` is not an argument",
            ),
            // An id without its type used to read every copy of this workstation instead.
            (
                "read_copies",
                json!({ "id": shop }),
                "`id` and `name` name a system only with its `type`",
            ),
            (
                "read_copies",
                json!({ "referenceId": "r", "type": "application", "name": "Shop" }),
                "not both",
            ),
            (
                "read_copies",
                json!({ "type": "application" }),
                "Name the system by `id` or by `name`.",
            ),
            (
                "record_observation",
                json!({ "type": "application", "id": shop, "result": "healthy", "summary": "ok", "validForMinutes": 30.5 }),
                "`validForMinutes` must be a whole number, not 30.5.",
            ),
            (
                "record_observation",
                json!({ "type": "application", "id": shop, "result": "healthy", "summary": "ok", "validForMinutes": 0 }),
                "`validForMinutes` must be at least 1, not 0.",
            ),
            (
                "history",
                json!({ "type": "application", "id": shop, "records": "observations" }),
                "`before` is required for observations and events",
            ),
            (
                "history",
                json!({ "type": "application", "id": shop, "records": "events", "before": null }),
                "`before` is required for observations and events",
            ),
            (
                "save_check",
                json!({ "type": "application", "id": shop, "url": "https://x.example/", "expectedStatus": 700 }),
                "`expectedStatus` must be at most 599, not 700.",
            ),
            (
                "save_resource",
                json!({ "description": "No name, no kind" }),
                "name is required to add a resource. kind is required to add a resource",
            ),
        ] {
            let refused = use_tool(&mut agent, tool, arguments.clone()).unwrap_err();
            assert!(refused.contains(why), "{tool} {arguments}: {refused}");
            assert!(refused.ends_with("(invalid_input)"), "{refused}");
        }
        assert_eq!(rows(&path), before);
        assert_eq!(shop_now(&path), kept);

        // Both selectors of one system, in any case: that system.
        let detail = use_tool(
            &mut agent,
            "get_system",
            json!({ "type": "application", "id": shop, "name": " SHOP " }),
        )
        .unwrap();
        assert_eq!(detail["system"]["system"]["id"], shop.as_str());
        let deleted = use_tool(
            &mut agent,
            "delete_resource",
            json!({ "id": spare, "name": "spare BOX" }),
        )
        .unwrap();
        assert_eq!(deleted["deleted"]["name"], "Spare box");
        // A whole number written with a fractional part of zero is a whole number.
        let observed = use_tool(
            &mut agent,
            "record_observation",
            json!({ "type": "application", "id": shop, "result": "healthy", "summary": "ok", "validForMinutes": 30.0 }),
        )
        .unwrap();
        let observation = &observed["observation"];
        let lasts = |field: &str| {
            time::OffsetDateTime::parse(
                observation[field].as_str().unwrap(),
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap()
        };
        assert_eq!(
            lasts("validUntil") - lasts("observedAt"),
            time::Duration::minutes(30)
        );
        let checked = use_tool(
            &mut agent,
            "save_check",
            json!({ "type": "application", "id": shop, "url": "https://x.example/", "timeoutSeconds": 2.0 }),
        )
        .unwrap();
        assert_eq!(checked["httpCheck"]["timeoutSeconds"], 2);
        // Only what was accepted is logged.
        let actions: Vec<String> = changes(&path)
            .into_iter()
            .map(|(_, action, _)| action)
            .collect();
        assert_eq!(actions, ["deleted", "check_saved"]);
        assert!(inventory::list(&path)
            .unwrap()
            .resources
            .iter()
            .any(|item| item.id == host));
        remove(&path);
    }

    /// Null is taken where the schema says so, and means what the tool's description says.
    #[test]
    fn takes_null_only_where_the_schema_does() {
        let path = path();
        let (shop, _) = seed(&path);
        crate::access::set_agent_access(&path, "write").unwrap();
        let mut agent = session(&path, "claude-code");
        let api = inventory::list(&path).unwrap().applications[0].components[0]
            .id
            .clone();
        let read = based_on(&mut agent, "application", &shop);
        let saved = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": shop, "basedOn": read, "version": null,
                    "components": [{ "id": api, "runsOn": null }],
                    "references": [{ "kind": "repository", "target": "https://github.com/me/shop", "label": null, "machine": null }] }),
        )
        .unwrap();
        assert_eq!(saved["application"]["version"], Value::Null);
        assert_eq!(
            saved["application"]["components"][0]["resourceId"],
            Value::Null
        );
        let checked = use_tool(
            &mut agent,
            "save_check",
            json!({ "type": "application", "id": shop, "url": "https://shop.example/", "supervisionIntervalMinutes": null }),
        )
        .unwrap();
        assert_eq!(checked["httpCheck"]["monitor"], "off");
        let readings = use_tool(
            &mut agent,
            "history",
            json!({ "type": "application", "id": shop, "records": "readings", "before": null }),
        )
        .unwrap();
        assert_eq!(readings["readings"], json!([]));
        for (tool, arguments, why) in [
            (
                "save_application",
                json!({ "id": null, "name": "Other", "lifecycle": "development" }),
                "`id` must be a string, not null.",
            ),
            (
                "open_incident",
                json!({ "type": "application", "id": shop, "title": "Down", "severity": "warning", "description": null }),
                "`description` must be a string, not null.",
            ),
            (
                "get_system",
                json!({ "type": "application", "id": shop, "name": null }),
                "`name` must be a string, not null.",
            ),
        ] {
            let refused = use_tool(&mut agent, tool, arguments).unwrap_err();
            assert!(refused.contains(why), "{tool}: {refused}");
        }
        remove(&path);
    }

    /// The tool's arguments are checked, never the rest of the request: `_meta` and whatever a
    /// later revision of the protocol adds to `params` stay the protocol's.
    #[test]
    fn checks_the_arguments_and_leaves_the_protocol_extensible() {
        let path = path();
        seed(&path);
        let replies = exchange(
            &path,
            &[
                json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
                    "name": "get_system", "arguments": { "type": "resource", "name": "Web host" },
                    "_meta": { "progressToken": "p-1", "io.example/trace": { "span": 7 } },
                } }),
                json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
                    "name": "status", "_meta": {}, "task": { "ttl": 60000 },
                } }),
                json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "attention", "arguments": null } }),
                json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list", "params": { "_meta": { "progressToken": 2 } } }),
                // `_meta` inside the arguments is an argument like any other: the tool takes none.
                json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {
                    "name": "attention", "arguments": { "_meta": {} },
                } }),
            ],
        );
        assert_eq!(replies[0]["result"]["isError"], false, "{:#}", replies[0]);
        assert_eq!(
            replies[0]["result"]["structuredContent"]["system"]["system"]["name"],
            "Web host"
        );
        assert_eq!(replies[1]["result"]["isError"], false, "{:#}", replies[1]);
        assert_eq!(replies[2]["result"]["isError"], false, "{:#}", replies[2]);
        assert_eq!(replies[3]["result"]["tools"].as_array().unwrap().len(), 19);
        assert_eq!(replies[4]["result"]["isError"], true);
        remove(&path);
    }

    /// Every entry of `folder`, with a file's bytes and when each was last written: what a read
    /// through the gate must leave exactly as it found it.
    fn files(folder: &Path) -> Vec<(String, Option<Vec<u8>>, std::time::SystemTime)> {
        let mut found: Vec<_> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let metadata = entry.metadata().unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    metadata
                        .is_file()
                        .then(|| std::fs::read(entry.path()).unwrap()),
                    metadata.modified().unwrap(),
                )
            })
            .collect();
        found.sort();
        found
    }

    /// What lies at the gate's path, as each case makes it.
    type Make = fn(&Path);

    /// A case: what it makes at the gate's path, what the gate must find there (whether it
    /// exists, the schema version it stores, its state, the access and counts status gives) and
    /// the code every read and write is refused with; None when reads are answered.
    type Case = (
        &'static str,
        Make,
        bool,
        Value,
        &'static str,
        Value,
        Value,
        Option<&'static str>,
    );

    /// A v13 inventory as `seed` leaves it (write-ahead logged, Read only), replaced by a backup
    /// of itself: a rollback-journal file, as every DevHQ backup is.
    fn backup_copy(path: &Path) {
        seed(path);
        let copy = crate::backup::back_up(path).unwrap();
        std::fs::copy(&copy.path, path).unwrap();
    }

    fn sqlite(path: &Path, sql: &str) {
        rusqlite::Connection::open(path)
            .unwrap()
            .execute_batch(sql)
            .unwrap();
    }

    /// A read never creates, upgrades, converts or touches an inventory, whatever the file is;
    /// status says what it is, and nothing it could not read is presented as known.
    #[test]
    fn reads_leave_every_file_as_they_found_it_and_status_says_what_it_is() {
        use crate::inventory::tests::{database_at, kill, writer};
        let counts = json!({ "applications": 1, "resources": 2 });
        let cases: [Case; 19] = [
            (
                "v6, rollback journal",
                |path| drop(database_at(path, 6)),
                true,
                json!(6),
                "outdated",
                Value::Null,
                Value::Null,
                Some("schema_outdated"),
            ),
            (
                "v6, write-ahead log",
                |path| {
                    database_at(path, 6)
                        .pragma_update_and_check(None, "journal_mode", "WAL", |row| {
                            row.get::<_, String>(0)
                        })
                        .unwrap();
                },
                true,
                json!(6),
                "outdated",
                Value::Null,
                Value::Null,
                Some("schema_outdated"),
            ),
            (
                "v12, Read only",
                |path| {
                    database_at(path, 12)
                        .execute(
                            "INSERT INTO agent_access (id, mode, changed_at) VALUES (1, 'read', 0)",
                            [],
                        )
                        .unwrap();
                },
                true,
                json!(12),
                "outdated",
                Value::Null,
                Value::Null,
                Some("schema_outdated"),
            ),
            (
                "v13 backup, rollback journal, Read only",
                backup_copy,
                true,
                json!(13),
                "current",
                json!("read"),
                counts.clone(),
                None,
            ),
            (
                "v13, write-ahead log, Read only",
                |path| drop(seed(path)),
                true,
                json!(13),
                "current",
                json!("read"),
                counts.clone(),
                None,
            ),
            (
                "v13, write-ahead log, Off",
                |path| {
                    seed(path);
                    crate::access::set_agent_access(path, "off").unwrap();
                },
                true,
                json!(13),
                "current",
                json!("off"),
                Value::Null,
                Some("access_denied"),
            ),
            (
                "v14",
                |path| {
                    database_at(path, 13)
                        .pragma_update(None, "user_version", 14)
                        .unwrap();
                },
                true,
                json!(14),
                "newer",
                Value::Null,
                Value::Null,
                Some("schema_newer"),
            ),
            (
                "empty file",
                |path| std::fs::write(path, b"").unwrap(),
                true,
                Value::Null,
                "not_an_inventory",
                Value::Null,
                Value::Null,
                Some("not_an_inventory"),
            ),
            (
                "another program's SQLite database",
                |path| {
                    sqlite(
                        path,
                        "CREATE TABLE notes (body TEXT); INSERT INTO notes VALUES ('mine');",
                    )
                },
                true,
                Value::Null,
                "not_an_inventory",
                Value::Null,
                Value::Null,
                Some("not_an_inventory"),
            ),
            (
                "a SQLite database stamped 13 without DevHQ's tables",
                |path| {
                    sqlite(
                        path,
                        "CREATE TABLE notes (body TEXT); PRAGMA user_version = 13;",
                    )
                },
                true,
                Value::Null,
                "not_an_inventory",
                Value::Null,
                Value::Null,
                Some("not_an_inventory"),
            ),
            (
                "v13 stamped, with only the tables that tell an inventory apart",
                |path| {
                    sqlite(
                        path,
                        "CREATE TABLE applications (id TEXT PRIMARY KEY);
                         CREATE TABLE resources (id TEXT PRIMARY KEY);
                         PRAGMA user_version = 13;",
                    )
                },
                true,
                json!(13),
                "mismatch",
                Value::Null,
                Value::Null,
                Some("schema_mismatch"),
            ),
            (
                "v13 without agent_access",
                |path| {
                    seed(path);
                    sqlite(path, "DROP TABLE agent_access");
                },
                true,
                json!(13),
                "mismatch",
                Value::Null,
                Value::Null,
                Some("schema_mismatch"),
            ),
            (
                "v13, Off, its write-ahead log left by a writer that was killed",
                |path| {
                    seed(path);
                    crate::access::set_agent_access(path, "off").unwrap();
                    kill(writer(path, "log"), path);
                },
                true,
                Value::Null,
                "needs_recovery",
                Value::Null,
                Value::Null,
                Some("needs_recovery"),
            ),
            (
                "v13, Read only, its write-ahead log left without its index",
                |path| {
                    seed(path);
                    kill(writer(path, "log"), path);
                    let mut index = path.as_os_str().to_owned();
                    index.push("-shm");
                    std::fs::remove_file(index).unwrap();
                },
                true,
                Value::Null,
                "needs_recovery",
                Value::Null,
                Value::Null,
                Some("needs_recovery"),
            ),
            (
                "v6, rollback journal left by a write that was killed",
                |path| {
                    drop(database_at(path, 6));
                    kill(writer(path, "journal"), path);
                },
                true,
                Value::Null,
                "needs_recovery",
                Value::Null,
                Value::Null,
                Some("needs_recovery"),
            ),
            (
                "v13 backup, Read only, rollback journal left by a write that was killed",
                |path| {
                    backup_copy(path);
                    kill(writer(path, "journal"), path);
                },
                true,
                Value::Null,
                "needs_recovery",
                Value::Null,
                Value::Null,
                Some("needs_recovery"),
            ),
            (
                "a text file",
                |path| std::fs::write(path, "hello\n").unwrap(),
                true,
                Value::Null,
                "unreadable",
                Value::Null,
                Value::Null,
                Some("database_error"),
            ),
            (
                "a folder",
                |path| std::fs::create_dir(path).unwrap(),
                false,
                Value::Null,
                "unreadable",
                Value::Null,
                Value::Null,
                Some("database_error"),
            ),
            (
                "nothing",
                |_| {},
                false,
                Value::Null,
                "missing",
                Value::Null,
                Value::Null,
                Some("invalid_input"),
            ),
        ];
        let reads = [
            ("attention", json!({})),
            ("list_systems", json!({})),
            (
                "get_system",
                json!({ "type": "resource", "name": "Web host" }),
            ),
            (
                "history",
                json!({ "type": "resource", "name": "Web host", "records": "readings" }),
            ),
        ];
        // None of these may write, the 14 tools that can: the gate is at most open for reading in
        // every case, and no file that is not a current inventory is ever opened to write.
        let writes = [
            (
                "save_application",
                json!({ "name": "New", "lifecycle": "development" }),
            ),
            ("save_resource", json!({ "name": "New", "kind": "server" })),
            ("delete_application", json!({ "name": "Shop" })),
            ("delete_resource", json!({ "name": "Spare box" })),
            (
                "save_check",
                json!({ "type": "resource", "name": "Spare box", "url": "https://spare.example/" }),
            ),
            (
                "set_check_paused",
                json!({ "type": "resource", "name": "Web host", "paused": true }),
            ),
            (
                "remove_check",
                json!({ "type": "resource", "name": "Web host" }),
            ),
            (
                "run_check",
                json!({ "type": "resource", "name": "Web host" }),
            ),
            (
                "record_observation",
                json!({ "type": "resource", "name": "Web host", "result": "healthy", "summary": "Fine", "validForMinutes": 60 }),
            ),
            (
                "open_incident",
                json!({ "type": "resource", "name": "Web host", "title": "Down", "severity": "critical" }),
            ),
            (
                "update_incident",
                json!({ "incidentId": "any", "status": "resolved" }),
            ),
            (
                "record_event",
                json!({ "type": "resource", "name": "Web host", "kind": "note", "outcome": "info", "title": "Noted" }),
            ),
            ("backup", json!({})),
            ("read_copies", json!({})),
        ];
        assert_eq!(
            1 + reads.len() + writes.len(),
            tools().len(),
            "every tool is tried"
        );
        for (case, make, exists, stored, state, access, counted, refused) in cases {
            let path = nothing();
            let folder = path.parent().unwrap().to_owned();
            make(&path);
            let before = files(&folder);
            let mut agent = session(&path, "claude-code");

            let status = use_tool(&mut agent, "status", json!({})).unwrap();
            assert_eq!(status["exists"], exists, "{case}: {status:#}");
            assert_eq!(status["schemaVersion"], stored, "{case}: {status:#}");
            assert_eq!(
                (
                    status["schema"]["stored"].clone(),
                    status["schema"]["supported"].clone(),
                    status["schema"]["state"].as_str()
                ),
                (
                    stored.clone(),
                    json!(inventory::SCHEMA_VERSION),
                    Some(state)
                ),
                "{case}: {status:#}"
            );
            assert!(status["schema"]["meaning"].is_string(), "{case}");
            // Whether it has this schema's structure only once it was compared, and its
            // integrity never as checked.
            let structure = match state {
                "current" => json!("matches"),
                "mismatch" => json!("differs"),
                "outdated" | "newer" => json!("not_checked"),
                _ => Value::Null,
            };
            assert_eq!(
                (
                    status["schema"]["structure"].clone(),
                    status["schema"]["integrity"].clone()
                ),
                (
                    structure,
                    if stored.is_null() {
                        Value::Null
                    } else {
                        json!("not_checked")
                    }
                ),
                "{case}: {status:#}"
            );
            assert_eq!(status["access"], access, "{case}: {status:#}");
            assert_eq!(status["counts"], counted, "{case}: {status:#}");
            assert_eq!(
                status["thisWorkstation"].is_null(),
                counted.is_null(),
                "{case}: {status:#}"
            );

            for (tool, arguments) in &reads {
                match (refused, use_tool(&mut agent, tool, arguments.clone())) {
                    (None, Ok(_)) => {}
                    (Some(code), Err(text)) => {
                        assert!(
                            text.ends_with(&format!("({code})")),
                            "{case}, {tool}: {text}"
                        );
                        if !matches!(code, "access_denied" | "invalid_input") {
                            assert!(
                                text.contains("Nothing was read or changed."),
                                "{case}, {tool}: {text}"
                            );
                        }
                    }
                    (expected, outcome) => {
                        panic!("{case}, {tool}: expected {expected:?}, got {outcome:?}")
                    }
                }
            }
            for (tool, arguments) in &writes {
                let text = use_tool(&mut agent, tool, arguments.clone()).unwrap_err();
                let code = refused.unwrap_or("access_denied");
                assert!(
                    text.ends_with(&format!("({code})")),
                    "{case}, {tool}: {text}"
                );
            }
            assert_eq!(files(&folder), before, "{case}: the folder changed");
            std::fs::remove_dir_all(folder).unwrap();
        }

        // Without even its folder, nothing is created either.
        let base = path();
        let nowhere = base
            .parent()
            .unwrap()
            .join("nowhere")
            .join("inventory.sqlite3");
        let status = answer(&nowhere, "status", json!({}));
        assert_eq!(status["schema"]["state"], "missing");
        assert!(!nowhere.parent().unwrap().exists());
        remove(&base);
    }

    /// An inventory that a connection holds open, as a window does, is read beside it without
    /// touching any of its files, what that connection left in its log included. Once that
    /// connection ends without folding the log, the gate says the inventory needs recovering and
    /// goes on changing nothing, also for a call that access alone would refuse.
    #[test]
    fn reads_beside_an_open_connection_and_never_recovers_what_it_left() {
        use crate::inventory::tests::{kill, writer};
        let path = path();
        let folder = path.parent().unwrap().to_owned();
        seed(&path);
        let live_box = |systems: &Value| {
            systems["resources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|resource| resource["system"]["name"] == "Live box")
        };
        let mut agent = session(&path, "claude-code");
        let write = json!({ "name": "New", "kind": "server" });

        let holder = writer(&path, "log");
        let before = files(&folder);
        let names: Vec<_> = before.iter().map(|(name, _, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "inventory.sqlite3",
                "inventory.sqlite3-shm",
                "inventory.sqlite3-wal"
            ]
        );
        let status = use_tool(&mut agent, "status", json!({})).unwrap();
        assert_eq!(
            (
                status["schema"]["state"].as_str(),
                status["access"].as_str()
            ),
            (Some("current"), Some("read")),
            "{status:#}"
        );
        // «Live box» is only in the holder's log.
        let systems = use_tool(&mut agent, "list_systems", json!({})).unwrap();
        assert!(live_box(&systems), "{systems:#}");
        use_tool(&mut agent, "attention", json!({})).unwrap();
        let refused = use_tool(&mut agent, "save_resource", write.clone()).unwrap_err();
        assert!(refused.ends_with("(access_denied)"), "{refused}");
        assert_eq!(
            files(&folder),
            before,
            "reading beside the holder changed a file"
        );

        kill(holder, &path);
        let before = files(&folder);
        let status = use_tool(&mut agent, "status", json!({})).unwrap();
        assert_eq!(status["schema"]["state"], "needs_recovery", "{status:#}");
        assert_eq!(
            (
                &status["schemaVersion"],
                &status["access"],
                &status["counts"]
            ),
            (&Value::Null, &Value::Null, &Value::Null),
            "{status:#}"
        );
        for (tool, arguments) in [
            ("attention", json!({})),
            ("list_systems", json!({})),
            ("save_resource", write),
        ] {
            let refused = use_tool(&mut agent, tool, arguments).unwrap_err();
            assert!(refused.ends_with("(needs_recovery)"), "{tool}: {refused}");
        }
        assert_eq!(
            files(&folder),
            before,
            "the gate recovered what the holder left"
        );

        // DevHQ's window recovers it when it starts, and nothing committed was lost.
        inventory::start(&path).unwrap();
        let systems = use_tool(&mut agent, "list_systems", json!({})).unwrap();
        assert!(live_box(&systems), "{systems:#}");
        remove(&path);
    }

    /// A write is decided in its own transaction, not by the look before it: while `run_check`
    /// waits on the network (when the gate holds no connection), an inventory replaced by an older
    /// one is refused as it is, never upgraded or written; one that went is never created again;
    /// and a gate the person closed keeps neither the observation nor the entry saying who asked.
    #[test]
    fn a_write_never_lands_on_an_inventory_replaced_after_the_look() {
        use crate::inventory::tests::database_at;
        use std::io::{Read, Write as _};
        use std::net::TcpListener;
        for replacement in ["v11", "nothing", "closed"] {
            let path = path();
            seed(&path);
            crate::access::set_agent_access(&path, "write").unwrap();
            let old = path.with_file_name("older.sqlite3");
            drop(database_at(&old, 11));
            let older = std::fs::read(&old).unwrap();
            // The checked system answers only once the inventory was replaced.
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/health", listener.local_addr().unwrap());
            crate::http_check::save_http_check(
                &path,
                crate::http_check::HttpCheckInput {
                    subject: Subject::Resource {
                        id: inventory::list(&path)
                            .unwrap()
                            .resources
                            .into_iter()
                            .find(|resource| resource.name == "Web host")
                            .unwrap()
                            .id,
                    },
                    url,
                    expected_status: 200,
                    timeout_seconds: 10,
                    valid_for_minutes: 60,
                    monitor_interval_minutes: None,
                },
            )
            .unwrap();
            let inventory = path.clone();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request).unwrap();
                match replacement {
                    "closed" => drop(crate::access::set_agent_access(&inventory, "read").unwrap()),
                    _ => std::fs::remove_file(&inventory).unwrap(),
                }
                if replacement == "v11" {
                    std::fs::rename(&old, &inventory).unwrap();
                }
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .unwrap();
            });
            let mut agent = session(&path, "claude-code");
            let refused = use_tool(
                &mut agent,
                "run_check",
                json!({ "type": "resource", "name": "Web host" }),
            )
            .unwrap_err();
            server.join().unwrap();
            match replacement {
                "v11" => {
                    assert!(refused.ends_with("(schema_outdated)"), "{refused}");
                    assert_eq!(
                        std::fs::read(&path).unwrap(),
                        older,
                        "the older file changed"
                    );
                }
                "nothing" => {
                    assert!(
                        refused.contains("There is no DevHQ inventory at"),
                        "{refused}"
                    );
                    assert!(!path.exists(), "an inventory was created");
                }
                _ => {
                    assert!(refused.ends_with("(access_denied)"), "{refused}");
                    let kept = inventory::list(&path).unwrap();
                    assert!(
                        kept.observations.iter().all(|item| item.source != "http"),
                        "an observation was kept"
                    );
                    assert!(changes(&path).is_empty(), "the run was logged");
                }
            }
            remove(&path);
        }
    }

    /// The read tools, as `valid_calls` calls them.
    fn read_calls() -> Vec<(&'static str, Value)> {
        valid_calls()
            .into_iter()
            .filter(|(tool, _)| {
                [
                    "status",
                    "attention",
                    "list_systems",
                    "get_system",
                    "history",
                ]
                .contains(tool)
            })
            .collect()
    }

    /// Read only holds until the person changes it: every new session, with no DevHQ window open,
    /// reads all the gate offers, and nothing changes; Read and maintain reads the same.
    #[test]
    fn read_only_lasts_for_every_session_and_changes_nothing() {
        let path = path();
        seed(&path);
        let before = rows(&path);
        for (mode, sessions) in [("read", 3), ("maintain", 1)] {
            crate::access::set_agent_access(&path, mode).unwrap();
            let before = rows(&path);
            for _ in 0..sessions {
                let mut messages = vec![
                    json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
                    "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "claude-code", "version": "2" },
                } }),
                ];
                for (index, (tool, arguments)) in read_calls().into_iter().enumerate() {
                    messages.push(
                        json!({ "jsonrpc": "2.0", "id": index + 1, "method": "tools/call",
                                          "params": { "name": tool, "arguments": arguments } }),
                    );
                }
                let replies = exchange(&path, &messages);
                assert_eq!(replies.len(), 6);
                for reply in &replies[1..] {
                    assert_eq!(reply["result"]["isError"], false, "{mode}: {reply:#}");
                }
                let status = &replies[1]["result"]["structuredContent"];
                assert_eq!(
                    (&status["access"], &status["windowOpen"]),
                    (&json!(mode), &json!(false))
                );
                assert_eq!(rows(&path), before, "{mode}: reading changed a row");
            }
        }
        // Only the person's choices changed anything: no agent's.
        assert_eq!(
            rows(&path)
                .into_iter()
                .filter(|(table, _)| table != "agent_access")
                .collect::<Vec<_>>(),
            before
                .into_iter()
                .filter(|(table, _)| table != "agent_access")
                .collect::<Vec<_>>()
        );
        assert!(changes(&path).is_empty());
        remove(&path);
    }

    /// With Read and maintain, an agent adds and edits applications and resources with their
    /// components and references: declared inventory, logged as its own change, never evidence.
    /// Everything else that changes something is refused before anything is written, and Read only
    /// refuses the maintenance too.
    #[test]
    fn read_and_maintain_adds_and_edits_and_refuses_everything_else() {
        let path = path();
        seed(&path);
        crate::access::set_agent_access(&path, "maintain").unwrap();
        let mut agent = session(&path, "codex");
        assert_eq!(
            use_tool(&mut agent, "status", json!({})).unwrap()["access"],
            "maintain"
        );
        let vps = use_tool(
            &mut agent,
            "save_resource",
            json!({ "name": "VPS", "kind": "server", "operatingSystem": "Ubuntu 24.04" }),
        )
        .unwrap();
        let vps_id = vps["resource"]["id"].as_str().unwrap().to_owned();
        let spare = use_tool(
            &mut agent,
            "get_system",
            json!({ "type": "resource", "name": "Spare box" }),
        )
        .unwrap()["system"]["system"]["id"]
            .clone();
        let added = use_tool(
            &mut agent,
            "save_application",
            json!({ "name": "Finanzas", "lifecycle": "development", "version": "0.3.0",
                    "description": "Cuentas de casa,\nen dos líneas",
                    "components": [{ "name": "API", "kind": "backend", "runsOn": vps_id },
                                   { "name": "Base de datos", "kind": "database", "runsOn": vps_id }],
                    "references": [
                        { "kind": "repository", "target": "https://github.com/me/finanzas" },
                        { "kind": "local_copy", "target": r"C:\src\finanzas", "machine": spare },
                        { "kind": "documentation", "target": "https://github.com/me/finanzas#readme", "label": "README" },
                    ] }),
        )
        .unwrap();
        let finanzas = added["application"]["id"].as_str().unwrap().to_owned();
        assert_eq!(
            (
                &added["change"]["action"],
                &added["change"]["source"],
                &added["change"]["agent"]
            ),
            (&json!("created"), &json!("agent"), &json!("codex"))
        );
        // Declared, not observed: no record is made, and the system is a gap, never healthy.
        let listed = use_tool(&mut agent, "list_systems", json!({})).unwrap();
        let shown = listed["applications"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["system"]["id"] == finanzas.as_str())
            .unwrap()
            .clone();
        assert_eq!(shown["evidence"]["state"], "unobserved");
        assert_eq!(shown["latestChange"], added["change"]["id"]);
        let attention = use_tool(&mut agent, "attention", json!({})).unwrap();
        assert!(attention["neverObserved"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "Finanzas"));
        assert_ne!(attention["status"], "covered");
        let held = inventory::list(&path).unwrap();
        let mine = Subject::Application {
            id: finanzas.clone(),
        };
        assert!(held.observations.iter().all(|item| item.subject != mine));
        assert!(held.incidents.iter().all(|item| item.subject != mine));
        assert!(held.events.iter().all(|item| item.subject != mine));

        // An edit changes what it names and keeps everything else.
        let components = added["application"]["components"].clone();
        let references = added["application"]["references"].clone();
        let edited = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": finanzas, "basedOn": added["change"]["id"], "version": "0.4.0",
                    "components": [{ "id": components[0]["id"], "name": "API pública" }],
                    "references": [{ "kind": "dashboard", "target": "https://grafana.example/d/finanzas" }],
                    "removeReferences": [references[2]["id"]] }),
        )
        .unwrap();
        let application = &edited["application"];
        assert_eq!(application["version"], "0.4.0");
        assert_eq!(
            application["description"],
            "Cuentas de casa,\nen dos líneas"
        );
        assert_eq!(application["components"][0]["name"], "API pública");
        assert_eq!(application["components"][0]["resourceId"], vps_id.as_str());
        assert_eq!(application["components"][1], components[1]);
        assert_eq!(application["references"][0], references[0]);
        assert_eq!(application["references"][1], references[1]);
        assert_eq!(application["references"][2]["kind"], "dashboard");
        assert_eq!(application["references"].as_array().unwrap().len(), 3);
        let fields: Vec<&str> = edited["change"]["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|change| change["field"].as_str().unwrap())
            .collect();
        assert_eq!(fields, ["version", "components", "references"]);
        let described = use_tool(
            &mut agent,
            "save_resource",
            json!({ "id": vps_id, "basedOn": vps["change"]["id"], "description": "Hetzner CX22" }),
        )
        .unwrap();
        assert_eq!(described["resource"]["operatingSystem"], "Ubuntu 24.04");
        assert_eq!(described["change"]["agent"], "codex");

        // Every other tool that changes something is refused before it writes anything.
        let before = rows(&path);
        for (tool, arguments) in valid_calls() {
            if read_calls().iter().any(|(read, _)| *read == tool)
                || writes::MAINTAIN.contains(&tool)
            {
                continue;
            }
            let refused = use_tool(&mut agent, tool, arguments).unwrap_err();
            assert!(
                refused.contains("read and maintain the inventory")
                    && refused.ends_with("(access_denied)"),
                "{tool}: {refused}"
            );
        }
        assert_eq!(rows(&path), before);
        assert!(!path.parent().unwrap().join("backups").exists());
        // Each tool says which access it needs.
        for tool in tools() {
            let name = tool["name"].as_str().unwrap();
            let description = tool["description"].as_str().unwrap();
            if writes::MAINTAIN.contains(&name) {
                assert!(
                    description.starts_with("Needs Read and maintain."),
                    "{name}"
                );
            } else if tool["annotations"]["readOnlyHint"] == false {
                assert!(description.starts_with("Needs Read and write."), "{name}");
            }
        }

        // Read only refuses the maintenance, and says what the person could choose.
        crate::access::set_agent_access(&path, "read").unwrap();
        let before = rows(&path);
        let refused = use_tool(
            &mut agent,
            "save_application",
            json!({ "name": "Otra", "lifecycle": "development" }),
        )
        .unwrap_err();
        assert!(refused.contains("choose Read and maintain"), "{refused}");
        assert_eq!(rows(&path), before);
        remove(&path);
    }

    /// An agent never leaves two applications or resources with one name: the same call twice, the
    /// same name in other letters, or a rename onto another's name add nothing. An edit repeated
    /// after its answer was lost is refused too, since the first one changed what both were based
    /// on.
    #[test]
    fn a_repeated_call_adds_and_changes_nothing_twice() {
        let path = path();
        seed(&path);
        crate::access::set_agent_access(&path, "maintain").unwrap();
        let mut agent = session(&path, "claude-code");
        let call = json!({ "name": "Finanzas", "lifecycle": "development",
                           "components": [{ "name": "API", "kind": "backend" }] });
        let added = use_tool(&mut agent, "save_application", call.clone()).unwrap();
        let id = added["application"]["id"].as_str().unwrap().to_owned();
        let logged = changes(&path).len();
        let before = rows(&path);
        for (tool, arguments, names) in [
            ("save_application", call.clone(), "«Finanzas»"),
            (
                "save_application",
                json!({ "name": "  fINANZAS ", "lifecycle": "testing" }),
                "«Finanzas»",
            ),
            (
                "save_resource",
                json!({ "name": "SPARE BOX", "kind": "server" }),
                "«Spare box»",
            ),
            (
                "save_application",
                json!({ "id": id, "basedOn": added["change"]["id"], "name": "shop" }),
                "«Shop»",
            ),
            (
                "save_application",
                json!({ "id": id, "basedOn": added["change"]["id"],
                        "components": [{ "name": "api", "kind": "frontend" }] }),
                "already has a component named «API»",
            ),
        ] {
            let refused = use_tool(&mut agent, tool, arguments).unwrap_err();
            assert!(
                refused.contains(names) && refused.ends_with("(already_exists)"),
                "{refused}"
            );
        }
        assert_eq!(rows(&path), before);
        assert_eq!(changes(&path).len(), logged);
        // The refusal names the one that is there, so that the agent can edit it instead.
        let refused = use_tool(&mut agent, "save_application", call).unwrap_err();
        assert!(refused.contains(&format!("(id {id})")), "{refused}");

        let edit = json!({ "id": id, "basedOn": added["change"]["id"], "version": "1.0.0" });
        let edited = use_tool(&mut agent, "save_application", edit.clone()).unwrap();
        let after = rows(&path);
        let refused = use_tool(&mut agent, "save_application", edit).unwrap_err();
        assert!(refused.ends_with("(changed_since_read)"), "{refused}");
        assert!(
            refused.contains(&format!(
                "its latestChange is now {}",
                edited["change"]["id"].as_str().unwrap()
            )),
            "{refused}"
        );
        assert_eq!(rows(&path), after);

        // Keeping a name the window let two systems share is not refused.
        save_resource(
            &path,
            ResourceInput {
                id: None,
                name: "Spare box".into(),
                kind: "server".into(),
                description: String::new(),
                operating_system: String::new(),
                references: vec![],
            },
        )
        .unwrap();
        let twin = inventory::list(&path)
            .unwrap()
            .resources
            .into_iter()
            .find(|item| item.name == "Spare box" && item.kind == "server")
            .unwrap()
            .id;
        let read = based_on(&mut agent, "resource", &twin);
        use_tool(
            &mut agent,
            "save_resource",
            json!({ "id": twin, "basedOn": read, "operatingSystem": "Linux" }),
        )
        .unwrap();
        remove(&path);
    }

    /// An edit is checked against the state it was based on inside its own transaction, with
    /// SQLite's write lock held: none overwrites a change it never saw, the person's included, and
    /// of several sessions that read the same state and edit it at once, one does and every other is
    /// refused. Several adding the same application at once add one.
    #[test]
    fn edits_made_at_once_never_overwrite_each_other() {
        use crate::inventory::tests::{kill, writer};
        let path = path();
        let (shop, _) = seed(&path);
        crate::access::set_agent_access(&path, "maintain").unwrap();
        // A connection held open by a process of its own, as the window holds one, so that what
        // this measures is the transaction's check, and not the gate's look at a log that comes and
        // goes as connections open and close (see docs/puerta-para-agentes.md). In this process,
        // Unix would hide its lock from that look (see `log_held`).
        let window = writer(&path, "connection");
        let mut first = session(&path, "claude-code");
        let mut second = session(&path, "codex");
        let read = based_on(&mut first, "application", &shop);
        assert_eq!(based_on(&mut second, "application", &shop), read);
        let current = |path: &Path| {
            inventory::list(path)
                .unwrap()
                .applications
                .into_iter()
                .find(|item| item.id == shop)
                .unwrap()
        };
        // The person changes it in the window after both agents read it.
        let shown = current(&path);
        save_application(
            &path,
            ApplicationInput {
                id: Some(shop.clone()),
                name: shown.name.clone(),
                description: "Changed by the person".into(),
                lifecycle: shown.lifecycle.clone(),
                version: shown.version.clone(),
                components: shown
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
            },
        )
        .unwrap();
        let before = rows(&path);
        for agent in [&mut first, &mut second] {
            let refused = use_tool(
                agent,
                "save_application",
                json!({ "id": shop, "basedOn": read, "description": "Overwritten" }),
            )
            .unwrap_err();
            assert!(refused.ends_with("(changed_since_read)"), "{refused}");
        }
        assert_eq!(rows(&path), before);
        // Read again, the edit goes in on top of what the person did.
        let read = based_on(&mut first, "application", &shop);
        use_tool(
            &mut first,
            "save_application",
            json!({ "id": shop, "basedOn": read, "version": "2.5.0" }),
        )
        .unwrap();
        let now = current(&path);
        assert_eq!(
            (now.description.as_str(), now.version.as_deref()),
            ("Changed by the person", Some("2.5.0"))
        );

        // Eight sessions at once, all based on the same state, each adding a reference.
        let at_once = |arguments: &dyn Fn(usize) -> Value| edits_at_once(&path, 8, arguments);
        let read = based_on(&mut first, "application", &shop);
        let outcomes = at_once(&|number| {
            json!({ "id": shop, "basedOn": read,
                    "references": [{ "kind": "documentation", "target": format!("https://docs.example/{number}") }] })
        });
        let done = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
        assert_eq!(done, 1, "{outcomes:#?}");
        assert!(
            outcomes.iter().all(|outcome| outcome
                .as_ref()
                .err()
                .is_none_or(|refused| refused.ends_with("(changed_since_read)"))),
            "{outcomes:#?}"
        );
        assert_eq!(current(&path).references.len(), 1);

        let outcomes = at_once(&|_| json!({ "name": "Nómina", "lifecycle": "development" }));
        let done = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
        assert_eq!(done, 1, "{outcomes:#?}");
        assert!(
            outcomes.iter().all(|outcome| outcome
                .as_ref()
                .err()
                .is_none_or(|refused| refused.ends_with("(already_exists)"))),
            "{outcomes:#?}"
        );
        let named = inventory::list(&path)
            .unwrap()
            .applications
            .iter()
            .filter(|item| item.name == "Nómina")
            .count();
        assert_eq!(named, 1);
        kill(window, &path);
        remove(&path);
    }

    /// The environment variable that makes `edits_when_told` edit: the inventory's path, the
    /// agent's name, the arguments of its save_application, the file whose appearance starts the
    /// edit, the file its outcome goes to and the file to create once it is ready, one per line.
    const EDITOR: &str = "DEVHQ_TEST_GATE_EDITOR";

    /// What `count` agents' save_application with `arguments(number)` give when they run at once
    /// on the inventory at `path`: each in a process of its own, as each agent's gate is, all
    /// started together once every one is ready. Threads of one process would not do on Unix: the
    /// gate looks at the log's locks before it opens a connection (`log_held`), and there that look
    /// drops every lock its process holds on the log, the other sessions' included.
    fn edits_at_once(
        path: &Path,
        count: usize,
        arguments: &dyn Fn(usize) -> Value,
    ) -> Vec<Result<Value, String>> {
        let folder = path.parent().unwrap();
        let start = folder.join(format!("start-{}", uuid::Uuid::new_v4()));
        let editors: Vec<_> = (0..count)
            .map(|number| {
                let outcome = folder.join(format!("outcome-{}", uuid::Uuid::new_v4()));
                let spec = [
                    path.display().to_string(),
                    format!("agent-{number}"),
                    arguments(number).to_string(),
                    start.display().to_string(),
                    outcome.display().to_string(),
                ]
                .join("\n");
                let editor =
                    crate::inventory::tests::helper("gate::tests::edits_when_told", EDITOR, &spec);
                (editor, outcome)
            })
            .collect();
        std::fs::write(&start, "").unwrap();
        let outcomes = editors
            .into_iter()
            .map(|(editor, outcome)| {
                assert!(editor.finished(), "an editor failed");
                let written: Value =
                    serde_json::from_slice(&std::fs::read(&outcome).unwrap()).unwrap();
                std::fs::remove_file(outcome).unwrap();
                match written["refused"].as_str() {
                    Some(refused) => Err(refused.to_owned()),
                    None => Ok(written["saved"].clone()),
                }
            })
            .collect();
        std::fs::remove_file(start).unwrap();
        outcomes
    }

    /// Run by `edits_at_once` in a process of its own, as one agent's gate: a session that, once
    /// the start file appears, calls save_application and writes what it gave; on its own, it does
    /// nothing.
    #[test]
    #[ignore = "a helper that other tests run in a process of its own"]
    fn edits_when_told() {
        let Some(spec) = std::env::var_os(EDITOR) else {
            return;
        };
        let spec = spec.into_string().unwrap();
        let [path, agent, arguments, start, outcome, ready] =
            spec.split('\n').collect::<Vec<_>>()[..]
        else {
            panic!("{spec}")
        };
        let mut session = session(Path::new(path), agent);
        let arguments: Value = serde_json::from_str(arguments).unwrap();
        std::fs::write(ready, "").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !Path::new(start).exists() {
            assert!(std::time::Instant::now() < deadline, "never told to start");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let written = match use_tool(&mut session, "save_application", arguments) {
            Ok(saved) => json!({ "saved": saved }),
            Err(refused) => json!({ "refused": refused }),
        };
        std::fs::write(outcome, written.to_string()).unwrap();
    }

    /// Leaving a component or a reference out of an edit never removes it. Removing one takes
    /// naming it, and naming one the system does not have, another system's included, does
    /// nothing at all.
    #[test]
    fn leaving_an_item_out_never_removes_it() {
        let path = path();
        let (shop, host) = seed(&path);
        crate::access::set_agent_access(&path, "maintain").unwrap();
        let mut agent = session(&path, "claude-code");
        let added = use_tool(
            &mut agent,
            "save_application",
            json!({ "name": "Finanzas", "lifecycle": "development",
                    "components": [{ "name": "API", "kind": "backend", "runsOn": host }, { "name": "Web", "kind": "frontend" }],
                    "references": [{ "kind": "repository", "target": "https://github.com/me/finanzas" },
                                   { "kind": "documentation", "target": "https://docs.example/finanzas", "label": "Docs" }] }),
        )
        .unwrap();
        let id = added["application"]["id"].as_str().unwrap().to_owned();
        let component = |index: usize| added["application"]["components"][index]["id"].clone();
        let reference = |index: usize| added["application"]["references"][index]["id"].clone();
        let others = inventory::list(&path)
            .unwrap()
            .applications
            .into_iter()
            .find(|item| item.id == shop)
            .unwrap()
            .components[0]
            .id
            .clone();
        let names = |saved: &Value| -> Vec<String> {
            saved["application"]["components"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["name"].as_str().unwrap().to_owned())
                .collect()
        };
        // Given only a new component, an edit keeps both it had.
        let grown = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": id, "basedOn": added["change"]["id"],
                    "components": [{ "name": "Worker", "kind": "docker" }] }),
        )
        .unwrap();
        assert_eq!(names(&grown), ["API", "Web", "Worker"]);
        // Changing one field of one reference keeps the other reference and the other fields.
        let moved = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": id, "basedOn": grown["change"]["id"],
                    "references": [{ "id": reference(0), "target": "https://github.com/me/finanzas-v2" }] }),
        )
        .unwrap();
        let kept = &moved["application"]["references"];
        assert_eq!(
            (
                &kept[0]["id"],
                &kept[0]["target"],
                &kept[0]["kind"],
                &kept[0]["label"]
            ),
            (
                &reference(0),
                &json!("https://github.com/me/finanzas-v2"),
                &json!("repository"),
                &Value::Null
            )
        );
        assert_eq!(kept[1], added["application"]["references"][1]);

        let at = moved["change"]["id"].clone();
        let before = rows(&path);
        for (arguments, why) in [
            (
                json!({ "id": id, "basedOn": at, "removeComponents": ["ghost"] }),
                "«Finanzas» has no component with the id ghost".to_owned(),
            ),
            (
                json!({ "id": id, "basedOn": at, "removeComponents": [others] }),
                format!("«Finanzas» has no component with the id {others}"),
            ),
            (
                json!({ "id": id, "basedOn": at, "components": [{ "id": others, "name": "Taken" }] }),
                format!("«Finanzas» has no component with the id {others}"),
            ),
            (
                json!({ "id": id, "basedOn": at, "removeReferences": ["ghost"] }),
                "«Finanzas» has no reference with the id ghost".to_owned(),
            ),
            (
                json!({ "id": id, "basedOn": at, "removeComponents": [component(0)],
                        "components": [{ "id": component(0), "name": "Both" }] }),
                "change that component or remove it, not both".to_owned(),
            ),
        ] {
            let refused = use_tool(&mut agent, "save_application", arguments).unwrap_err();
            assert!(refused.contains(&why), "{refused}");
        }
        assert_eq!(rows(&path), before);

        // Named, it goes, and the change log says what it was. `runsOn: null` unlinks only the
        // component it names.
        let trimmed = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": id, "basedOn": at, "removeComponents": [component(1)],
                    "removeReferences": [reference(1)],
                    "components": [{ "id": component(0), "runsOn": null }] }),
        )
        .unwrap();
        assert_eq!(names(&trimmed), ["API", "Worker"]);
        assert_eq!(
            trimmed["application"]["components"][0]["resourceId"],
            Value::Null
        );
        assert_eq!(
            trimmed["application"]["references"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let fields: Vec<&str> = trimmed["change"]["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|change| change["field"].as_str().unwrap())
            .collect();
        assert_eq!(fields, ["components", "references"]);
        remove(&path);
    }

    /// An agent's mistaken addition is put right. The agent can edit it (a name, a lifecycle) but
    /// not delete it; the person deletes it in the window while it has no history, and once it has
    /// some it is kept and archived. Every step stays in the change log.
    #[test]
    fn a_mistaken_addition_is_corrected_archived_or_deleted_by_the_person() {
        let path = path();
        seed(&path);
        crate::access::set_agent_access(&path, "maintain").unwrap();
        let mut agent = session(&path, "claude-code");
        let mut add = |name: &str| {
            let added = use_tool(
                &mut agent,
                "save_application",
                json!({ "name": name, "lifecycle": "development" }),
            )
            .unwrap();
            (
                added["application"]["id"].as_str().unwrap().to_owned(),
                added["change"]["id"].clone(),
            )
        };
        let (typo, typo_read) = add("Finanzaz");
        let (wrong, _) = add("Prueba");
        let (observed, _) = add("Temporal");
        let refused =
            use_tool(&mut agent, "delete_application", json!({ "id": typo })).unwrap_err();
        assert!(refused.ends_with("(access_denied)"), "{refused}");
        let fixed = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": typo, "basedOn": typo_read, "name": "Finanzas" }),
        )
        .unwrap();
        assert_eq!(
            fixed["change"]["summary"],
            "Updated application «Finanzas»: name «Finanzaz» → «Finanzas»"
        );
        // Added altogether by mistake, and with no history: the person deletes it.
        inventory::delete_application(&path, &wrong).unwrap();
        // Once it has history it is kept: archived instead.
        record_observation(
            &path,
            ObservationInput {
                subject: Subject::Application {
                    id: observed.clone(),
                },
                result: "healthy".into(),
                summary: "Seen running".into(),
                observed_at: None,
                valid_for_minutes: 60,
            },
        )
        .unwrap();
        let kept = inventory::delete_application(&path, &observed).unwrap_err();
        assert!(
            kept.message.contains("DevHQ keeps history"),
            "{}",
            kept.message
        );
        let read = based_on(&mut agent, "application", &observed);
        use_tool(
            &mut agent,
            "save_application",
            json!({ "id": observed, "basedOn": read, "lifecycle": "archived" }),
        )
        .unwrap();
        let held = inventory::list(&path).unwrap();
        let names: Vec<&str> = held
            .applications
            .iter()
            .map(|item| item.name.as_str())
            .collect();
        // By name, as the inventory lists them.
        assert_eq!(names, ["Finanzas", "Shop", "Temporal"]);
        // The log, oldest first: who added, fixed, deleted and archived what.
        let mut steps: Vec<(String, String, String)> = held
            .changes
            .iter()
            .filter(|change| {
                matches!(&change.subject, Subject::Application { id } if [&typo, &wrong, &observed].contains(&id))
            })
            .map(|change| {
                (
                    change.subject_name.clone(),
                    change.action.clone(),
                    change.source.clone(),
                )
            })
            .collect();
        steps.reverse();
        let step = |name: &str, action: &str, source: &str| {
            (name.to_owned(), action.to_owned(), source.to_owned())
        };
        assert_eq!(
            steps,
            [
                step("Finanzaz", "created", "agent"),
                step("Prueba", "created", "agent"),
                step("Temporal", "created", "agent"),
                step("Finanzas", "updated", "agent"),
                step("Prueba", "deleted", "manual"),
                step("Temporal", "updated", "agent"),
            ]
        );
        remove(&path);
    }

    /// What a writer killed while it held the log leaves needs recovering: the gate refuses to
    /// read or maintain it, changing no file, and once the window's start recovers it, every change
    /// the agent made is there once, and the agent maintains it again.
    #[test]
    fn recovers_what_a_killed_writer_left_with_each_change_kept_once() {
        let path = path();
        seed(&path);
        crate::access::set_agent_access(&path, "maintain").unwrap();
        let mut agent = session(&path, "claude-code");
        use_tool(
            &mut agent,
            "save_application",
            json!({ "name": "Finanzas", "lifecycle": "development" }),
        )
        .unwrap();
        crate::inventory::tests::kill(crate::inventory::tests::writer(&path, "log"), &path);
        let folder = path.parent().unwrap().to_owned();
        let left = files(&folder);
        for (tool, arguments) in [
            ("list_systems", json!({})),
            (
                "save_application",
                json!({ "name": "Otra", "lifecycle": "development" }),
            ),
        ] {
            let refused = use_tool(&mut agent, tool, arguments).unwrap_err();
            assert!(refused.ends_with("(needs_recovery)"), "{tool}: {refused}");
        }
        assert_eq!(files(&folder), left);
        // The window's start recovers it.
        inventory::start(&path).unwrap();
        let held = inventory::list(&path).unwrap();
        let named = |name: &str| {
            held.applications
                .iter()
                .filter(|item| item.name == name)
                .count()
                + held
                    .resources
                    .iter()
                    .filter(|item| item.name == name)
                    .count()
        };
        assert_eq!(
            (named("Finanzas"), named("Live box"), named("Otra")),
            (1, 1, 0)
        );
        use_tool(
            &mut agent,
            "save_application",
            json!({ "name": "Otra", "lifecycle": "development" }),
        )
        .unwrap();
        remove(&path);
    }

    thread_local! {
        /// What a test does between the gate's read of the inventory and its read of each system's
        /// latest change (see `inventory_with_latest`): the instant in which another connection
        /// commits a change.
        static BETWEEN_READS: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
            const { std::cell::RefCell::new(None) };
    }

    pub(super) fn between_reads() {
        BETWEEN_READS.with(|hook| {
            if let Some(hook) = hook.borrow_mut().as_mut() {
                hook();
            }
        });
    }

    /// The one with `id` among the `components` of a system as get_system gives it.
    fn component_of<'a>(system: &'a Value, id: &str) -> &'a Value {
        system["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|component| component["id"] == id)
            .unwrap()
    }

    /// A relation never follows a name to another resource. Shop's API runs on «Host» (A). After
    /// the agent read Shop, another session renames A and gives its name to a new resource, B:
    /// neither change is Shop's, so Shop's latestChange, and the edit's `basedOn`, still hold. An
    /// edit that named the resource «Host» would move the API to B, which the agent never saw; so
    /// would a new component or a path named so, also in a new application. Each is refused and
    /// nothing changes. Named by A's id, as get_system gave it, the edit keeps the API on A, under
    /// its new name. A resource being saved can still be the machine of its own paths by its name.
    #[test]
    fn a_relation_never_follows_a_name_to_another_resource() {
        let path = path();
        inventory::start(&path).unwrap();
        crate::access::set_agent_access(&path, "maintain").unwrap();
        let mut agent = session(&path, "claude-code");
        let mut other = session(&path, "codex");
        let host = use_tool(
            &mut agent,
            "save_resource",
            json!({ "name": "Host", "kind": "server" }),
        )
        .unwrap()["resource"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let shop = use_tool(
            &mut agent,
            "save_application",
            json!({ "name": "Shop", "lifecycle": "production",
                    "components": [{ "name": "API", "kind": "backend", "runsOn": host }] }),
        )
        .unwrap()["application"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let read = use_tool(
            &mut agent,
            "get_system",
            json!({ "type": "application", "id": shop }),
        )
        .unwrap()["system"]
            .clone();
        let shop_read = read["latestChange"].clone();
        let api = read["components"][0]["id"].as_str().unwrap().to_owned();
        assert_eq!(
            read["components"][0]["runsOn"],
            json!({ "type": "resource", "id": host, "name": "Host" })
        );

        // Another session renames A, then gives its name to B.
        let host_read = based_on(&mut other, "resource", &host);
        use_tool(
            &mut other,
            "save_resource",
            json!({ "id": host, "basedOn": host_read, "name": "Old host" }),
        )
        .unwrap();
        let b = use_tool(
            &mut other,
            "save_resource",
            json!({ "name": "Host", "kind": "server" }),
        )
        .unwrap()["resource"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(based_on(&mut agent, "application", &shop), shop_read);

        let before = rows(&path);
        let logged = changes(&path).len();
        for (arguments, at) in [
            (
                json!({ "id": shop, "basedOn": shop_read, "components": [{ "id": api, "runsOn": "Host" }] }),
                "components[0].runsOn",
            ),
            (
                json!({ "id": shop, "basedOn": shop_read,
                        "components": [{ "name": "Worker", "kind": "backend", "runsOn": " host" }] }),
                "components[0].runsOn",
            ),
            (
                json!({ "id": shop, "basedOn": shop_read,
                        "references": [{ "kind": "repository", "target": "https://git.example/shop" },
                                       { "kind": "logs", "target": "/var/log/shop", "machine": "Host" }] }),
                "references[1].machine",
            ),
            (
                json!({ "name": "Blog", "lifecycle": "development",
                        "components": [{ "name": "Web", "kind": "frontend", "runsOn": "HOST" }] }),
                "components[0].runsOn",
            ),
        ] {
            let refused = use_tool(&mut agent, "save_application", arguments).unwrap_err();
            assert!(
                refused.starts_with(&format!("`{at}` is «"))
                    && refused.contains("a relation names a resource by its id")
                    && refused.ends_with("(invalid_input)"),
                "{refused}"
            );
            assert_eq!(rows(&path), before);
        }
        assert_eq!(changes(&path).len(), logged);

        // By its id the relation stays with A, whatever A is called now; B stays unused.
        let edited = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": shop, "basedOn": shop_read, "version": "2",
                    "components": [{ "id": api, "runsOn": host }] }),
        )
        .unwrap();
        assert_eq!(edited["application"]["version"], "2");
        let shown = use_tool(
            &mut agent,
            "get_system",
            json!({ "type": "application", "id": shop }),
        )
        .unwrap()["system"]
            .clone();
        assert_eq!(
            component_of(&shown, &api)["runsOn"],
            json!({ "type": "resource", "id": host, "name": "Old host" })
        );
        let unused = use_tool(
            &mut agent,
            "get_system",
            json!({ "type": "resource", "id": b }),
        )
        .unwrap()["system"]
            .clone();
        assert_eq!(unused["usedBy"], json!([]));

        // A resource being saved names itself as the machine of its own paths, by its name.
        let laptop = use_tool(
            &mut agent,
            "save_resource",
            json!({ "name": "Laptop", "kind": "workstation",
                    "references": [{ "kind": "local_copy", "target": "/home/me/notes", "machine": "laptop" }] }),
        )
        .unwrap()["resource"]
            .clone();
        assert_eq!(laptop["references"][0]["machineId"], laptop["id"]);
        remove(&path);
    }

    /// The names of an application's components are checked on the list as it would be saved,
    /// whatever the order of the call's items: no component the call adds or renames ends with the
    /// name of another, so renaming API to DB beside a DB is refused, and so is adding a component
    /// with the name another one is being renamed to. A rename that frees a name for another in the
    /// same call, a swap, or a rename into the name of a component the call removes, leaves no two
    /// with one name and is saved. Two components the person gave one name in the window stay as
    /// they are while an edit leaves their names alone, and no edit adds a third.
    #[test]
    fn component_names_are_checked_on_the_list_as_it_would_be_saved() {
        let path = path();
        let (shop, _) = seed(&path);
        crate::access::set_agent_access(&path, "maintain").unwrap();
        let mut agent = session(&path, "claude-code");
        let shown = use_tool(
            &mut agent,
            "get_system",
            json!({ "type": "application", "id": shop }),
        )
        .unwrap()["system"]
            .clone();
        let api = shown["components"][0]["id"].as_str().unwrap().to_owned();
        let added = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": shop, "basedOn": shown["latestChange"],
                    "components": [{ "name": "DB", "kind": "database" }] }),
        )
        .unwrap();
        let db = added["application"]["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|component| component["name"] == "DB")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let names = |agent: &mut Session| -> Vec<String> {
            let shown = use_tool(
                agent,
                "get_system",
                json!({ "type": "application", "id": shop }),
            )
            .unwrap()["system"]
                .clone();
            let mut names: Vec<String> = shown["components"]
                .as_array()
                .unwrap()
                .iter()
                .map(|component| component["name"].as_str().unwrap().to_owned())
                .collect();
            names.sort();
            names
        };

        let before = rows(&path);
        let based = based_on(&mut agent, "application", &shop);
        for (components, says) in [
            (
                json!({ "components": [{ "id": api, "name": "DB" }] }),
                format!("already has a component named «DB» (id {db}), so the component {api} cannot take that name"),
            ),
            (
                json!({ "components": [{ "id": api, "name": " db" }] }),
                format!("already has a component named «DB» (id {db}), so the component {api} cannot take that name"),
            ),
            (
                json!({ "components": [{ "name": "db", "kind": "database" }] }),
                format!("already has a component named «DB» (id {db}). Nothing was done: to change it, name it by its id."),
            ),
            (
                json!({ "components": [{ "name": "Cache", "kind": "database" }, { "id": api, "name": "cache" }] }),
                "This call would leave «Shop» with two components named «cache»".to_owned(),
            ),
            (
                json!({ "components": [{ "id": api, "name": "Cache" }, { "name": "Cache", "kind": "database" }] }),
                "This call would leave «Shop» with two components named «Cache»".to_owned(),
            ),
            (
                json!({ "components": [{ "name": "Queue", "kind": "backend" }, { "name": "QUEUE", "kind": "backend" }] }),
                "This call would leave «Shop» with two components named «Queue»".to_owned(),
            ),
        ] {
            let mut arguments = components.as_object().unwrap().clone();
            arguments.insert("id".into(), json!(shop));
            arguments.insert("basedOn".into(), based.clone());
            let refused =
                use_tool(&mut agent, "save_application", Value::Object(arguments)).unwrap_err();
            assert!(
                refused.contains(&says) && refused.ends_with("(already_exists)"),
                "{says}\n{refused}"
            );
            assert_eq!(rows(&path), before);
        }

        // What leaves no two with one name is saved, whatever the order of the items.
        let mut based = based;
        for (components, after) in [
            // A new API added before the old one is renamed, which frees its name.
            (
                json!({ "components": [{ "name": "API", "kind": "backend" }, { "id": api, "name": "Gateway" }] }),
                vec!["API", "DB", "Gateway"],
            ),
            // A swap.
            (
                json!({ "components": [{ "id": api, "name": "DB" }, { "id": db, "name": "Gateway" }] }),
                vec!["API", "DB", "Gateway"],
            ),
            // A rename into the name of a component the call removes.
            (
                json!({ "removeComponents": [db], "components": [{ "id": api, "name": "Gateway" }] }),
                vec!["API", "Gateway"],
            ),
        ] {
            let mut arguments = components.as_object().unwrap().clone();
            arguments.insert("id".into(), json!(shop));
            arguments.insert("basedOn".into(), based.clone());
            let saved = use_tool(&mut agent, "save_application", Value::Object(arguments)).unwrap();
            based = saved["change"]["id"].clone();
            assert_eq!(names(&mut agent), after);
        }

        // Two «Worker» components the person saved in the window, which allows it.
        let stored = inventory::list(&path).unwrap();
        let mine = stored
            .applications
            .iter()
            .find(|item| item.id == shop)
            .unwrap();
        let mut components: Vec<ComponentInput> = mine
            .components
            .iter()
            .map(|component| ComponentInput {
                id: Some(component.id.clone()),
                name: component.name.clone(),
                kind: component.kind.clone(),
                resource_id: component.resource_id.clone(),
            })
            .collect();
        for _ in 0..2 {
            components.push(ComponentInput {
                id: None,
                name: "Worker".into(),
                kind: "backend".into(),
                resource_id: None,
            });
        }
        save_application(
            &path,
            ApplicationInput {
                id: Some(shop.clone()),
                name: mine.name.clone(),
                description: mine.description.clone(),
                lifecycle: mine.lifecycle.clone(),
                version: mine.version.clone(),
                components,
                references: mine
                    .references
                    .iter()
                    .map(crate::references::ReferenceInput::from)
                    .collect(),
            },
        )
        .unwrap();
        let shown = use_tool(
            &mut agent,
            "get_system",
            json!({ "type": "application", "id": shop }),
        )
        .unwrap()["system"]
            .clone();
        let workers: Vec<String> = shown["components"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|component| component["name"] == "Worker")
            .map(|component| component["id"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(workers.len(), 2);
        let mut based = shown["latestChange"].clone();
        // Left alone, or changed without taking a new name, they stay as they are.
        for edit in [
            json!({ "version": "3" }),
            json!({ "components": [{ "id": workers[0], "kind": "docker" }, { "id": workers[1], "name": "worker" }] }),
        ] {
            let mut arguments = edit.as_object().unwrap().clone();
            arguments.insert("id".into(), json!(shop));
            arguments.insert("basedOn".into(), based.clone());
            let saved = use_tool(&mut agent, "save_application", Value::Object(arguments)).unwrap();
            based = saved["change"]["id"].clone();
        }
        assert_eq!(names(&mut agent), ["API", "Gateway", "Worker", "worker"]);
        // No edit adds a third, nor gives their name to another.
        let before = rows(&path);
        for edit in [
            json!({ "components": [{ "name": "WORKER", "kind": "backend" }] }),
            json!({ "components": [{ "id": api, "name": "Worker" }] }),
        ] {
            let mut arguments = edit.as_object().unwrap().clone();
            arguments.insert("id".into(), json!(shop));
            arguments.insert("basedOn".into(), based.clone());
            let refused =
                use_tool(&mut agent, "save_application", Value::Object(arguments)).unwrap_err();
            assert!(refused.ends_with("(already_exists)"), "{refused}");
            assert_eq!(rows(&path), before);
        }
        // Renaming one of them away from the shared name is fine: it leaves none repeated.
        let mut arguments = Map::new();
        arguments.insert("id".into(), json!(shop));
        arguments.insert("basedOn".into(), based);
        arguments.insert(
            "components".into(),
            json!([{ "id": workers[1], "name": "Scheduler" }]),
        );
        use_tool(&mut agent, "save_application", Value::Object(arguments)).unwrap();
        assert_eq!(names(&mut agent), ["API", "Gateway", "Scheduler", "Worker"]);
        remove(&path);
    }

    /// What an agent is shown and the latestChange its edit is based on are of one state of the
    /// inventory. With a connection holding the log, as an open window does, the gate reads through
    /// it; the person's change commits between the gate's read of Shop and its read of Shop's latest
    /// change. Read in one transaction, both are of the state before that change: the agent is
    /// shown the old version with the old latestChange, and an edit based on it is refused. Read in
    /// two, the agent would be shown the old version with the new latestChange, and its edit would
    /// overwrite a change it never saw.
    #[test]
    fn what_an_agent_is_shown_and_what_its_edit_is_based_on_are_one_state() {
        let path = path();
        let (shop, _) = seed(&path);
        crate::access::set_agent_access(&path, "maintain").unwrap();
        // The window is a process of its own, which holds the log and commits the person's change
        // when told. In this process neither would do: on Unix the gate's look at the log never sees
        // a lock this process holds (see `log_held`), and a write here, beside the gate's read-only
        // connection, would share the read-only view of the log's index that connection opened.
        let folder = path.parent().unwrap();
        let (go, changed) = (folder.join("go"), folder.join("changed"));
        let spec = [&path, &go, &changed].map(|file| file.display().to_string());
        let window = crate::inventory::tests::helper(
            "gate::tests::window_changes_when_told",
            WINDOW,
            &format!("{}\n{shop}\n{}\n{}", spec[0], spec[1], spec[2]),
        );
        let mut agent = session(&path, "claude-code");
        let before = based_on(&mut agent, "application", &shop);

        let mut done = false;
        BETWEEN_READS.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                if std::mem::replace(&mut done, true) {
                    return;
                }
                std::fs::write(&go, "").unwrap();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
                while !changed.exists() {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "the window never changed Shop"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }));
        });
        let shown = use_tool(
            &mut agent,
            "get_system",
            json!({ "type": "application", "id": shop }),
        );
        BETWEEN_READS.with(|hook| *hook.borrow_mut() = None);
        let shown = shown.unwrap()["system"].clone();
        assert_eq!(shown["application"]["version"], "2.4.1");
        assert_eq!(shown["latestChange"], before);

        let refused = use_tool(
            &mut agent,
            "save_application",
            json!({ "id": shop, "basedOn": shown["latestChange"], "description": "Shop front" }),
        )
        .unwrap_err();
        assert!(refused.ends_with("(changed_since_read)"), "{refused}");
        let now = use_tool(
            &mut agent,
            "get_system",
            json!({ "type": "application", "id": shop }),
        )
        .unwrap()["system"]
            .clone();
        assert_eq!(
            (
                &now["application"]["version"],
                &now["application"]["description"]
            ),
            (&json!("9.9.9"), &json!("Store front"))
        );
        crate::inventory::tests::kill(window, &path);
        remove(&path);
    }

    /// The environment variable that makes `window_changes_when_told` act: the inventory's path,
    /// the application's id, the file whose appearance makes it change the application, the file it
    /// creates once it has and the file to create once it is ready, one per line.
    const WINDOW: &str = "DEVHQ_TEST_GATE_WINDOW";

    /// Run by `what_an_agent_is_shown_and_what_its_edit_is_based_on_are_one_state` in a process of
    /// its own, as the window: holds a connection to the inventory open, and once told, saves the
    /// person's change of the application's version to 9.9.9, then holds the connection until it is
    /// killed; on its own, it does nothing.
    #[test]
    #[ignore = "a helper that other tests run in a process of its own"]
    fn window_changes_when_told() {
        let Some(spec) = std::env::var_os(WINDOW) else {
            return;
        };
        let spec = spec.into_string().unwrap();
        let [path, id, go, changed, ready] = spec.split('\n').collect::<Vec<_>>()[..] else {
            panic!("{spec}")
        };
        let path = Path::new(path);
        // A plain connection, as `writes_until_killed` holds one: its first read opens the log.
        let held = rusqlite::Connection::open(path).unwrap();
        held.query_row("SELECT count(*) FROM applications", [], |_| Ok(()))
            .unwrap();
        std::fs::write(ready, "").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !Path::new(go).exists() {
            assert!(std::time::Instant::now() < deadline, "never told to change");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let stored = inventory::list(path).unwrap();
        let shop = stored
            .applications
            .iter()
            .find(|item| item.id == id)
            .unwrap();
        save_application(
            path,
            ApplicationInput {
                id: Some(shop.id.clone()),
                name: shop.name.clone(),
                description: shop.description.clone(),
                lifecycle: shop.lifecycle.clone(),
                version: Some("9.9.9".into()),
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
                references: shop
                    .references
                    .iter()
                    .map(crate::references::ReferenceInput::from)
                    .collect(),
            },
        )
        .unwrap();
        std::fs::write(changed, "").unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    }
}
