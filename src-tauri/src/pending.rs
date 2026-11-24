//! «What work is waiting for me?»: what the readings of local copies and installations say needs
//! looking at, ranked. Answered in Rust for the agent gate and in `src/features/inventory/pending.ts`
//! for the window; both are held to the same fixtures in `contract/pending-work/`, so they can never
//! disagree. Every rule below mirrors one there, down to how ties are broken.
//!
//! It is not health and never enters «Needs attention» or coverage: a reading says what a copy or a
//! file held when it was read, which may no longer be so, and each item carries that reading's
//! moment. It exists because a personal ecosystem of desktop applications has nothing an HTTP check
//! can ask, so without it «what needs my attention?» would always come back empty.
//!
//! - Each declared copy or installation gives at most one item, with every reason it has, most
//!   urgent first; the item ranks by its first reason.
//! - Reasons, most urgent first: an operation left halfway in a copy (`operation`), changed, new or
//!   unverifiable files (`changes`), commits on no remote (`unpublished`), stashes (`stashes`), an
//!   installed file that declared another version than its application (`version_differs`), not
//!   found or not readable where declared (`not_found`), never read on this workstation
//!   (`never_read`), and a reading on this workstation older than `OLD_READING_MS`
//!   (`old_reading`), which DevHQ could renew by reading it again.
//! - Ties keep the order given: copies in the inventory's order, then installations in theirs.
//! - Systems of archived applications are left out, as from Read all.

use crate::installations::readings::Installation;
use crate::inventory::ApiError;
use crate::local_copies::readings::LocalCopy;
use crate::records::{parse_time, Subject};
use serde::{Deserialize, Serialize};

/// A reading older than this, on this workstation, is worth renewing: three days.
pub const OLD_READING_MS: i64 = 3 * 24 * 60 * 60 * 1000;

/// Every reason, most urgent first: an item ranks by the position of its first one.
const REASONS: [&str; 8] = [
    "operation",
    "changes",
    "unpublished",
    "stashes",
    "version_differs",
    "not_found",
    "never_read",
    "old_reading",
];

/// What the rules need of a copy's newest reading.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyReadingFacts {
    pub outcome: String,
    pub observed_at: String,
    pub unpublished_on_head: Option<i64>,
    pub unpublished_elsewhere: Option<i64>,
    pub stashes: Option<i64>,
    pub changed: Option<i64>,
    pub untracked: Option<i64>,
    pub unverifiable: Option<i64>,
    pub operation: Option<String>,
}

/// What the rules need of a declared local copy.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyFacts {
    pub reference_id: String,
    pub subject: Subject,
    pub machine_id: String,
    pub latest: Option<CopyReadingFacts>,
}

/// What the rules need of an installation's newest reading.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationReadingFacts {
    pub outcome: String,
    pub observed_at: String,
}

/// What the rules need of a declared installation.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationFacts {
    pub reference_id: String,
    pub subject: Subject,
    pub machine_id: String,
    pub latest: Option<InstallationReadingFacts>,
    /// `matches`, `differs`, `undeclared`, `unknown`, or None (see `Installation::version`).
    pub version: Option<String>,
}

impl From<&LocalCopy> for CopyFacts {
    fn from(copy: &LocalCopy) -> Self {
        CopyFacts {
            reference_id: copy.reference_id.clone(),
            subject: copy.subject.clone(),
            machine_id: copy.machine_id.clone(),
            latest: copy.latest.as_ref().map(|reading| CopyReadingFacts {
                outcome: reading.outcome.clone(),
                observed_at: reading.observed_at.clone(),
                unpublished_on_head: reading.unpublished_on_head,
                unpublished_elsewhere: reading.unpublished_elsewhere,
                stashes: reading.stashes,
                changed: reading.changed,
                untracked: reading.untracked,
                unverifiable: reading.unverifiable,
                operation: reading.operation.clone(),
            }),
        }
    }
}

impl From<&Installation> for InstallationFacts {
    fn from(installation: &Installation) -> Self {
        InstallationFacts {
            reference_id: installation.reference_id.clone(),
            subject: installation.subject.clone(),
            machine_id: installation.machine_id.clone(),
            latest: installation
                .latest
                .as_ref()
                .map(|reading| InstallationReadingFacts {
                    outcome: reading.outcome.clone(),
                    observed_at: reading.observed_at.clone(),
                }),
            version: installation.version.clone(),
        }
    }
}

/// What a declared copy or installation is waiting for.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingItem {
    /// `localCopy` or `installation`.
    pub kind: &'static str,
    pub reference_id: String,
    pub subject: Subject,
    /// Most urgent first; never empty.
    pub reasons: Vec<&'static str>,
}

/// A time read from the inventory. One that cannot be read is a damaged record, never a guess.
fn millis(value: &str) -> Result<i64, ApiError> {
    parse_time(value, "A stored time").map_err(|error| ApiError::database(error.message))
}

/// Whether a reading on this workstation is old enough to be worth renewing.
fn old(observed_at: &str, here: bool, now: i64) -> Result<bool, ApiError> {
    Ok(here && now - millis(observed_at)? > OLD_READING_MS)
}

/// The pending work among the copies and installations given, most urgent first, at `now`
/// (milliseconds since the Unix epoch). `this` is the workstation this computer is, if DevHQ knows
/// it; `archived` names the applications that are archived.
pub fn assess(
    copies: &[CopyFacts],
    installations: &[InstallationFacts],
    this: Option<&str>,
    archived: &[String],
    now: i64,
) -> Result<Vec<PendingItem>, ApiError> {
    let left_out = |subject: &Subject| match subject {
        Subject::Application { id } => archived.contains(id),
        Subject::Resource { .. } => false,
    };
    let some = |count: Option<i64>| count.is_some_and(|count| count > 0);
    let mut items = Vec::new();
    for copy in copies.iter().filter(|copy| !left_out(&copy.subject)) {
        let here = this == Some(copy.machine_id.as_str());
        let mut reasons = Vec::new();
        match &copy.latest {
            None if here => reasons.push("never_read"),
            None => {}
            Some(reading) => {
                if reading.outcome == "read" {
                    if reading.operation.is_some() {
                        reasons.push("operation");
                    }
                    if some(reading.changed)
                        || some(reading.untracked)
                        || some(reading.unverifiable)
                    {
                        reasons.push("changes");
                    }
                    if some(reading.unpublished_on_head) || some(reading.unpublished_elsewhere) {
                        reasons.push("unpublished");
                    }
                    if some(reading.stashes) {
                        reasons.push("stashes");
                    }
                } else {
                    reasons.push("not_found");
                }
                if old(&reading.observed_at, here, now)? {
                    reasons.push("old_reading");
                }
            }
        }
        if !reasons.is_empty() {
            items.push(PendingItem {
                kind: "localCopy",
                reference_id: copy.reference_id.clone(),
                subject: copy.subject.clone(),
                reasons,
            });
        }
    }
    for installation in installations
        .iter()
        .filter(|installation| !left_out(&installation.subject))
    {
        let here = this == Some(installation.machine_id.as_str());
        let mut reasons = Vec::new();
        match &installation.latest {
            None if here => reasons.push("never_read"),
            None => {}
            Some(reading) => {
                if reading.outcome != "read" {
                    reasons.push("not_found");
                } else if installation.version.as_deref() == Some("differs") {
                    reasons.push("version_differs");
                }
                if old(&reading.observed_at, here, now)? {
                    reasons.push("old_reading");
                }
            }
        }
        if !reasons.is_empty() {
            items.push(PendingItem {
                kind: "installation",
                reference_id: installation.reference_id.clone(),
                subject: installation.subject.clone(),
                reasons,
            });
        }
    }
    let rank = |item: &PendingItem| {
        REASONS
            .iter()
            .position(|reason| *reason == item.reasons[0])
            .unwrap_or(REASONS.len())
    };
    // Stable: equal ranks keep the order they were gathered in.
    items.sort_by_key(rank);
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc_contract::fixtures;
    use serde_json::{json, Value};

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Case {
        now: String,
        this_workstation: Option<String>,
        archived: Vec<String>,
        local_copies: Vec<CopyFacts>,
        installations: Vec<InstallationFacts>,
        expected: Value,
    }

    #[test]
    fn answers_every_pending_work_fixture_as_the_window_does() {
        let cases = fixtures("pending-work");
        assert!(!cases.is_empty());
        for (name, case) in cases {
            let case: Case = serde_json::from_value(case).unwrap_or_else(|error| {
                panic!("contract/pending-work/{name}.json: {error}");
            });
            let now = parse_time(&case.now, "now").unwrap();
            let items = assess(
                &case.local_copies,
                &case.installations,
                case.this_workstation.as_deref(),
                &case.archived,
                now,
            )
            .unwrap();
            assert_eq!(
                json!(items),
                case.expected,
                "contract/pending-work/{name}.json"
            );
        }
    }

    #[test]
    fn an_unreadable_stored_time_is_an_error_not_a_guess() {
        let copy = CopyFacts {
            reference_id: "ref".into(),
            subject: Subject::Application { id: "app".into() },
            machine_id: "laptop".into(),
            latest: Some(CopyReadingFacts {
                outcome: "read".into(),
                observed_at: "yesterday".into(),
                unpublished_on_head: None,
                unpublished_elsewhere: None,
                stashes: None,
                changed: None,
                untracked: None,
                unverifiable: None,
                operation: None,
            }),
        };
        let error = assess(&[copy], &[], Some("laptop"), &[], 0).unwrap_err();
        assert_eq!(error.code, "database_error");
    }
}
