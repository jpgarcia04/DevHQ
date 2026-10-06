//! «What needs my attention?», answered in Rust for the agent gate, which has no window to ask.
//! The window answers it in `src/domain/evidence.ts`; both are held to the same fixtures in
//! `contract/attention/`, so the gate and the window can never disagree. Every rule below mirrors
//! one there, down to how ties are broken.
//!
//! - A subject's evidence is its newest observation (by observed time, then recorded time; the
//!   first of identical ones), which supersedes older ones even while they are still valid. It
//!   is current while its validity ends after now, and expired from that instant on.
//! - Never observed is a coverage gap, not a result: it needs no attention, but an inventory with
//!   such a gap is never reported as covered.
//! - Urgency: critical incidents, observed failures, warnings, degraded results, expired
//!   problems, informational incidents, then expired healthy results; within a rank, the newest
//!   first, and otherwise incidents in the order given and then subjects in the order given.

use crate::inventory::ApiError;
use crate::records::{parse_time, Incident, Observation, Subject};
use serde::Serialize;
use std::cmp::Reverse;

/// Something that needs attention: an incident that is not resolved, or a subject whose newest
/// observation reports a problem or has expired.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AttentionItem {
    Incident {
        subject: Subject,
        incident: Incident,
    },
    Evidence {
        subject: Subject,
        /// `degraded`, `unhealthy` or `stale` (expired, whatever it said).
        state: &'static str,
        /// The newest observation, which the state is about.
        observation: Observation,
    },
}

/// How many subjects have current evidence, expired evidence or none at all.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub current: usize,
    pub stale: usize,
    pub unobserved: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    /// Most urgent first.
    pub items: Vec<AttentionItem>,
    pub coverage: Coverage,
    /// `empty` (nothing to observe), `attention` (something above), `unobserved` (nothing above,
    /// but a subject was never observed) or `covered` (every subject has current evidence and
    /// nothing needs attention). Only `covered` is reassuring.
    pub status: &'static str,
}

/// A time read from the inventory. One that cannot be read is a damaged record, never a guess.
fn millis(value: &str) -> Result<i64, ApiError> {
    parse_time(value, "A stored time").map_err(|error| ApiError::database(error.message))
}

/// The subject's newest observation: the latest observed, then the latest recorded, and among
/// identical ones the first given, as a stable sort newest first would leave it.
fn newest<'a>(
    observations: &'a [Observation],
    subject: &Subject,
) -> Result<Option<&'a Observation>, ApiError> {
    let mut newest: Option<(&Observation, (i64, i64))> = None;
    for observation in observations.iter().filter(|item| &item.subject == subject) {
        let key = (
            millis(&observation.observed_at)?,
            millis(&observation.recorded_at)?,
        );
        if newest.is_none_or(|(_, best)| key > best) {
            newest = Some((observation, key));
        }
    }
    Ok(newest.map(|(observation, _)| observation))
}

/// The subject's evidence state (`unobserved`, `stale`, or the result it reports) and the
/// observation it comes from.
pub fn evidence<'a>(
    observations: &'a [Observation],
    subject: &Subject,
    now: i64,
) -> Result<(&'static str, Option<&'a Observation>), ApiError> {
    let Some(latest) = newest(observations, subject)? else {
        return Ok(("unobserved", None));
    };
    let state = if millis(&latest.valid_until)? > now {
        match latest.result.as_str() {
            "healthy" => "healthy",
            "degraded" => "degraded",
            "unhealthy" => "unhealthy",
            other => {
                return Err(ApiError::database(format!(
                    "unknown observation result {other}"
                )))
            }
        }
    } else {
        "stale"
    };
    Ok((state, Some(latest)))
}

fn incident_rank(severity: &str) -> u8 {
    match severity {
        "critical" => 0,
        "warning" => 2,
        _ => 5,
    }
}

/// Healthy evidence needs no attention; never observed subjects are a coverage gap instead.
fn evidence_rank(state: &str, latest: &Observation) -> Option<u8> {
    match state {
        "unhealthy" => Some(1),
        "degraded" => Some(3),
        "stale" if latest.result == "healthy" => Some(6),
        "stale" => Some(4),
        _ => None,
    }
}

/// What needs attention among `subjects`, how well they are covered, and what that adds up to,
/// at `now` (milliseconds since the Unix epoch).
pub fn assess(
    subjects: &[Subject],
    observations: &[Observation],
    incidents: &[Incident],
    now: i64,
) -> Result<Attention, ApiError> {
    let mut ranked = Vec::new();
    for incident in incidents.iter().filter(|item| item.status != "resolved") {
        ranked.push((
            incident_rank(&incident.severity),
            millis(&incident.opened_at)?,
            AttentionItem::Incident {
                subject: incident.subject.clone(),
                incident: incident.clone(),
            },
        ));
    }
    let mut coverage = Coverage::default();
    for subject in subjects {
        let (state, latest) = evidence(observations, subject, now)?;
        match state {
            "unobserved" => coverage.unobserved += 1,
            "stale" => coverage.stale += 1,
            _ => coverage.current += 1,
        }
        let Some(latest) = latest else { continue };
        if let Some(rank) = evidence_rank(state, latest) {
            ranked.push((
                rank,
                millis(&latest.observed_at)?,
                AttentionItem::Evidence {
                    subject: subject.clone(),
                    state,
                    observation: latest.clone(),
                },
            ));
        }
    }
    // Stable: equal rank and time keep the order they were gathered in.
    ranked.sort_by_key(|(rank, at, _)| (*rank, Reverse(*at)));
    let items: Vec<_> = ranked.into_iter().map(|(_, _, item)| item).collect();
    let status = if subjects.is_empty() {
        "empty"
    } else if !items.is_empty() {
        "attention"
    } else if coverage.unobserved > 0 {
        "unobserved"
    } else {
        "covered"
    };
    Ok(Attention {
        items,
        coverage,
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc_contract::fixtures;
    use serde::Deserialize;
    use serde_json::{json, Value};

    #[derive(Deserialize)]
    struct Case {
        now: String,
        subjects: Vec<Subject>,
        observations: Vec<Observation>,
        incidents: Vec<Incident>,
        expected: Value,
    }

    /// The fixtures' compact form of an item: what it is about, not everything it carries.
    fn compact(item: &AttentionItem) -> Value {
        match item {
            AttentionItem::Incident { subject, incident } => json!({
                "kind": "incident",
                "subject": subject,
                "incidentId": incident.id,
            }),
            AttentionItem::Evidence {
                subject,
                state,
                observation,
            } => json!({
                "kind": "evidence",
                "subject": subject,
                "state": state,
                "observationId": observation.id,
            }),
        }
    }

    #[test]
    fn answers_every_attention_fixture_as_the_window_does() {
        let cases = fixtures("attention");
        assert!(!cases.is_empty());
        for (name, case) in cases {
            let case: Case = serde_json::from_value(case).unwrap_or_else(|error| {
                panic!("contract/attention/{name}.json: {error}");
            });
            let now = parse_time(&case.now, "now").unwrap();
            let attention =
                assess(&case.subjects, &case.observations, &case.incidents, now).unwrap();
            assert_eq!(
                json!({
                    "attention": attention.items.iter().map(compact).collect::<Vec<_>>(),
                    "coverage": attention.coverage,
                    "status": attention.status,
                }),
                case.expected,
                "contract/attention/{name}.json"
            );
        }
    }

    #[test]
    fn an_unreadable_stored_time_is_an_error_not_a_guess() {
        let observation = Observation {
            id: "broken".into(),
            subject: Subject::Resource { id: "host".into() },
            result: "healthy".into(),
            summary: "ok".into(),
            source: "manual".into(),
            observed_at: "yesterday".into(),
            valid_until: "2026-01-01T00:00:00Z".into(),
            recorded_at: "2026-01-01T00:00:00Z".into(),
            http: None,
            agent: None,
        };
        let subjects = [observation.subject.clone()];
        let error = assess(&subjects, &[observation], &[], 0).unwrap_err();
        assert_eq!(error.code, "database_error");
    }
}
