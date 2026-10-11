//! One log of every significant change to the inventory's configuration (applications, resources,
//! components, references and HTTP checks), which has no `source` of its own, whoever makes it: the person in
//! the window (`manual`) or an agent through the agent gate (`agent`, with its name). Records
//! (observations, incidents, events) say who reported them in their own `source`, so they are not
//! repeated here. The use cases write each entry in the same transaction as its change, so there
//! is never a change without its entry or an entry without its change; a refused change, or a save
//! that changes nothing, leaves none. Entries are never changed or deleted, and outlive the
//! subject they name.
//!
//! Each entry keeps a summary to read and, field by field, what changed: the value before and
//! after, as the window sees that field. The inventory itself keeps only how things are now; this
//! is how they came to be so.

use crate::http_check::HttpCheck;
use crate::inventory::{ApiError, Application, Resource};
use crate::records::{format_time, now_ms, time_at, Actor, Subject};
use crate::references::{self, Reference};
use rusqlite::types::Type;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Longest summary kept, in characters as the window counts them; longer ones are cut with «…».
const MAX_SUMMARY: usize = 500;

/// The fields each kind of configuration logs, by the keys the window knows them by.
const APPLICATION_FIELDS: [&str; 6] = [
    "name",
    "lifecycle",
    "version",
    "description",
    "components",
    "references",
];
const RESOURCE_FIELDS: [&str; 5] = [
    "name",
    "kind",
    "operatingSystem",
    "description",
    "references",
];
const CHECK_FIELDS: [&str; 6] = [
    "url",
    "expectedStatus",
    "timeoutSeconds",
    "validForMinutes",
    "monitor",
    "monitorIntervalMinutes",
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub id: String,
    pub subject: Subject,
    /// The subject's name when the change was made: it may since have been renamed or deleted.
    pub subject_name: String,
    /// Who made it: `manual` (the person, in the window) or `agent`.
    pub source: String,
    /// The agent, as its client named itself (unverified); None for the person.
    pub agent: Option<String>,
    /// created, updated, deleted, check_saved, check_paused, check_resumed, check_removed or
    /// check_run.
    pub action: String,
    pub summary: String,
    /// What changed, field by field. None only for the agents' entries kept from before v10,
    /// which have just their summary; a run of a check changes no field.
    pub changes: Option<Vec<FieldChange>>,
    pub occurred_at: String,
}

/// One field of an application, resource or HTTP check, before and after a change: null where
/// it had no value or did not exist (before a creation, after a deletion).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldChange {
    pub field: String,
    pub before: Value,
    pub after: Value,
}

fn change_from_row(row: &Row) -> rusqlite::Result<Change> {
    let changes = match row.get::<_, Option<String>>(8)? {
        Some(json) => Some(serde_json::from_str(&json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(8, Type::Text, Box::new(error))
        })?),
        None => None,
    };
    Ok(Change {
        id: row.get(0)?,
        subject: Subject::from_columns(row.get(1)?, row.get(2)?)?,
        subject_name: row.get(3)?,
        source: row.get(4)?,
        agent: row.get(5)?,
        action: row.get(6)?,
        summary: row.get(7)?,
        changes,
        occurred_at: time_at(row, 9)?,
    })
}

const SELECT: &str = "SELECT id, application_id, resource_id, subject_name, source, agent, action, summary, changes, occurred_at FROM change_log";

/// A summary within its limit, cut on a character boundary.
fn bounded(summary: &str) -> String {
    let summary = summary.trim();
    if summary.encode_utf16().count() <= MAX_SUMMARY {
        return summary.to_owned();
    }
    let mut cut = String::new();
    for character in summary.chars() {
        if cut.encode_utf16().count() + character.len_utf16() > MAX_SUMMARY - 1 {
            break;
        }
        cut.push(character);
    }
    cut.push('…');
    cut
}

/// Records a change inside the caller's write transaction, which also makes the change.
fn record(
    db: &Connection,
    by: &Actor,
    action: &str,
    subject: &Subject,
    subject_name: &str,
    summary: &str,
    changes: Option<&[FieldChange]>,
) -> Result<Change, ApiError> {
    let id = Uuid::new_v4().to_string();
    let (application_id, resource_id) = subject.columns();
    let changes = changes
        .map(serde_json::to_string)
        .transpose()
        .map_err(ApiError::database)?;
    db.execute(
        "INSERT INTO change_log (id, application_id, resource_id, subject_name, source, agent, action, summary, changes, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![id, application_id, resource_id, subject_name, by.source(), by.name(), action, bounded(summary), changes, now_ms()],
    )
    .map_err(ApiError::database)?;
    db.query_row(&format!("{SELECT} WHERE id = ?1"), [&id], change_from_row)
        .map_err(ApiError::database)
}

/// Every change, newest first. They grow only when someone changes the configuration, like
/// manual records, so a snapshot carries them all.
pub(crate) fn load(db: &Connection) -> Result<Vec<Change>, ApiError> {
    let mut statement = db
        .prepare(&format!("{SELECT} ORDER BY occurred_at DESC, rowid DESC"))
        .map_err(ApiError::database)?;
    let rows = statement
        .query_map([], change_from_row)
        .map_err(ApiError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApiError::database);
    rows
}

/// From when the person's changes are in the log: the moment v10 upgraded an inventory that
/// already held applications or resources. None when the log was always complete.
pub(crate) fn complete_since(db: &Connection) -> Result<Option<String>, ApiError> {
    let started: Option<i64> = db
        .query_row(
            "SELECT started_at FROM change_log_start WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(ApiError::database)?;
    Ok(started.and_then(format_time))
}

/// Nothing to tell: no value, empty text or an empty list.
fn blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

/// Each of `fields` whose value differs between two states of the same thing, with its value
/// before and after. A state that does not exist (before a creation, after a deletion) reads as
/// null in every field, and a field blank in both did not change.
fn diff<T: Serialize>(before: Option<&T>, after: Option<&T>, fields: &[&str]) -> Vec<FieldChange> {
    let state = |value: Option<&T>| {
        value
            .and_then(|value| serde_json::to_value(value).ok())
            .unwrap_or(Value::Null)
    };
    let (before, after) = (state(before), state(after));
    fields
        .iter()
        .filter_map(|field| {
            let was = before.get(field).cloned().unwrap_or(Value::Null);
            let is = after.get(field).cloned().unwrap_or(Value::Null);
            (was != is && !(blank(&was) && blank(&is))).then(|| FieldChange {
                field: (*field).to_owned(),
                before: was,
                after: is,
            })
        })
        .collect()
}

/// A value for a summary: «name» in quotes, «none» for nothing.
fn quoted(value: Option<&str>) -> String {
    match value {
        Some(value) if !value.is_empty() => format!("«{value}»"),
        _ => "none".to_owned(),
    }
}

/// Every resource, as (ID, name), so a summary can say what a component runs on.
fn resource_names(db: &Connection) -> Result<Vec<(String, String)>, ApiError> {
    let mut statement = db
        .prepare("SELECT id, name FROM resources ORDER BY rowid")
        .map_err(ApiError::database)?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(ApiError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApiError::database);
    rows
}

/// The subject's current name, read inside the caller's transaction.
fn name_of(db: &Connection, subject: &Subject) -> Result<String, ApiError> {
    let (sql, id) = match subject {
        Subject::Application { id } => ("SELECT name FROM applications WHERE id = ?1", id),
        Subject::Resource { id } => ("SELECT name FROM resources WHERE id = ?1", id),
    };
    db.query_row(sql, [id], |row| row.get(0))
        .optional()
        .map_err(ApiError::database)?
        .ok_or_else(|| ApiError::invalid(format!("The {} {id} does not exist.", subject.label())))
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

/// Each reference to read: «local copy «Clone» C:\src\shop on Laptop», with the machine's name.
fn reference_summary(references: &[Reference], resources: &[(String, String)]) -> Vec<String> {
    references
        .iter()
        .map(|reference| {
            let label = reference
                .label
                .as_ref()
                .map(|label| format!(" «{label}»"))
                .unwrap_or_default();
            let on = reference
                .machine_id
                .as_ref()
                .map(|id| {
                    let machine = resources
                        .iter()
                        .find(|(resource, _)| resource == id)
                        .map_or(id.as_str(), |(_, name)| name.as_str());
                    format!(" on {machine}")
                })
                .unwrap_or_default();
            format!(
                "{}{label} {}{on}",
                references::kind_label(&reference.kind),
                reference.target
            )
        })
        .collect()
}

/// The references a system was created with, for its summary: nothing when there are none.
fn created_with_references(references: &[Reference], resources: &[(String, String)]) -> String {
    if references.is_empty() {
        String::new()
    } else {
        format!(
            "; references: {}",
            reference_summary(references, resources).join(", ")
        )
    }
}

/// How a system's references changed, to read, if they did.
fn reference_changes(
    before: &[Reference],
    after: &[Reference],
    resources: &[(String, String)],
) -> Option<String> {
    let (was, is) = (
        reference_summary(before, resources),
        reference_summary(after, resources),
    );
    if was != is {
        Some(format!("references {} → {}", listed(&was), listed(&is)))
    } else if before != after {
        // The same kinds, targets, labels and machines, but other references (removed and added
        // again).
        Some(format!("references replaced: {}", listed(&is)))
    } else {
        None
    }
}

fn listed(parts: &[String]) -> String {
    if parts.is_empty() {
        "none".to_owned()
    } else {
        parts.join(", ")
    }
}

/// What changed from `before` to `after`, field by field, to read.
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
        changes.push(format!("components {} → {}", listed(&was), listed(&is)));
    } else if before.components != after.components {
        // The same names, kinds and resources, but other components (removed and added again).
        changes.push(format!("components replaced: {}", listed(&is)));
    }
    changes.extend(reference_changes(
        &before.references,
        &after.references,
        resources,
    ));
    changes
}

fn resource_changes(
    before: &Resource,
    after: &Resource,
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
    changes.extend(reference_changes(
        &before.references,
        &after.references,
        resources,
    ));
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

/// Logs a saved application: its creation, or what an edit changed. A save that changed nothing
/// is no change, and logs nothing.
pub(crate) fn application_saved(
    db: &Connection,
    by: &Actor,
    before: Option<&Application>,
    after: &Application,
) -> Result<Option<Change>, ApiError> {
    let changes = diff(before, Some(after), &APPLICATION_FIELDS);
    if before.is_some() && changes.is_empty() {
        return Ok(None);
    }
    let resources = resource_names(db)?;
    let (action, summary) = match before {
        None => {
            let parts = component_summary(after, &resources);
            (
                "created",
                format!(
                    "Added application «{}» ({}{}){}{}",
                    after.name,
                    after.lifecycle,
                    after
                        .version
                        .as_ref()
                        .map(|version| format!(", version {version}"))
                        .unwrap_or_default(),
                    if parts.is_empty() {
                        String::new()
                    } else {
                        format!(" with {}", parts.join(", "))
                    },
                    created_with_references(&after.references, &resources)
                ),
            )
        }
        Some(before) => (
            "updated",
            format!(
                "Updated application «{}»: {}",
                after.name,
                application_changes(before, after, &resources).join("; ")
            ),
        ),
    };
    let subject = Subject::Application {
        id: after.id.clone(),
    };
    record(
        db,
        by,
        action,
        &subject,
        &after.name,
        &summary,
        Some(&changes),
    )
    .map(Some)
}

/// Logs a saved resource: its creation, or what an edit changed. A save that changed nothing
/// logs nothing.
pub(crate) fn resource_saved(
    db: &Connection,
    by: &Actor,
    before: Option<&Resource>,
    after: &Resource,
) -> Result<Option<Change>, ApiError> {
    let changes = diff(before, Some(after), &RESOURCE_FIELDS);
    if before.is_some() && changes.is_empty() {
        return Ok(None);
    }
    let resources = resource_names(db)?;
    let (action, summary) = match before {
        None => (
            "created",
            format!(
                "Added resource «{}» ({}{}){}",
                after.name,
                after.kind,
                if after.operating_system.is_empty() {
                    String::new()
                } else {
                    format!(", {}", after.operating_system)
                },
                created_with_references(&after.references, &resources)
            ),
        ),
        Some(before) => (
            "updated",
            format!(
                "Updated resource «{}»: {}",
                after.name,
                resource_changes(before, after, &resources).join("; ")
            ),
        ),
    };
    let subject = Subject::Resource {
        id: after.id.clone(),
    };
    record(
        db,
        by,
        action,
        &subject,
        &after.name,
        &summary,
        Some(&changes),
    )
    .map(Some)
}

/// Logs a deleted application with every field it had, so that what is gone stays known.
pub(crate) fn application_deleted(
    db: &Connection,
    by: &Actor,
    before: &Application,
) -> Result<Change, ApiError> {
    let subject = Subject::Application {
        id: before.id.clone(),
    };
    let summary = format!(
        "Deleted application «{}», which had no history",
        before.name
    );
    let changes = diff(Some(before), None, &APPLICATION_FIELDS);
    record(
        db,
        by,
        "deleted",
        &subject,
        &before.name,
        &summary,
        Some(&changes),
    )
}

/// Logs a deleted resource with every field it had.
pub(crate) fn resource_deleted(
    db: &Connection,
    by: &Actor,
    before: &Resource,
) -> Result<Change, ApiError> {
    let subject = Subject::Resource {
        id: before.id.clone(),
    };
    let summary = format!("Deleted resource «{}», which had no history", before.name);
    let changes = diff(Some(before), None, &RESOURCE_FIELDS);
    record(
        db,
        by,
        "deleted",
        &subject,
        &before.name,
        &summary,
        Some(&changes),
    )
}

/// Logs a check set up or changed. Saving it unchanged logs nothing.
pub(crate) fn check_saved(
    db: &Connection,
    by: &Actor,
    before: Option<&HttpCheck>,
    after: &HttpCheck,
) -> Result<Option<Change>, ApiError> {
    let changes = diff(before, Some(after), &CHECK_FIELDS);
    if before.is_some() && changes.is_empty() {
        return Ok(None);
    }
    let name = name_of(db, &after.subject)?;
    let summary = format!(
        "{} the HTTP check of «{name}»: {}",
        if before.is_some() {
            "Changed"
        } else {
            "Set up"
        },
        check_summary(after)
    );
    record(
        db,
        by,
        "check_saved",
        &after.subject,
        &name,
        &summary,
        Some(&changes),
    )
    .map(Some)
}

/// Logs a check's supervision paused or resumed. Pausing what was already paused, or resuming
/// what was already active, logs nothing.
pub(crate) fn check_paused(
    db: &Connection,
    by: &Actor,
    before: &HttpCheck,
    after: &HttpCheck,
) -> Result<Option<Change>, ApiError> {
    let changes = diff(Some(before), Some(after), &CHECK_FIELDS);
    if changes.is_empty() {
        return Ok(None);
    }
    let name = name_of(db, &after.subject)?;
    let (action, summary) = if after.monitor == "paused" {
        (
            "check_paused",
            format!("Paused the supervision of «{name}»"),
        )
    } else {
        (
            "check_resumed",
            format!(
                "Resumed the supervision of «{name}»: {}",
                check_summary(after)
            ),
        )
    };
    record(
        db,
        by,
        action,
        &after.subject,
        &name,
        &summary,
        Some(&changes),
    )
    .map(Some)
}

/// Logs a removed check with every field it had; the evidence it recorded stays.
pub(crate) fn check_removed(
    db: &Connection,
    by: &Actor,
    before: &HttpCheck,
) -> Result<Change, ApiError> {
    let name = name_of(db, &before.subject)?;
    let summary = format!(
        "Removed the HTTP check of «{name}» ({}); the evidence it recorded stays",
        before.url
    );
    let changes = diff(Some(before), None, &CHECK_FIELDS);
    record(
        db,
        by,
        "check_removed",
        &before.subject,
        &name,
        &summary,
        Some(&changes),
    )
}

/// Logs who asked for a run of a check. It changes no configuration, and its observation is
/// DevHQ's own evidence (`source` http), which cannot say who asked: only the agent gate logs
/// it, so that a request an agent sent out from this computer is always traced to that agent.
pub(crate) fn check_run(
    db: &Connection,
    by: &Actor,
    subject: &Subject,
    result: &str,
) -> Result<Change, ApiError> {
    let name = name_of(db, subject)?;
    let summary = format!("Ran the HTTP check of «{name}»: {result}");
    record(db, by, "check_run", subject, &name, &summary, Some(&[]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_summary_within_its_limit_without_splitting_a_character() {
        assert_eq!(bounded("  Added «Shop»  "), "Added «Shop»");
        let exact = "a".repeat(MAX_SUMMARY);
        assert_eq!(bounded(&exact), exact);
        // Each emoji is two UTF-16 units, as the window counts them.
        let long = "🙂".repeat(MAX_SUMMARY);
        let cut = bounded(&long);
        assert!(cut.ends_with('…'));
        assert!(cut.encode_utf16().count() <= MAX_SUMMARY);
        assert_eq!(
            cut.trim_end_matches('…').chars().count(),
            (MAX_SUMMARY - 1) / 2
        );
    }

    fn resource(name: &str, operating_system: &str, description: &str) -> Resource {
        Resource {
            id: "host".into(),
            name: name.into(),
            kind: "server".into(),
            description: description.into(),
            operating_system: operating_system.into(),
            references: vec![],
        }
    }

    #[test]
    fn diffs_only_the_fields_that_changed_and_reads_what_is_missing_as_null() {
        let before = resource("Host", "Debian 12", "");
        let after = resource("Host", "Debian 13", "Front door");
        assert_eq!(
            diff(Some(&before), Some(&after), &RESOURCE_FIELDS),
            [
                FieldChange {
                    field: "operatingSystem".into(),
                    before: "Debian 12".into(),
                    after: "Debian 13".into(),
                },
                FieldChange {
                    field: "description".into(),
                    before: "".into(),
                    after: "Front door".into(),
                },
            ]
        );
        assert!(diff(Some(&before), Some(&before), &RESOURCE_FIELDS).is_empty());
        // A creation lists what it was created with; blank fields tell nothing.
        let created = diff(None, Some(&before), &RESOURCE_FIELDS);
        let fields: Vec<_> = created.iter().map(|change| change.field.as_str()).collect();
        assert_eq!(fields, ["name", "kind", "operatingSystem"]);
        assert!(created.iter().all(|change| change.before.is_null()));
        // A deletion keeps every value it had.
        let deleted = diff(Some(&after), None, &RESOURCE_FIELDS);
        assert_eq!(deleted.len(), 4);
        assert!(deleted.iter().all(|change| change.after.is_null()));
    }
}
