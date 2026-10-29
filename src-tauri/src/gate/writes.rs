//! What an agent changes through the gate, as far as the person lets agents: adding and editing
//! applications and resources, reading local copies and recording an observation that vouches for
//! at most a week need Read and maintain (`maintain`), anything else Read and write (`write`).
//! Every tool goes through the window's own use cases, with their validation, limits and
//! transactions:
//! - Records (observations, incidents, events) carry the agent as their source and name.
//! - Configuration (applications, resources, components, HTTP checks) has no source, so the use
//!   case logs each change in the change log, as the agent's, in the same transaction, after the
//!   gate checks there, with SQLite's write lock held, that the person still lets agents do it.
//! - An agent never adds an application or resource with a name one already has, so that two
//!   sessions asked the same, or a call repeated after its answer was lost, add one.
//! - An edit says which state of the system it was made on (`basedOn`, the last entry of its change
//!   log): if anyone changed the system after that, the transaction refuses it, so that no edit
//!   overwrites what its agent never saw.
//! - Edits are partial: what an agent leaves out is kept, components and references included. A
//!   list it gives adds the items without id and changes the ones it names by id; nothing is ever
//!   removed by leaving it out, only by naming it (`removeComponents`, `removeReferences`).

use super::{pick, system_type, text, Session};
use crate::access;
use crate::backup;
use crate::change_log;
use crate::http_check::{self, HttpCheckInput};
use crate::inventory::{
    self, ApiError, ApplicationInput, Component, ComponentInput, ResourceInput,
};
use crate::local_copies::readings::{self, Which};
use crate::local_copies::Reader;
use crate::records::{
    self, Actor, EventInput, IncidentInput, IncidentStatusInput, Lifeline, ObservationInput,
    Subject,
};
use crate::references::{Reference, ReferenceInput};
use rusqlite::Transaction;
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

/// A whole number however JSON writes it (`30` or `30.0`), as the schema accepts it.
fn optional_integer(arguments: &Map<String, Value>, key: &str) -> Result<Option<i64>, ApiError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => super::arguments::whole(value).map(Some).ok_or_else(|| {
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

/// The resource a relation names (a component's `runsOn`, a path's `machine`, given at `path` in
/// the call), by its ID as list_systems and get_system give it: its ID. Never by its name, which can
/// change hands between the agent's read and its write: if the person or another session renames
/// that resource and gives its name to another, a link made by name would silently point at a
/// resource the agent never saw, and `basedOn` would not tell, since neither change is the
/// system's. An ID stays with its resource whatever it is called. `saving` is the resource being
/// saved (its ID and its name as it will be), which can be the machine of its own paths even before
/// it exists, by its ID or by that name: within the transaction that saves it, that name is its
/// own. An ID no resource has is passed on, and the use case refuses it with its own message.
fn resource_named(
    tx: &Transaction,
    named: &str,
    saving: Option<(&str, &str)>,
    path: &str,
) -> Result<String, ApiError> {
    let resources = candidates(tx, "resource")?;
    if saving.is_some_and(|(id, _)| id == named) || resources.iter().any(|(id, _)| id == named) {
        return Ok(named.to_owned());
    }
    if let Some((id, _)) = saving.filter(|(_, name)| same_name(name, named)) {
        return Ok(id.to_owned());
    }
    if resources.iter().any(|(_, name)| same_name(name, named)) {
        return Err(ApiError::invalid(format!(
            "`{path}` is «{named}», a resource's name: a relation names a resource by its id, as list_systems and get_system give it, so that a resource renamed or added since it was read is never linked in its place. Nothing was done."
        )));
    }
    Ok(named.to_owned())
}

/// The objects of the list `key`, none when it is left out.
fn items<'a>(
    arguments: &'a Map<String, Value>,
    key: &str,
) -> Result<Vec<&'a Map<String, Value>>, ApiError> {
    match arguments.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_object().ok_or_else(|| {
                    ApiError::invalid(format!("Each item of {key} must be an object."))
                })
            })
            .collect(),
        Some(_) => Err(ApiError::invalid(format!("{key} must be a list."))),
    }
}

/// The ids the list `key` names, none when it is left out.
fn ids(arguments: &Map<String, Value>, key: &str) -> Vec<String> {
    arguments
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The complete reference list after an agent's call, read inside the transaction: the stored
/// references (`kept`; none for an addition), each changed where an item of `references` names it
/// by `id`, keeping the fields the item leaves out (`label: null` and `machine: null` clear theirs),
/// without the ones `removeReferences` names, and then the new ones, the items without `id`, in
/// their order. A reference the call does not name stays as it is: leaving it out never removes
/// it. Each `machine` is a resource's ID (see `resource_named`); `saving` is the resource being
/// saved, which can be the machine of its own paths. `owner` names the system in a refusal.
fn references_argument(
    tx: &Transaction,
    arguments: &Map<String, Value>,
    kept: &[Reference],
    owner: &str,
    saving: Option<(&str, &str)>,
) -> Result<Vec<ReferenceInput>, ApiError> {
    let removed = ids(arguments, "removeReferences");
    if let Some(unknown) = removed
        .iter()
        .find(|id| !kept.iter().any(|reference| &reference.id == *id))
    {
        return Err(ApiError::invalid(format!(
            "{owner} has no reference with the id {unknown}; get_system lists them. Nothing was done."
        )));
    }
    let mut list: Vec<ReferenceInput> = kept
        .iter()
        .filter(|reference| !removed.contains(&reference.id))
        .map(ReferenceInput::from)
        .collect();
    for (index, item) in items(arguments, "references")?.into_iter().enumerate() {
        let machine = |named: &str| {
            resource_named(tx, named, saving, &format!("references[{index}].machine"))
        };
        match optional_text(item, "id")? {
            Some(id) => {
                let reference = list
                    .iter_mut()
                    .find(|reference| reference.id.as_deref() == Some(id))
                    .ok_or_else(|| {
                        ApiError::invalid(format!(
                            "{owner} has no reference with the id {id}; get_system lists them, and a new reference takes no id. Nothing was done."
                        ))
                    })?;
                if let Some(kind) = optional_text(item, "kind")? {
                    kind.clone_into(&mut reference.kind);
                }
                if let Some(target) = optional_text(item, "target")? {
                    target.clone_into(&mut reference.target);
                }
                match field(item, "label")? {
                    Field::Keep => {}
                    Field::Clear => reference.label = None,
                    Field::Set(label) => reference.label = Some(label.to_owned()),
                }
                match field(item, "machine")? {
                    Field::Keep => {}
                    Field::Clear => reference.machine_id = None,
                    Field::Set(named) => reference.machine_id = Some(machine(named)?),
                }
            }
            None => list.push(ReferenceInput {
                id: None,
                kind: text(item, "kind")?.to_owned(),
                target: text(item, "target")?.to_owned(),
                label: optional_text(item, "label")?.map(str::to_owned),
                machine_id: optional_text(item, "machine")?.map(machine).transpose()?,
            }),
        }
    }
    Ok(list)
}

/// The complete component list after an agent's call, built as `references_argument` builds the
/// reference list: the stored components (`kept`), each changed where an item names it by `id`
/// (`runsOn`, a resource's ID, see `resource_named`; null unlinks it), without the ones
/// `removeComponents` names, then the new ones. Then the names of the list as it would be saved,
/// whatever the order of the call's items: a component the call adds or renames may not share its
/// name with any other, since naming that one by its id changes it, so that a call repeated after
/// its answer was lost adds nothing twice. Two components the application already had with one
/// name (the window allows it) are kept as they are while the call gives neither of them that name.
fn components_argument(
    tx: &Transaction,
    arguments: &Map<String, Value>,
    kept: &[Component],
    owner: &str,
) -> Result<Vec<ComponentInput>, ApiError> {
    let removed = ids(arguments, "removeComponents");
    if let Some(unknown) = removed
        .iter()
        .find(|id| !kept.iter().any(|component| &component.id == *id))
    {
        return Err(ApiError::invalid(format!(
            "{owner} has no component with the id {unknown}; get_system lists them. Nothing was done."
        )));
    }
    let mut list: Vec<ComponentInput> = kept
        .iter()
        .filter(|component| !removed.contains(&component.id))
        .map(|component| ComponentInput {
            id: Some(component.id.clone()),
            name: component.name.clone(),
            kind: component.kind.clone(),
            resource_id: component.resource_id.clone(),
        })
        .collect();
    // Which of `list` the call names anew: each component it adds, and each it renames.
    let mut named_anew = vec![false; list.len()];
    for (index, item) in items(arguments, "components")?.into_iter().enumerate() {
        let runs_on =
            |named: &str| resource_named(tx, named, None, &format!("components[{index}].runsOn"));
        match optional_text(item, "id")? {
            Some(id) => {
                let at = list
                    .iter()
                    .position(|component| component.id.as_deref() == Some(id))
                    .ok_or_else(|| {
                        ApiError::invalid(format!(
                            "{owner} has no component with the id {id}; get_system lists them, and a new component takes no id. Nothing was done."
                        ))
                    })?;
                let component = &mut list[at];
                if let Some(name) = optional_text(item, "name")? {
                    if !same_name(&component.name, name) {
                        named_anew[at] = true;
                    }
                    name.clone_into(&mut component.name);
                }
                if let Some(kind) = optional_text(item, "kind")? {
                    kind.clone_into(&mut component.kind);
                }
                match field(item, "runsOn")? {
                    Field::Keep => {}
                    Field::Clear => component.resource_id = None,
                    Field::Set(named) => component.resource_id = Some(runs_on(named)?),
                }
            }
            None => {
                list.push(ComponentInput {
                    id: None,
                    name: text(item, "name")?.to_owned(),
                    kind: text(item, "kind")?.to_owned(),
                    resource_id: optional_text(item, "runsOn")?.map(runs_on).transpose()?,
                });
                named_anew.push(true);
            }
        }
    }
    for (at, component) in list.iter().enumerate() {
        if !named_anew[at] {
            continue;
        }
        let Some((other, same)) = list
            .iter()
            .enumerate()
            .find(|&(other, item)| other != at && same_name(&item.name, &component.name))
        else {
            continue;
        };
        return Err(ApiError::exists(
            match (&component.id, named_anew[other], &same.id) {
                (None, false, Some(kept)) => format!(
                    "{owner} already has a component named «{}» (id {kept}). Nothing was done: to change it, name it by its id.",
                    same.name
                ),
                (Some(renamed), false, Some(kept)) => format!(
                    "{owner} already has a component named «{}» (id {kept}), so the component {renamed} cannot take that name. Nothing was done.",
                    same.name
                ),
                _ => format!(
                    "This call would leave {owner} with two components named «{}». Nothing was done: give each component a name no other has.",
                    component.name
                ),
            },
        ));
    }
    Ok(list)
}

/// Whether two names are the same name, as the gate finds a system by its name: ignoring case and
/// the spaces around it.
fn same_name(one: &str, other: &str) -> bool {
    one.trim().to_lowercase() == other.trim().to_lowercase()
}

/// That no other application (or resource, by `kind`) has `name`: an agent never adds a second
/// one, nor renames one to another's name, so that two sessions asked the same, or a call repeated
/// after its answer was lost, never leave two. Read inside the transaction, with SQLite's write lock
/// held, so that two calls at once are told apart. `current` is the system being edited, with the
/// name it has: keeping a name it already shares with another, which the window allows, is fine.
fn unique_name(
    tx: &Transaction,
    kind: &str,
    current: Option<(&str, &str)>,
    name: &str,
) -> Result<(), ApiError> {
    if current.is_some_and(|(_, stored)| same_name(stored, name)) {
        return Ok(());
    }
    let others = candidates(tx, kind)?;
    let taken = others.iter().find(|(id, other)| {
        current.is_none_or(|(current, _)| current != id) && same_name(other, name)
    });
    match taken {
        Some((id, other)) => Err(ApiError::exists(format!(
            "{} named «{other}» is already in the inventory (id {id}). Nothing was {}: to change that one, edit it by its id. For agents, DevHQ keeps one {kind} per name.",
            if kind == "application" { "An application" } else { "A resource" },
            if current.is_none() { "added" } else { "renamed" },
        ))),
        None => Ok(()),
    }
}

/// That the system an edit names is as the agent read it: inside the transaction, with SQLite's
/// write lock held, the last entry of its change log (see `change_log::latest_of`) is the one the
/// edit is based on (`basedOn`; null when the log had none). Otherwise someone changed it after the
/// agent read it, the person or another session, and the edit, made without seeing that change,
/// is refused before anything is written.
fn as_read(
    tx: &Transaction,
    subject: &Subject,
    name: &str,
    arguments: &Map<String, Value>,
) -> Result<(), ApiError> {
    let based_on = arguments.get("basedOn").and_then(Value::as_str);
    let latest = change_log::latest_of(tx, subject)?;
    if latest.as_deref() == based_on {
        return Ok(());
    }
    let shown = |id: Option<&str>| id.map_or_else(|| "null".to_owned(), str::to_owned);
    Err(ApiError::changed(format!(
        "«{name}» changed after the state this edit is based on (basedOn {}; its latestChange is now {}). Nothing was done: read it again with get_system, and base the edit on what it holds now.",
        shown(based_on),
        shown(latest.as_deref())
    )))
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
    /// person lets agents do what it `need`s (`maintain` or `write`, see `access`): asked first
    /// through a look that cannot change any file, so that a change refused for access opens nothing
    /// that can write, and asked again inside the transaction, under SQLite's write lock, which
    /// decides, so that changing the access in Settings can never race a change. The inventory is
    /// opened as it is: never created, upgraded or converted to another journal mode, and refused
    /// unless it is in this build's schema, which is checked again in that transaction, so that a
    /// file replaced after the look is not written.
    fn change(
        &self,
        need: &str,
        apply: impl FnOnce(&Transaction, &Actor) -> Result<Value, ApiError>,
    ) -> Result<Value, ApiError> {
        let actor = self.actor()?;
        self.allow(need)?;
        let mut db = inventory::open_to_write(&self.database)?;
        let tx = inventory::begin_write_for(&mut db, &actor, need)?;
        let value = apply(&tx, &actor)?;
        tx.commit().map_err(ApiError::database)?;
        Ok(value)
    }

    pub(super) fn save_application(
        &self,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ApiError> {
        self.change("maintain", |tx, agent| {
            let existing = match optional_text(arguments, "id")? {
                Some(id) => Some(inventory::application_in(tx, id)?.ok_or_else(|| {
                    ApiError::invalid(format!(
                        "No application has the id {id}. list_systems lists them; leave id out to add one."
                    ))
                })?),
                None => None,
            };
            if let Some(item) = &existing {
                let subject = Subject::Application {
                    id: item.id.clone(),
                };
                as_read(tx, &subject, &item.name, arguments)?;
            }
            let name = match optional_text(arguments, "name")? {
                Some(name) => name.to_owned(),
                None => existing
                    .as_ref()
                    .map(|item| item.name.clone())
                    .ok_or_else(|| ApiError::invalid("name is required to add an application."))?,
            };
            unique_name(
                tx,
                "application",
                existing
                    .as_ref()
                    .map(|item| (item.id.as_str(), item.name.as_str())),
                &name,
            )?;
            let owner = existing
                .as_ref()
                .map_or_else(|| format!("«{}»", name.trim()), |item| format!("«{}»", item.name));
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
            let components = components_argument(
                tx,
                arguments,
                existing.as_ref().map_or(&[][..], |item| &item.components),
                &owner,
            )?;
            let references = references_argument(
                tx,
                arguments,
                existing.as_ref().map_or(&[][..], |item| &item.references),
                &owner,
                None,
            )?;
            let (saved, change) = inventory::save_application_in(
                tx,
                ApplicationInput {
                    id: existing.as_ref().map(|item| item.id.clone()),
                    name,
                    description,
                    lifecycle,
                    version,
                    components,
                    references,
                },
                agent,
            )?;
            Ok(json!({ "application": saved, "change": change }))
        })
    }

    pub(super) fn save_resource(&self, arguments: &Map<String, Value>) -> Result<Value, ApiError> {
        self.change("maintain", |tx, agent| {
            let existing = match optional_text(arguments, "id")? {
                Some(id) => Some(inventory::resource_in(tx, id)?.ok_or_else(|| {
                    ApiError::invalid(format!(
                        "No resource has the id {id}. list_systems lists them; leave id out to add one."
                    ))
                })?),
                None => None,
            };
            if let Some(item) = &existing {
                let subject = Subject::Resource {
                    id: item.id.clone(),
                };
                as_read(tx, &subject, &item.name, arguments)?;
            }
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
            // A new resource gets its ID here, so that its own paths can name it as their machine.
            let id = existing
                .as_ref()
                .map_or_else(|| uuid::Uuid::new_v4().to_string(), |item| item.id.clone());
            let name = keep(
                "name",
                existing.as_ref().map(|item| item.name.clone()),
                Some("name is required to add a resource."),
            )?;
            unique_name(
                tx,
                "resource",
                existing
                    .as_ref()
                    .map(|item| (item.id.as_str(), item.name.as_str())),
                &name,
            )?;
            let owner = existing
                .as_ref()
                .map_or_else(|| format!("«{}»", name.trim()), |item| format!("«{}»", item.name));
            let references = references_argument(
                tx,
                arguments,
                existing.as_ref().map_or(&[][..], |item| &item.references),
                &owner,
                Some((&id, &name)),
            )?;
            let (saved, change) = inventory::save_resource_in(
                tx,
                ResourceInput {
                    id: Some(id.clone()),
                    name,
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
                    references,
                },
                agent,
            )?;
            Ok(json!({ "resource": saved, "change": change }))
        })
    }

    pub(super) fn delete(
        &self,
        kind: &'static str,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ApiError> {
        self.change("write", |tx, agent| {
            let (subject, _) = subject_in(tx, Some(kind), arguments)?;
            let id = match &subject {
                Subject::Application { id } | Subject::Resource { id } => id.clone(),
            };
            let (name, change) = if kind == "application" {
                inventory::delete_application_in(tx, &id, agent)?
            } else {
                inventory::delete_resource_in(tx, &id, agent)?
            };
            Ok(json!({ "deleted": { "type": kind, "id": id, "name": name }, "change": change }))
        })
    }

    pub(super) fn save_check(&self, arguments: &Map<String, Value>) -> Result<Value, ApiError> {
        self.change("write", |tx, agent| {
            let (subject, _) = subject_in(tx, None, arguments)?;
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
            let (saved, change) = http_check::save_http_check_in(
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
                agent,
            )?;
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
        self.change("write", |tx, agent| {
            let (subject, _) = subject_in(tx, None, arguments)?;
            let (saved, change) =
                http_check::set_http_check_paused_in(tx, &subject, paused, agent)?;
            Ok(json!({ "httpCheck": saved, "change": change }))
        })
    }

    pub(super) fn remove_check(&self, arguments: &Map<String, Value>) -> Result<Value, ApiError> {
        self.change("write", |tx, agent| {
            let (subject, _) = subject_in(tx, None, arguments)?;
            let change = http_check::remove_http_check_in(tx, &subject, agent)?;
            Ok(json!({ "removed": true, "change": change }))
        })
    }

    /// Runs the check now, as Run check does, and records its observation as DevHQ's own
    /// evidence (`source` http) and, in the same transaction, the log entry that says which agent
    /// asked for it: both, only while the person lets agents write, or neither.
    pub(super) fn run_check(&self, arguments: &Map<String, Value>) -> Result<Value, ApiError> {
        let inventory = self.inventory()?;
        let subject = super::find_system(&inventory, arguments)?;
        let agent = self.actor()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(ApiError::check)?;
        let (observation, change) = runtime.block_on(http_check::run_http_check_by(
            &self.database,
            subject,
            &Lifeline::default(),
            Some(&agent),
        ))?;
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
        // Asked first through the look `allow` made, so that a validity the access does not allow
        // opens nothing that can write; the transaction asks again, and that decides.
        access::observation_validity(&self.read(|db| access::mode(db))?, valid_for_minutes)?;
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

    /// Run once `allow` found the inventory current and open for writing, which the backup asks
    /// again on the connection that copies it, just before the copy.
    pub(super) fn backup(&self) -> Result<Value, ApiError> {
        Ok(json!({ "backup": backup::back_up_by(&self.database, &self.actor()?)? }))
    }

    /// Reads local copies of this workstation in confined readers, as Read does in the window:
    /// one reference, the copies of one system, or all of them. Each reading is DevHQ's own
    /// evidence and keeps the agent that asked; it changes no configuration, so the change log gets
    /// nothing. Each is kept only while the person still lets agents write, checked inside its
    /// transaction with SQLite's write lock held.
    pub(super) fn read_copies(&self, arguments: &Map<String, Value>) -> Result<Value, ApiError> {
        // The rules already refused a reference with a system, and a system without its type.
        let which = if let Some(reference) = optional_text(arguments, "referenceId")? {
            Which::References(vec![reference.to_owned()])
        } else if arguments.contains_key("type") {
            Which::System(super::find_system(&self.inventory()?, arguments)?)
        } else {
            Which::ThisWorkstation
        };
        let reader = match &self.reader {
            Some(reader) => reader.clone(),
            None => Reader::this_executable()?,
        };
        let outcome = readings::read_copies(
            &self.database,
            &self.computer,
            &reader,
            &which,
            &self.actor()?,
        )?;
        Ok(json!(outcome))
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

/// Where a system is, as save_application and save_resource take it.
fn references_schema() -> Value {
    json!({
        "type": "array",
        "description": "Where the system is: inventory the agent declares, which DevHQ never opens or checks. An item without id adds a reference (kind and target required); an item with an existing reference's id changes it, keeping the fields the item leaves out. In an edit, references not named are kept: leaving one out never removes it (removeReferences does). The same kind, target and machine cannot appear twice.",
        "items": { "type": "object", "properties": {
            "id": { "type": "string", "description": "An existing reference's id (from get_system), to change it. Leave it out for a new reference." },
            "kind": { "type": "string", "enum": ["repository", "local_copy", "documentation", "logs", "dashboard"], "description": "Required for a new reference. repository and dashboard take an http(s) URL; local_copy an absolute path on a workstation; documentation and logs either." },
            "target": { "type": "string", "description": "Required for a new reference. An http(s) URL without user name or password, or an absolute path (C:\\…, \\\\server\\share\\… or /…), as it should be shown and copied. A repository is the code's canonical web address, never a local remote or an SSH URL." },
            "label": { "type": ["string", "null"], "description": "An optional name for people that tells two references of one kind apart. Never a locator. null clears it." },
            "machine": { "type": ["string", "null"], "description": "For a path, required: the id of the resource it is on, as list_systems or get_system gives it, never its name (in save_resource, the resource being saved can also be named by its own name). For a URL, leave it out, or null to clear it." },
        }, "required": [], "additionalProperties": false },
    })
}

/// What an edit is based on, as save_application and save_resource take it.
fn based_on_schema() -> Value {
    json!({
        "type": ["string", "null"],
        "description": "Required to edit, with id: the system's latestChange as list_systems or get_system gave it (null when they gave null). If the system changed after that, by the person or another session, nothing is done (changed_since_read): read it again and edit what is there now.",
    })
}

/// The ids an edit removes from one of the system's lists.
fn removals_schema(what: &str) -> Value {
    json!({
        "type": "array",
        "items": { "type": "string" },
        "description": format!("Only in an edit: the ids of the {what} to remove. Nothing is removed by leaving it out."),
    })
}

fn named(kind: &str) -> Value {
    json!({
        "id": { "type": "string", "description": format!("The {kind}'s id, from list_systems. Give id, name, or both.") },
        "name": { "type": "string", "description": format!("Or its exact name, ignoring case. Given with id, it must be that {kind}'s name, or nothing is done.") },
    })
}

/// The tools Read and maintain allows: adding and editing applications and resources, reading
/// local copies (DevHQ's own evidence, which changes no configuration and never leaves this
/// computer) and recording what the agent observed, for at most a week. Every other tool that
/// changes something needs Read and write.
pub(super) const MAINTAIN: [&str; 4] = [
    "save_application",
    "save_resource",
    "record_observation",
    "read_copies",
];

/// The tools that change something, each description starting with the access it needs. Saving
/// and deleting overwrite or remove configuration, so they are marked destructive; adding a record
/// never changes another.
pub(super) fn tools() -> Vec<Value> {
    let mut tools = changing_tools();
    for tool in &mut tools {
        let need = if MAINTAIN.contains(&tool["name"].as_str().unwrap_or_default()) {
            "Read and maintain"
        } else {
            "Read and write"
        };
        let described = format!(
            "Needs {need}. {}",
            tool["description"].as_str().unwrap_or_default()
        );
        tool["description"] = json!(described);
    }
    tools
}

fn changing_tools() -> Vec<Value> {
    let additive = json!({ "destructiveHint": false, "idempotentHint": false });
    let overwriting = json!({ "destructiveHint": true, "idempotentHint": false });
    vec![
        tool(
            "save_application",
            "Add or edit an application",
            "Adds an application (name and lifecycle required), or edits the one with the given id. Adding is refused when an application already has that name, ignoring case, so that a repeated call never adds two; a new application's components and references take no id. An edit needs `basedOn` (its latestChange from list_systems or get_system) and is refused if the application changed since. An edit keeps everything it leaves out: `components` adds the items without id (name and kind required; one with the name of a kept component is refused) and changes the ones it names by id, keeping the fields an item leaves out; `runsOn` is a resource's id (from list_systems or get_system), never its name, and null unlinks it. Components it does not name are kept; `removeComponents` removes the ones it names. A component it adds or renames may not end with the name of another one of the application, ignoring case, whatever the order of the items. `references` and `removeReferences` work the same way (see the item's description). `version: null` clears the version. No text may hold control characters or invisible formatting ones (line breaks only in description). What it saves is declared inventory, never evidence, logged as the agent's change with each field before and after.",
            json!({
                "id": { "type": "string", "description": "The application to edit; leave out to add one." },
                "basedOn": based_on_schema(),
                "name": { "type": "string" },
                "lifecycle": { "type": "string", "enum": ["development", "testing", "production", "archived"] },
                "description": { "type": "string" },
                "version": { "type": ["string", "null"] },
                "components": { "type": "array", "items": { "type": "object", "properties": {
                    "id": { "type": "string", "description": "An existing component's id (from get_system), to change it. Leave it out for a new component." },
                    "name": { "type": "string", "description": "Required for a new component." },
                    "kind": { "type": "string", "enum": ["backend", "database", "docker", "frontend"], "description": "Required for a new component." },
                    "runsOn": { "type": ["string", "null"], "description": "A resource's id, as list_systems or get_system gives it, never its name; null unlinks the component." },
                }, "required": [], "additionalProperties": false } },
                "removeComponents": removals_schema("components"),
                "references": references_schema(),
                "removeReferences": removals_schema("references"),
            }),
            &[],
            overwriting.clone(),
        ),
        tool(
            "save_resource",
            "Add or edit a resource",
            "Adds a resource (name and kind required), or edits the one with the given id, keeping what the edit leaves out. Adding is refused when a resource already has that name, ignoring case; a new resource's references take no id. An edit needs `basedOn` (its latestChange from list_systems or get_system) and is refused if the resource changed since. `references` adds and changes what it names and keeps the rest; `removeReferences` removes the ones it names (see the item's description). A path's `machine` is a resource's id, never its name; the resource being saved can be the machine of its own paths, named by its id or by its own name. One that holds other systems' local copies cannot stop being a workstation. No text may hold control characters or invisible formatting ones (line breaks only in description). What it saves is declared inventory, never evidence, logged as the agent's change with each field before and after.",
            json!({
                "id": { "type": "string", "description": "The resource to edit; leave out to add one." },
                "basedOn": based_on_schema(),
                "name": { "type": "string" },
                "kind": { "type": "string", "enum": ["server", "workstation"] },
                "description": { "type": "string" },
                "operatingSystem": { "type": "string" },
                "references": references_schema(),
                "removeReferences": removals_schema("references"),
            }),
            &[],
            overwriting.clone(),
        ),
        tool(
            "delete_application",
            "Delete an application without history",
            "Deletes an application that has no observations, incidents or events, with its components, references and HTTP check. One with history is refused: DevHQ never erases history; set its lifecycle to archived instead. Logged as the agent's change.",
            named("application"),
            &[],
            json!({ "destructiveHint": true, "idempotentHint": false }),
        ),
        tool(
            "delete_resource",
            "Delete a resource without history",
            "Deletes a resource that no component runs on, that holds no other system's paths and that has no observations, incidents or events, with its references and its HTTP check. Anything else is refused and explained. Logged as the agent's change.",
            named("resource"),
            &[],
            json!({ "destructiveHint": true, "idempotentHint": false }),
        ),
        tool(
            "record_observation",
            "Record an observation",
            "Records what the agent observed about a system, tagged as the agent's: a result, what it saw, and how long that vouches for the system (validForMinutes, 1 to 43200; with Read and maintain, at most 10080, 7 days, so that a review nobody repeats expires and shows as a gap again). observedAt (RFC 3339) defaults to now and cannot be in the future. Observations are immutable; record another to correct one.",
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
                "expectedStatus": { "type": "integer", "minimum": 200, "maximum": 599, "description": "A final HTTP status, never a 3xx: redirects are followed." },
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
        tool(
            "read_copies",
            "Read local copies",
            "Reads, now, the local copies declared on this workstation (status: thisWorkstation), as Read does in DevHQ: one reference (referenceId), the copies of one system (type and id or name), or, with neither, every one but those of archived applications. Each reading is kept as DevHQ's own evidence (source read) with this agent as who asked, and says what the copy held at that moment: whether it was there, its branch and HEAD, its newest commit date, commits on no remote, stashes, remotes (without credentials), an operation left halfway, and how many files changed, are new or could not be verified. Never file names, contents, authors or messages. DevHQ reads with libgit2 in a process that cannot start programs, so nothing the repository names runs, and it writes nothing in the copy; code that hostile data managed to run inside that reader would still have the person's rights. Nothing leaves this computer, and no configuration changes. Copies on other machines are read by DevHQ there.",
            json!({
                "referenceId": { "type": "string", "description": "One local copy's reference id, from list_systems. Never with type, id or name." },
                "type": { "type": "string", "enum": ["application", "resource"], "description": "With id or name: the system whose copies to read." },
                "id": { "type": "string", "description": "The system's id, from list_systems; only with type." },
                "name": { "type": "string", "description": "Or its exact name, ignoring case; only with type. Given with id, it must be that system's name, or nothing is done." },
            }),
            &[],
            json!({ "destructiveHint": false, "idempotentHint": false, "openWorldHint": false }),
        ),
    ]
}
