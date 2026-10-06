//! What agents changed through the agent gate in the inventory's configuration (applications,
//! resources, components and HTTP checks), which has no `source` of its own. Each entry is
//! written in the same transaction as its change, so there is never a change without its entry
//! or an entry without its change. Entries are never changed or deleted, and outlive the subject
//! they name.

use crate::inventory::ApiError;
use crate::records::{now_ms, time_at, Subject};
use rusqlite::{params, Row, Transaction};
use serde::Serialize;
use uuid::Uuid;

/// Longest summary kept, in characters as the window counts them; longer ones are cut with «…».
const MAX_SUMMARY: usize = 500;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentChange {
    pub id: String,
    pub subject: Subject,
    /// The subject's name when the change was made: it may since have been renamed or deleted.
    pub subject_name: String,
    /// The agent, as its client named itself (unverified).
    pub agent: String,
    /// created, updated, deleted, check_saved, check_paused, check_resumed, check_removed or
    /// check_run.
    pub action: String,
    pub summary: String,
    pub occurred_at: String,
}

fn change_from_row(row: &Row) -> rusqlite::Result<AgentChange> {
    Ok(AgentChange {
        id: row.get(0)?,
        subject: Subject::from_columns(row.get(1)?, row.get(2)?)?,
        subject_name: row.get(3)?,
        agent: row.get(4)?,
        action: row.get(5)?,
        summary: row.get(6)?,
        occurred_at: time_at(row, 7)?,
    })
}

const SELECT: &str = "SELECT id, application_id, resource_id, subject_name, agent, action, summary, occurred_at FROM agent_log";

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
pub(crate) fn record(
    tx: &Transaction,
    agent: &str,
    action: &str,
    subject: &Subject,
    subject_name: &str,
    summary: &str,
) -> Result<AgentChange, ApiError> {
    let id = Uuid::new_v4().to_string();
    let (application_id, resource_id) = subject.columns();
    tx.execute(
        "INSERT INTO agent_log (id, application_id, resource_id, subject_name, agent, action, summary, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, application_id, resource_id, subject_name, agent, action, bounded(summary), now_ms()],
    )
    .map_err(ApiError::database)?;
    tx.query_row(&format!("{SELECT} WHERE id = ?1"), [&id], change_from_row)
        .map_err(ApiError::database)
}

/// Every change, newest first. They grow only when a person asks an agent for something, like
/// manual records, so a snapshot carries them all.
pub(crate) fn load(tx: &Transaction) -> Result<Vec<AgentChange>, ApiError> {
    let mut statement = tx
        .prepare(&format!("{SELECT} ORDER BY occurred_at DESC, rowid DESC"))
        .map_err(ApiError::database)?;
    let rows = statement
        .query_map([], change_from_row)
        .map_err(ApiError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApiError::database);
    rows
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
}
