//! Whether agents may use the agent gate, and how far: `off` (the default), `read`, `maintain`
//! (reading; adding and editing applications and resources with their components and references;
//! reading local copies; and recording what the agent observed, vouching for at most a week), or
//! `write` (everything the gate offers). The person decides it at the window; the
//! gate (another process) reads it on every call, so it lives in the inventory, which both open.
//! It holds until the person changes it, for every agent session that opens the inventory: it is
//! never an approval of one change. It is a statement of intent, not a security barrier: the
//! inventory is the person's own file on their own disk.

use crate::inventory::{self, allowed, plain, ApiError};
use crate::records::Actor;
use crate::records::{format_time, now_ms};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::path::Path;

pub(crate) const MODES: [&str; 4] = ["off", "read", "maintain", "write"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentAccess {
    pub mode: String,
    /// When the person last changed it; None while it has never been changed (and is off).
    pub changed_at: Option<String>,
    /// This executable, for the command that registers the gate with an agent. None if the
    /// system cannot say.
    pub executable: Option<String>,
}

/// How long an agent's observation may vouch for a system under Read and maintain: a week, so that
/// a review nobody repeats expires and shows as a gap again. Read and write allows what the
/// person's own observations do (`records::MAX_VALIDITY_MINUTES`).
pub(crate) const MAINTAIN_VALIDITY_MINUTES: i64 = 7 * 24 * 60;

/// Whether an agent's observation may vouch for `minutes` under the access mode in force.
pub(crate) fn observation_validity(mode: &str, minutes: i64) -> Result<(), ApiError> {
    if mode == "maintain" && minutes > MAINTAIN_VALIDITY_MINUTES {
        return Err(ApiError::denied(format!(
            "With Read and maintain, an agent's observation vouches for at most 7 days (validForMinutes up to {MAINTAIN_VALIDITY_MINUTES}). Record it for 7 days or less; only Read and write, which only the person chooses in DevHQ under Settings → Agent gate, allows up to 30 days."
        )));
    }
    Ok(())
}

/// Whether the access mode in force allows what an agent needs: `read`, `maintain` (adding and
/// editing applications and resources, reading local copies and recording observations) or
/// `write` (anything else that changes something). Each mode allows what the ones before it do.
pub(crate) fn permitted(mode: &str, need: &str) -> Result<(), ApiError> {
    let enough = match need {
        "read" => mode != "off",
        "maintain" => mode == "maintain" || mode == "write",
        _ => mode == "write",
    };
    match (enough, mode, need) {
        (true, _, _) => Ok(()),
        (false, "off", _) => Err(ApiError::denied(
            "The person has not opened the agent gate. They can choose Read only, Read and maintain or Read and write in DevHQ, under Settings → Agent gate.",
        )),
        (false, "read", "maintain") => Err(ApiError::denied(
            "The person opened the agent gate for reading only. To let agents add and edit applications and resources, read local copies and record what they observed, they can choose Read and maintain in DevHQ, under Settings → Agent gate.",
        )),
        (false, "read", _) => Err(ApiError::denied(
            "The person opened the agent gate for reading only. This needs Read and write, which only the person chooses, in DevHQ under Settings → Agent gate; or the person can do it there.",
        )),
        (false, _, _) => Err(ApiError::denied(
            "The person lets agents read and maintain the inventory (add and edit applications and resources, with their components and references; read local copies; record what they observed), not this. This needs Read and write, which only the person chooses, in DevHQ under Settings → Agent gate; or the person can do it there.",
        )),
    }
}

/// The mode in force, read with the caller's connection.
pub(crate) fn mode(db: &Connection) -> Result<String, ApiError> {
    Ok(stored(db)?.map_or_else(|| "off".to_owned(), |(mode, _)| mode))
}

fn stored(db: &Connection) -> Result<Option<(String, i64)>, ApiError> {
    db.query_row(
        "SELECT mode, changed_at FROM agent_access WHERE id = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(ApiError::database)
}

fn answer(db: &Connection) -> Result<AgentAccess, ApiError> {
    let stored = stored(db)?;
    Ok(AgentAccess {
        mode: stored
            .as_ref()
            .map_or_else(|| "off".to_owned(), |(mode, _)| mode.clone()),
        changed_at: stored.and_then(|(_, at)| format_time(at)),
        executable: std::env::current_exe().ok().map(|path| plain(&path)),
    })
}

pub fn agent_access(path: &Path) -> Result<AgentAccess, ApiError> {
    let mut db = inventory::open_current(path)?;
    let tx = inventory::begin(&mut db, TransactionBehavior::Deferred)?;
    answer(&tx)
}

/// Only the person changes it, at the window: never an agent.
pub fn set_agent_access(path: &Path, mode: &str) -> Result<AgentAccess, ApiError> {
    allowed(mode, &MODES, "agent gate access")?;
    inventory::write(path, &Actor::Person, |tx| {
        tx.execute(
            "INSERT INTO agent_access (id, mode, changed_at) VALUES (1, ?1, ?2)
             ON CONFLICT (id) DO UPDATE SET mode = excluded.mode, changed_at = excluded.changed_at",
            params![mode, now_ms()],
        )
        .map_err(ApiError::database)?;
        answer(tx)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::open;

    fn path() -> std::path::PathBuf {
        let path = crate::inventory::tests::disposable(std::env::temp_dir().join(format!(
            "devhq-access-test-{}.sqlite3",
            uuid::Uuid::new_v4()
        )));
        inventory::start(&path).unwrap();
        path
    }

    #[test]
    fn starts_off_and_keeps_what_the_person_chose() {
        let path = path();
        let initial = agent_access(&path).unwrap();
        assert_eq!((initial.mode.as_str(), initial.changed_at), ("off", None));
        assert!(initial.executable.is_some());
        let read = set_agent_access(&path, "read").unwrap();
        assert_eq!(read.mode, "read");
        assert!(read.changed_at.is_some());
        assert_eq!(set_agent_access(&path, "write").unwrap().mode, "write");
        assert_eq!(mode(&open(&path).unwrap()).unwrap(), "write");
        let refused = set_agent_access(&path, "admin").unwrap_err();
        assert_eq!(refused.code, "invalid_input");
        assert_eq!(agent_access(&path).unwrap().mode, "write");
        // One row, whatever happens.
        let rows: i64 = open(&path)
            .unwrap()
            .query_row("SELECT count(*) FROM agent_access", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);
        assert!(open(&path)
            .unwrap()
            .execute("INSERT INTO agent_access VALUES (2, 'read', 0)", [])
            .is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn each_mode_allows_what_the_ones_before_it_do_and_holds_until_changed() {
        let allows = |mode: &str| -> Vec<&str> {
            ["read", "maintain", "write"]
                .into_iter()
                .filter(|need| permitted(mode, need).is_ok())
                .collect()
        };
        assert_eq!(allows("off"), Vec::<&str>::new());
        assert_eq!(allows("read"), ["read"]);
        assert_eq!(allows("maintain"), ["read", "maintain"]);
        assert_eq!(allows("write"), ["read", "maintain", "write"]);
        // Each refusal says what the person can choose, and only the person.
        for (mode, need, says) in [
            ("off", "read", "has not opened the agent gate"),
            ("read", "maintain", "choose Read and maintain"),
            ("read", "write", "This needs Read and write"),
            ("maintain", "write", "This needs Read and write"),
        ] {
            let refused = permitted(mode, need).unwrap_err();
            assert_eq!(refused.code, "access_denied");
            assert!(
                refused.message.contains(says),
                "{mode}/{need}: {}",
                refused.message
            );
        }
        // An agent's observation vouches for a week at most under Read and maintain; Read and
        // write allows what the person's own observations do.
        assert!(observation_validity("maintain", MAINTAIN_VALIDITY_MINUTES).is_ok());
        let longer = observation_validity("maintain", MAINTAIN_VALIDITY_MINUTES + 1).unwrap_err();
        assert_eq!(longer.code, "access_denied");
        assert!(observation_validity("write", crate::records::MAX_VALIDITY_MINUTES).is_ok());

        // Kept in the inventory: another connection, like the gate's, reads the same mode.
        let path = path();
        let chosen = set_agent_access(&path, "maintain").unwrap();
        assert_eq!(chosen.mode, "maintain");
        assert_eq!(mode(&open(&path).unwrap()).unwrap(), "maintain");
        assert_eq!(agent_access(&path).unwrap(), chosen);
        std::fs::remove_file(path).unwrap();
    }
}
