//! Readings of installations as DevHQ keeps them (`installation_readings`, v14), what it derives
//! from them at each read of the inventory, and the use case that reads installations when asked.
//!
//! Like a reading of a local copy, a reading is evidence of its own, never health: it enters
//! neither the attention list nor health coverage, and it has no validity, because it never
//! claimed to stay true. It says what the file was when it was read, and its moment is always
//! shown: «two processes were running from it», never «it is open». It copies the reference, the
//! machine and the path it read, so editing or removing the reference never rewrites it, and it
//! makes its system's history: a system with readings is archived, not deleted.
//!
//! Four things are derived, at each read and never stored: whether the version the application
//! declares is the one its file declared; between which two readings the build changed, with both
//! moments and never a date of its own; whether the reading was made somewhere else than the
//! reference points now; and how the installed build compares with the newest reading of a local
//! copy of the same system on the same machine (`against_copy`).

use super::{Facts, Reader, DEADLINE, READER};
use crate::inventory::{self, open_current, ApiError, Application, Resource};
use crate::local_copies::readings::{ReadFailure, Which};
use crate::records::{self, now_ms, time_at, Actor, Subject};
use crate::workstation::{Computer, ThisWorkstation};
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction, TransactionBehavior};
use serde::Serialize;
use std::path::Path;
use uuid::Uuid;

/// Readings a page of a system's history carries.
pub(crate) const READINGS_PAGE: i64 = 200;
/// How many readings before the newest one DevHQ looks at to say since when the build is the
/// same. The cost of a read of the inventory must not grow with the history, so the look back
/// stops here and says so.
pub(crate) const LOOK_BACK: i64 = 200;

/// What DevHQ read of an installed executable's file, at one moment, as it keeps it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Deserialize))]
#[serde(rename_all = "camelCase")]
pub struct InstallationReading {
    pub id: String,
    pub subject: Subject,
    /// The `installation` reference read, and where it pointed then: copied, never linked.
    pub reference_id: String,
    pub machine_id: String,
    pub path: String,
    /// Always `read`: DevHQ read it itself.
    pub source: String,
    /// DevHQ's reader and its version («DevHQ installation reader 2»).
    pub reader: String,
    /// The agent that asked for it through the gate; None when the person asked in the window.
    pub agent: Option<String>,
    pub observed_at: String,
    pub recorded_at: String,
    /// `read`, `missing`, `unreadable` or `not_an_executable`.
    pub outcome: String,
    /// Why it was not read; only then.
    pub detail: Option<String>,
    /// Where the system really kept the file, when that was not the path declared.
    pub stored_at: Option<String>,
    /// SHA-256 of the file's bytes, which tells one build from another, and how many they were.
    pub sha256: Option<String>,
    pub size_bytes: Option<i64>,
    pub modified_at: Option<String>,
    /// What the file's version resource declared, as the file said it…
    pub product_version: Option<String>,
    pub file_version: Option<String>,
    pub product_name: Option<String>,
    /// …or why that is unknown.
    pub version_unknown: Option<String>,
    /// The processes that were running from the file, and the ones the system did not let DevHQ
    /// look at…
    pub processes: Option<i64>,
    pub processes_unexamined: Option<i64>,
    /// …or why they are unknown.
    pub processes_unknown: Option<String>,
}

const SELECT: &str = "SELECT id, application_id, resource_id, reference_id, machine_id, path, source, reader, agent, observed_at, recorded_at, outcome, detail, stored_at, sha256, size_bytes, modified_at, product_version, file_version, product_name, version_unknown, processes, processes_unexamined, processes_unknown FROM installation_readings";

fn from_row(row: &Row) -> rusqlite::Result<InstallationReading> {
    let modified_at = match row.get::<_, Option<i64>>(16)? {
        Some(_) => Some(time_at(row, 16)?),
        None => None,
    };
    Ok(InstallationReading {
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
        stored_at: row.get(13)?,
        sha256: row.get(14)?,
        size_bytes: row.get(15)?,
        modified_at,
        product_version: row.get(17)?,
        file_version: row.get(18)?,
        product_name: row.get(19)?,
        version_unknown: row.get(20)?,
        processes: row.get(21)?,
        processes_unexamined: row.get(22)?,
        processes_unknown: row.get(23)?,
    })
}

/// The newest readings of a reference, newest first, through its index: the first is what every
/// read of the inventory carries for each declared installation, and the rest, at most
/// `LOOK_BACK` of them, say since when its build is the same.
pub(crate) const NEWEST: &str = "WHERE reference_id = ?1
     ORDER BY observed_at DESC, recorded_at DESC, rowid DESC LIMIT ?2";

/// A system's readings older than a cursor, newest first, through its subject's index (see
/// `Subject::per_type`), as `local_copies::readings::OLDER`.
pub(crate) const OLDER: [&str; 2] = [
    "WHERE application_id = ?1 AND observed_at <= ?2
       AND (observed_at, recorded_at, rowid) < (?2, ?3, ?4)
     ORDER BY observed_at DESC, recorded_at DESC, rowid DESC LIMIT ?5",
    "WHERE resource_id = ?1 AND observed_at <= ?2
       AND (observed_at, recorded_at, rowid) < (?2, ?3, ?4)
     ORDER BY observed_at DESC, recorded_at DESC, rowid DESC LIMIT ?5",
];

/// The reading that found another build before the one installed now.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreviousBuild {
    pub observed_at: String,
    pub sha256: String,
    pub product_version: Option<String>,
}

/// Since when the file is the build its newest reading found, as far as the readings say: DevHQ
/// knows a build only at the moments it read the file, so a change of build happened between two
/// readings, and neither moment is when it was installed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Build {
    /// The oldest of the readings that found this same build without another one in between, at
    /// the same path on the same machine.
    pub same_since: String,
    /// How many readings found it, that one and the newest included.
    pub readings: i64,
    /// The reading before them, which found another build: the build changed between it and
    /// `same_since`. None when no reading DevHQ looked at found another.
    pub previous: Option<PreviousBuild>,
    /// Whether DevHQ looked at every earlier reading of this file. False when it stopped at
    /// `LOOK_BACK` readings, all of this build: it was the same before `same_since` too, and since
    /// when is not known.
    pub complete: bool,
}

/// An installation a system declares now, with its newest reading and what DevHQ derives from it
/// at this read of the inventory. Never stored.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Installation {
    pub reference_id: String,
    pub subject: Subject,
    /// Where the reference says the executable is now.
    pub machine_id: String,
    pub path: String,
    /// Its newest reading, wherever it was read; None if it was never read.
    pub latest: Option<InstallationReading>,
    /// The newest reading was made at another path or on another machine than the reference
    /// declares now.
    pub moved_since_read: bool,
    /// How the version the application declares compares with the ProductVersion its file declared
    /// at the newest reading (see `same_version`): `matches`, `differs`, `undeclared` when the
    /// application declares none, or `unknown` when the file's is not known. None when there is
    /// nothing to compare: a resource, which declares no version, or a file that was not read.
    pub version: Option<String>,
    /// Since when the file is the build the newest reading found. None when it was not read.
    pub build: Option<Build>,
    /// How the installed build compares with what a local copy of the same system on the same
    /// machine held at its newest reading. None when there is nothing to compare: the file or no
    /// such copy was read.
    pub against_copy: Option<AgainstCopy>,
}

/// The installed build beside a local copy's newest reading (see `compare_with_copies`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgainstCopy {
    /// `head`: the file declared the commit HEAD was on. `other_commit`: it declared another
    /// commit, so the copy's HEAD is not what is installed (newer, older or on another branch:
    /// DevHQ does not walk the history to say which). `newer_commits`: it declared no commit, and
    /// the copy held a commit dated after the file was last modified, which may or may not be in
    /// it. `not_newer`: it declared no commit, and no commit of the copy was dated after the file.
    /// `unknown`: neither comparison was possible.
    pub state: String,
    /// The copy compared with, and the reading it rests on.
    pub copy_reference_id: String,
    pub copy_reading_id: String,
    /// The commit the file declared after a `+` in its ProductVersion, as written.
    pub installed_commit: Option<String>,
    /// HEAD's commit at the copy's reading.
    pub copy_head: Option<String>,
}

/// The commit a build wrote as SemVer build metadata in its version («1.4.0+3f9c2ab01d4e», also
/// «1.4.0+3f9c2ab01d4e.dirty»): from 7 to 40 hexadecimal digits right after the `+`. None for any
/// other version, so that nothing that merely looks like a commit is compared as one.
pub(crate) fn declared_commit(version: &str) -> Option<String> {
    let metadata = version.split_once('+')?.1.trim();
    let commit = metadata.split('.').next()?;
    ((7..=40).contains(&commit.len()) && commit.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| commit.to_ascii_lowercase())
}

/// Compares each read installation with the newest successful reading of a local copy of the
/// same system on the same machine (the newest by its observed time; among equal ones, the first
/// declared). A commit the file declared is compared with HEAD's exactly, by prefix; without one,
/// the file's modification time with the copy's newest commit date, which says only that a newer
/// commit existed, never that it is missing from the build.
pub(crate) fn compare_with_copies(
    installations: &mut [Installation],
    copies: &[crate::local_copies::readings::LocalCopy],
) {
    for installation in installations.iter_mut() {
        let Some(read) = installation
            .latest
            .as_ref()
            .filter(|reading| reading.outcome == "read")
        else {
            continue;
        };
        let mut newest: Option<(&crate::local_copies::readings::CopyReading, i64)> = None;
        let mut reference = None;
        for copy in copies.iter().filter(|copy| {
            copy.subject == installation.subject && copy.machine_id == installation.machine_id
        }) {
            let Some(reading) = copy
                .latest
                .as_ref()
                .filter(|reading| reading.outcome == "read")
            else {
                continue;
            };
            // A stored time that cannot be read never wins: it is no evidence of being newest.
            let Ok(at) = records::parse_time(&reading.observed_at, "") else {
                continue;
            };
            if newest.is_none_or(|(_, best)| at > best) {
                newest = Some((reading, at));
                reference = Some(copy.reference_id.clone());
            }
        }
        let (Some((reading, _)), Some(reference)) = (newest, reference) else {
            continue;
        };
        let installed_commit = read.product_version.as_deref().and_then(declared_commit);
        let state = match (&installed_commit, &reading.head) {
            (Some(commit), Some(head)) if head.to_ascii_lowercase().starts_with(commit) => "head",
            (Some(_), Some(_)) => "other_commit",
            (Some(_), None) => "unknown",
            (None, _) => match (
                read.modified_at
                    .as_deref()
                    .map(|at| records::parse_time(at, "")),
                reading
                    .last_commit_at
                    .as_deref()
                    .map(|at| records::parse_time(at, "")),
            ) {
                (Some(Ok(modified)), Some(Ok(commit))) if commit > modified => "newer_commits",
                (Some(Ok(_)), Some(Ok(_))) => "not_newer",
                _ => "unknown",
            },
        };
        installation.against_copy = Some(AgainstCopy {
            state: state.to_owned(),
            copy_reference_id: reference,
            copy_reading_id: reading.id.clone(),
            installed_commit,
            copy_head: reading.head.clone(),
        });
    }
}

/// A version as it is compared: without surrounding whitespace, without a leading `v` before a
/// digit, and without build metadata (what follows a `+`, where a build writes its commit).
fn comparable(version: &str) -> &str {
    let version = version.trim();
    let version = version.split('+').next().unwrap_or(version).trim();
    match version.strip_prefix(['v', 'V']) {
        Some(rest) if rest.starts_with(|c: char| c.is_ascii_digit()) => rest,
        _ => version,
    }
}

/// The parts of a version made only of numbers and dots, without the zeros at its end: «1.4.0.0»
/// and «1.4» are both `[1, 4]`. None for any other text.
fn numbers(version: &str) -> Option<Vec<u64>> {
    let mut parts = version
        .split('.')
        .map(|part| {
            (!part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| part.parse().ok())
                .flatten()
        })
        .collect::<Option<Vec<u64>>>()?;
    while parts.last() == Some(&0) {
        parts.pop();
    }
    Some(parts)
}

/// Whether a declared version and the one a file declares are the same version: equal once each is
/// made comparable, ignoring case, or, when both are numbers and dots, equal but for zeros at the
/// end. «1.4.0» is «v1.4.0», «1.4.0+3f9c2ab» and «1.4.0.0»; it is not «1.4.1» or «1.4.0-beta».
pub(crate) fn same_version(declared: &str, installed: &str) -> bool {
    let (declared, installed) = (comparable(declared), comparable(installed));
    declared.eq_ignore_ascii_case(installed)
        || matches!((numbers(declared), numbers(installed)), (Some(a), Some(b)) if a == b)
}

/// Since when the file is the build `newest` found, from the readings before it, newest first:
/// the ones made of the same file (same path, same machine) until one found another build.
fn build_since(
    newest: &InstallationReading,
    earlier: &[InstallationReading],
    more: bool,
) -> Option<Build> {
    let hash = newest.sha256.as_ref()?;
    let mut build = Build {
        same_since: newest.observed_at.clone(),
        readings: 1,
        previous: None,
        complete: !more,
    };
    for reading in earlier {
        // A reading made somewhere else was of another file: nothing before it is about this one.
        if reading.path != newest.path || reading.machine_id != newest.machine_id {
            build.complete = true;
            break;
        }
        // A reading that did not read the file found no build, this one or another.
        let Some(found) = &reading.sha256 else {
            continue;
        };
        if found == hash {
            build.same_since.clone_from(&reading.observed_at);
            build.readings += 1;
        } else {
            build.previous = Some(PreviousBuild {
                observed_at: reading.observed_at.clone(),
                sha256: found.clone(),
                product_version: reading.product_version.clone(),
            });
            build.complete = true;
            break;
        }
    }
    Some(build)
}

/// Each installation the systems declare, in the inventory's order (applications, then resources,
/// each in the order of its references), with its newest reading and what is derived from it,
/// read inside the caller's transaction.
pub(crate) fn installations(
    tx: &Transaction,
    applications: &[Application],
    resources: &[Resource],
) -> Result<Vec<Installation>, ApiError> {
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
                Some(item.version.as_deref()),
            )
        })
        .chain(resources.iter().map(|item| {
            (
                Subject::Resource {
                    id: item.id.clone(),
                },
                &item.references,
                None,
            )
        }));
    let mut found = Vec::new();
    for (subject, references, declares) in systems {
        for reference in references.iter().filter(|item| item.kind == "installation") {
            let Some(machine_id) = reference.machine_id.clone() else {
                continue;
            };
            let mut readings = newest
                .query_map(params![reference.id, LOOK_BACK + 2], from_row)
                .map_err(ApiError::database)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(ApiError::database)?;
            // The newest, at most LOOK_BACK before it, and one more that only says there are more.
            let more = readings.len() > usize::try_from(LOOK_BACK + 1).unwrap_or(usize::MAX);
            readings.truncate(usize::try_from(LOOK_BACK + 1).unwrap_or(usize::MAX));
            let mut readings = readings.into_iter();
            let latest = readings.next();
            let earlier: Vec<InstallationReading> = readings.collect();
            let read = latest.as_ref().filter(|reading| reading.outcome == "read");
            let version = match (declares, read) {
                (Some(declared), Some(reading)) => Some(
                    match (declared, reading.product_version.as_deref()) {
                        (_, None) => "unknown",
                        (None, Some(_)) => "undeclared",
                        (Some(declared), Some(installed)) if same_version(declared, installed) => {
                            "matches"
                        }
                        (Some(_), Some(_)) => "differs",
                    }
                    .to_owned(),
                ),
                _ => None,
            };
            found.push(Installation {
                reference_id: reference.id.clone(),
                subject: subject.clone(),
                moved_since_read: latest.as_ref().is_some_and(|reading| {
                    reading.path != reference.target || reading.machine_id != machine_id
                }),
                machine_id,
                path: reference.target.clone(),
                version,
                build: read.and_then(|reading| build_since(reading, &earlier, more)),
                against_copy: None,
                latest,
            });
        }
    }
    Ok(found)
}

/// A page of a system's readings.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationReadingPage {
    pub readings: Vec<InstallationReading>,
    /// Older readings remain after this page.
    pub more: bool,
}

/// The subject's readings older than `before`, one of its readings already shown, or its newest
/// ones when `before` is None: newest first, at most `READINGS_PAGE`.
pub fn older_readings(
    path: &Path,
    subject: &Subject,
    before: Option<&str>,
) -> Result<InstallationReadingPage, ApiError> {
    older_readings_in(&mut open_current(path)?, subject, before)
}

/// `older_readings` on a connection the caller opened: the agent gate's, which opens the inventory
/// as it finds it.
pub(crate) fn older_readings_in(
    db: &mut Connection,
    subject: &Subject,
    before: Option<&str>,
) -> Result<InstallationReadingPage, ApiError> {
    let tx = inventory::begin(db, TransactionBehavior::Deferred)?;
    records::ensure_subject(&tx, subject)?;
    let (application_id, resource_id) = subject.columns();
    let cursor = match before {
        None => (i64::MAX, i64::MAX, i64::MAX),
        Some(before) => tx
            .query_row(
                "SELECT observed_at, recorded_at, rowid FROM installation_readings
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
    Ok(InstallationReadingPage { readings, more })
}

/// An installation DevHQ was asked to read: which reference, of which system, and where.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    pub reference_id: String,
    pub subject: Subject,
    pub machine_id: String,
    pub path: String,
}

/// Keeps what a reader read in `target`, inside one write transaction: only while the reference
/// still points where it did when the reading began (an installation removed or moved meanwhile
/// records nothing), and, for an agent's request, only while the person lets agents maintain the
/// inventory, checked with SQLite's write lock held (see `inventory::begin_write_for`).
pub(crate) fn store(
    path: &Path,
    target: &Target,
    facts: &Facts,
    observed_at: i64,
    by: &Actor,
) -> Result<InstallationReading, ApiError> {
    let mut db = open_current(path)?;
    let tx = inventory::begin_write_for(&mut db, by, "maintain")?;
    let (application_id, resource_id) = target.subject.columns();
    let unchanged: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM system_references WHERE id = ?1 AND kind = 'installation'
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
            "The installation {} was removed or moved while DevHQ read it. Nothing was kept.",
            target.path
        )));
    }
    let size_bytes = facts
        .size_bytes
        .map(i64::try_from)
        .transpose()
        .map_err(ApiError::database)?;
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO installation_readings (id, application_id, resource_id, reference_id, machine_id, path, source, reader, agent,
             observed_at, recorded_at, outcome, detail, stored_at, sha256, size_bytes, modified_at, product_version, file_version,
             product_name, version_unknown, processes, processes_unexamined, processes_unknown)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'read', ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
        params![
            id,
            application_id,
            resource_id,
            target.reference_id,
            target.machine_id,
            target.path,
            READER,
            by.name(),
            observed_at,
            now_ms(),
            facts.outcome,
            facts.detail,
            facts.stored_at,
            facts.sha256,
            size_bytes,
            facts.modified_at,
            facts.product_version,
            facts.file_version,
            facts.product_name,
            facts.version_unknown,
            facts.processes,
            facts.processes_unexamined,
            facts.processes_unknown,
        ],
    )
    .map_err(ApiError::database)?;
    let reading = tx
        .query_row(&format!("{SELECT} WHERE id = ?1"), [&id], from_row)
        .map_err(ApiError::database)?;
    tx.commit().map_err(ApiError::database)?;
    Ok(reading)
}

/// What reading installations did: the readings kept, the installations DevHQ could not read, and
/// the workstation it read them on.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallationsRead {
    pub readings: Vec<InstallationReading>,
    pub failures: Vec<ReadFailure>,
    pub this_workstation: ThisWorkstation,
}

/// Reads the installations `which` names, one after another, each in a reader of its own, and
/// keeps each reading as `by`'s request. Only installations on this workstation are read: another
/// machine's are read by DevHQ on that machine. Without a known workstation nothing is read.
pub(crate) fn read_installations(
    database: &Path,
    computer: &Computer,
    reader: &Reader,
    which: &Which,
    by: &Actor,
) -> Result<InstallationsRead, ApiError> {
    let snapshot = inventory::list(database)?;
    let this_workstation = computer.resolve(&snapshot.resources);
    let Some(machine) = this_workstation.resource_id.clone() else {
        return Err(ApiError::invalid(format!(
            "DevHQ does not know which workstation this computer is, so it reads no installation: {}",
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
        .installations
        .iter()
        .map(|installation| Target {
            reference_id: installation.reference_id.clone(),
            subject: installation.subject.clone(),
            machine_id: installation.machine_id.clone(),
            path: installation.path.clone(),
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
                    "This {} declares no installation on this computer ({}).",
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
                        "Reference {id} is not an installation any system declares."
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
            .and_then(|facts| store(database, &target, &facts, observed_at, by));
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
    Ok(InstallationsRead {
        readings,
        failures,
        this_workstation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installations::pe::tests::{image, version_data};
    use crate::installations::tests::{folder, path_text, sha256_of, test_reader};
    use crate::inventory::open;
    use crate::inventory::{
        delete_application, list, save_application, save_resource, ApplicationInput, ResourceInput,
    };
    use crate::references::ReferenceInput;
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
            let database = folder.join("inventory.sqlite3");
            inventory::start(&database).unwrap();
            Self {
                database,
                computer: Computer {
                    local: folder.join("local"),
                    fingerprint: Some("installations-test".into()),
                },
                reader: test_reader("confined"),
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

        fn read(&self, which: &Which) -> Result<InstallationsRead, ApiError> {
            read_installations(
                &self.database,
                &self.computer,
                &self.reader,
                which,
                &Actor::Person,
            )
        }

        /// A synthetic executable at `name` in the world's folder that declares `version`: its
        /// path and the hash of its bytes.
        fn install(&self, name: &str, version: &str) -> (PathBuf, String) {
            let (bytes, _) = image(
                &version_data(&[("ProductVersion", version), ("ProductName", "Atlas Notes")]),
                true,
            );
            let path = self.folder.join(name);
            std::fs::write(&path, &bytes).unwrap();
            (path, sha256_of(&bytes))
        }

        fn installation(&self, reference: &str) -> Installation {
            list(&self.database)
                .unwrap()
                .installations
                .into_iter()
                .find(|item| item.reference_id == reference)
                .unwrap()
        }
    }

    fn installed_at(path: &Path, machine: &str) -> ReferenceInput {
        ReferenceInput {
            id: None,
            kind: "installation".into(),
            target: path_text(path),
            label: None,
            machine_id: Some(machine.into()),
        }
    }

    fn application(
        name: &str,
        version: Option<&str>,
        references: Vec<ReferenceInput>,
    ) -> ApplicationInput {
        ApplicationInput {
            id: None,
            name: name.into(),
            description: String::new(),
            lifecycle: "production".into(),
            version: version.map(Into::into),
            components: vec![],
            references,
        }
    }

    #[test]
    fn compares_the_declared_version_with_the_one_the_file_declares() {
        for (declared, installed) in [
            ("1.4.0", "1.4.0"),
            ("1.4.0", "1.4.0+3f9c2ab"),
            ("1.4.0", "1.4.0+3f9c2ab.dirty"),
            ("v1.4.0", "1.4.0"),
            (" 1.4.0 ", "V1.4.0"),
            ("1.4.0", "1.4.0.0"),
            ("1.4", "1.4.0.0"),
            ("1.4.0-RC1", "1.4.0-rc1"),
            ("2026.11", "2026.11.0"),
        ] {
            assert!(
                same_version(declared, installed),
                "{declared} / {installed}"
            );
            assert!(
                same_version(installed, declared),
                "{installed} / {declared}"
            );
        }
        for (declared, installed) in [
            ("1.4.0", "1.4.1"),
            ("1.4.0", "0.1.0"),
            ("1.4.0", "1.4.0-beta"),
            ("1.4.0", "10.4.0"),
            ("1.4", "1.40"),
            ("version 1", "1"),
            ("1.4.0", ""),
        ] {
            assert!(
                !same_version(declared, installed),
                "{declared} / {installed}"
            );
        }
    }

    #[test]
    fn keeps_each_reading_and_says_when_the_build_changed_between_two_of_them() {
        let world = World::new("installation-history");
        let laptop = world.workstation("Laptop");
        world.choose(&laptop);
        let (path, first_hash) = world.install("atlas-notes.exe", "1.4.0+3f9c2ab");
        let notes = save_application(
            &world.database,
            application(
                "Atlas Notes",
                Some("1.3.0"),
                vec![installed_at(&path, &laptop)],
            ),
        )
        .unwrap();
        let reference = notes.references[0].id.clone();
        // Declared, never read: nothing is derived from nothing.
        let never = world.installation(&reference);
        assert_eq!(
            (&never.latest, &never.version, &never.build),
            (&None, &None, &None)
        );
        assert!(!never.moved_since_read);

        let first = world.read(&Which::ThisWorkstation).unwrap();
        assert_eq!(first.failures, []);
        let reading = &first.readings[0];
        assert_eq!(
            (
                reading.outcome.as_str(),
                reading.source.as_str(),
                reading.reader.as_str(),
                &reading.agent,
                reading.path.as_str(),
                reading.machine_id.as_str()
            ),
            (
                "read",
                "read",
                READER,
                &None,
                path_text(&path).as_str(),
                laptop.as_str()
            )
        );
        assert_eq!(reading.sha256.as_deref(), Some(first_hash.as_str()));
        assert_eq!(reading.product_version.as_deref(), Some("1.4.0+3f9c2ab"));
        assert_eq!(reading.product_name.as_deref(), Some("Atlas Notes"));
        assert_eq!(
            (reading.processes, reading.stored_at.clone()),
            (Some(0), None)
        );
        // The declared version is not the installed one, which the file says.
        let after_first = world.installation(&reference);
        assert_eq!(after_first.version.as_deref(), Some("differs"));
        assert_eq!(
            after_first.build,
            Some(Build {
                same_since: reading.observed_at.clone(),
                readings: 1,
                previous: None,
                complete: true,
            })
        );
        // Read again: the same build, since the first reading.
        let second = world
            .read(&Which::References(vec![reference.clone()]))
            .unwrap();
        let build = world.installation(&reference).build.unwrap();
        assert_eq!(
            (
                build.same_since.as_str(),
                build.readings,
                &build.previous,
                build.complete
            ),
            (reading.observed_at.as_str(), 2, &None, true)
        );
        // Another build is installed, and the application's version is corrected.
        let (_, second_hash) = world.install("atlas-notes.exe", "1.5.0+77aa01c");
        assert_ne!(first_hash, second_hash);
        save_application(
            &world.database,
            ApplicationInput {
                id: Some(notes.id.clone()),
                ..application(
                    "Atlas Notes",
                    Some("v1.5.0"),
                    vec![ReferenceInput::from(&notes.references[0])],
                )
            },
        )
        .unwrap();
        // Until it is read again, DevHQ still says what the last reading found.
        assert_eq!(
            world.installation(&reference).version.as_deref(),
            Some("differs")
        );
        let third = world
            .read(&Which::System(Subject::Application {
                id: notes.id.clone(),
            }))
            .unwrap();
        let now = world.installation(&reference);
        assert_eq!(now.version.as_deref(), Some("matches"));
        assert_eq!(
            now.build,
            Some(Build {
                same_since: third.readings[0].observed_at.clone(),
                readings: 1,
                // Between the second reading and the third: both moments, never one of DevHQ's.
                previous: Some(PreviousBuild {
                    observed_at: second.readings[0].observed_at.clone(),
                    sha256: first_hash.clone(),
                    product_version: Some("1.4.0+3f9c2ab".into()),
                }),
                complete: true,
            })
        );
        // Uninstalled: the reading says so, and nothing is derived from a file that was not read.
        std::fs::remove_file(&path).unwrap();
        let gone = world.read(&Which::ThisWorkstation).unwrap();
        assert_eq!(gone.readings[0].outcome, "missing");
        let missing = world.installation(&reference);
        assert_eq!((&missing.version, &missing.build), (&None, &None));
        // Installed again, the same build: a reading that read nothing found no other build.
        world.install("atlas-notes.exe", "1.5.0+77aa01c");
        world.read(&Which::ThisWorkstation).unwrap();
        let back = world.installation(&reference).build.unwrap();
        assert_eq!(
            (back.same_since.as_str(), back.readings),
            (third.readings[0].observed_at.as_str(), 2)
        );
        assert_eq!(back.previous.unwrap().sha256, first_hash);
        // An application that declares no version, and a resource, which has none to declare.
        let (tool, _) = world.install("tool.exe", "3.0");
        let undeclared = save_application(
            &world.database,
            application("Tool", None, vec![installed_at(&tool, &laptop)]),
        )
        .unwrap();
        let (driver, _) = world.install("driver.exe", "9");
        save_resource(
            &world.database,
            ResourceInput {
                id: Some(laptop.clone()),
                name: "Laptop".into(),
                kind: "workstation".into(),
                description: String::new(),
                operating_system: String::new(),
                references: vec![installed_at(&driver, &laptop)],
            },
        )
        .unwrap();
        // A file that declares no version: unknown, never «differs».
        let plain = world.folder.join("plain.exe");
        std::fs::write(
            &plain,
            image(&version_data(&[("CompanyName", "Ana Ruiz")]), true).0,
        )
        .unwrap();
        let unversioned = save_application(
            &world.database,
            application("Plain", Some("1.0"), vec![installed_at(&plain, &laptop)]),
        )
        .unwrap();
        world.read(&Which::ThisWorkstation).unwrap();
        let all = list(&world.database).unwrap().installations;
        let version_of = |subject: &Subject| {
            all.iter()
                .find(|item| &item.subject == subject)
                .unwrap()
                .version
                .clone()
        };
        assert_eq!(
            version_of(&Subject::Application { id: undeclared.id }).as_deref(),
            Some("undeclared")
        );
        assert_eq!(
            version_of(&Subject::Application { id: unversioned.id }).as_deref(),
            Some("unknown")
        );
        assert_eq!(version_of(&Subject::Resource { id: laptop.clone() }), None);
        // The reference moved: the newest reading was made somewhere else, and says so.
        let (moved, _) = world.install("moved.exe", "1.5.0+77aa01c");
        let mut elsewhere = ReferenceInput::from(&notes.references[0]);
        elsewhere.target = path_text(&moved);
        save_application(
            &world.database,
            ApplicationInput {
                id: Some(notes.id.clone()),
                ..application("Atlas Notes", Some("1.5.0"), vec![elsewhere])
            },
        )
        .unwrap();
        assert!(world.installation(&reference).moved_since_read);
        // Read there, it is another file: what was read at the old path says nothing of it.
        let there = world
            .read(&Which::References(vec![reference.clone()]))
            .unwrap();
        let fresh = world.installation(&reference);
        assert!(!fresh.moved_since_read);
        assert_eq!(
            fresh.build,
            Some(Build {
                same_since: there.readings[0].observed_at.clone(),
                readings: 1,
                previous: None,
                complete: true,
            })
        );
        // Readings are history: the application cannot be deleted, and removing its reference
        // rewrites none of them.
        let refused = delete_application(&world.database, &notes.id).unwrap_err();
        assert!(
            refused.message.contains("readings of its installations"),
            "{}",
            refused.message
        );
        save_application(
            &world.database,
            ApplicationInput {
                id: Some(notes.id.clone()),
                ..application("Atlas Notes", Some("1.5.0"), vec![])
            },
        )
        .unwrap();
        let page = older_readings(
            &world.database,
            &Subject::Application {
                id: notes.id.clone(),
            },
            None,
        )
        .unwrap();
        assert_eq!((page.readings.len(), page.more), (7, false));
        assert!(page
            .readings
            .iter()
            .all(|reading| reading.reference_id == reference));
        crate::inventory::tests::remove_now(&world.folder);
    }

    /// The look back stops at `LOOK_BACK` readings and says so, rather than give the oldest it
    /// looked at for the first that found the build.
    #[test]
    fn says_when_it_looked_no_further_back_for_the_same_build() {
        let world = World::new("installation-look-back");
        let db = open(&world.database).unwrap();
        db.execute_batch(
            "INSERT INTO resources (id, name, kind, description, operating_system) VALUES ('laptop', 'Laptop', 'workstation', '', '');
             INSERT INTO applications (id, name, lifecycle, description, version) VALUES ('notes', 'Atlas Notes', 'production', '', '1.4.0');
             INSERT INTO system_references (id, application_id, kind, target, machine_id)
               VALUES ('installed', 'notes', 'installation', '/opt/atlas/atlas-notes', 'laptop');",
        )
        .unwrap();
        let hash = |build: char| build.to_string().repeat(64);
        let reading = |at: i64, build: char| {
            db.execute(
                "INSERT INTO installation_readings (id, application_id, reference_id, machine_id, path, source, reader, observed_at, recorded_at,
                     outcome, sha256, size_bytes, product_version, processes, processes_unexamined)
                 VALUES (?1, 'notes', 'installed', 'laptop', '/opt/atlas/atlas-notes', 'read', 'reader', ?2, ?2, 'read', ?3, 10, '1.4.0', 0, 0)",
                params![format!("reading-{at}"), at * 1000, hash(build)],
            )
            .unwrap();
        };
        // One reading of build a, then exactly LOOK_BACK + 1 of build b: the newest and LOOK_BACK
        // before it are all b, and the reading of a is one further back than DevHQ looks.
        reading(0, 'a');
        for at in 1..=LOOK_BACK + 1 {
            reading(at, 'b');
        }
        let build = || world.installation("installed").build.unwrap();
        let cut = build();
        assert_eq!(
            (
                cut.same_since.as_str(),
                cut.readings,
                &cut.previous,
                cut.complete
            ),
            ("1970-01-01T00:00:01Z", LOOK_BACK + 1, &None, false)
        );
        // One reading fewer of b before the newest, and the reading of a is within the look back.
        db.execute(
            "DELETE FROM installation_readings WHERE id = 'reading-1'",
            [],
        )
        .unwrap();
        let whole = build();
        assert_eq!(
            (whole.same_since.as_str(), whole.readings, whole.complete),
            ("1970-01-01T00:00:02Z", LOOK_BACK, true)
        );
        assert_eq!(whole.previous.unwrap().observed_at, "1970-01-01T00:00:00Z");
        drop(db);
        crate::inventory::tests::remove_now(&world.folder);
    }

    #[test]
    fn reads_only_installations_of_this_workstation_and_says_why_not() {
        let world = World::new("installation-workstations");
        let laptop = world.workstation("Laptop");
        let desktop = world.workstation("Desktop");
        let (here, _) = world.install("here.exe", "1.0");
        let (old, _) = world.install("old.exe", "0.9");
        let notes = save_application(
            &world.database,
            application(
                "Atlas Notes",
                Some("1.0"),
                vec![
                    installed_at(&here, &laptop),
                    installed_at(Path::new("/opt/atlas/atlas-notes"), &desktop),
                ],
            ),
        )
        .unwrap();
        let archived = save_application(
            &world.database,
            ApplicationInput {
                lifecycle: "archived".into(),
                ..application("Old Notes", None, vec![installed_at(&old, &laptop)])
            },
        )
        .unwrap();
        // No workstation chosen: nothing is read, and the reason is said.
        let unknown = world.read(&Which::ThisWorkstation).unwrap_err();
        assert!(
            unknown.message.contains("so it reads no installation"),
            "{}",
            unknown.message
        );
        world.choose(&laptop);
        // All of this workstation: not the desktop's, not the archived application's.
        let all = world.read(&Which::ThisWorkstation).unwrap();
        assert_eq!(all.readings.len(), 1);
        assert_eq!(all.readings[0].reference_id, notes.references[0].id);
        assert_eq!(
            all.this_workstation.resource_id.as_deref(),
            Some(laptop.as_str())
        );
        // Named, an archived application's is read; another machine's is refused, and so is
        // what is no installation.
        let named = world
            .read(&Which::References(vec![archived.references[0].id.clone()]))
            .unwrap();
        assert_eq!(named.readings[0].product_version.as_deref(), Some("0.9"));
        for (which, message) in [
            (
                Which::References(vec![notes.references[1].id.clone()]),
                "is on Desktop, not on this computer",
            ),
            (
                Which::References(vec!["ghost".into()]),
                "is not an installation any system declares",
            ),
            (
                Which::System(Subject::Resource {
                    id: desktop.clone(),
                }),
                "declares no installation on this computer (Laptop)",
            ),
            (
                Which::System(Subject::Application { id: "ghost".into() }),
                "The application does not exist.",
            ),
        ] {
            let error = world.read(&which).unwrap_err();
            assert_eq!(error.code, "invalid_input");
            assert!(error.message.contains(message), "{}", error.message);
        }
        crate::inventory::tests::remove_now(&world.folder);
    }

    #[test]
    fn an_installation_removed_or_moved_while_it_was_read_keeps_nothing() {
        let world = World::new("installation-in-flight");
        let laptop = world.workstation("Laptop");
        let (path, _) = world.install("atlas-notes.exe", "1.4.0");
        let notes = save_application(
            &world.database,
            application("Atlas Notes", None, vec![installed_at(&path, &laptop)]),
        )
        .unwrap();
        let target = Target {
            reference_id: notes.references[0].id.clone(),
            subject: Subject::Application {
                id: notes.id.clone(),
            },
            machine_id: laptop.clone(),
            path: path_text(&path),
        };
        let facts = world.reader.read(&target.path, DEADLINE).unwrap();
        let keep =
            |target: &Target, by: &Actor| store(&world.database, target, &facts, now_ms(), by);
        let elsewhere = Target {
            path: "/moved".into(),
            ..target.clone()
        };
        let other_machine = Target {
            machine_id: "desktop".into(),
            ..target.clone()
        };
        for moved in [&elsewhere, &other_machine] {
            assert_eq!(keep(moved, &Actor::Person).unwrap_err().code, "cancelled");
        }
        // Asked by an agent while the person does not let agents maintain the inventory.
        let agent = Actor::agent("Reader").unwrap();
        assert_eq!(keep(&target, &agent).unwrap_err().code, "access_denied");
        // A local copy at the same path is not this installation.
        let mut as_copy = ReferenceInput::from(&notes.references[0]);
        as_copy.kind = "local_copy".into();
        save_application(
            &world.database,
            ApplicationInput {
                id: Some(notes.id.clone()),
                ..application("Atlas Notes", None, vec![as_copy])
            },
        )
        .unwrap();
        assert_eq!(keep(&target, &Actor::Person).unwrap_err().code, "cancelled");
        let db = open(&world.database).unwrap();
        let rows: i64 = db
            .query_row("SELECT count(*) FROM installation_readings", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 0);
        drop(db);
        crate::inventory::tests::remove_now(&world.folder);
    }

    #[test]
    fn pages_through_a_systems_readings_without_gaps_or_repeats() {
        let world = World::new("installation-pages");
        let db = open(&world.database).unwrap();
        db.execute_batch(
            "INSERT INTO applications (id, name, lifecycle, description) VALUES ('notes', 'Atlas Notes', 'production', ''), ('tool', 'Tool', 'production', '')",
        )
        .unwrap();
        // 450 readings of one application, three at each instant, and some of another between.
        for index in 0..450 {
            for (subject, id) in [
                ("notes", format!("notes-{index}")),
                ("tool", format!("tool-{index}")),
            ] {
                if subject == "tool" && index % 50 != 0 {
                    continue;
                }
                db.execute(
                    "INSERT INTO installation_readings (id, application_id, reference_id, machine_id, path, source, reader, observed_at, recorded_at, outcome, detail)
                     VALUES (?1, ?2, 'installed', 'laptop', '/opt/x', 'read', 'reader', ?3, ?3, 'missing', 'Nothing is at this path.')",
                    params![id, subject, index / 3],
                )
                .unwrap();
            }
        }
        drop(db);
        let notes = Subject::Application { id: "notes".into() };
        let mut seen: Vec<String> = Vec::new();
        let mut before: Option<String> = None;
        loop {
            let page = older_readings(&world.database, &notes, before.as_deref()).unwrap();
            assert!(page.readings.len() <= 200);
            seen.extend(page.readings.iter().map(|reading| reading.id.clone()));
            before = page.readings.last().map(|reading| reading.id.clone());
            if !page.more {
                break;
            }
        }
        let expected: Vec<String> = (0..450)
            .rev()
            .map(|index| format!("notes-{index}"))
            .collect();
        assert_eq!(seen, expected);
        let error = older_readings(&world.database, &notes, Some("tool-0")).unwrap_err();
        assert!(
            error.message.contains("is not one of this application's"),
            "{}",
            error.message
        );
        crate::inventory::tests::remove_now(&world.folder);
    }

    /// Also with statistics another tool gathered on the file, which change what SQLite prefers.
    #[test]
    fn every_query_of_readings_follows_an_index() {
        let world = World::new("installation-plans");
        let db = open(&world.database).unwrap();
        db.execute_batch(
            "INSERT INTO applications (id, name, lifecycle, description) VALUES ('notes', 'Atlas Notes', 'production', '')",
        )
        .unwrap();
        for index in 0..300 {
            db.execute(
                "INSERT INTO installation_readings (id, application_id, reference_id, machine_id, path, source, reader, observed_at, recorded_at, outcome, detail)
                 VALUES (?1, 'notes', ?2, 'laptop', '/opt/x', 'read', 'reader', ?3, ?3, 'missing', 'Nothing is at this path.')",
                params![format!("reading-{index}"), format!("installed-{}", index % 3), index],
            )
            .unwrap();
        }
        let older = OLDER.map(|filter| format!("{SELECT} {filter}"));
        let newest = format!("{SELECT} {NEWEST}");
        for analyzed in [false, true] {
            if analyzed {
                db.execute_batch("ANALYZE").unwrap();
            }
            let cases: [(&str, &[&dyn rusqlite::ToSql], &str); 5] = [
                (
                    &newest,
                    &[&"installed-1", &202],
                    "installation_readings_reference_id",
                ),
                (
                    &older[0],
                    &[&"notes", &9, &9, &9, &1],
                    "installation_readings_application_id",
                ),
                (
                    &older[1],
                    &[&"host", &9, &9, &9, &1],
                    "installation_readings_resource_id",
                ),
                // What `history_of` asks before a system is deleted.
                (
                    "SELECT EXISTS(SELECT 1 FROM installation_readings WHERE application_id = ?1)",
                    &[&"notes"],
                    "installation_readings_application_id",
                ),
                (
                    "SELECT EXISTS(SELECT 1 FROM installation_readings WHERE resource_id = ?1)",
                    &[&"host"],
                    "installation_readings_resource_id",
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
                    "analyzed {analyzed}: {sql}\n{steps:#?}"
                );
                // Never the whole table, and never a sort of everything the index found: only of
                // the readings made at one same instant, which the index leaves together.
                assert!(
                    steps
                        .iter()
                        .all(|step| !step.starts_with("SCAN installation_readings")
                            && step != "USE TEMP B-TREE FOR ORDER BY"),
                    "analyzed {analyzed}: {sql}\n{steps:#?}"
                );
            }
        }
        drop(db);
        crate::inventory::tests::remove_now(&world.folder);
    }

    /// The installation of «notes» on the laptop, read, whose file declared `version` and was
    /// last modified at `modified`.
    fn read_installation(version: Option<&str>, modified: Option<&str>) -> Installation {
        let latest: InstallationReading = serde_json::from_value(serde_json::json!({
            "id": "installed-reading", "subject": { "type": "application", "id": "notes" },
            "referenceId": "installed", "machineId": "laptop", "path": "C:\\Notes\\notes.exe",
            "source": "read", "reader": "test", "observedAt": "2026-11-24T10:00:00Z",
            "recordedAt": "2026-11-24T10:00:00Z", "outcome": "read",
            "productVersion": version, "modifiedAt": modified,
        }))
        .unwrap();
        Installation {
            reference_id: "installed".into(),
            subject: latest.subject.clone(),
            machine_id: "laptop".into(),
            path: latest.path.clone(),
            latest: Some(latest),
            moved_since_read: false,
            version: None,
            build: None,
            against_copy: None,
        }
    }

    /// A copy of «notes» on `machine`, read at `observed` with `head` and its newest commit.
    fn read_copy(
        reference: &str,
        machine: &str,
        observed: &str,
        head: Option<&str>,
        last_commit: Option<&str>,
        outcome: &str,
    ) -> crate::local_copies::readings::LocalCopy {
        let latest: crate::local_copies::readings::CopyReading =
            serde_json::from_value(serde_json::json!({
                "id": format!("{reference}-reading"),
                "subject": { "type": "application", "id": "notes" },
                "referenceId": reference, "machineId": machine, "path": "C:\\src\\notes",
                "source": "read", "reader": "test", "observedAt": observed,
                "recordedAt": observed, "outcome": outcome, "head": head,
                "lastCommitAt": last_commit,
            }))
            .unwrap();
        crate::local_copies::readings::LocalCopy {
            reference_id: reference.into(),
            subject: latest.subject.clone(),
            machine_id: machine.into(),
            path: latest.path.clone(),
            latest: Some(latest),
            moved_since_read: false,
            repository: None,
            local_work: false,
        }
    }

    fn compared(
        installation: Installation,
        copies: &[crate::local_copies::readings::LocalCopy],
    ) -> Option<AgainstCopy> {
        let mut installations = [installation];
        compare_with_copies(&mut installations, copies);
        installations[0].against_copy.clone()
    }

    #[test]
    fn a_declared_commit_is_only_hexadecimal_digits_right_after_the_plus() {
        assert_eq!(
            declared_commit("0.2.2+3F9C2AB01D4E").as_deref(),
            Some("3f9c2ab01d4e")
        );
        assert_eq!(
            declared_commit("1.0+3f9c2ab.dirty").as_deref(),
            Some("3f9c2ab")
        );
        assert_eq!(
            declared_commit("1.0+3f9c2a"),
            None,
            "six digits are too few"
        );
        assert_eq!(declared_commit("1.0+build7"), None);
        assert_eq!(declared_commit("1.0.0"), None);
        assert_eq!(declared_commit(&format!("1.0+{}", "a".repeat(41))), None);
    }

    #[test]
    fn compares_the_installed_commit_with_the_newest_reading_of_a_copy_on_the_same_machine() {
        let head = "3f9c2ab01d4e00000000000000000000000000aa";
        let other = "11d2e4f000000000000000000000000000000000";
        let copies = [
            // Older reading on the laptop, HEAD then on the installed commit.
            read_copy(
                "old",
                "laptop",
                "2026-11-20T10:00:00Z",
                Some(head),
                None,
                "read",
            ),
            // The newest reading on the laptop: HEAD moved on.
            read_copy(
                "new",
                "laptop",
                "2026-11-24T09:00:00Z",
                Some(other),
                None,
                "read",
            ),
            // Newer, but not found, and on another machine: neither counts.
            read_copy(
                "gone",
                "laptop",
                "2026-11-24T11:00:00Z",
                None,
                None,
                "missing",
            ),
            read_copy(
                "desk",
                "desk",
                "2026-11-24T11:00:00Z",
                Some(head),
                None,
                "read",
            ),
        ];
        let against =
            compared(read_installation(Some("1.5.0+3f9c2ab01d4e"), None), &copies).unwrap();
        assert_eq!(against.state, "other_commit");
        assert_eq!(against.copy_reference_id, "new");
        assert_eq!(against.copy_reading_id, "new-reading");
        assert_eq!(against.installed_commit.as_deref(), Some("3f9c2ab01d4e"));
        assert_eq!(against.copy_head.as_deref(), Some(other));
        let against = compared(
            read_installation(Some("1.5.0+3f9c2ab01d4e"), None),
            &copies[..1],
        )
        .unwrap();
        assert_eq!(against.state, "head");
        // HEAD unknown (a copy without commits): nothing to say.
        let empty = [read_copy(
            "empty",
            "laptop",
            "2026-11-24T09:00:00Z",
            None,
            None,
            "read",
        )];
        let against = compared(read_installation(Some("1.5.0+3f9c2ab"), None), &empty).unwrap();
        assert_eq!(against.state, "unknown");
    }

    #[test]
    fn without_a_declared_commit_it_only_says_whether_a_newer_commit_existed() {
        let copy = |last: &str| {
            [read_copy(
                "copy",
                "laptop",
                "2026-11-24T09:00:00Z",
                None,
                Some(last),
                "read",
            )]
        };
        let installed = || read_installation(Some("0.1.0"), Some("2026-11-10T12:00:00Z"));
        let state = |copies: &[crate::local_copies::readings::LocalCopy]| {
            compared(installed(), copies).unwrap().state
        };
        assert_eq!(state(&copy("2026-11-12T08:00:00Z")), "newer_commits");
        assert_eq!(state(&copy("2026-11-10T12:00:00Z")), "not_newer");
        assert_eq!(state(&copy("2026-11-01T08:00:00Z")), "not_newer");
        let unmodified = read_installation(None, None);
        assert_eq!(
            compared(unmodified, &copy("2026-11-12T08:00:00Z"))
                .unwrap()
                .state,
            "unknown"
        );
        // No copy read, or the file not read: nothing compared.
        assert_eq!(compared(installed(), &[]), None);
        let mut missing = installed();
        missing.latest.as_mut().unwrap().outcome = "missing".into();
        assert_eq!(compared(missing, &copy("2026-11-12T08:00:00Z")), None);
    }
}
