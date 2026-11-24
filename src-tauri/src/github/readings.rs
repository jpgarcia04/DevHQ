//! Readings of GitHub repositories as DevHQ keeps them (`github_readings`, v15), the repositories
//! every read of the inventory carries, and the use case that reads them when asked.
//!
//! Like a reading of a local copy, a reading is evidence of its own, never health: it enters neither
//! the attention list nor coverage, and it has no validity. It says what GitHub answered at one
//! moment, which is always shown. It copies the reference and the address it read, so editing or
//! removing the reference never rewrites it, and it makes its system's history.

use super::{read, repository, Facts, Fetch, PullRequest, Workflow, READER};
use crate::inventory::{self, open_current, ApiError, Application, Resource};
use crate::records::{now_ms, time_at, Actor, Subject};
use rusqlite::{params, Row, Transaction};
use serde::Serialize;
use std::path::Path;
use uuid::Uuid;

/// What DevHQ read of a repository on GitHub, at one moment, as it keeps it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Deserialize))]
#[serde(rename_all = "camelCase")]
pub struct GithubReading {
    pub id: String,
    pub subject: Subject,
    /// The `repository` reference read, and the address it named then: copied, never linked.
    pub reference_id: String,
    pub repository: String,
    /// Always `github`.
    pub source: String,
    /// DevHQ's reader and the API version it asked for.
    pub reader: String,
    /// The agent that asked for it through the gate; None when the person asked or the window
    /// opened. The gate cannot ask today: it never reaches the token.
    pub agent: Option<String>,
    pub observed_at: String,
    pub recorded_at: String,
    /// `read`, `unreadable` or `refused`.
    pub outcome: String,
    /// Why it was not read; only then.
    pub detail: Option<String>,
    /// What GitHub declared and answered; only when read.
    pub default_branch: Option<String>,
    pub workflows: Option<Vec<Workflow>>,
    pub workflows_more: Option<bool>,
    pub pull_requests: Option<Vec<PullRequest>>,
    pub pull_requests_more: Option<bool>,
}

const SELECT: &str = "SELECT id, application_id, resource_id, reference_id, repository, source, reader, agent, observed_at, recorded_at, outcome, detail, default_branch, workflows, workflows_more, pull_requests, pull_requests_more FROM github_readings";

/// A list kept as JSON, which Rust checked before writing it. One that does not read back is a
/// damaged record, never an empty list.
fn list<T: serde::de::DeserializeOwned>(row: &Row, index: usize) -> rusqlite::Result<Option<T>> {
    match row.get::<_, Option<String>>(index)? {
        None => Ok(None),
        Some(json) => serde_json::from_str(&json).map(Some).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        }),
    }
}

fn from_row(row: &Row) -> rusqlite::Result<GithubReading> {
    Ok(GithubReading {
        id: row.get(0)?,
        subject: Subject::from_columns(row.get(1)?, row.get(2)?)?,
        reference_id: row.get(3)?,
        repository: row.get(4)?,
        source: row.get(5)?,
        reader: row.get(6)?,
        agent: row.get(7)?,
        observed_at: time_at(row, 8)?,
        recorded_at: time_at(row, 9)?,
        outcome: row.get(10)?,
        detail: row.get(11)?,
        default_branch: row.get(12)?,
        workflows: list(row, 13)?,
        workflows_more: row.get(14)?,
        pull_requests: list(row, 15)?,
        pull_requests_more: row.get(16)?,
    })
}

/// The newest reading of a reference, through its index.
const NEWEST: &str =
    "WHERE reference_id = ?1 ORDER BY observed_at DESC, recorded_at DESC, rowid DESC LIMIT 1";

/// A repository on GitHub a system declares now, with its newest reading. Never stored.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubRepository {
    pub reference_id: String,
    pub subject: Subject,
    /// The address the reference names now.
    pub repository: String,
    /// Its newest reading, of whatever address it named then; None if it was never read.
    pub latest: Option<GithubReading>,
    /// The newest reading was of another address than the reference names now.
    pub moved_since_read: bool,
}

/// Each repository on GitHub the systems declare, in the inventory's order (applications, then
/// resources, each in the order of its references), with its newest reading, read inside the
/// caller's transaction. A `repository` reference elsewhere than github.com is not one.
pub(crate) fn repositories(
    tx: &Transaction,
    applications: &[Application],
    resources: &[Resource],
) -> Result<Vec<GithubRepository>, ApiError> {
    let mut newest = tx
        .prepare_cached(&format!("{SELECT} {NEWEST}"))
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
    let mut found = Vec::new();
    for (subject, references) in systems {
        for reference in references
            .iter()
            .filter(|item| item.kind == "repository" && repository(&item.target).is_some())
        {
            let latest = newest
                .query_map(params![reference.id], from_row)
                .map_err(ApiError::database)?
                .next()
                .transpose()
                .map_err(ApiError::database)?;
            found.push(GithubRepository {
                reference_id: reference.id.clone(),
                subject: subject.clone(),
                moved_since_read: latest
                    .as_ref()
                    .is_some_and(|reading| reading.repository != reference.target),
                repository: reference.target.clone(),
                latest,
            });
        }
    }
    Ok(found)
}

/// A repository DevHQ was asked to read: which reference, of which system, and its address.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    pub reference_id: String,
    pub subject: Subject,
    pub repository: String,
}

/// Keeps what the reader found of `target`, inside one write transaction, only while the reference
/// still names the address it named when the reading began: a repository removed or changed
/// meanwhile records nothing.
pub(crate) fn store(
    path: &Path,
    target: &Target,
    facts: &Facts,
    observed_at: i64,
    by: &Actor,
) -> Result<GithubReading, ApiError> {
    let mut db = open_current(path)?;
    let tx = inventory::begin_write_for(&mut db, by, "maintain")?;
    let (application_id, resource_id) = target.subject.columns();
    let unchanged: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM system_references WHERE id = ?1 AND kind = 'repository'
               AND application_id IS ?2 AND resource_id IS ?3 AND target = ?4)",
            params![
                target.reference_id,
                application_id,
                resource_id,
                target.repository
            ],
            |row| row.get(0),
        )
        .map_err(ApiError::database)?;
    if !unchanged {
        return Err(ApiError::cancelled(format!(
            "The repository {} was removed or changed while DevHQ read it. Nothing was kept.",
            target.repository
        )));
    }
    let read = facts.outcome == "read";
    let json = |value: serde_json::Result<String>| value.map_err(ApiError::database);
    let (workflows, pull_requests) = if read {
        (
            Some(json(serde_json::to_string(&facts.workflows))?),
            Some(json(serde_json::to_string(&facts.pull_requests))?),
        )
    } else {
        (None, None)
    };
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO github_readings (id, application_id, resource_id, reference_id, repository, source, reader, agent,
             observed_at, recorded_at, outcome, detail, default_branch, workflows, workflows_more, pull_requests,
             pull_requests_more)
         VALUES (?1, ?2, ?3, ?4, ?5, 'github', ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            id,
            application_id,
            resource_id,
            target.reference_id,
            target.repository,
            READER,
            by.name(),
            observed_at,
            now_ms(),
            facts.outcome,
            facts.detail,
            facts.default_branch,
            workflows,
            read.then_some(facts.workflows_more),
            pull_requests,
            read.then_some(facts.pull_requests_more),
        ],
    )
    .map_err(ApiError::database)?;
    let reading = tx
        .query_row(&format!("{SELECT} WHERE id = ?1"), [&id], from_row)
        .map_err(ApiError::database)?;
    tx.commit().map_err(ApiError::database)?;
    Ok(reading)
}

/// A repository DevHQ could not keep a reading of: a failure of DevHQ, kept nowhere.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubFailure {
    pub reference_id: String,
    pub subject: Subject,
    pub repository: String,
    pub message: String,
}

/// What reading GitHub did: the readings kept, among them what GitHub refused, and the
/// repositories DevHQ could not keep a reading of.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubRead {
    pub readings: Vec<GithubReading>,
    pub failures: Vec<GithubFailure>,
}

/// The repositories `reference_ids` names, or, when None, every repository on GitHub of every
/// system but archived applications, as Read all reads.
pub(crate) fn targets(
    database: &Path,
    reference_ids: Option<&[String]>,
) -> Result<Vec<Target>, ApiError> {
    let snapshot = inventory::list(database)?;
    let archived = |subject: &Subject| match subject {
        Subject::Application { id } => snapshot
            .applications
            .iter()
            .any(|item| &item.id == id && item.lifecycle == "archived"),
        Subject::Resource { .. } => false,
    };
    let declared = snapshot.github_repositories.iter().map(|item| Target {
        reference_id: item.reference_id.clone(),
        subject: item.subject.clone(),
        repository: item.repository.clone(),
    });
    let Some(ids) = reference_ids else {
        return Ok(declared
            .filter(|target| !archived(&target.subject))
            .collect());
    };
    let declared: Vec<Target> = declared.collect();
    let mut chosen: Vec<Target> = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(target) = declared.iter().find(|target| &target.reference_id == id) else {
            return Err(ApiError::invalid(format!(
                "Reference {id} is not a repository on GitHub that any system declares."
            )));
        };
        if !chosen.iter().any(|item| &item.reference_id == id) {
            chosen.push(target.clone());
        }
    }
    Ok(chosen)
}

/// Reads each target on GitHub, one after another, and keeps each reading as `by`'s request.
pub(crate) async fn read_github(
    database: &Path,
    fetch: &impl Fetch,
    targets: Vec<Target>,
    by: &Actor,
) -> Result<GithubRead, ApiError> {
    let mut readings = Vec::new();
    let mut failures = Vec::new();
    for target in targets {
        let Some((owner, name)) = repository(&target.repository) else {
            continue;
        };
        let observed_at = now_ms();
        let facts = read(fetch, &owner, &name).await;
        match store(database, &target, &facts, observed_at, by) {
            Ok(reading) => readings.push(reading),
            Err(error) if error.code == "access_denied" => return Err(error),
            Err(error) => failures.push(GithubFailure {
                reference_id: target.reference_id,
                subject: target.subject,
                repository: target.repository,
                message: error.message,
            }),
        }
    }
    Ok(GithubRead { readings, failures })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::tests::{pull, shop, PULLS, REPO};
    use crate::inventory::{save_application, ApplicationInput};
    use crate::references::ReferenceInput;
    use serde_json::json;

    fn block<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(future)
    }

    /// An inventory with an application that declares an invented repository on GitHub and one
    /// elsewhere, and an archived application that declares another; and the references' ids, in
    /// that order.
    fn inventory() -> (std::path::PathBuf, Vec<String>) {
        let folder = crate::installations::tests::folder("github-readings");
        let path = folder.join("inventory.sqlite3");
        inventory::start(&path).unwrap();
        let repository = |target: &str| ReferenceInput {
            id: None,
            kind: "repository".into(),
            target: target.into(),
            label: None,
            machine_id: None,
        };
        let save = |name: &str, lifecycle: &str, references| {
            save_application(
                &path,
                ApplicationInput {
                    id: None,
                    name: name.into(),
                    description: String::new(),
                    lifecycle: lifecycle.into(),
                    version: None,
                    components: vec![],
                    references,
                },
            )
            .unwrap()
        };
        let shop = save(
            "Shop",
            "production",
            vec![
                repository("https://github.com/ana-ruiz/shop"),
                repository("https://git.example/ana-ruiz/shop"),
            ],
        );
        let old = save(
            "Old",
            "archived",
            vec![repository("https://github.com/ana-ruiz/old")],
        );
        let ids = shop
            .references
            .iter()
            .chain(&old.references)
            .map(|reference| reference.id.clone())
            .collect();
        (path, ids)
    }

    fn of<'a>(snapshot: &'a crate::inventory::Inventory, id: &str) -> &'a GithubRepository {
        snapshot
            .github_repositories
            .iter()
            .find(|repository| repository.reference_id == id)
            .unwrap()
    }

    fn retarget(path: &std::path::Path, id: &str) {
        let db = rusqlite::Connection::open(path).unwrap();
        db.execute(
            "UPDATE system_references SET target = 'https://github.com/ana-ruiz/shop-v2' WHERE id = ?1",
            [id],
        )
        .unwrap();
    }

    #[test]
    fn keeps_a_reading_of_each_repository_on_github_and_serves_the_newest() {
        let (path, ids) = inventory();
        let targets = targets(&path, None).unwrap();
        // Not the one elsewhere, not the archived application's.
        assert_eq!(
            targets
                .iter()
                .map(|target| target.reference_id.as_str())
                .collect::<Vec<_>>(),
            [ids[0].as_str()]
        );
        let first = block(read_github(&path, &shop(), targets.clone(), &Actor::Person)).unwrap();
        assert!(first.failures.is_empty());
        assert_eq!(first.readings[0].outcome, "read");
        assert_eq!(first.readings[0].source, "github");
        assert_eq!(first.readings[0].reader, READER);
        assert_eq!(first.readings[0].workflows.as_ref().unwrap().len(), 2);
        let refused = shop().answer(REPO, 404, json!({}));
        let second = block(read_github(&path, &refused, targets, &Actor::Person)).unwrap();
        assert_eq!(second.readings[0].outcome, "refused");
        assert_eq!(second.readings[0].workflows, None);
        assert_eq!(second.readings[0].pull_requests_more, None);

        let snapshot = crate::inventory::list(&path).unwrap();
        assert_eq!(snapshot.github_repositories.len(), 2);
        let shop_repo = of(&snapshot, &ids[0]);
        assert_eq!(shop_repo.latest.as_ref().unwrap().id, second.readings[0].id);
        assert!(!shop_repo.moved_since_read);
        assert_eq!(of(&snapshot, &ids[2]).latest, None);
        // The first reading is history, never rewritten.
        let db = rusqlite::Connection::open(&path).unwrap();
        let kept: i64 = db
            .query_row("SELECT count(*) FROM github_readings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(kept, 2);
    }

    #[test]
    fn asks_only_for_what_is_named_and_refuses_what_is_not_a_repository_on_github() {
        let (path, ids) = inventory();
        let error = targets(&path, Some(&[ids[1].clone()])).unwrap_err();
        assert_eq!(error.code, "invalid_input");
        // Named, an archived application's repository is read; named twice, once.
        let chosen = targets(&path, Some(&[ids[2].clone(), ids[2].clone()])).unwrap();
        assert_eq!(chosen.len(), 1);
    }

    #[test]
    fn keeps_nothing_when_the_reference_changed_while_reading() {
        let (path, ids) = inventory();
        let targets = targets(&path, None).unwrap();
        retarget(&path, &ids[0]);
        let outcome = block(read_github(&path, &shop(), targets, &Actor::Person)).unwrap();
        assert!(outcome.readings.is_empty());
        assert!(outcome.failures[0].message.contains("removed or changed"));
        let snapshot = crate::inventory::list(&path).unwrap();
        assert_eq!(of(&snapshot, &ids[0]).latest, None);
    }

    #[test]
    fn a_reading_of_another_address_says_the_reference_moved_since() {
        let (path, ids) = inventory();
        let targets = targets(&path, None).unwrap();
        block(read_github(&path, &shop(), targets, &Actor::Person)).unwrap();
        retarget(&path, &ids[0]);
        let snapshot = crate::inventory::list(&path).unwrap();
        assert!(of(&snapshot, &ids[0]).moved_since_read);
        assert_eq!(
            of(&snapshot, &ids[0]).latest.as_ref().unwrap().repository,
            "https://github.com/ana-ruiz/shop"
        );
    }

    #[test]
    fn what_github_answered_reads_back_as_it_was_kept() {
        let (path, ids) = inventory();
        let targets = targets(&path, None).unwrap();
        let fetch = shop().answer(
            PULLS,
            200,
            json!([pull(3, "Draft", "2026-11-01T00:00:00Z")]),
        );
        let kept = block(read_github(&path, &fetch, targets, &Actor::Person)).unwrap();
        let snapshot = crate::inventory::list(&path).unwrap();
        assert_eq!(
            of(&snapshot, &ids[0]).latest.as_ref(),
            Some(&kept.readings[0])
        );
    }
}
