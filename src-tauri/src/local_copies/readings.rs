//! Readings of local copies as DevHQ keeps them (`copy_readings`, v12), what it derives from them
//! at each read of the inventory, and the use case that reads copies when asked.
//!
//! A reading is evidence of its own, never health: it enters neither the attention list nor
//! health coverage, and it has no validity, because it never claimed to stay true. It says what
//! the copy held when it was read, and its moment is always shown. It copies the reference, the
//! machine and the path it read, so editing or removing the reference never rewrites it, and it
//! makes its system's history: a system with readings is archived, not deleted.

use super::{remotes, Facts, Reader, Remote, DEADLINE};
use crate::inventory::{self, open, ApiError, Application, Resource};
use crate::records::{self, now_ms, time_at, Actor, Subject};
use crate::workstation::{Computer, ThisWorkstation};
use rusqlite::types::Type;
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction, TransactionBehavior};
use serde::Serialize;
use std::path::Path;
use uuid::Uuid;

/// Readings a page of a system's history carries.
pub(crate) const READINGS_PAGE: i64 = 200;

/// What DevHQ read in a local copy, at one moment, as it keeps it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Deserialize))]
#[serde(rename_all = "camelCase")]
pub struct CopyReading {
    pub id: String,
    pub subject: Subject,
    /// The `local_copy` reference read, and where it pointed then: copied, never linked.
    pub reference_id: String,
    pub machine_id: String,
    pub path: String,
    /// Always `read`: DevHQ read it itself.
    pub source: String,
    /// The library and its version («libgit2 1.9.7»).
    pub reader: String,
    /// The agent that asked for it through the gate; None when the person asked in the window.
    pub agent: Option<String>,
    pub observed_at: String,
    pub recorded_at: String,
    /// `read`, `missing`, `not_a_copy` or `unreadable`.
    pub outcome: String,
    /// Why it was not read; only then.
    pub detail: Option<String>,
    /// The branch HEAD was on (None: detached).
    pub branch: Option<String>,
    /// HEAD's commit (None: no commit yet).
    pub head: Option<String>,
    /// The newest commit date of HEAD and the local branches, as the commits say.
    pub last_commit_at: Option<String>,
    /// Commits on HEAD, and on the other local branches, that no remote branch of the copy held;
    /// 1001 means more than 1000.
    pub unpublished_on_head: Option<i64>,
    pub unpublished_elsewhere: Option<i64>,
    pub stashes: Option<i64>,
    /// When the copy last talked to a remote, by its own files: DevHQ never fetches.
    pub synced_at: Option<String>,
    pub remotes: Option<Vec<Remote>>,
    /// An operation left halfway: merge, rebase, cherry_pick, revert, bisect or apply_mailbox.
    pub operation: Option<String>,
    /// The working tree: tracked files changed, new entries and files DevHQ could not verify…
    pub changed: Option<i64>,
    pub untracked: Option<i64>,
    pub unverifiable: Option<i64>,
    /// …or why it is unknown.
    pub worktree_unknown: Option<String>,
}

const SELECT: &str = "SELECT id, application_id, resource_id, reference_id, machine_id, path, source, reader, agent, observed_at, recorded_at, outcome, detail, branch, head, last_commit_at, unpublished_on_head, unpublished_elsewhere, stashes, synced_at, remotes, operation, changed, untracked, unverifiable, worktree_unknown FROM copy_readings";

fn optional_time(row: &Row, index: usize) -> rusqlite::Result<Option<String>> {
    match row.get::<_, Option<i64>>(index)? {
        Some(_) => time_at(row, index).map(Some),
        None => Ok(None),
    }
}

fn from_row(row: &Row) -> rusqlite::Result<CopyReading> {
    let remotes = match row.get::<_, Option<String>>(20)? {
        Some(text) => Some(serde_json::from_str(&text).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(20, Type::Text, Box::new(error))
        })?),
        None => None,
    };
    Ok(CopyReading {
        id: row.get(0)?,
        subject: Subject::from_columns(row.get(1)?, row.get(2)?)?,
        reference_id: row.get(3)?,
        machine_id: row.get(4)?,
        path: row.get(5)?,
        source: row.get(6)?,
        reader: row.get(7)?,
        agent: row.get(8)?,
        observed_at: time_at(row, 9)?,
        recorded_at: time_at(row, 10)?,
        outcome: row.get(11)?,
        detail: row.get(12)?,
        branch: row.get(13)?,
        head: row.get(14)?,
        last_commit_at: optional_time(row, 15)?,
        unpublished_on_head: row.get(16)?,
        unpublished_elsewhere: row.get(17)?,
        stashes: row.get(18)?,
        synced_at: optional_time(row, 19)?,
        remotes,
        operation: row.get(21)?,
        changed: row.get(22)?,
        untracked: row.get(23)?,
        unverifiable: row.get(24)?,
        worktree_unknown: row.get(25)?,
    })
}

/// The newest reading of a reference, through its index: what every read of the inventory
/// carries for each declared copy.
pub(crate) const LATEST: &str = "WHERE reference_id = ?1
     ORDER BY observed_at DESC, recorded_at DESC, rowid DESC LIMIT 1";

/// A system's readings older than a cursor, newest first, through its subject's index (see
/// `Subject::per_type`). The plain bound on observed_at lets the index find the start; the row value
/// breaks ties exactly.
pub(crate) const OLDER: [&str; 2] = [
    "WHERE application_id = ?1 AND observed_at <= ?2
       AND (observed_at, recorded_at, rowid) < (?2, ?3, ?4)
     ORDER BY observed_at DESC, recorded_at DESC, rowid DESC LIMIT ?5",
    "WHERE resource_id = ?1 AND observed_at <= ?2
       AND (observed_at, recorded_at, rowid) < (?2, ?3, ?4)
     ORDER BY observed_at DESC, recorded_at DESC, rowid DESC LIMIT ?5",
];

/// A local copy a system declares now, with its latest reading and what DevHQ derives from it at
/// this read of the inventory. Never stored.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalCopy {
    pub reference_id: String,
    pub subject: Subject,
    /// Where the reference says the copy is now.
    pub machine_id: String,
    pub path: String,
    /// Its newest reading, wherever it was read; None if it was never read.
    pub latest: Option<CopyReading>,
    /// The newest reading was made at another path or on another machine than the reference
    /// declares now.
    pub moved_since_read: bool,
    /// How the newest reading's remotes compare with the repositories the system declares, both
    /// normalized (see `remotes::location`): `matches`, `differs`, or `undeclared` when the system
    /// declares none and the copy had remotes. None when there is nothing to compare.
    pub repository: Option<String>,
    /// When it was read, the copy held work that existed only there: commits on no remote,
    /// stashes, changes, new entries, files DevHQ could not verify, or an operation left halfway.
    pub local_work: bool,
}

/// Whether a reading found work that existed only in its copy.
fn local_work(reading: &CopyReading) -> bool {
    let some = |count: Option<i64>| count.is_some_and(|count| count > 0);
    reading.outcome == "read"
        && (some(reading.unpublished_on_head)
            || some(reading.unpublished_elsewhere)
            || some(reading.stashes)
            || some(reading.changed)
            || some(reading.untracked)
            || some(reading.unverifiable)
            || reading.operation.is_some())
}

/// Each local copy the systems declare, in the inventory's order (applications, then resources,
/// each in the order of its references), with its newest reading, read inside the caller's
/// transaction.
pub(crate) fn local_copies(
    tx: &Transaction,
    applications: &[Application],
    resources: &[Resource],
) -> Result<Vec<LocalCopy>, ApiError> {
    let mut latest = tx
        .prepare_cached(&format!("{SELECT} {LATEST}"))
        .map_err(ApiError::database)?;
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
    let mut copies = Vec::new();
    for (subject, references) in systems {
        let declared: Vec<(String, String)> = references
            .iter()
            .filter(|reference| reference.kind == "repository")
            .filter_map(|reference| remotes::location(&reference.target))
            .collect();
        for reference in references.iter().filter(|item| item.kind == "local_copy") {
            let Some(machine_id) = reference.machine_id.clone() else {
                continue;
            };
            let reading = latest
                .query_row([&reference.id], from_row)
                .optional()
                .map_err(ApiError::database)?;
            let moved_since_read = reading.as_ref().is_some_and(|reading| {
                reading.path != reference.target || reading.machine_id != machine_id
            });
            let repository = reading
                .as_ref()
                .and_then(|reading| repository_match(reading, &declared));
            copies.push(LocalCopy {
                reference_id: reference.id.clone(),
                subject: subject.clone(),
                machine_id,
                path: reference.target.clone(),
                local_work: reading.as_ref().is_some_and(local_work),
                latest: reading,
                moved_since_read,
                repository,
            });
        }
    }
    Ok(copies)
}

/// Whether any of the reading's remotes points where a declared repository does.
fn repository_match(reading: &CopyReading, declared: &[(String, String)]) -> Option<String> {
    let found: Vec<(String, String)> = reading
        .remotes
        .iter()
        .flatten()
        .filter_map(|remote| remote.url.as_deref().and_then(remotes::location))
        .collect();
    if found.is_empty() {
        return None;
    }
    let verdict = if declared.is_empty() {
        "undeclared"
    } else if found.iter().any(|location| declared.contains(location)) {
        "matches"
    } else {
        "differs"
    };
    Some(verdict.to_owned())
}

/// A page of a system's readings.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadingPage {
    pub readings: Vec<CopyReading>,
    /// Older readings remain after this page.
    pub more: bool,
}

/// The subject's readings older than `before`, one of its readings already shown, or its newest
/// ones when `before` is None: newest first, at most `READINGS_PAGE`. Anchoring on a shown reading,
/// rather than counting, keeps pages exact while new readings arrive.
pub fn older_readings(
    path: &Path,
    subject: &Subject,
    before: Option<&str>,
) -> Result<ReadingPage, ApiError> {
    let mut db = open(path)?;
    let tx = db.transaction().map_err(ApiError::database)?;
    records::ensure_subject(&tx, subject)?;
    let (application_id, resource_id) = subject.columns();
    let cursor = match before {
        None => (i64::MAX, i64::MAX, i64::MAX),
        Some(before) => tx
            .query_row(
                "SELECT observed_at, recorded_at, rowid FROM copy_readings
                 WHERE id = ?1 AND application_id IS ?2 AND resource_id IS ?3",
                params![before, application_id, resource_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(ApiError::database)?
            .ok_or_else(|| {
                ApiError::invalid(format!(
                    "Reading {before} is not one of this {}'s.",
                    subject.label()
                ))
            })?,
    };
    let (kind, id) = subject.per_type();
    let mut readings = {
        let mut statement = tx
            .prepare(&format!("{SELECT} {}", OLDER[kind]))
            .map_err(ApiError::database)?;
        let rows = statement
            .query_map(
                params![id, cursor.0, cursor.1, cursor.2, READINGS_PAGE + 1],
                from_row,
            )
            .map_err(ApiError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(ApiError::database)?;
        rows
    };
    let limit = usize::try_from(READINGS_PAGE).map_err(ApiError::database)?;
    let more = readings.len() > limit;
    readings.truncate(limit);
    tx.commit().map_err(ApiError::database)?;
    Ok(ReadingPage { readings, more })
}

/// A local copy DevHQ was asked to read: which reference, of which system, and where.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    pub reference_id: String,
    pub subject: Subject,
    pub machine_id: String,
    pub path: String,
}

/// Keeps what a reader read in `target`, inside one write transaction: only while the reference
/// still points where it did when the reading began (a copy removed or moved meanwhile records
/// nothing, as a check removed in flight), and only while `authorize` lets this write happen,
/// checked with SQLite's write lock held.
pub(crate) fn store(
    path: &Path,
    target: &Target,
    facts: &Facts,
    observed_at: i64,
    by: &Actor,
    authorize: &dyn Fn(&Connection) -> Result<(), ApiError>,
) -> Result<CopyReading, ApiError> {
    let mut db = open(path)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    authorize(&tx)?;
    let (application_id, resource_id) = target.subject.columns();
    let unchanged: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM system_references WHERE id = ?1 AND kind = 'local_copy'
               AND application_id IS ?2 AND resource_id IS ?3 AND machine_id = ?4 AND target = ?5)",
            params![
                target.reference_id,
                application_id,
                resource_id,
                target.machine_id,
                target.path
            ],
            |row| row.get(0),
        )
        .map_err(ApiError::database)?;
    if !unchanged {
        return Err(ApiError::cancelled(format!(
            "The local copy {} was removed or moved while DevHQ read it. Nothing was kept.",
            target.path
        )));
    }
    let refs = &facts.refs;
    let worktree = facts.worktree.clone().unwrap_or_default();
    let remotes = refs
        .remotes
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(ApiError::database)?;
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO copy_readings (id, application_id, resource_id, reference_id, machine_id, path, source, reader, agent,
             observed_at, recorded_at, outcome, detail, branch, head, last_commit_at, unpublished_on_head,
             unpublished_elsewhere, stashes, synced_at, remotes, operation, changed, untracked, unverifiable, worktree_unknown)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'read', ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
        params![
            id,
            application_id,
            resource_id,
            target.reference_id,
            target.machine_id,
            target.path,
            super::reader_name(),
            by.name(),
            observed_at,
            now_ms(),
            refs.outcome,
            refs.detail,
            refs.branch,
            refs.head,
            refs.last_commit_at,
            refs.unpublished_on_head,
            refs.unpublished_elsewhere,
            refs.stashes,
            refs.synced_at,
            remotes,
            refs.operation,
            worktree.changed,
            worktree.untracked,
            worktree.unverifiable,
            worktree.unknown,
        ],
    )
    .map_err(ApiError::database)?;
    let reading = tx
        .query_row(&format!("{SELECT} WHERE id = ?1"), [&id], from_row)
        .map_err(ApiError::database)?;
    tx.commit().map_err(ApiError::database)?;
    Ok(reading)
}

/// Which copies to read: every one of this workstation (but those of archived applications), the
/// ones of one system, or the references named.
#[derive(Debug, Clone)]
pub(crate) enum Which {
    ThisWorkstation,
    System(Subject),
    References(Vec<String>),
}

/// A copy DevHQ could not read: a failure of DevHQ, kept nowhere.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReadFailure {
    pub reference_id: String,
    pub subject: Subject,
    pub path: String,
    pub message: String,
}

/// What reading copies did: the readings kept, the copies DevHQ could not read, and the
/// workstation it read them on.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReadOutcome {
    pub readings: Vec<CopyReading>,
    pub failures: Vec<ReadFailure>,
    pub this_workstation: ThisWorkstation,
}

/// Reads the copies `which` names, one after another, each in a reader of its own, and keeps
/// each reading as `by`'s request. Only copies on this workstation are read: another machine's are
/// read by DevHQ on that machine. Without a known workstation nothing is read.
pub(crate) fn read_copies(
    database: &Path,
    computer: &Computer,
    reader: &Reader,
    which: &Which,
    by: &Actor,
    authorize: &dyn Fn(&Connection) -> Result<(), ApiError>,
) -> Result<ReadOutcome, ApiError> {
    let snapshot = inventory::list(database)?;
    let this_workstation = computer.resolve(&snapshot.resources);
    let Some(machine) = this_workstation.resource_id.clone() else {
        return Err(ApiError::invalid(format!(
            "DevHQ does not know which workstation this computer is, so it reads no local copy: {}",
            this_workstation.message.as_deref().unwrap_or_default()
        )));
    };
    let machine_name = |id: &str| {
        snapshot
            .resources
            .iter()
            .find(|resource| resource.id == id)
            .map_or_else(
                || "another machine".to_owned(),
                |resource| resource.name.clone(),
            )
    };
    let archived = |subject: &Subject| match subject {
        Subject::Application { id } => snapshot
            .applications
            .iter()
            .any(|item| &item.id == id && item.lifecycle == "archived"),
        Subject::Resource { .. } => false,
    };
    let declared: Vec<Target> = snapshot
        .local_copies
        .iter()
        .map(|copy| Target {
            reference_id: copy.reference_id.clone(),
            subject: copy.subject.clone(),
            machine_id: copy.machine_id.clone(),
            path: copy.path.clone(),
        })
        .collect();
    let targets: Vec<Target> = match which {
        Which::ThisWorkstation => declared
            .into_iter()
            .filter(|target| target.machine_id == machine && !archived(&target.subject))
            .collect(),
        Which::System(subject) => {
            let exists = match subject {
                Subject::Application { id } => {
                    snapshot.applications.iter().any(|item| &item.id == id)
                }
                Subject::Resource { id } => snapshot.resources.iter().any(|item| &item.id == id),
            };
            if !exists {
                return Err(ApiError::invalid(format!(
                    "The {} does not exist.",
                    subject.label()
                )));
            }
            let mine: Vec<Target> = declared
                .into_iter()
                .filter(|target| &target.subject == subject && target.machine_id == machine)
                .collect();
            if mine.is_empty() {
                return Err(ApiError::invalid(format!(
                    "This {} declares no local copy on this computer ({}).",
                    subject.label(),
                    machine_name(&machine)
                )));
            }
            mine
        }
        Which::References(ids) => {
            let mut chosen = Vec::with_capacity(ids.len());
            for id in ids {
                let Some(target) = declared.iter().find(|target| &target.reference_id == id) else {
                    return Err(ApiError::invalid(format!(
                        "Reference {id} is not a local copy any system declares."
                    )));
                };
                if target.machine_id != machine {
                    return Err(ApiError::invalid(format!(
                        "{} is on {}, not on this computer: DevHQ on that machine reads it.",
                        target.path,
                        machine_name(&target.machine_id)
                    )));
                }
                if !chosen.iter().any(|item: &Target| &item.reference_id == id) {
                    chosen.push(target.clone());
                }
            }
            chosen
        }
    };
    let mut readings = Vec::new();
    let mut failures = Vec::new();
    for target in targets {
        let observed_at = now_ms();
        let result = reader
            .read(&target.path, DEADLINE)
            .and_then(|facts| store(database, &target, &facts, observed_at, by, authorize));
        match result {
            Ok(reading) => readings.push(reading),
            // The gate closed meanwhile: nothing more may be written.
            Err(error) if error.code == "access_denied" => return Err(error),
            Err(error) => failures.push(ReadFailure {
                reference_id: target.reference_id,
                subject: target.subject,
                path: target.path,
                message: error.message,
            }),
        }
    }
    Ok(ReadOutcome {
        readings,
        failures,
        this_workstation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{
        delete_application, list, save_application, save_resource, ApplicationInput, ResourceInput,
    };
    use crate::local_copies::tests::{folder, git, path_text, test_reader};
    use crate::references::{Reference, ReferenceInput};
    use std::path::PathBuf;

    /// An inventory in a folder of its own, a computer whose local folder is beside it, and the
    /// confined reader of the test executable.
    struct World {
        folder: PathBuf,
        database: PathBuf,
        computer: Computer,
        reader: Reader,
    }

    impl World {
        fn new(name: &str) -> Self {
            let folder = folder(name);
            Self {
                database: folder.join("inventory.sqlite3"),
                computer: Computer {
                    local: folder.join("local"),
                    fingerprint: Some("readings-test".into()),
                },
                reader: test_reader("confined", &[]),
                folder,
            }
        }

        fn workstation(&self, name: &str) -> String {
            save_resource(
                &self.database,
                ResourceInput {
                    id: None,
                    name: name.into(),
                    kind: "workstation".into(),
                    description: String::new(),
                    operating_system: String::new(),
                    references: vec![],
                },
            )
            .unwrap()
            .id
        }

        fn choose(&self, id: &str) {
            let resources = list(&self.database).unwrap().resources;
            self.computer.choose(&resources, Some(id)).unwrap();
        }

        fn read(&self, which: &Which) -> Result<ReadOutcome, ApiError> {
            read_copies(
                &self.database,
                &self.computer,
                &self.reader,
                which,
                &Actor::Person,
                &|_| Ok(()),
            )
        }

        /// A repository at `name` in the world's folder, whose origin is `remote`.
        fn repository(&self, name: &str, remote: &str) -> PathBuf {
            let dir = self.folder.join(name);
            std::fs::create_dir(&dir).unwrap();
            git(&dir, &["init", "-q", "-b", "main"]);
            std::fs::write(dir.join("a.txt"), "a\n").unwrap();
            git(&dir, &["add", "-A"]);
            git(&dir, &["commit", "-q", "-m", "a"]);
            git(&dir, &["remote", "add", "origin", remote]);
            dir
        }
    }

    fn copy_at(path: &Path, machine: &str) -> ReferenceInput {
        ReferenceInput {
            id: None,
            kind: "local_copy".into(),
            target: path_text(path),
            label: None,
            machine_id: Some(machine.into()),
        }
    }

    fn repository(url: &str) -> ReferenceInput {
        ReferenceInput {
            id: None,
            kind: "repository".into(),
            target: url.into(),
            label: None,
            machine_id: None,
        }
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

    fn kept(references: &[Reference]) -> Vec<ReferenceInput> {
        references.iter().map(ReferenceInput::from).collect()
    }

    #[test]
    fn keeps_each_reading_as_history_whatever_happens_to_its_reference() {
        let world = World::new("readings-history");
        let laptop = world.workstation("Laptop");
        let desktop = world.workstation("Desktop");
        let shop_copy = world.repository("shop", "git@github.com:me/shop.git");
        let blog_copy = world.repository("blog", "git@github.com:me/blog.git");
        let shop = save_application(
            &world.database,
            application(
                "Shop",
                vec![
                    repository("https://github.com/me/shop"),
                    copy_at(&shop_copy, &laptop),
                    copy_at(Path::new("/elsewhere/shop"), &desktop),
                ],
            ),
        )
        .unwrap();
        // Its remote is not the repository it declares; another declares none.
        let blog = save_application(
            &world.database,
            application(
                "Blog",
                vec![
                    repository("https://gitlab.com/me/blog"),
                    copy_at(&blog_copy, &laptop),
                ],
            ),
        )
        .unwrap();
        let docs = save_application(
            &world.database,
            application("Docs", vec![copy_at(&blog_copy, &laptop)]),
        )
        .unwrap();

        // Nothing is read until DevHQ knows which workstation this computer is.
        let error = world.read(&Which::ThisWorkstation).unwrap_err();
        assert!(
            error.message.contains("does not know which workstation"),
            "{}",
            error.message
        );
        world.choose(&laptop);
        let read = world.read(&Which::ThisWorkstation).unwrap();
        assert!(read.failures.is_empty(), "{:?}", read.failures);
        // Only this workstation's copies: the desktop's is read by DevHQ on the desktop.
        assert_eq!(read.readings.len(), 3);
        assert!(read.readings.iter().all(|reading| reading.outcome == "read"
            && reading.source == "read"
            && reading.agent.is_none()
            && reading.reader.starts_with("libgit2 ")));
        let snapshot = list(&world.database).unwrap();
        let derived = |reference: &str| -> LocalCopy {
            snapshot
                .local_copies
                .iter()
                .find(|copy| copy.reference_id == reference)
                .unwrap()
                .clone()
        };
        let shop_on_laptop = derived(&shop.references[1].id);
        assert_eq!(shop_on_laptop.repository.as_deref(), Some("matches"));
        assert!(!shop_on_laptop.moved_since_read);
        // Its only commit is on no remote branch: work that exists only there.
        assert!(shop_on_laptop.local_work);
        assert!(derived(&shop.references[2].id).latest.is_none());
        assert_eq!(
            derived(&blog.references[1].id).repository.as_deref(),
            Some("differs")
        );
        assert_eq!(
            derived(&docs.references[0].id).repository.as_deref(),
            Some("undeclared")
        );

        // History: the system cannot be deleted, only archived.
        let error = delete_application(&world.database, &shop.id).unwrap_err();
        assert!(
            error
                .message
                .contains("has recorded readings of its local copies"),
            "{}",
            error.message
        );
        // Moving the copy keeps its readings and says the last one was made elsewhere.
        let mut moved = kept(&shop.references);
        moved[1].target = path_text(&world.folder.join("moved"));
        save_application(
            &world.database,
            ApplicationInput {
                id: Some(shop.id.clone()),
                references: moved,
                ..application("Shop", vec![])
            },
        )
        .unwrap();
        let moved = list(&world.database).unwrap().local_copies;
        assert!(
            moved
                .iter()
                .find(|copy| copy.reference_id == shop.references[1].id)
                .unwrap()
                .moved_since_read
        );
        // Removing it keeps them too: they are paged with the system.
        save_application(
            &world.database,
            ApplicationInput {
                id: Some(shop.id.clone()),
                references: vec![],
                ..application("Shop", vec![])
            },
        )
        .unwrap();
        let page = older_readings(
            &world.database,
            &Subject::Application {
                id: shop.id.clone(),
            },
            None,
        )
        .unwrap();
        assert_eq!(page.readings.len(), 1);
        assert!(!page.more);

        // An archived application's copies are left out of reading them all, not out of asking.
        save_application(
            &world.database,
            ApplicationInput {
                id: Some(blog.id.clone()),
                lifecycle: "archived".into(),
                references: kept(&blog.references),
                ..application("Blog", vec![])
            },
        )
        .unwrap();
        let read = world.read(&Which::ThisWorkstation).unwrap();
        assert_eq!(
            read.readings
                .iter()
                .map(|reading| reading.subject.clone())
                .collect::<Vec<_>>(),
            [Subject::Application {
                id: docs.id.clone()
            }]
        );
        let asked = world
            .read(&Which::References(vec![blog.references[1].id.clone()]))
            .unwrap();
        assert_eq!(asked.readings.len(), 1);
        std::fs::remove_dir_all(&world.folder).unwrap();
    }

    #[test]
    fn reads_only_local_copies_of_this_workstation_and_says_why_not() {
        let world = World::new("readings-refused");
        let laptop = world.workstation("Laptop");
        let desktop = world.workstation("Desktop");
        let shop = save_application(
            &world.database,
            application(
                "Shop",
                vec![
                    repository("https://github.com/me/shop"),
                    copy_at(Path::new("/srv/shop"), &desktop),
                ],
            ),
        )
        .unwrap();
        world.choose(&laptop);
        for (which, message) in [
            (
                Which::References(vec![shop.references[1].id.clone()]),
                "is on Desktop, not on this computer",
            ),
            (
                Which::References(vec![shop.references[0].id.clone()]),
                "is not a local copy",
            ),
            (
                Which::System(Subject::Application {
                    id: shop.id.clone(),
                }),
                "declares no local copy on this computer (Laptop)",
            ),
            (
                Which::System(Subject::Application { id: "ghost".into() }),
                "does not exist",
            ),
        ] {
            let error = world.read(&which).unwrap_err();
            assert_eq!(error.code, "invalid_input");
            assert!(error.message.contains(message), "{}", error.message);
        }
        // Nothing to read is no error: the list is simply empty.
        let read = world.read(&Which::ThisWorkstation).unwrap();
        assert!(read.readings.is_empty() && read.failures.is_empty());
        assert_eq!(
            read.this_workstation.resource_id.as_deref(),
            Some(laptop.as_str())
        );
        std::fs::remove_dir_all(&world.folder).unwrap();
    }

    /// A credential in a remote's URL reaches neither the reader's answer nor the inventory, in any
    /// form of URL DevHQ keeps: the reading's remotes are cleaned, and the secret is nowhere in the
    /// database's files.
    #[test]
    fn keeps_no_credential_of_a_remote_in_a_reading_or_the_inventory() {
        let world = World::new("readings-credentials");
        let laptop = world.workstation("Laptop");
        let copy = world.repository(
            "shop",
            "file://demo-user:demo-pass@server.example/share/repo",
        );
        for (name, url) in [
            (
                "web",
                "https://web-user:web-pass@github.com/me/shop?private_token=query-pass",
            ),
            ("token", "https://token-pass@github.com/me/shop"),
            ("daemon", "git://daemon-pass@git.example/shop.git"),
            ("unc", r"file://unc-user:unc-pass@server.example\share\repo"),
            ("ssh", "ssh://git:ssh-pass@host.example/shop.git"),
        ] {
            git(&copy, &["remote", "add", name, url]);
        }
        save_application(
            &world.database,
            application("Shop", vec![copy_at(&copy, &laptop)]),
        )
        .unwrap();
        world.choose(&laptop);
        let read = world.read(&Which::ThisWorkstation).unwrap();
        assert!(read.failures.is_empty(), "{:?}", read.failures);
        let mut urls: Vec<(String, Option<String>)> = read.readings[0]
            .remotes
            .clone()
            .unwrap()
            .into_iter()
            .map(|remote| (remote.name, remote.url))
            .collect();
        urls.sort();
        let kept = |name: &str, url: &str| (name.to_owned(), Some(url.to_owned()));
        assert_eq!(
            urls,
            [
                kept("daemon", "git://git.example/shop.git"),
                kept("origin", "file://server.example/share/repo"),
                kept("ssh", "ssh://git@host.example/shop.git"),
                kept("token", "https://github.com/me/shop"),
                kept("unc", r"file://server.example\share\repo"),
                kept("web", "https://github.com/me/shop"),
            ]
        );
        let secrets = [
            "demo-user",
            "demo-pass",
            "web-user",
            "web-pass",
            "query-pass",
            "token-pass",
            "daemon-pass",
            "unc-user",
            "unc-pass",
            "ssh-pass",
        ];
        // What the window and the gate show is what the inventory keeps.
        let shown = serde_json::to_string(&list(&world.database).unwrap()).unwrap();
        // The database's own files, whatever SQLite left in its log.
        let mut stored = Vec::new();
        for suffix in ["", "-wal"] {
            let file = PathBuf::from(format!("{}{suffix}", world.database.display()));
            if let Ok(bytes) = std::fs::read(file) {
                stored.extend(bytes);
            }
        }
        let stored = String::from_utf8_lossy(&stored);
        assert!(stored.contains("server.example/share/repo"));
        for secret in secrets {
            assert!(!shown.contains(secret), "{secret} shown");
            assert!(!stored.contains(secret), "{secret} stored");
        }
        std::fs::remove_dir_all(&world.folder).unwrap();
    }

    #[test]
    fn a_copy_removed_or_moved_while_it_was_read_keeps_nothing() {
        let world = World::new("readings-in-flight");
        let laptop = world.workstation("Laptop");
        let copy = world.repository("shop", "git@github.com:me/shop.git");
        let shop = save_application(
            &world.database,
            application("Shop", vec![copy_at(&copy, &laptop)]),
        )
        .unwrap();
        let target = Target {
            reference_id: shop.references[0].id.clone(),
            subject: Subject::Application {
                id: shop.id.clone(),
            },
            machine_id: laptop.clone(),
            path: path_text(&copy),
        };
        let facts = world.reader.read(&target.path, DEADLINE).unwrap();
        let keep = |target: &Target, authorize: &dyn Fn(&Connection) -> Result<(), ApiError>| {
            store(
                &world.database,
                target,
                &facts,
                now_ms(),
                &Actor::Person,
                authorize,
            )
        };
        // Moved meanwhile: another path, or another machine.
        let elsewhere = Target {
            path: "/moved".into(),
            ..target.clone()
        };
        let other_machine = Target {
            machine_id: "desktop".into(),
            ..target.clone()
        };
        for moved in [&elsewhere, &other_machine] {
            assert_eq!(keep(moved, &|_| Ok(())).unwrap_err().code, "cancelled");
        }
        // Refused by whoever authorizes the write (the gate, once closed): nothing kept.
        let refused = keep(&target, &|_| Err(ApiError::denied("closed")));
        assert_eq!(refused.unwrap_err().code, "access_denied");
        // Removed meanwhile.
        save_application(
            &world.database,
            ApplicationInput {
                id: Some(shop.id.clone()),
                ..application("Shop", vec![])
            },
        )
        .unwrap();
        assert_eq!(keep(&target, &|_| Ok(())).unwrap_err().code, "cancelled");
        let db = open(&world.database).unwrap();
        let rows: i64 = db
            .query_row("SELECT count(*) FROM copy_readings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 0);
        drop(db);
        std::fs::remove_dir_all(&world.folder).unwrap();
    }

    #[test]
    fn the_schema_refuses_readings_that_do_not_hold_together() {
        let world = World::new("readings-schema");
        let db = open(&world.database).unwrap();
        db.execute_batch(
            "INSERT INTO applications (id, name, lifecycle, description) VALUES ('shop', 'Shop', 'production', '')",
        )
        .unwrap();
        // A reading of the shop with `columns` set to `values`, for the schema to judge.
        let insert = |columns: &str, values: &str| {
            db.execute(
                &format!(
                    "INSERT INTO copy_readings (id, application_id, reference_id, machine_id, path, source, reader, observed_at, recorded_at{columns})
                     VALUES ('{}', 'shop', 'copy', 'laptop', '/src/shop', 'read', 'libgit2 1.9.7', 1, 1{values})",
                    Uuid::new_v4()
                ),
                [],
            )
        };
        let facts = ", outcome, unpublished_on_head, unpublished_elsewhere, stashes, remotes";
        let counted = ", changed, untracked, unverifiable";
        let read = format!("{facts}{counted}");
        let read_values = ", 'read', 0, 0, 0, '[]', 0, 0, 0";
        // A full reading, one with its working tree unknown, and one not read.
        for (columns, values) in [
            (read.clone(), read_values.to_owned()),
            (
                format!("{facts}, worktree_unknown"),
                ", 'read', 0, 0, 0, '[]', 'took too long'".to_owned(),
            ),
            (
                ", outcome, detail".to_owned(),
                ", 'missing', 'Nothing is at this path.'".to_owned(),
            ),
        ] {
            assert!(insert(&columns, &values).is_ok(), "{columns}");
        }
        let refused: [(String, String, &str); 14] = [
            (
                ", outcome, detail".into(),
                ", 'gone', 'x'".into(),
                "an unknown outcome",
            ),
            (
                ", outcome".into(),
                ", 'missing'".into(),
                "a copy not read without why",
            ),
            (
                ", outcome, detail, branch".into(),
                ", 'missing', 'x', 'main'".into(),
                "facts of a copy not read",
            ),
            (
                facts.into(),
                ", 'read', 0, 0, 0, '[]'".into(),
                "a working tree neither counted nor unknown",
            ),
            (
                format!("{read}, worktree_unknown"),
                format!("{read_values}, 'x'"),
                "a working tree both counted and unknown",
            ),
            (
                format!("{facts}, changed, untracked"),
                ", 'read', 0, 0, 0, '[]', 0, 0".into(),
                "half a working tree",
            ),
            (
                format!(", outcome, unpublished_elsewhere, stashes, remotes{counted}"),
                ", 'read', 0, 0, '[]', 0, 0, 0".into(),
                "a read copy without its facts",
            ),
            (
                format!("{read}, detail"),
                format!("{read_values}, 'why?'"),
                "a reason for a read copy",
            ),
            (
                format!("{read}, head"),
                format!("{read_values}, 'HEAD'"),
                "a HEAD that is no commit ID",
            ),
            (
                read.clone(),
                ", 'read', 1, 0, 0, '[]', 0, 0, 0".into(),
                "unpublished commits on no HEAD",
            ),
            (
                read.clone(),
                ", 'read', 0, 1002, 0, '[]', 0, 0, 0".into(),
                "more unpublished than counted",
            ),
            (
                read.clone(),
                ", 'read', 0, 0, 0, '{}', 0, 0, 0".into(),
                "remotes that are no list",
            ),
            (
                format!("{read}, operation"),
                format!("{read_values}, 'push'"),
                "an unknown operation",
            ),
            (
                format!("{read}, agent"),
                format!("{read_values}, ' '"),
                "a blank agent",
            ),
        ];
        for (columns, values, why) in refused {
            assert!(insert(&columns, &values).is_err(), "{why}");
        }
        // A reading names exactly one existing system, and keeps it.
        for (application, resource) in [("NULL", "NULL"), ("'ghost'", "NULL"), ("'shop'", "'shop'")]
        {
            assert!(db
                .execute(
                    &format!(
                        "INSERT INTO copy_readings (id, application_id, resource_id, reference_id, machine_id, path, source, reader, observed_at, recorded_at, outcome, detail)
                         VALUES ('{}', {application}, {resource}, 'copy', 'laptop', '/src/shop', 'read', 'libgit2', 1, 1, 'missing', 'x')",
                        Uuid::new_v4()
                    ),
                    [],
                )
                .is_err());
        }
        assert!(db
            .execute("DELETE FROM applications WHERE id = 'shop'", [])
            .is_err());
        drop(db);
        std::fs::remove_dir_all(&world.folder).unwrap();
    }

    #[test]
    fn pages_through_a_systems_readings_without_gaps_or_repeats() {
        let world = World::new("readings-pages");
        let db = open(&world.database).unwrap();
        db.execute_batch(
            "INSERT INTO applications (id, name, lifecycle, description) VALUES ('shop', 'Shop', 'production', ''), ('blog', 'Blog', 'production', '')",
        )
        .unwrap();
        // 450 readings of the shop, three at each instant, and some of the blog between them.
        for index in 0..450 {
            for (subject, id) in [
                ("shop", format!("shop-{index}")),
                ("blog", format!("blog-{index}")),
            ] {
                if subject == "blog" && index % 50 != 0 {
                    continue;
                }
                db.execute(
                    "INSERT INTO copy_readings (id, application_id, reference_id, machine_id, path, source, reader, observed_at, recorded_at, outcome, detail)
                     VALUES (?1, ?2, 'copy', 'laptop', '/src', 'read', 'libgit2', ?3, ?3, 'missing', 'Nothing is at this path.')",
                    params![id, subject, index / 3],
                )
                .unwrap();
            }
        }
        drop(db);
        let shop = Subject::Application { id: "shop".into() };
        let mut seen: Vec<String> = Vec::new();
        let mut before: Option<String> = None;
        loop {
            let page = older_readings(&world.database, &shop, before.as_deref()).unwrap();
            assert!(page.readings.len() <= 200);
            seen.extend(page.readings.iter().map(|reading| reading.id.clone()));
            before = page.readings.last().map(|reading| reading.id.clone());
            if !page.more {
                break;
            }
        }
        let expected: Vec<String> = (0..450)
            .rev()
            .map(|index| format!("shop-{index}"))
            .collect();
        assert_eq!(seen, expected);
        let error = older_readings(&world.database, &shop, Some("blog-0")).unwrap_err();
        assert!(
            error.message.contains("is not one of this application's"),
            "{}",
            error.message
        );
        std::fs::remove_dir_all(&world.folder).unwrap();
    }

    #[test]
    fn every_query_of_readings_follows_an_index() {
        let world = World::new("readings-plans");
        let db = open(&world.database).unwrap();
        let older = OLDER.map(|filter| format!("{SELECT} {filter}"));
        let latest = format!("{SELECT} {LATEST}");
        let cases: [(&str, &[&dyn rusqlite::ToSql], &str); 5] = [
            (&latest, &[&"copy"], "copy_readings_reference_id"),
            (
                &older[0],
                &[&"shop", &9, &9, &9, &1],
                "copy_readings_application_id",
            ),
            (
                &older[1],
                &[&"host", &9, &9, &9, &1],
                "copy_readings_resource_id",
            ),
            // What `history_of` asks before a system is deleted.
            (
                "SELECT EXISTS(SELECT 1 FROM copy_readings WHERE application_id = ?1)",
                &[&"shop"],
                "copy_readings_application_id",
            ),
            (
                "SELECT EXISTS(SELECT 1 FROM copy_readings WHERE resource_id = ?1)",
                &[&"host"],
                "copy_readings_resource_id",
            ),
        ];
        for (sql, params, index) in cases {
            let steps: Vec<String> = db
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap()
                .query_map(params, |row| row.get(3))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert!(
                steps.iter().any(|step| step.contains(index)),
                "{sql}\n{steps:#?}"
            );
            assert!(
                steps
                    .iter()
                    .all(|step| !step.starts_with("SCAN copy_readings")),
                "{sql}\n{steps:#?}"
            );
        }
        drop(db);
        std::fs::remove_dir_all(&world.folder).unwrap();
    }
}
