//! How many processes were running from a file, when DevHQ looked.
//!
//! This reads the system, not the file: the list of processes, and for each one the path of its
//! executable, which the system gives without a look at the process's memory or command line
//! (`PROCESS_QUERY_LIMITED_INFORMATION` on Windows, the `/proc/<pid>/exe` link on Linux). Only two
//! numbers leave this module: how many processes ran from the file, and how many the system did
//! not let DevHQ look at (another user's, a protected one) that could have: those whose
//! executable's name, which the system lists without opening the process (the snapshot's entry on
//! Windows, `/proc/<pid>/comm` on Linux), is the file's. Without that filter the second number is
//! every service and every process of the system, hundreds on a desktop, and says nothing of the
//! file. Never an identifier, a name or a path of any process.
//!
//! «Running from the file» is all it says. What a process running means depends on the program: a
//! window, a background helper, or, for DevHQ itself, an agent's gate with no window at all.

use std::path::Path;

/// Processes found for a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Count {
    /// Processes whose executable is the file, but this process.
    pub running: u32,
    /// Processes the system did not let this one look at, whose executable is named like the file:
    /// any of them may run from it.
    pub unexamined: u32,
}

/// How a path is compared with a process's: as the system compares names.
fn comparable(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\").to_lowercase()
    } else {
        path.to_owned()
    }
}

/// How many processes run from the file known by any of `names` (the path declared and the one
/// the system resolves it to), not counting this process: a reader started from the very file it
/// reads is DevHQ's instrument, not a use of the program. An error says why the system gave no
/// list.
pub(crate) fn running_from(names: &[&Path]) -> Result<Count, String> {
    let wanted: Vec<String> = names
        .iter()
        .map(|name| comparable(&name.display().to_string()))
        .collect();
    let wanted_names: Vec<String> = names
        .iter()
        .filter_map(|name| name.file_name())
        .map(|name| comparable(&name.to_string_lossy()))
        .collect();
    let mut count = Count {
        running: 0,
        unexamined: 0,
    };
    imp::each_process(&mut |seen| match seen {
        Seen::Path(path) => {
            if wanted.contains(&comparable(&path)) {
                count.running += 1;
            }
        }
        Seen::Refused(name) => {
            if name.is_none_or(|name| imp::same_name(&wanted_names, &comparable(&name))) {
                count.unexamined += 1;
            }
        }
    })?;
    Ok(count)
}

/// What the system let DevHQ see of one process other than this one.
enum Seen {
    /// The path of its executable.
    Path(String),
    /// Only the name of its executable, from the list itself, or not even that: the system
    /// refused the rest. A process whose name is unknown could be running from the file.
    Refused(Option<String>),
}

#[cfg(windows)]
mod imp {
    use super::Seen;
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// The longest path Windows gives, in UTF-16 units.
    const LONGEST_PATH: usize = 32_768;

    /// The snapshot lists each process's executable by name, whole: the names compare exactly.
    pub(super) fn same_name(wanted: &[String], listed: &str) -> bool {
        wanted.iter().any(|name| name == listed)
    }

    /// The executable's name in a snapshot entry, up to its NUL.
    fn listed_name(entry: &PROCESSENTRY32W) -> String {
        let name = &entry.szExeFile;
        let end = name
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(name.len());
        String::from_utf16_lossy(&name[..end])
    }

    pub(super) fn each_process(see: &mut dyn FnMut(Seen)) -> Result<(), String> {
        let own = std::process::id();
        // SAFETY: plain Win32 calls on a zeroed structure the API defines, whose size is set as
        // the API asks, on a buffer whose length is passed with it, and on handles the calls
        // return, each closed once.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(format!(
                    "Windows did not list its processes: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let mut entry: PROCESSENTRY32W = zeroed();
            entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snapshot, &raw mut entry) != 0;
            let mut path = vec![0_u16; LONGEST_PATH];
            while more {
                let id = entry.th32ProcessID;
                // Process 0 is the idle process, which runs from no file.
                if id != 0 && id != own {
                    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, id);
                    if process.is_null() {
                        // Refused is one thing; a process that ended since the list was taken,
                        // which Windows reports as an invalid parameter, is no process at all.
                        if GetLastError() == ERROR_ACCESS_DENIED {
                            see(Seen::Refused(Some(listed_name(&entry))));
                        }
                    } else {
                        let mut length = path.len() as u32;
                        let named = QueryFullProcessImageNameW(
                            process,
                            0,
                            path.as_mut_ptr(),
                            &raw mut length,
                        ) != 0;
                        let refused = !named && GetLastError() == ERROR_ACCESS_DENIED;
                        CloseHandle(process);
                        if named {
                            see(Seen::Path(String::from_utf16_lossy(
                                &path[..(length as usize).min(path.len())],
                            )));
                        } else if refused {
                            see(Seen::Refused(Some(listed_name(&entry))));
                        }
                    }
                }
                more = Process32NextW(snapshot, &raw mut entry) != 0;
            }
            CloseHandle(snapshot);
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::Seen;
    use std::io::ErrorKind;

    /// Linux keeps the first 15 bytes of the executable's name as the process's `comm`, which
    /// every process lets anyone read; so a name compares by those bytes.
    pub(super) fn same_name(wanted: &[String], listed: &str) -> bool {
        const COMM_LENGTH: usize = 15;
        let first = |name: &str| {
            let end = name
                .char_indices()
                .map(|(index, c)| index + c.len_utf8())
                .take_while(|end| *end <= COMM_LENGTH)
                .last()
                .unwrap_or(0);
            name[..end].to_owned()
        };
        let listed = first(listed);
        wanted.iter().any(|name| first(name) == listed)
    }

    pub(super) fn each_process(see: &mut dyn FnMut(Seen)) -> Result<(), String> {
        let own = std::process::id().to_string();
        let processes = std::fs::read_dir("/proc")
            .map_err(|error| format!("Linux did not list its processes (/proc): {error}"))?;
        for entry in processes.flatten() {
            let name = entry.file_name();
            let Some(id) = name.to_str() else { continue };
            if id == own || id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
                continue;
            }
            match std::fs::read_link(entry.path().join("exe")) {
                Ok(path) => see(Seen::Path(path.display().to_string())),
                Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                    // Linux cuts `comm` at 15 bytes, maybe inside a character, so it is read
                    // as bytes and a cut character becomes U+FFFD, which `same_name` leaves out
                    // with the rest of the character. A process whose name cannot be read
                    // either is counted: it could be any.
                    let comm = std::fs::read(entry.path().join("comm")).ok().map(|comm| {
                        String::from_utf8_lossy(&comm)
                            .trim_end_matches('\n')
                            .to_owned()
                    });
                    see(Seen::Refused(comm));
                }
                // A kernel thread has no executable, and a process can end meanwhile.
                Err(_) => {}
            }
        }
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use super::Seen;

    pub(super) fn same_name(_wanted: &[String], _listed: &str) -> bool {
        true
    }

    pub(super) fn each_process(_see: &mut dyn FnMut(Seen)) -> Result<(), String> {
        Err("DevHQ does not know how to list processes on this system.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The system lists every process it refuses to show by name, so a file named like none of
    /// them has no unexamined process, however many the system hides (hundreds on a desktop
    /// without privileges). This process does not count itself, and the readers other tests
    /// start from this same executable come and go, so of it only the list is asked.
    #[test]
    fn counts_as_unexamined_only_what_is_named_like_the_file() {
        let nowhere = std::env::temp_dir().join(format!("devhq-{}.exe", uuid::Uuid::new_v4()));
        let count = running_from(&[&nowhere]).unwrap();
        assert_eq!(
            count,
            Count {
                running: 0,
                unexamined: 0
            }
        );
        let own = std::env::current_exe().unwrap();
        running_from(&[&own]).unwrap();
    }
}
