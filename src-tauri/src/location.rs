//! Whether DevHQ may create or change the inventory where it looks for it, given where the system
//! keeps that file and what DevHQ's previous location still holds.
//!
//! DevHQ used to keep its data in AppData (`identity::previous_database`), which Windows redirects
//! for a gate started from a packaged app. It now keeps it in a folder of the home. DevHQ never
//! moves, opens or changes the inventory at the previous location: the person copies it, once, and
//! renames the previous folder after checking the copy (README «Trasladar el inventario fuera de
//! AppData»). Until then, two rules keep a change from landing in the wrong inventory:
//!
//! - The window never starts an empty inventory at the new location while the previous one still
//!   holds data (`refuse_to_create`): it would look like the person's and hold none of it.
//! - Agents cannot write through the gate while the previous location holds an inventory of the
//!   person's (`refuse_agent_write`): until that folder is renamed, two files look like theirs, and
//!   an agent's change must land in the one the person checked. Nor when the system keeps the file
//!   somewhere other than where DevHQ asked (a link, a junction or a packaged app's redirection,
//!   `inventory::stored_at`), where a DevHQ started another way may see another inventory.
//!
//! Both rules only look at fixed paths, never search a disk, and never write anything.

use crate::inventory::{stored_at, ApiError};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// The files that hold an inventory's data: the database, and the write-ahead log or rollback
/// journal SQLite would apply to it. A shared-memory file (`-shm`) alone holds none.
const DATA_SUFFIXES: [&str; 3] = ["", "-wal", "-journal"];

/// What DevHQ's previous location still holds.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Previous {
    /// The inventory's path there, as DevHQ asks for it.
    pub database: String,
    /// The files of the inventory found there, by name.
    pub files: Vec<String>,
    /// Where the system really keeps the first of them, when that is another path. Seen only
    /// through a packaged app's redirection, the file is that app's own copy of AppData, not the
    /// person's folder; the person's folder may hold another one, or none.
    pub stored_at: Option<String>,
}

fn with_suffix(database: &Path, suffix: &str) -> PathBuf {
    let mut name = database.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn data_files(database: &Path) -> Vec<PathBuf> {
    DATA_SUFFIXES
        .iter()
        .map(|suffix| with_suffix(database, suffix))
        .filter(|file| file.is_file())
        .collect()
}

/// What the inventory at `previous`, DevHQ's previous location, holds; None when it holds nothing.
pub(crate) fn previous_at(previous: &Path) -> Option<Previous> {
    let files = data_files(previous);
    let first = files.first()?;
    Some(Previous {
        database: previous.display().to_string(),
        files: files
            .iter()
            .filter_map(|file| file.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect(),
        stored_at: stored_at(first),
    })
}

/// Why the window does not start an inventory at `database`: nothing is there yet, and the
/// previous location still holds one, which `inventory::start` would leave behind for an empty
/// one. Ok when the new location holds an inventory (`start` decides about it) or the previous
/// one holds nothing.
pub(crate) fn refuse_to_create(
    database: &Path,
    previous: Option<&Previous>,
) -> Result<(), ApiError> {
    if !data_files(database).is_empty() {
        return Ok(());
    }
    let Some(previous) = previous else {
        return Ok(());
    };
    let redirected = previous.stored_at.as_ref().map_or_else(String::new, |kept| {
        format!(
            " (the system keeps that file at {kept}: this DevHQ was started from a packaged app, which sees its own copy of AppData)"
        )
    });
    Err(ApiError {
        code: "move_pending",
        message: format!(
            "Your inventory has not been moved yet: it is still at {}{redirected}, and DevHQ now keeps it at {}. DevHQ starts no empty inventory there and does not open the previous one. Copy it as the README says («Trasladar el inventario fuera de AppData»), then start DevHQ again.",
            previous.database,
            database.display()
        ),
    })
}

/// Why an agent may not change the inventory at `database`, given what DevHQ's previous location
/// holds (None when the gate was told `--database`, or the previous location holds nothing).
/// Nothing is opened: an agent's write is refused before it starts.
pub(crate) fn refuse_agent_write(
    database: &Path,
    previous: Option<&Previous>,
) -> Result<(), ApiError> {
    if let Some(kept) = stored_at(database) {
        return Err(ApiError {
            code: "location_redirected",
            message: format!(
                "Nothing was written: the system keeps this inventory at {kept}, not at {}, where DevHQ asked for it. A link, a junction or a packaged app's redirection leads there, and a DevHQ started another way may see another inventory, so agents can read this one but not change it.",
                database.display()
            ),
        });
    }
    // A previous inventory seen only through a redirection is the packaged app's own copy: the
    // person's folder is not there to rename, and the note in `status` says so.
    if let Some(previous) = previous.filter(|previous| previous.stored_at.is_none()) {
        return Err(ApiError {
            code: "move_pending",
            message: format!(
                "Nothing was written: DevHQ's previous location still holds an inventory ({} in {}), so two files look like the person's. Agents can change the inventory at {} once the person has moved it and renamed the previous folder, as the README says («Trasladar el inventario fuera de AppData»).",
                previous.files.join(", "),
                Path::new(&previous.database)
                    .parent()
                    .map_or_else(String::new, |folder| folder.display().to_string()),
                database.display()
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new and a previous inventory path, each in a folder of its own that does not exist yet.
    /// Under the temporary folder as the file system names it: a short 8.3 name would make every
    /// file there one the system keeps elsewhere (`stored_at`).
    fn locations() -> (PathBuf, PathBuf, PathBuf) {
        let root = crate::inventory::tests::disposable(
            crate::inventory::tests::resolved_temp_dir()
                .join(format!("devhq-location-test-{}", uuid::Uuid::new_v4())),
        );
        let new = root.join("DevHQ").join("inventory.sqlite3");
        let previous = root
            .join("AppData")
            .join("com.devhq.desktop")
            .join("inventory.sqlite3");
        std::fs::create_dir_all(previous.parent().unwrap()).unwrap();
        (root, new, previous)
    }

    fn write(file: &Path) {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, b"synthetic").unwrap();
    }

    #[test]
    fn the_previous_location_holds_an_inventory_by_its_database_log_or_journal() {
        let (root, _, previous) = locations();
        assert_eq!(previous_at(&previous), None);
        // A shared-memory file alone holds no data, nor does a backups folder beside it.
        write(&with_suffix(&previous, "-shm"));
        std::fs::create_dir(previous.parent().unwrap().join("backups")).unwrap();
        assert_eq!(previous_at(&previous), None);
        for suffix in ["-journal", "-wal", ""] {
            write(&with_suffix(&previous, suffix));
            let found = previous_at(&previous).unwrap();
            assert_eq!(found.database, previous.display().to_string());
            assert_eq!(found.stored_at, None);
            assert!(found.files.contains(&format!("inventory.sqlite3{suffix}")));
        }
        assert_eq!(
            previous_at(&previous).unwrap().files,
            [
                "inventory.sqlite3",
                "inventory.sqlite3-wal",
                "inventory.sqlite3-journal"
            ]
        );
        crate::inventory::tests::remove_now(&root);
    }

    #[test]
    fn the_window_starts_no_empty_inventory_while_the_previous_one_waits() {
        let (root, new, previous) = locations();
        // Nothing anywhere: the window may create the inventory.
        assert!(refuse_to_create(&new, previous_at(&previous).as_ref()).is_ok());
        // The previous location holds one and the new none: refused, and nothing is created.
        write(&previous);
        let refused = refuse_to_create(&new, previous_at(&previous).as_ref()).unwrap_err();
        assert_eq!(refused.code, "move_pending");
        assert!(refused.message.contains(&previous.display().to_string()));
        assert!(refused.message.contains(&new.display().to_string()));
        assert!(!new.parent().unwrap().exists());
        // A log left at the new location is data there: `start` decides about it.
        write(&with_suffix(&new, "-wal"));
        assert!(refuse_to_create(&new, previous_at(&previous).as_ref()).is_ok());
        std::fs::remove_file(with_suffix(&new, "-wal")).unwrap();
        // Once moved, the new inventory is served, whatever the previous folder still holds.
        write(&new);
        assert!(refuse_to_create(&new, previous_at(&previous).as_ref()).is_ok());
        crate::inventory::tests::remove_now(&root);
    }

    #[test]
    fn agents_cannot_write_until_the_previous_folder_is_renamed() {
        let (root, new, previous) = locations();
        write(&new);
        assert!(refuse_agent_write(&new, previous_at(&previous).as_ref()).is_ok());
        write(&previous);
        let refused = refuse_agent_write(&new, previous_at(&previous).as_ref()).unwrap_err();
        assert_eq!(refused.code, "move_pending");
        assert!(refused.message.starts_with("Nothing was written"));
        // Renamed, as the person does once the copy is checked: agents may write again.
        let folder = previous.parent().unwrap();
        std::fs::rename(folder, folder.with_extension("desktop.trasladado")).unwrap();
        assert!(refuse_agent_write(&new, previous_at(&previous).as_ref()).is_ok());
        // Told `--database`, the gate has no previous location to look at.
        assert!(refuse_agent_write(&new, None).is_ok());
        crate::inventory::tests::remove_now(&root);
    }

    /// A previous inventory the system keeps elsewhere is a packaged app's copy, not the person's
    /// folder: reported, never a reason to stop agents.
    #[test]
    fn a_previous_inventory_seen_through_a_redirection_does_not_stop_agents() {
        let (root, new, previous) = locations();
        write(&new);
        let seen = Previous {
            database: previous.display().to_string(),
            files: vec!["inventory.sqlite3".into()],
            stored_at: Some("C:\\Users\\Ana\\AppData\\Local\\Packages\\Claude_x\\LocalCache\\Roaming\\com.devhq.desktop\\inventory.sqlite3".into()),
        };
        assert!(refuse_agent_write(&new, Some(&seen)).is_ok());
        // The window still starts no empty inventory beside it, and says why.
        std::fs::remove_file(&new).unwrap();
        let refused = refuse_to_create(&new, Some(&seen)).unwrap_err();
        assert!(refused.message.contains("packaged app"));
        crate::inventory::tests::remove_now(&root);
    }

    /// Reached through a link, as through Windows' AppData redirection, the inventory is somewhere
    /// else than DevHQ asked: agents cannot change it. Asked where it really is, they can.
    #[test]
    fn agents_cannot_write_to_an_inventory_the_system_keeps_elsewhere() {
        let (real, link) = crate::inventory::tests::linked_folder();
        let kept = real.join("inventory.sqlite3");
        write(&kept);
        let refused = refuse_agent_write(&link.join("inventory.sqlite3"), None).unwrap_err();
        assert_eq!(refused.code, "location_redirected");
        assert!(refused.message.contains(&kept.display().to_string()));
        assert!(refuse_agent_write(&kept, None).is_ok());
        crate::inventory::tests::remove_linked((real, link));
    }
}
