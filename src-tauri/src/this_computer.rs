//! Whether a declared path stays on this computer's disks, decided before anything opens it.
//!
//! DevHQ reads only what the inventory names on this workstation, and promises that reading a
//! local copy or an installation sends nothing anywhere. A path can break that promise before a
//! single byte of the file is read: on Windows, looking at `\\server\share\…` (also written
//! `//server/share/…`, or `\\?\UNC\server\share\…`), at a letter mapped to a share, or at a link on
//! a local disk that leads to one, makes the system connect to that server and authenticate the
//! person to it (NTLM) just to answer `stat`. A device path (`\\.\…`, `\??\…`, `\\?\GLOBALROOT\…`)
//! names a driver, not a file on a disk. So every reader asks here first, with the path's text and
//! what the system knows without touching the file (the drive's type, a link's target), and
//! refuses what leads off this computer with a reading that says so. The declared path is untrusted
//! data, whoever declared it: it is read here, never interpreted by the shell or a program.
//!
//! What is not decided here: on Linux, a network file system mounted into the tree (NFS, SMB) looks
//! like any folder, and DevHQ reads it as one; the mount holds the credentials, DevHQ sends none.
//! And a repository's own files can name paths libgit2 opens (`objects/info/alternates`, an
//! `include.path`): the copy reader checks the two it already reads before libgit2 does, `.git`
//! and `commondir`, and the README says what remains.

use std::path::{Component, Path, PathBuf};

/// What a path's text alone says of where it leads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Form {
    /// A disk of some computer: `C:\…`, `C:/…`, `/…`, or Windows' own `\\?\C:\…` and
    /// `\\?\Volume{…}\…`, which it resolves links to. Which computer, the system says (`Where`).
    Disk,
    /// A share on the network: `\\server\share\…`, `//server/share/…` or `\\?\UNC\server\share\…`.
    Network,
    /// A device or an object of the kernel's, not a file on a disk: `\\.\…`, `\??\…` or any other
    /// `\\?\…`.
    Device,
    /// Nothing DevHQ reads: a relative path, `~`, a bare drive, a URL.
    Other,
}

/// The form of `text`, on any system: a path can be declared on a Windows machine from a Linux
/// one and the other way round, and a link's target comes in Windows' verbatim form.
pub(crate) fn form(text: &str) -> Form {
    let bytes = text.as_bytes();
    if text.starts_with(r"\??\") {
        return Form::Device;
    }
    if bytes.len() >= 2 && is_separator(bytes[0]) && is_separator(bytes[1]) {
        let rest = &text[2..];
        // The verbatim and device prefixes are only ever written with backslashes; with slashes,
        // Windows takes `?` or `.` for a server's name and looks for it on the network.
        if bytes[0] == b'\\' && bytes[1] == b'\\' {
            if let Some(verbatim) = rest.strip_prefix(r"?\") {
                return if verbatim.len() >= 4 && verbatim[..4].eq_ignore_ascii_case("UNC\\") {
                    Form::Network
                } else if is_drive(verbatim) || verbatim.starts_with("Volume{") {
                    Form::Disk
                } else {
                    Form::Device
                };
            }
            if rest.starts_with(r".\") {
                return Form::Device;
            }
        }
        let mut parts = rest.split(is_separator_char);
        let (server, share) = (
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
        );
        return if server.is_empty() || share.is_empty() {
            Form::Other
        } else {
            Form::Network
        };
    }
    if bytes.first() == Some(&b'/') {
        return Form::Disk;
    }
    if is_drive(text) {
        return Form::Disk;
    }
    Form::Other
}

fn is_separator(byte: u8) -> bool {
    byte == b'\\' || byte == b'/'
}

fn is_separator_char(c: char) -> bool {
    c == '\\' || c == '/'
}

/// `X:\` or `X:/`, a drive and its root.
fn is_drive(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && is_separator(bytes[2])
}

/// The drive letter of a path on a disk, if it names one: `C:\…` or `\\?\C:\…`.
fn drive_letter(text: &str) -> Option<u8> {
    let plain = text.strip_prefix(r"\\?\").unwrap_or(text);
    is_drive(plain).then(|| plain.as_bytes()[0].to_ascii_uppercase())
}

/// The sentence every refusal ends with: what DevHQ did, and did not do, about the path.
const KEPT_HERE: &str =
    "DevHQ reads only this computer's disks, so it did not open it, and nothing left this computer.";

/// Why DevHQ does not open `path`, if its text, the drive it names or a link on the way leads off
/// this computer; `None` when nothing seen without opening the file says so. Asks the system only
/// what it answers without touching the file: a drive's type and a link's target.
pub(crate) fn refusal(path: &Path) -> Option<String> {
    if let Some(why) = refusal_by_form(&path.display().to_string()) {
        return Some(why);
    }
    // The path as the system will open it. Windows completes it from the text alone, before it
    // looks at any folder (GetFullPathNameW, which opens nothing): `\…` and `/…` take the current
    // drive, which may be a share, and `.` and `..` go by the text, so `C:\gone\..\away` is
    // `C:\away` even where `gone` does not exist. Elsewhere it changes nothing that matters here.
    let whole = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let text = whole.display().to_string();
    if let Some(why) = refusal_by_form(&text) {
        return Some(why);
    }
    if let Some(letter) = drive_letter(&text) {
        if system::drive_is_remote(letter) {
            return Some(format!(
                "A network drive ({}:): {KEPT_HERE}",
                char::from(letter)
            ));
        }
    }
    through_a_link(&whole)
}

/// Why `text`, by its form alone, is not a path on a disk DevHQ opens.
pub(crate) fn refusal_by_form(text: &str) -> Option<String> {
    match form(text) {
        Form::Disk => None,
        Form::Network => Some(format!(r"A network path (\\server\share\…): {KEPT_HERE}")),
        Form::Device => Some(format!(
            r"A device path (\\.\…, \??\… or \\?\…), not a file on a disk: {KEPT_HERE}"
        )),
        Form::Other => Some(format!("Not an absolute path: {KEPT_HERE}")),
    }
}

/// Links the system follows on the way to `path`, and how many before it gives up.
const MOST_LINKS: usize = 63;

/// Why a link on the way to `path`, a complete path, leads off this computer, if one does. Each
/// folder on the way is looked at without following it (`symlink_metadata`) and, when it is a link
/// or a junction, its target is read (`read_link`), which the system answers from the link itself;
/// then the walk goes on from the target, since it may be a link too. A folder that is not there
/// ends the walk, and so does one the system will not let DevHQ look at: the reader will report
/// what the system says of the path.
fn through_a_link(path: &Path) -> Option<String> {
    if !cfg!(windows) {
        // A link on Unix can only lead into the tree, which is this computer's as far as the path
        // can tell (see the module's notes on mounted file systems).
        return None;
    }
    let mut current = PathBuf::new();
    let mut links = 0;
    for component in path.components() {
        current.push(component);
        // A drive's letter alone (`C:`) names the current folder of that drive, not its root:
        // the walk starts at the root, which comes next.
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        loop {
            let found = match std::fs::symlink_metadata(&current) {
                Ok(found) => found,
                Err(_) => return None,
            };
            if !found.file_type().is_symlink() {
                break;
            }
            links += 1;
            if links > MOST_LINKS {
                return None;
            }
            let Ok(target) = std::fs::read_link(&current) else {
                return None;
            };
            let target = if target.is_absolute() {
                target
            } else {
                // A relative target goes from the link's folder, and its `..` by the text, as
                // the system resolves it; joined to a verbatim path (`\\?\…`, the form a
                // junction's target comes in) nothing else would resolve them.
                let mut joined = current.parent().map(Path::to_path_buf).unwrap_or_default();
                for part in target.components() {
                    match part {
                        Component::ParentDir => {
                            joined.pop();
                        }
                        Component::CurDir => {}
                        part => joined.push(part),
                    }
                }
                joined
            };
            let text = target.display().to_string();
            if let Some(why) = refusal_by_form(&text) {
                return Some(format!("A link on the way leads off this computer. {why}"));
            }
            if let Some(letter) = drive_letter(&text) {
                if system::drive_is_remote(letter) {
                    return Some(format!(
                        "A link on the way leads to a network drive ({}:): {KEPT_HERE}",
                        char::from(letter)
                    ));
                }
            }
            current = target;
        }
    }
    None
}

#[cfg(windows)]
mod system {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows_sys::Win32::System::WindowsProgramming::DRIVE_REMOTE;

    /// Whether the letter is mapped to a share. The system answers from what it keeps of the
    /// mapping, without connecting to it.
    pub(super) fn drive_is_remote(letter: u8) -> bool {
        let root: Vec<u16> = [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0].to_vec();
        // SAFETY: a plain Win32 call on a NUL-terminated UTF-16 string that outlives it.
        unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_REMOTE }
    }
}

#[cfg(not(windows))]
mod system {
    /// Drive letters belong to Windows; elsewhere a path that names one is a folder called `C:`.
    pub(super) fn drive_is_remote(_letter: u8) -> bool {
        false
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn tells_a_disk_from_the_network_and_from_a_device_by_the_text_alone() {
        for (text, expected) in [
            (r"C:\Program Files\Shop\shop.exe", Form::Disk),
            ("C:/Users/ana/src/shop", Form::Disk),
            ("/home/ana/src/shop", Form::Disk),
            (r"\\?\C:\Users\ana\src\shop", Form::Disk),
            (
                r"\\?\Volume{3b1c6e8a-0000-0000-0000-100000000000}\shop",
                Form::Disk,
            ),
            (r"\\nas\src\shop", Form::Network),
            (r"\\nas.example\src\shop\shop.exe", Form::Network),
            ("//nas/src/shop", Form::Network),
            (r"\\localhost\c$\src\shop", Form::Network),
            (r"\\127.0.0.1\c$\src\shop", Form::Network),
            (r"\\?\UNC\nas\src\shop", Form::Network),
            (r"\\?\unc\nas\src\shop", Form::Network),
            // With slashes, `?` and `.` are servers to Windows.
            ("//?/C:/src/shop", Form::Network),
            ("//./C:/src/shop", Form::Network),
            (r"\\.\C:\src\shop", Form::Device),
            (r"\\.\PhysicalDrive0", Form::Device),
            (r"\??\C:\src\shop", Form::Device),
            (r"\\?\GLOBALROOT\Device\HarddiskVolume3\shop", Form::Device),
            (r"\\?\pipe\shop", Form::Device),
            (r"\\nas", Form::Other),
            (r"\\nas\", Form::Other),
            (r"\\\share", Form::Other),
            ("src/shop", Form::Other),
            ("~/src/shop", Form::Other),
            ("C:shop", Form::Other),
            ("C:", Form::Other),
            (r"\shop", Form::Other),
            ("https://github.com/me/shop", Form::Other),
            ("", Form::Other),
        ] {
            assert_eq!(form(text), expected, "{text:?}");
        }
    }

    #[test]
    fn refuses_by_form_what_leads_off_this_computer_without_touching_it() {
        let network = refusal_by_form(r"\\nas.invalid\src\shop").unwrap();
        assert!(network.starts_with("A network path"), "{network}");
        assert!(
            network.ends_with("nothing left this computer."),
            "{network}"
        );
        let device = refusal_by_form(r"\\.\PhysicalDrive0").unwrap();
        assert!(device.starts_with("A device path"), "{device}");
        assert!(refusal_by_form("src/shop")
            .unwrap()
            .starts_with("Not an absolute path"));
        assert_eq!(refusal_by_form(r"C:\src\shop"), None);
        assert_eq!(refusal_by_form("/home/ana/src/shop"), None);
        // What a reader does with the path as a whole: the same, and nothing more is asked for a
        // form the system never sees.
        let started = std::time::Instant::now();
        let whole = refusal(Path::new(r"\\nas.invalid\src\shop")).unwrap();
        assert_eq!(whole, network);
        assert_eq!(
            refusal(Path::new("//nas.invalid/src/shop")).as_deref(),
            Some(network.as_str())
        );
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "the system was asked about the network"
        );
        let own = std::env::current_exe().unwrap();
        assert_eq!(refusal(&own), None);
    }

    /// A junction at `link` whose target is the share `target` (`\\server\share\…`), written as
    /// `mklink /J` writes one, with the reparse point set on the folder directly: `mklink` and
    /// PowerShell refuse a target off a local volume (PowerShell by looking it up on the network
    /// first), and a symbolic link needs a privilege. The system never follows it to make it.
    #[cfg(windows)]
    pub(crate) fn junction_to_share(link: &Path, target: &str) {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };
        use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
        use windows_sys::Win32::System::IO::DeviceIoControl;

        const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
        std::fs::create_dir(link).unwrap();
        let substitute: Vec<u16> = format!(r"\??\UNC\{}", target.trim_start_matches('\\'))
            .encode_utf16()
            .collect();
        let print: Vec<u16> = target.encode_utf16().collect();
        let substitute_len = u16::try_from(substitute.len() * 2).unwrap();
        let print_len = u16::try_from(print.len() * 2).unwrap();
        // REPARSE_DATA_BUFFER for a mount point: tag, data length, reserved, then the offsets
        // and lengths of the two names, then both names, each followed by a NUL.
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        data.extend_from_slice(&(8 + substitute_len + 2 + print_len + 2).to_le_bytes());
        data.extend_from_slice(&0_u16.to_le_bytes());
        data.extend_from_slice(&0_u16.to_le_bytes());
        data.extend_from_slice(&substitute_len.to_le_bytes());
        data.extend_from_slice(&(substitute_len + 2).to_le_bytes());
        data.extend_from_slice(&print_len.to_le_bytes());
        for unit in substitute.iter().chain(&[0]).chain(&print).chain(&[0]) {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        let folder = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(link)
            .unwrap();
        let mut returned = 0_u32;
        // SAFETY: a plain Win32 call on an open handle, a buffer passed with its length and an
        // output count that outlives the call.
        let set = unsafe {
            DeviceIoControl(
                folder.as_raw_handle(),
                FSCTL_SET_REPARSE_POINT,
                data.as_ptr().cast(),
                u32::try_from(data.len()).unwrap(),
                std::ptr::null_mut(),
                0,
                &raw mut returned,
                std::ptr::null_mut(),
            )
        };
        assert!(set != 0, "{}", std::io::Error::last_os_error());
    }

    /// A junction on a local disk whose target is on the network, as `mklink /J` writes one;
    /// beside it, a junction to a local folder, which the walk follows and accepts.
    #[cfg(windows)]
    #[test]
    fn refuses_a_link_on_this_computer_that_leads_to_the_network() {
        let (real, link) = crate::inventory::tests::linked_folder();
        let away = real.join("away");
        junction_to_share(&away, r"\\nas.invalid\src\shop");
        let started = std::time::Instant::now();
        // Through the local junction and then through the one that leaves: the path's text says
        // nothing, the second link does.
        let why = refusal(&link.join("away").join("shop.exe")).unwrap();
        assert!(
            why.starts_with("A link on the way leads off this computer. A network path"),
            "{why}"
        );
        assert!(why.ends_with("nothing left this computer."), "{why}");
        // Windows resolves `..` by the text before it looks at any folder: through a folder that
        // is not there, the path still goes through the junction that leaves.
        assert_eq!(
            refusal(&link.join("gone").join("..").join("away").join("shop.exe")),
            Some(why.clone())
        );
        assert_eq!(refusal(&away), Some(why));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "the system was asked about the network"
        );
        // The local junction alone is this computer, and so is what is not there.
        std::fs::write(real.join("shop.exe"), "x").unwrap();
        assert_eq!(refusal(&link.join("shop.exe")), None);
        assert_eq!(refusal(&link.join("gone").join("shop.exe")), None);
        std::fs::remove_dir(&away).unwrap();
        crate::inventory::tests::remove_linked((real, link));
    }

    #[cfg(windows)]
    #[test]
    fn asks_the_system_which_drives_are_mapped_to_a_share() {
        // The system drive is never a share; a letter no drive has is not one either.
        let home = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        assert!(!system::drive_is_remote(home.as_bytes()[0]));
        let unused = (b'A'..=b'Z')
            .find(|letter| !Path::new(&format!("{}:\\", char::from(*letter))).exists())
            .unwrap();
        assert!(!system::drive_is_remote(unused));
    }
}
