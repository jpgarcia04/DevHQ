//! Where each system is in the world: its repository, its local copies on each workstation, its
//! documentation, its logs and its dashboard, so that a person or an agent who arrives at a system
//! knows where to look, and an agent working in a folder can tell which system that folder is.
//!
//! References are inventory that the person or an agent declares, never evidence: they carry no
//! `source`, and their trace is the change log, where they are one more field of their
//! application's or resource's entries. DevHQ neither opens nor checks them; the window only copies
//! them. They are edited with their system, as components are: a save gives the complete list,
//! which replaces the stored one in the same transaction and keeps the IDs it names.
//!
//! A target is an http(s) URL or an absolute path. A path says which machine (a resource) it is on,
//! since the same project sits in its own folder on each workstation; a URL is on none. A
//! repository and a dashboard are URLs, a local copy is a path on a workstation, and documentation
//! and logs can be either. Which systems declare the same repository (a monorepo) is derived at
//! each read, by exact equality of its URL, and never stored.

use crate::http_check;
use crate::inventory::{allowed, bounded_text, ApiError, Application, Resource, MAX_NAME};
use crate::records::Subject;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// The kinds of reference: concepts, never providers.
pub(crate) const KINDS: [&str; 5] = [
    "repository",
    "local_copy",
    "documentation",
    "logs",
    "dashboard",
];

/// The longest path DevHQ keeps, as long as the longest URL.
const MAX_PATH: usize = 2048;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    pub id: String,
    /// repository, local_copy, documentation, logs or dashboard.
    pub kind: String,
    /// An http(s) URL or an absolute path, as it was typed, without surrounding whitespace.
    pub target: String,
    /// A name for people that tells two references of one kind apart; never a locator.
    pub label: Option<String>,
    /// The resource a path is on; None for a URL.
    pub machine_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
// Serialized only by tests: ipc_contract finds the fixture keys Rust never reads.
#[cfg_attr(test, derive(Serialize))]
#[serde(rename_all = "camelCase")]
pub struct ReferenceInput {
    pub id: Option<String>,
    pub kind: String,
    pub target: String,
    pub label: Option<String>,
    pub machine_id: Option<String>,
}

impl From<&Reference> for ReferenceInput {
    /// A stored reference given back unchanged, as an edit that leaves it out keeps it.
    fn from(reference: &Reference) -> Self {
        Self {
            id: Some(reference.id.clone()),
            kind: reference.kind.clone(),
            target: reference.target.clone(),
            label: reference.label.clone(),
            machine_id: reference.machine_id.clone(),
        }
    }
}

/// A repository URL that more than one system declares, as the parts of a monorepo do. Derived at
/// each read, by exact equality of the URL; never stored.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SharedRepository {
    pub target: String,
    /// Applications first, then resources, in the inventory's order.
    pub systems: Vec<Subject>,
}

const SELECT: &str = "SELECT id, kind, target, label, machine_id FROM system_references";

fn from_row(row: &Row) -> rusqlite::Result<Reference> {
    Ok(Reference {
        id: row.get(0)?,
        kind: row.get(1)?,
        target: row.get(2)?,
        label: row.get(3)?,
        machine_id: row.get(4)?,
    })
}

/// The subject's references, in the order they were saved, read inside the caller's transaction
/// through the index of the subject's column.
pub(crate) fn of(db: &Connection, subject: &Subject) -> Result<Vec<Reference>, ApiError> {
    let (column, id) = match subject {
        Subject::Application { id } => ("application_id", id),
        Subject::Resource { id } => ("resource_id", id),
    };
    let mut statement = db
        .prepare_cached(&format!("{SELECT} WHERE {column} = ?1 ORDER BY rowid"))
        .map_err(ApiError::database)?;
    let rows = statement
        .query_map([id], from_row)
        .map_err(ApiError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApiError::database);
    rows
}

/// Whether a target is a web address or a path on a machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Web,
    Path,
}

/// Starts with http:// or https://, in any case, as SQLite's LIKE compares it in the schema.
fn web_scheme(text: &str) -> bool {
    let bytes = text.as_bytes();
    [b"http://".as_slice(), b"https://"].iter().any(|prefix| {
        bytes.len() >= prefix.len() && bytes[..prefix.len()].eq_ignore_ascii_case(prefix)
    })
}

/// An absolute path in the form of any system, whatever system DevHQ runs on, since the path can be
/// on another machine: `C:\…` (or `C:/…`), `\\server\share\…` or `/…`.
fn absolute_path(text: &str) -> bool {
    if text.starts_with('/') {
        return true;
    }
    if let Some(rest) = text.strip_prefix(r"\\") {
        let mut parts = rest.split('\\');
        let (server, share) = (
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
        );
        // `\\?\…` and `\\.\…` name a verbatim path or a device, not a server's share.
        return !server.is_empty() && !share.is_empty() && server != "?" && server != ".";
    }
    let bytes = text.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

/// The target as stored, and whether it is a URL or a path, if it suits the kind.
fn target(kind: &str, value: &str) -> Result<(String, Place), ApiError> {
    let text = value.trim();
    if text.is_empty() {
        return Err(ApiError::invalid("A reference needs its URL or its path."));
    }
    if text.chars().any(char::is_control) {
        return Err(ApiError::invalid(
            "A reference's URL or path cannot hold line breaks, tabs or other control characters.",
        ));
    }
    let place = if web_scheme(text) {
        // The rules of the HTTP check's URL: a host, no user name or password, 2048 at most.
        http_check::web_url(text, "The reference URL")?;
        Place::Web
    } else if absolute_path(text) {
        bounded_text(text, MAX_PATH, "The reference path")?;
        Place::Path
    } else {
        return Err(ApiError::invalid(format!(
            r"{text} is neither an http(s) URL nor an absolute path (C:\…, \\server\share\… or /…)."
        )));
    };
    match (kind, place) {
        ("repository", Place::Path) => Err(ApiError::invalid(
            "A repository is the web address of the code (http or https). A clone on a machine is a local copy.",
        )),
        ("dashboard", Place::Path) => Err(ApiError::invalid(
            "A dashboard is a web address (http or https).",
        )),
        ("local_copy", Place::Web) => Err(ApiError::invalid(
            "A local copy is a folder on a workstation: an absolute path, not a URL.",
        )),
        _ => Ok((text.to_owned(), place)),
    }
}

/// The kind as people read it: «local copy».
pub(crate) fn kind_label(kind: &str) -> &str {
    match kind {
        "local_copy" => "local copy",
        other => other,
    }
}

/// The resource that will be saved along with these references, as it will be once saved: it can
/// be the machine of its own paths, even before it exists.
pub(crate) struct Machine<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub kind: &'a str,
}

/// The name and kind of the resource `id`, as it will be after this save.
fn machine(
    db: &Connection,
    id: &str,
    saving: Option<&Machine>,
) -> Result<Option<(String, String)>, ApiError> {
    if let Some(machine) = saving.filter(|machine| machine.id == id) {
        return Ok(Some((machine.name.to_owned(), machine.kind.to_owned())));
    }
    db.query_row(
        "SELECT name, kind FROM resources WHERE id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(ApiError::database)
}

/// Checks the complete list a save gives for `subject` inside the caller's transaction, before
/// anything is written, and returns the references to store, in order. `saving` is the resource
/// being saved with them, if any.
pub(crate) fn validated(
    db: &Connection,
    subject: &Subject,
    inputs: Vec<ReferenceInput>,
    saving: Option<&Machine>,
) -> Result<Vec<Reference>, ApiError> {
    let mut references: Vec<Reference> = Vec::with_capacity(inputs.len());
    for input in inputs {
        allowed(&input.kind, &KINDS, "reference kind")?;
        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        if id.is_empty() {
            return Err(ApiError::invalid("Reference ID is required."));
        }
        if references.iter().any(|item| item.id == id) {
            return Err(ApiError::invalid(format!(
                "Reference {id} appears twice. Each reference can be listed once."
            )));
        }
        // A reference belongs to one system; reusing its ID elsewhere would move it.
        let owner: Option<(Option<String>, Option<String>)> = db
            .query_row(
                "SELECT application_id, resource_id FROM system_references WHERE id = ?1",
                [&id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(ApiError::database)?;
        if let Some((application_id, resource_id)) = owner {
            if Subject::from_columns(application_id, resource_id)
                .ok()
                .as_ref()
                != Some(subject)
            {
                return Err(ApiError::invalid(format!(
                    "Reference {id} belongs to another system."
                )));
            }
        }
        let (target, place) = target(&input.kind, &input.target)?;
        let label = match &input.label {
            Some(label) => Some(bounded_text(label, MAX_NAME, "Reference label")?),
            None => None,
        }
        .filter(|label| !label.is_empty());
        let machine_id = match (place, input.machine_id) {
            (Place::Web, None) => None,
            (Place::Web, Some(_)) => {
                return Err(ApiError::invalid(format!(
                    "{target} is a URL, which is on no machine: leave the machine out."
                )))
            }
            (Place::Path, None) => {
                return Err(ApiError::invalid(format!(
                    "Say which machine {target} is on: a path means something only on one."
                )))
            }
            (Place::Path, Some(id)) => {
                let Some((name, kind)) = machine(db, &id, saving)? else {
                    return Err(ApiError::invalid(format!(
                        "Resource {id} does not exist. Choose an existing resource as the machine of {target}."
                    )));
                };
                if input.kind == "local_copy" && kind != "workstation" {
                    return Err(ApiError::invalid(format!(
                        "{name} is a {kind}, not a workstation. A local copy is a working copy on a workstation."
                    )));
                }
                Some(id)
            }
        };
        if references.iter().any(|item| {
            item.kind == input.kind && item.target == target && item.machine_id == machine_id
        }) {
            return Err(ApiError::invalid(format!(
                "The same {} {target} is listed twice.",
                kind_label(&input.kind)
            )));
        }
        references.push(Reference {
            id,
            kind: input.kind,
            target,
            label,
            machine_id,
        });
    }
    Ok(references)
}

/// Replaces the subject's references with `references` inside the caller's transaction: the list
/// given is the complete set, so what it leaves out is removed.
pub(crate) fn replace(
    db: &Connection,
    subject: &Subject,
    references: &[Reference],
) -> Result<(), ApiError> {
    remove_all(db, subject)?;
    let (application_id, resource_id) = subject.columns();
    for reference in references {
        db.execute(
            "INSERT INTO system_references (id, application_id, resource_id, kind, target, label, machine_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                reference.id,
                application_id,
                resource_id,
                reference.kind,
                reference.target,
                reference.label,
                reference.machine_id
            ],
        )
        .map_err(ApiError::database)?;
    }
    Ok(())
}

/// Removes every reference of the subject, inside the caller's transaction.
pub(crate) fn remove_all(db: &Connection, subject: &Subject) -> Result<(), ApiError> {
    let (column, id) = match subject {
        Subject::Application { id } => ("application_id", id),
        Subject::Resource { id } => ("resource_id", id),
    };
    db.execute(
        &format!("DELETE FROM system_references WHERE {column} = ?1"),
        [id],
    )
    .map_err(ApiError::database)?;
    Ok(())
}

/// The paths of other systems that are on the resource `id`, to read: «Shop (local copy
/// C:\src\shop)», by system name. Only `kind` when given.
pub(crate) fn paths_on(
    db: &Connection,
    id: &str,
    kind: Option<&str>,
) -> Result<Vec<String>, ApiError> {
    let mut statement = db
        .prepare(
            "SELECT coalesce(a.name, o.name), s.kind, s.target FROM system_references s
             LEFT JOIN applications a ON a.id = s.application_id
             LEFT JOIN resources o ON o.id = s.resource_id
             WHERE s.machine_id = ?1 AND s.resource_id IS NOT ?1 AND (?2 IS NULL OR s.kind = ?2)
             ORDER BY 1 COLLATE NOCASE, s.rowid",
        )
        .map_err(ApiError::database)?;
    let rows = statement
        .query_map(params![id, kind], |row| {
            let (system, kind, target): (String, String, String) =
                (row.get(0)?, row.get(1)?, row.get(2)?);
            Ok(format!("{system} ({} {target})", kind_label(&kind)))
        })
        .map_err(ApiError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApiError::database);
    rows
}

/// Each repository URL that more than one system declares, in the order first declared
/// (applications, then resources, in the inventory's order), with every system that declares it.
pub(crate) fn shared_repositories(
    applications: &[Application],
    resources: &[Resource],
) -> Vec<SharedRepository> {
    let systems = applications
        .iter()
        .map(|item| {
            (
                Subject::Application {
                    id: item.id.clone(),
                },
                &item.references,
            )
        })
        .chain(resources.iter().map(|item| {
            (
                Subject::Resource {
                    id: item.id.clone(),
                },
                &item.references,
            )
        }));
    let mut repositories: Vec<SharedRepository> = Vec::new();
    let mut found: HashMap<&str, usize> = HashMap::new();
    for (subject, references) in systems {
        for reference in references.iter().filter(|item| item.kind == "repository") {
            let index = *found.entry(&reference.target).or_insert_with(|| {
                repositories.push(SharedRepository {
                    target: reference.target.clone(),
                    systems: Vec::new(),
                });
                repositories.len() - 1
            });
            let systems = &mut repositories[index].systems;
            if !systems.contains(&subject) {
                systems.push(subject.clone());
            }
        }
    }
    repositories.retain(|repository| repository.systems.len() > 1);
    repositories
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{
        delete_application, delete_resource, list, save_application, save_resource,
        ApplicationInput, Inventory, ResourceInput,
    };
    use serde_json::{json, Value};
    use std::path::{Path, PathBuf};

    fn path() -> PathBuf {
        std::env::temp_dir().join(format!("devhq-references-test-{}.sqlite3", Uuid::new_v4()))
    }

    fn at(kind: &str, target: &str, machine: Option<&str>) -> ReferenceInput {
        ReferenceInput {
            id: None,
            kind: kind.into(),
            target: target.into(),
            label: None,
            machine_id: machine.map(Into::into),
        }
    }

    fn machine(path: &Path, name: &str, kind: &str) -> Resource {
        save_resource(
            path,
            ResourceInput {
                id: None,
                name: name.into(),
                kind: kind.into(),
                description: String::new(),
                operating_system: String::new(),
                references: vec![],
            },
        )
        .unwrap()
    }

    fn application(name: &str, references: Vec<ReferenceInput>) -> ApplicationInput {
        ApplicationInput {
            id: None,
            name: name.into(),
            description: String::new(),
            lifecycle: "production".into(),
            version: None,
            components: vec![],
            references,
        }
    }

    /// An edit that gives the stored application back with another reference list.
    fn edited(stored: &Application, references: Vec<ReferenceInput>) -> ApplicationInput {
        ApplicationInput {
            id: Some(stored.id.clone()),
            references,
            ..application(&stored.name, vec![])
        }
    }

    fn kept(reference: &Reference) -> ReferenceInput {
        ReferenceInput::from(reference)
    }

    fn rows(path: &Path) -> Vec<String> {
        let db = crate::inventory::open(path).unwrap();
        crate::inventory::tests::dump(&db, "system_references")
    }

    #[test]
    fn keeps_each_system_where_it_is_and_replaces_the_list_as_a_whole() {
        let path = path();
        let laptop = machine(&path, "Laptop", "workstation");
        let vps = machine(&path, "VPS", "server");
        let mut docs = at("documentation", "https://docs.example/shop#deploy", None);
        docs.label = Some("  Runbook  ".into());
        let shop = save_application(
            &path,
            application(
                "Shop",
                vec![
                    at("repository", "  https://github.com/me/shop  ", None),
                    at("local_copy", r"C:\Users\me\src\shop", Some(&laptop.id)),
                    at("local_copy", "/home/me/src/shop", Some(&laptop.id)),
                    docs,
                    at("logs", "/var/log/shop", Some(&vps.id)),
                    at("logs", r"\\nas\logs\shop", Some(&vps.id)),
                    at("dashboard", "HTTPS://Grafana.example/d/shop", None),
                ],
            ),
        )
        .unwrap();
        // As typed, without surrounding whitespace: no URL is normalized, and a fragment stays.
        let stored: Vec<_> = shop
            .references
            .iter()
            .map(|item| {
                (
                    item.kind.as_str(),
                    item.target.as_str(),
                    item.label.as_deref(),
                    item.machine_id.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            stored,
            [
                ("repository", "https://github.com/me/shop", None, None),
                (
                    "local_copy",
                    r"C:\Users\me\src\shop",
                    None,
                    Some(laptop.id.as_str())
                ),
                (
                    "local_copy",
                    "/home/me/src/shop",
                    None,
                    Some(laptop.id.as_str())
                ),
                (
                    "documentation",
                    "https://docs.example/shop#deploy",
                    Some("Runbook"),
                    None
                ),
                ("logs", "/var/log/shop", None, Some(vps.id.as_str())),
                ("logs", r"\\nas\logs\shop", None, Some(vps.id.as_str())),
                ("dashboard", "HTTPS://Grafana.example/d/shop", None, None),
            ]
        );
        assert_eq!(list(&path).unwrap().applications, vec![shop.clone()]);

        // The list given is the complete set: the kept ones keep their IDs, the rest go.
        let mut moved = kept(&shop.references[1]);
        moved.target = r"D:\src\shop".into();
        let trimmed = save_application(
            &path,
            edited(
                &shop,
                vec![
                    kept(&shop.references[0]),
                    moved,
                    at("logs", "https://logs.example/shop", None),
                ],
            ),
        )
        .unwrap();
        assert_eq!(trimmed.references[0], shop.references[0]);
        assert_eq!(trimmed.references[1].id, shop.references[1].id);
        assert_eq!(trimmed.references[1].target, r"D:\src\shop");
        assert_ne!(trimmed.references[2].id, shop.references[6].id);
        assert_eq!(list(&path).unwrap().applications[0], trimmed);
        assert_eq!(rows(&path).len(), 3);

        // Deleting the application deletes its references; its machines stay.
        delete_application(&path, &shop.id).unwrap();
        assert!(rows(&path).is_empty());
        assert_eq!(list(&path).unwrap().resources.len(), 2);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn refuses_what_is_not_a_reference_without_partial_writes() {
        let path = path();
        let laptop = machine(&path, "Laptop", "workstation");
        let host = machine(&path, "Host", "server");
        let shop = save_application(
            &path,
            application(
                "Shop",
                vec![at("repository", "https://github.com/me/shop", None)],
            ),
        )
        .unwrap();
        let other = save_application(
            &path,
            application(
                "Blog",
                vec![at("dashboard", "https://grafana.example/d/blog", None)],
            ),
        )
        .unwrap();
        let before = list(&path).unwrap();
        let path_on = |kind: &str, target: &str| at(kind, target, Some(&laptop.id));
        let mut labelled = at("dashboard", "https://grafana.example/d/x", None);
        labelled.label = Some("é".repeat(121));
        let mut stolen = kept(&other.references[0]);
        stolen.target = "https://grafana.example/d/stolen".into();
        let mut twice = kept(&shop.references[0]);
        twice.target = "https://github.com/me/other".into();
        let attempts: Vec<(Vec<ReferenceInput>, &str)> = vec![
            (
                vec![at("wiki", "https://wiki.example", None)],
                "Invalid reference kind: wiki.",
            ),
            (
                vec![at("logs", "   ", None)],
                "A reference needs its URL or its path.",
            ),
            (
                vec![path_on("logs", "src/shop/logs")],
                "src/shop/logs is neither an http(s) URL nor an absolute path",
            ),
            (vec![path_on("logs", "~/logs")], "is neither"),
            (
                vec![at("repository", "git@github.com:me/shop.git", None)],
                "is neither",
            ),
            (
                vec![at("repository", "ssh://git@github.com/me/shop", None)],
                "is neither",
            ),
            (vec![path_on("logs", r"\\?\C:\logs")], "is neither"),
            (vec![path_on("logs", "C:logs")], "is neither"),
            (
                vec![at(
                    "dashboard",
                    "https://admin:secret@grafana.example/",
                    None,
                )],
                "Credentials in the URL are not supported",
            ),
            (
                vec![at("dashboard", "http://", None)],
                "The reference URL is not valid",
            ),
            (
                vec![at("dashboard", "https://grafana.example/\nd", None)],
                "control characters",
            ),
            (
                vec![at(
                    "dashboard",
                    &format!("https://e.example/{}", "a".repeat(2048)),
                    None,
                )],
                "The reference URL is longer than 2048",
            ),
            (
                vec![path_on("logs", &format!("/{}", "a".repeat(2048)))],
                "The reference path is longer than 2048",
            ),
            (
                vec![path_on("repository", "/srv/git/shop")],
                "A repository is the web address of the code",
            ),
            (
                vec![path_on("dashboard", "/srv/grafana")],
                "A dashboard is a web address",
            ),
            (
                vec![at("local_copy", "https://github.com/me/shop", None)],
                "A local copy is a folder on a workstation",
            ),
            (
                vec![at("logs", "/var/log/shop", None)],
                "Say which machine /var/log/shop is on",
            ),
            (
                vec![at("logs", "https://logs.example", Some(&laptop.id))],
                "https://logs.example is a URL, which is on no machine",
            ),
            (
                vec![at("logs", "/var/log/shop", Some("ghost"))],
                "Resource ghost does not exist",
            ),
            (
                vec![at("local_copy", "/srv/shop", Some(&host.id))],
                "Host is a server, not a workstation",
            ),
            (
                vec![
                    path_on("local_copy", "/home/me/shop"),
                    path_on("local_copy", " /home/me/shop "),
                ],
                "The same local copy /home/me/shop is listed twice",
            ),
            (vec![kept(&shop.references[0]), twice], "appears twice"),
            (vec![stolen], "belongs to another system"),
            (vec![labelled], "Reference label is longer than 120"),
        ];
        for (references, message) in attempts {
            let error = save_application(&path, edited(&shop, references)).unwrap_err();
            assert_eq!(error.code, "invalid_input");
            assert!(error.message.contains(message), "{}", error.message);
        }
        // Nothing was written, neither the references nor the rest of the application.
        let after = list(&path).unwrap();
        assert_eq!(after.applications, before.applications);
        assert_eq!(after.changes, before.changes);
        // The same path on two machines, or under two kinds, is two references.
        let desktop = machine(&path, "Desktop", "workstation");
        let both = save_application(
            &path,
            edited(
                &shop,
                vec![
                    path_on("local_copy", "/home/me/shop"),
                    at("local_copy", "/home/me/shop", Some(&desktop.id)),
                    path_on("documentation", "/home/me/shop"),
                ],
            ),
        )
        .unwrap();
        assert_eq!(both.references.len(), 3);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn says_which_systems_share_a_repository_by_its_exact_url() {
        let path = path();
        let repository = |target: &str| at("repository", target, None);
        let monorepo = "https://github.com/me/platform";
        let api = save_application(&path, application("API", vec![repository(monorepo)])).unwrap();
        let web = save_application(
            &path,
            application(
                "Web",
                vec![at("documentation", monorepo, None), repository(monorepo)],
            ),
        )
        .unwrap();
        // Not normalized: another spelling of the same place is another repository.
        save_application(
            &path,
            application("Docs", vec![repository(&format!("{monorepo}/"))]),
        )
        .unwrap();
        save_application(
            &path,
            application("Blog", vec![repository("https://github.com/me/blog")]),
        )
        .unwrap();
        let host = save_resource(
            &path,
            ResourceInput {
                id: None,
                name: "Host".into(),
                kind: "server".into(),
                description: String::new(),
                operating_system: String::new(),
                references: vec![repository(monorepo)],
            },
        )
        .unwrap();
        let shared = list(&path).unwrap().shared_repositories;
        assert_eq!(
            shared,
            [SharedRepository {
                target: monorepo.into(),
                systems: vec![
                    Subject::Application { id: api.id },
                    Subject::Application { id: web.id },
                    Subject::Resource { id: host.id },
                ],
            }]
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_resource_holding_local_copies_of_other_systems_stays_a_workstation() {
        let path = path();
        let laptop = machine(&path, "Laptop", "workstation");
        let shop = save_application(
            &path,
            application(
                "Shop",
                vec![at("local_copy", r"C:\src\shop", Some(&laptop.id))],
            ),
        )
        .unwrap();
        let as_server = |references: Vec<ReferenceInput>| ResourceInput {
            id: Some(laptop.id.clone()),
            name: "Laptop".into(),
            kind: "server".into(),
            description: String::new(),
            operating_system: String::new(),
            references,
        };
        let error = save_resource(&path, as_server(vec![])).unwrap_err();
        assert_eq!(
            error.message,
            r"Laptop holds local copies of other systems: Shop (local copy C:\src\shop). A local copy is on a workstation, so move them to another workstation, or remove them, before making Laptop a server."
        );
        // Its own local copies are checked against the kind it is about to have.
        save_application(&path, edited(&shop, vec![])).unwrap();
        let error = save_resource(
            &path,
            as_server(vec![at(
                "local_copy",
                "/home/me/dotfiles",
                Some(&laptop.id),
            )]),
        )
        .unwrap_err();
        assert!(
            error
                .message
                .contains("Laptop is a server, not a workstation"),
            "{}",
            error.message
        );
        assert_eq!(list(&path).unwrap().resources[0].kind, "workstation");
        // Its logs can stay on it whatever it is.
        let server = save_resource(
            &path,
            as_server(vec![at("logs", "/var/log/syslog", Some(&laptop.id))]),
        )
        .unwrap();
        assert_eq!(server.kind, "server");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_resource_holds_its_own_paths_and_goes_with_them_but_not_with_others() {
        let path = path();
        // A path on itself, from the save that creates it.
        let id = Uuid::new_v4().to_string();
        let vps = save_resource(
            &path,
            ResourceInput {
                id: Some(id.clone()),
                name: "VPS".into(),
                kind: "server".into(),
                description: String::new(),
                operating_system: "Debian 13".into(),
                references: vec![at("logs", "/var/log/syslog", Some(&id))],
            },
        )
        .unwrap();
        assert_eq!(vps.references[0].machine_id.as_deref(), Some(id.as_str()));
        let shop = save_application(
            &path,
            application("Shop", vec![at("logs", "/var/log/shop", Some(&id))]),
        )
        .unwrap();
        let blog = save_application(
            &path,
            application(
                "Blog",
                vec![at("documentation", "/srv/blog/README.md", Some(&id))],
            ),
        )
        .unwrap();
        // Another system's paths keep it: removing them would change that system unlogged.
        let error = delete_resource(&path, &id).unwrap_err();
        assert_eq!(
            error.message,
            "VPS holds paths of other systems: Blog (documentation /srv/blog/README.md) and Shop (logs /var/log/shop). Remove those references, or put them on another machine, first."
        );
        // The schema refuses it on its own as well.
        let db = crate::inventory::open(&path).unwrap();
        assert!(db
            .execute("DELETE FROM resources WHERE id = ?1", [&id])
            .is_err());
        drop(db);
        save_application(&path, edited(&shop, vec![])).unwrap();
        save_application(&path, edited(&blog, vec![])).unwrap();
        // Its own paths do not: they go first, before the machine's RESTRICT can refuse the
        // subject's CASCADE.
        delete_resource(&path, &id).unwrap();
        assert!(rows(&path).is_empty());
        assert!(list(&path).unwrap().resources.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    /// An entry as (action, summary, its references field before and after, if it has one).
    type Logged = (String, String, Option<(Value, Value)>);

    /// The log's entries, oldest first.
    fn logged(snapshot: &Inventory) -> Vec<Logged> {
        snapshot
            .changes
            .iter()
            .rev()
            .map(|change| {
                let field = change.changes.as_ref().and_then(|fields| {
                    fields
                        .iter()
                        .find(|item| item.field == "references")
                        .map(|item| (item.before.clone(), item.after.clone()))
                });
                (change.action.clone(), change.summary.clone(), field)
            })
            .collect()
    }

    #[test]
    fn logs_references_as_a_field_of_their_system() {
        let path = path();
        let laptop = machine(&path, "Laptop", "workstation");
        let mut clone = at("local_copy", r"C:\src\shop", Some(&laptop.id));
        clone.label = Some("Work".into());
        let shop = save_application(
            &path,
            application(
                "Shop",
                vec![at("repository", "https://github.com/me/shop", None), clone],
            ),
        )
        .unwrap();
        let mut relabelled = kept(&shop.references[1]);
        relabelled.label = None;
        save_application(
            &path,
            edited(&shop, vec![kept(&shop.references[0]), relabelled]),
        )
        .unwrap();
        // Saved unchanged, nothing to log.
        let current = list(&path).unwrap().applications.remove(0);
        save_application(
            &path,
            edited(&current, current.references.iter().map(kept).collect()),
        )
        .unwrap();
        // Removed and added again: other references, the same text.
        save_application(
            &path,
            edited(
                &current,
                current
                    .references
                    .iter()
                    .map(|item| ReferenceInput {
                        id: None,
                        ..kept(item)
                    })
                    .collect(),
            ),
        )
        .unwrap();
        delete_application(&path, &shop.id).unwrap();
        let snapshot = list(&path).unwrap();
        let entries = logged(&snapshot);
        let ours = &entries[1..];
        assert_eq!(ours.len(), 4, "{entries:#?}");
        assert_eq!(
            ours.iter()
                .map(|(action, summary, _)| (action.as_str(), summary.as_str()))
                .collect::<Vec<_>>(),
            [
                (
                    "created",
                    r"Added application «Shop» (production); references: repository https://github.com/me/shop, local copy «Work» C:\src\shop on Laptop"
                ),
                (
                    "updated",
                    r"Updated application «Shop»: references repository https://github.com/me/shop, local copy «Work» C:\src\shop on Laptop → repository https://github.com/me/shop, local copy C:\src\shop on Laptop"
                ),
                (
                    "updated",
                    r"Updated application «Shop»: references replaced: repository https://github.com/me/shop, local copy C:\src\shop on Laptop"
                ),
                (
                    "deleted",
                    "Deleted application «Shop», which had no history"
                ),
            ]
        );
        // Field by field, as the window knows the references: created with them, deleted with
        // the last ones it had.
        let (before, after) = ours[0].2.clone().unwrap();
        assert_eq!(before, Value::Null);
        assert_eq!(after, json!(shop.references));
        let (before, after) = ours[3].2.clone().unwrap();
        assert_eq!(after, Value::Null);
        assert_eq!(before[1]["machineId"], laptop.id.as_str());
        // A resource's references are logged the same way.
        let vps = save_resource(
            &path,
            ResourceInput {
                id: None,
                name: "VPS".into(),
                kind: "server".into(),
                description: String::new(),
                operating_system: String::new(),
                references: vec![at("dashboard", "https://grafana.example/d/vps", None)],
            },
        )
        .unwrap();
        let entry = list(&path).unwrap().changes.remove(0);
        assert_eq!(
            entry.summary,
            "Added resource «VPS» (server); references: dashboard https://grafana.example/d/vps"
        );
        assert_eq!(
            entry
                .changes
                .unwrap()
                .into_iter()
                .find(|item| item.field == "references")
                .unwrap()
                .after,
            json!(vps.references)
        );
        std::fs::remove_file(path).unwrap();
    }
}
