//! Whether agents may use the agent gate, and how far: `off` (the default), `read`, or `write`.
//! The person decides it at the window; the gate (another process) reads it on every call, so it
//! lives in the inventory, which both open. It is a statement of intent, not a security barrier:
//! the inventory is the person's own file on their own disk.

use crate::inventory::{self, allowed, plain, ApiError};
use crate::records::Actor;
use crate::records::{format_time, now_ms};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::path::Path;

pub(crate) const MODES: [&str; 3] = ["off", "read", "write"];

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

/// Whether the access mode in force allows what an agent needs: `read` or `write`.
pub(crate) fn permitted(mode: &str, need: &str) -> Result<(), ApiError> {
    let enough = match need {
        "read" => mode != "off",
        _ => mode == "write",
    };
    match (enough, mode) {
        (true, _) => Ok(()),
        (false, "off") => Err(ApiError::denied(
            "The person has not opened the agent gate. They can choose Read only or Read and write in DevHQ, under Settings → Agent gate.",
        )),
        (false, _) => Err(ApiError::denied(
            "The person opened the agent gate for reading only. To let agents change the inventory, they can choose Read and write in DevHQ, under Settings → Agent gate.",
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
}
