//! Which workstation this computer is. A `workstation` resource is shared inventory; that this
//! installation runs on that resource is not, so it lives outside `inventory.sqlite3`, in
//! `this-workstation.json` in DevHQ's local data folder (`%LOCALAPPDATA%\com.devhq.desktop` on
//! Windows, which a roaming profile does not carry; `com.devhq.desktop.dev` for development data,
//! see `identity`). Back up now does not copy it. Only the window changes it; the agent gate only
//! reads it, as it reads the access mode.
//!
//! The file keeps, per computer, the resource chosen in each inventory and when. A computer is
//! told apart by a fingerprint: the SHA-256 of a DevHQ label and the system's machine identifier
//! (`MachineGuid` on Windows, `/etc/machine-id` on Linux), never the identifier itself. On Linux
//! that folder is the inventory's own (`~/.local/share/<identifier>`), and copying a whole
//! home to another computer, or sharing one between two, is common: without the fingerprint the
//! other computer would take itself for this one and keep readings of its own paths as this one's.
//! With it, another computer finds no entry of its own and has simply not chosen yet, and two that
//! share a home live in the same file.

use crate::inventory::{plain, stored_at, ApiError, Resource};
use crate::records::{format_time, now_ms};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const FILE: &str = "this-workstation.json";
/// Choices a computer keeps, one per inventory it chose in; the oldest go first.
const MAX_CHOICES: usize = 50;

/// Which workstation this computer is, or why DevHQ does not know.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThisWorkstation {
    /// The resource, when known: chosen on this computer, in this inventory, and a workstation.
    pub resource_id: Option<String>,
    /// When it was chosen.
    pub chosen_at: Option<String>,
    /// Why it is unknown: `not_chosen`, `not_in_inventory`, `not_a_workstation`, `no_machine_id`
    /// or `unreadable`.
    pub problem: Option<String>,
    /// That reason, for people.
    pub message: Option<String>,
    /// Where the association is kept, and where the system really keeps it when that differs (a
    /// packaged app sees a redirected folder; see `inventory::stored_at`).
    pub file: String,
    pub stored_at: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Stored {
    computers: Vec<StoredComputer>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredComputer {
    fingerprint: String,
    /// Newest last.
    choices: Vec<Choice>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Choice {
    resource_id: String,
    chosen_at: String,
}

/// The file of the association, in the local data folder `local`.
pub(crate) fn file_in(local: &Path) -> PathBuf {
    local.join(FILE)
}

/// A computer as DevHQ tells it apart: its local data folder, which holds the association, and
/// its fingerprint. The window and the gate use this one; tests make their own.
#[derive(Debug, Clone)]
pub(crate) struct Computer {
    pub local: PathBuf,
    pub fingerprint: Option<String>,
}

impl Computer {
    /// This computer, with its association in `local`.
    pub(crate) fn this_one(local: PathBuf) -> Self {
        Self {
            local,
            fingerprint: fingerprint(),
        }
    }

    /// Which workstation this computer is in an inventory with `resources`.
    pub(crate) fn resolve(&self, resources: &[Resource]) -> ThisWorkstation {
        resolve_as(&self.local, self.fingerprint.as_deref(), resources)
    }

    /// Chooses `resource_id` as this computer in the inventory with `resources`, or forgets the
    /// choice with None. Only the window does this.
    pub(crate) fn choose(
        &self,
        resources: &[Resource],
        resource_id: Option<&str>,
    ) -> Result<ThisWorkstation, ApiError> {
        choose_as(
            &self.local,
            self.fingerprint.as_deref(),
            resources,
            resource_id,
            now_ms(),
        )
    }
}

/// This computer's fingerprint, or None when the system gives no machine identifier.
pub(crate) fn fingerprint() -> Option<String> {
    machine_id().map(|id| fingerprint_of(&id))
}

fn fingerprint_of(machine_id: &str) -> String {
    let digest = Sha256::new()
        .chain_update(b"DevHQ this workstation\0")
        .chain_update(machine_id.trim().as_bytes())
        .finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(windows)]
fn machine_id() -> Option<String> {
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY,
    };
    let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (key, value) = (
        wide(r"SOFTWARE\Microsoft\Cryptography"),
        wide("MachineGuid"),
    );
    let mut buffer = [0u16; 128];
    let mut size = u32::try_from(std::mem::size_of_val(&buffer)).ok()?;
    // SAFETY: the key and value names are NUL-terminated, and `size` is the buffer's size in
    // bytes, which RegGetValueW never writes past.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 {
        return None;
    }
    let length = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    let id = String::from_utf16(&buffer[..length]).ok()?;
    (!id.trim().is_empty()).then_some(id)
}

#[cfg(target_os = "linux")]
fn machine_id() -> Option<String> {
    ["/etc/machine-id", "/var/lib/dbus/machine-id"]
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .map(|id| id.trim().to_owned())
        .find(|id| !id.is_empty())
}

#[cfg(not(any(windows, target_os = "linux")))]
fn machine_id() -> Option<String> {
    None
}

fn load(file: &Path) -> Result<Stored, String> {
    match std::fs::read_to_string(file) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| format!("{} cannot be read: {error}", plain(file))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Stored::default()),
        Err(error) => Err(format!("{} cannot be read: {error}", plain(file))),
    }
}

/// Which workstation the computer whose fingerprint is `fingerprint` is in an inventory with
/// `resources`, by the association in `local`.
fn resolve_as(local: &Path, fingerprint: Option<&str>, resources: &[Resource]) -> ThisWorkstation {
    let file = file_in(local);
    let unknown = |problem: &str, message: String| ThisWorkstation {
        resource_id: None,
        chosen_at: None,
        problem: Some(problem.to_owned()),
        message: Some(message),
        file: plain(&file),
        stored_at: stored_at(&file),
    };
    let Some(fingerprint) = fingerprint else {
        return unknown(
            "no_machine_id",
            "This system gives DevHQ no machine identifier, so it cannot tell this computer from another.".into(),
        );
    };
    let stored = match load(&file) {
        Ok(stored) => stored,
        Err(message) => return unknown("unreadable", message),
    };
    let choices = stored
        .computers
        .iter()
        .find(|computer| computer.fingerprint == fingerprint)
        .map(|computer| computer.choices.as_slice())
        .unwrap_or_default();
    // The newest choice of a resource that is in this inventory.
    let Some((choice, resource)) = choices.iter().rev().find_map(|choice| {
        resources
            .iter()
            .find(|resource| resource.id == choice.resource_id)
            .map(|resource| (choice, resource))
    }) else {
        return if choices.is_empty() {
            unknown(
                "not_chosen",
                "No workstation has been chosen as this computer yet: choose one in Settings › This computer.".into(),
            )
        } else {
            unknown(
                "not_in_inventory",
                "The workstation chosen as this computer is not in this inventory: choose one in Settings › This computer.".into(),
            )
        };
    };
    if resource.kind != "workstation" {
        return unknown(
            "not_a_workstation",
            format!(
                "{} was chosen as this computer, but it is a {} now, not a workstation: choose a workstation in Settings › This computer.",
                resource.name, resource.kind
            ),
        );
    }
    ThisWorkstation {
        resource_id: Some(resource.id.clone()),
        chosen_at: Some(choice.chosen_at.clone()),
        problem: None,
        message: None,
        file: plain(&file),
        stored_at: stored_at(&file),
    }
}

fn choose_as(
    local: &Path,
    fingerprint: Option<&str>,
    resources: &[Resource],
    resource_id: Option<&str>,
    now: i64,
) -> Result<ThisWorkstation, ApiError> {
    let Some(fingerprint) = fingerprint else {
        return Err(ApiError::invalid(
            "This system gives DevHQ no machine identifier, so it cannot tell this computer from another.",
        ));
    };
    if let Some(id) = resource_id {
        let Some(resource) = resources.iter().find(|resource| resource.id == id) else {
            return Err(ApiError::invalid(format!(
                "Resource {id} does not exist. Choose an existing workstation."
            )));
        };
        if resource.kind != "workstation" {
            return Err(ApiError::invalid(format!(
                "{} is a {}, not a workstation.",
                resource.name, resource.kind
            )));
        }
    }
    let file = file_in(local);
    let mut stored = load(&file).map_err(|message| {
        ApiError::invalid(format!(
            "{message}. Fix or remove that file, then choose again."
        ))
    })?;
    let index = match stored
        .computers
        .iter()
        .position(|computer| computer.fingerprint == fingerprint)
    {
        Some(index) => index,
        None => {
            stored.computers.push(StoredComputer {
                fingerprint: fingerprint.to_owned(),
                choices: Vec::new(),
            });
            stored.computers.len() - 1
        }
    };
    let choices = &mut stored.computers[index].choices;
    // One choice per inventory: whatever was chosen among these resources goes.
    choices.retain(|choice| {
        !resources
            .iter()
            .any(|resource| resource.id == choice.resource_id)
    });
    if let Some(id) = resource_id {
        choices.push(Choice {
            resource_id: id.to_owned(),
            chosen_at: format_time(now)
                .ok_or_else(|| ApiError::database("the clock is out of range"))?,
        });
    }
    let excess = choices.len().saturating_sub(MAX_CHOICES);
    choices.drain(..excess);
    // Written beside, then renamed over the old file: a reader never sees half a file.
    std::fs::create_dir_all(local).map_err(ApiError::database)?;
    let partial = local.join(format!("{FILE}.partial"));
    let text = serde_json::to_string_pretty(&stored).map_err(ApiError::database)?;
    std::fs::write(&partial, text).map_err(ApiError::database)?;
    std::fs::rename(&partial, &file).map_err(ApiError::database)?;
    Ok(resolve_as(local, Some(fingerprint), resources))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(id: &str, kind: &str) -> Resource {
        Resource {
            id: id.into(),
            name: id.to_uppercase(),
            kind: kind.into(),
            description: String::new(),
            operating_system: String::new(),
            references: vec![],
        }
    }

    fn computer(local: &Path, fingerprint: &str) -> Computer {
        Computer {
            local: local.to_owned(),
            fingerprint: Some(fingerprint.into()),
        }
    }

    #[test]
    fn each_computer_knows_only_what_was_chosen_on_it() {
        let local = crate::local_copies::tests::folder("this-workstation");
        let laptop = computer(&local, "laptop-print");
        let desktop = computer(&local, "desktop-print");
        let mut resources = vec![
            resource("laptop", "workstation"),
            resource("desktop", "workstation"),
            resource("vps", "server"),
        ];
        assert_eq!(
            laptop.resolve(&resources).problem.as_deref(),
            Some("not_chosen")
        );
        let chosen = laptop.choose(&resources, Some("laptop")).unwrap();
        assert_eq!(chosen.resource_id.as_deref(), Some("laptop"));
        assert!(chosen.chosen_at.is_some());
        // The same file, copied or shared, means nothing to another computer…
        assert_eq!(
            desktop.resolve(&resources).problem.as_deref(),
            Some("not_chosen")
        );
        // …which chooses for itself without taking it from the first.
        desktop.choose(&resources, Some("desktop")).unwrap();
        assert_eq!(
            laptop.resolve(&resources).resource_id.as_deref(),
            Some("laptop")
        );
        assert_eq!(
            desktop.resolve(&resources).resource_id.as_deref(),
            Some("desktop")
        );
        // Choosing again replaces the choice in this inventory; null forgets it.
        laptop.choose(&resources, Some("desktop")).unwrap();
        assert_eq!(
            laptop.resolve(&resources).resource_id.as_deref(),
            Some("desktop")
        );
        laptop.choose(&resources, None).unwrap();
        assert_eq!(
            laptop.resolve(&resources).problem.as_deref(),
            Some("not_chosen")
        );
        // Only a workstation that exists can be chosen.
        for (id, message) in [("vps", "VPS is a server"), ("ghost", "does not exist")] {
            let error = laptop.choose(&resources, Some(id)).unwrap_err();
            assert!(error.message.contains(message), "{}", error.message);
        }
        // What was chosen stops counting when it leaves the inventory, or stops being a
        // workstation, and says so.
        laptop.choose(&resources, Some("laptop")).unwrap();
        resources[0].kind = "server".into();
        let this = laptop.resolve(&resources);
        assert_eq!(
            (this.resource_id, this.problem.as_deref()),
            (None, Some("not_a_workstation"))
        );
        resources.remove(0);
        assert_eq!(
            laptop.resolve(&resources).problem.as_deref(),
            Some("not_in_inventory")
        );
        // In another inventory, the choice of this one does not exist.
        let other = [resource("box", "workstation")];
        assert_eq!(
            laptop.resolve(&other).problem.as_deref(),
            Some("not_in_inventory")
        );
        // The file says nothing of the machine but a fingerprint.
        let text = std::fs::read_to_string(file_in(&local)).unwrap();
        assert!(text.contains("laptop-print") && text.contains("desktop-print"));
        crate::inventory::tests::remove_now(local);
    }

    #[test]
    fn a_file_it_cannot_read_is_said_and_never_overwritten() {
        let local = crate::local_copies::tests::folder("this-workstation-broken");
        std::fs::write(file_in(&local), "{ not json").unwrap();
        let laptop = computer(&local, "laptop-print");
        let resources = [resource("laptop", "workstation")];
        assert_eq!(
            laptop.resolve(&resources).problem.as_deref(),
            Some("unreadable")
        );
        let error = laptop.choose(&resources, Some("laptop")).unwrap_err();
        assert!(
            error.message.contains("Fix or remove that file"),
            "{}",
            error.message
        );
        assert_eq!(
            std::fs::read_to_string(file_in(&local)).unwrap(),
            "{ not json"
        );
        // Without a machine identifier there is nothing to tell computers apart by.
        let anonymous = Computer {
            local: local.clone(),
            fingerprint: None,
        };
        assert_eq!(
            anonymous.resolve(&resources).problem.as_deref(),
            Some("no_machine_id")
        );
        assert!(anonymous.choose(&resources, Some("laptop")).is_err());
        crate::inventory::tests::remove_now(local);
    }

    #[test]
    fn this_computer_has_a_fingerprint_that_never_shows_its_identifier() {
        let id = machine_id();
        // Windows always has one; a Linux system has /etc/machine-id unless a bare container.
        if cfg!(windows) || std::path::Path::new("/etc/machine-id").exists() {
            assert!(id.is_some());
        }
        if let Some(id) = id {
            let print = fingerprint().unwrap();
            assert_eq!(print, fingerprint_of(&id));
            assert_eq!(print.len(), 64);
            assert!(!print.contains(id.trim()));
            assert_ne!(fingerprint_of("one"), fingerprint_of("two"));
        }
    }
}
