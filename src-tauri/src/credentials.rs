//! The one credential DevHQ keeps: a fine-grained GitHub token, read only, that the person gives in
//! Settings so that DevHQ can read the repositories the inventory names (`docs/leer-github.md`).
//!
//! It lives in the system's credential store, under a name made of this build's identifier
//! (`identity::identifier`), so a development build never finds the installed DevHQ's token, and
//! nowhere else: never in the inventory, a backup, an export, a log, an error message, the agent
//! gate or the window, which only learns whether one is kept. The window sends it once, when the
//! person saves it; nothing ever sends it back.
//!
//! - Windows: the Credential Manager, a generic credential kept on this computer only
//!   (`CRED_PERSIST_LOCAL_MACHINE`), never roaming with the profile.
//! - Linux (development and CI only): the kernel's keyring of the login session, which the process
//!   always possesses, unlike the user keyring, which a service such as a CI runner may reach without
//!   the right to write to it. It lasts until the session ends; the specification says so.
//! - Anywhere else, nothing is kept and saving is refused.
//!
//! Only the GitHub reader (`github`) reads the token back, to send it to `api.github.com` and
//! nowhere else.

use crate::inventory::ApiError;
use serde::Serialize;

/// What the token is kept as, after this build's identifier: `com.devhq.desktop:github`.
const GITHUB: &str = "github";

/// The longest token accepted. GitHub's fine-grained tokens are 93 characters today; the margin
/// leaves room for a longer format without accepting a pasted page.
const LONGEST: usize = 255;

/// A secret's bytes, overwritten when dropped. Never printed and never serialized: its only way out
/// is the store it was given to. The copies Tauri and serde made on the way in are not overwritten.
pub(crate) struct Secret(Vec<u8>);

impl Secret {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0
    }
}

#[cfg(test)]
impl Secret {
    pub(crate) fn for_tests(text: &str) -> Self {
        Secret(text.as_bytes().to_vec())
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret(…)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

/// A place secrets are kept under a name: the system's store, or one in memory for tests.
pub(crate) trait Store: Send + Sync {
    /// What the window calls it: `windows_credential_manager`, `linux_session_keyring` or `none`.
    fn kind(&self) -> &'static str;
    /// Whether a secret is kept under `name`.
    fn holds(&self, name: &str) -> Result<bool, String>;
    /// The secret kept under `name`, if any.
    fn read(&self, name: &str) -> Result<Option<Secret>, String>;
    /// Keeps `secret` under `name`, in place of what was there.
    fn write(&self, name: &str, secret: &Secret) -> Result<(), String>;
    /// Removes what is kept under `name`; nothing kept is not an error.
    fn remove(&self, name: &str) -> Result<(), String>;
}

/// Whether DevHQ keeps a GitHub token, and where. Never the token.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubToken {
    pub stored: bool,
    /// `windows_credential_manager`, `linux_session_keyring` or `none` (this system keeps nothing).
    pub store: &'static str,
}

/// The credentials the window manages, in the store given, under this build's names.
pub struct Credentials {
    store: Box<dyn Store>,
    identifier: &'static str,
}

fn store_error(error: String) -> ApiError {
    ApiError {
        code: "credential_store_error",
        message: format!("The system's credential store refused: {error}"),
    }
}

impl Credentials {
    /// The system's credential store, under this build's identifier.
    pub fn system() -> Self {
        Self {
            store: system_store(),
            identifier: crate::identity::identifier(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with(store: Box<dyn Store>, identifier: &'static str) -> Self {
        Self { store, identifier }
    }

    fn name(&self) -> String {
        format!("{}:{GITHUB}", self.identifier)
    }

    pub fn github(&self) -> Result<GithubToken, ApiError> {
        Ok(GithubToken {
            stored: self.store.holds(&self.name()).map_err(store_error)?,
            store: self.store.kind(),
        })
    }

    /// Keeps `token` in place of any kept before, once it reads as a fine-grained token.
    pub fn save_github(&self, token: &str) -> Result<GithubToken, ApiError> {
        let secret = fine_grained(token)?;
        self.store
            .write(&self.name(), &secret)
            .map_err(store_error)?;
        self.github()
    }

    pub fn remove_github(&self) -> Result<GithubToken, ApiError> {
        self.store.remove(&self.name()).map_err(store_error)?;
        self.github()
    }

    /// The token itself, for the GitHub reader alone; None when none is kept.
    pub(crate) fn github_secret(&self) -> Result<Option<Secret>, ApiError> {
        self.store.read(&self.name()).map_err(store_error)
    }
}

/// The token in `text`, without the whitespace a paste brings, when it is a fine-grained token:
/// `github_pat_` and letters, digits and underscores. A classic token is refused by name, since it
/// reaches every repository the account can; anything else without saying what it held.
fn fine_grained(text: &str) -> Result<Secret, ApiError> {
    let token = text.trim_matches(|character: char| character.is_ascii_whitespace());
    let invalid = |message: &str| ApiError {
        code: "invalid_token",
        message: message.into(),
    };
    if ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"]
        .iter()
        .any(|prefix| token.starts_with(prefix))
    {
        return Err(invalid(
            "This is not a fine-grained token: a classic GitHub token reaches every repository the \
             account can. Create a fine-grained token with read-only access to Metadata, Actions \
             and Pull requests, for the repositories DevHQ should read.",
        ));
    }
    let body = token.strip_prefix("github_pat_").unwrap_or_default();
    if body.is_empty()
        || token.len() > LONGEST
        || !body
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(invalid(
            "A fine-grained GitHub token starts with github_pat_ and holds only letters, digits \
             and underscores.",
        ));
    }
    Ok(Secret(token.as_bytes().to_vec()))
}

#[cfg(windows)]
fn system_store() -> Box<dyn Store> {
    Box::new(windows::CredentialManager)
}

#[cfg(target_os = "linux")]
fn system_store() -> Box<dyn Store> {
    Box::new(linux::SessionKeyring)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn system_store() -> Box<dyn Store> {
    Box::new(Nowhere)
}

/// A system whose credential store DevHQ does not know: nothing is kept, and saving is refused.
#[cfg(any(test, not(any(windows, target_os = "linux"))))]
pub(crate) struct Nowhere;

#[cfg(any(test, not(any(windows, target_os = "linux"))))]
impl Store for Nowhere {
    fn kind(&self) -> &'static str {
        "none"
    }
    fn holds(&self, _: &str) -> Result<bool, String> {
        Ok(false)
    }
    fn read(&self, _: &str) -> Result<Option<Secret>, String> {
        Ok(None)
    }
    fn write(&self, _: &str, _: &Secret) -> Result<(), String> {
        Err("DevHQ keeps credentials only on Windows and Linux.".into())
    }
    fn remove(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(windows)]
mod windows {
    use super::{Secret, Store};
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    pub struct CredentialManager;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    fn last_error() -> String {
        std::io::Error::last_os_error().to_string()
    }

    impl Store for CredentialManager {
        fn kind(&self) -> &'static str {
            "windows_credential_manager"
        }

        fn holds(&self, name: &str) -> Result<bool, String> {
            let target = wide(name);
            let mut found: *mut CREDENTIALW = std::ptr::null_mut();
            // SAFETY: the name is NUL-terminated and `found` is a valid place for the pointer
            // CredReadW allocates, which is freed below and never read: only whether it exists.
            let read = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut found) };
            if read == 0 {
                // SAFETY: no call since CredReadW.
                return match unsafe { GetLastError() } {
                    ERROR_NOT_FOUND => Ok(false),
                    _ => Err(last_error()),
                };
            }
            // SAFETY: CredReadW succeeded, so `found` is its allocation, freed once.
            unsafe { CredFree(found.cast()) };
            Ok(true)
        }

        fn read(&self, name: &str) -> Result<Option<Secret>, String> {
            let target = wide(name);
            let mut found: *mut CREDENTIALW = std::ptr::null_mut();
            // SAFETY: as in `holds`.
            let read = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut found) };
            if read == 0 {
                // SAFETY: no call since CredReadW.
                return match unsafe { GetLastError() } {
                    ERROR_NOT_FOUND => Ok(None),
                    _ => Err(last_error()),
                };
            }
            // SAFETY: CredReadW succeeded, so `found` points to a credential whose blob holds
            // `CredentialBlobSize` bytes (or is null when empty). The bytes are copied, then
            // overwritten in the system's allocation before it is freed, once.
            let secret = unsafe {
                let credential = &*found;
                let size = credential.CredentialBlobSize as usize;
                let secret = if credential.CredentialBlob.is_null() || size == 0 {
                    Secret(Vec::new())
                } else {
                    let blob = std::slice::from_raw_parts_mut(credential.CredentialBlob, size);
                    let secret = Secret(blob.to_vec());
                    blob.fill(0);
                    secret
                };
                CredFree(found.cast());
                secret
            };
            Ok(Some(secret))
        }

        fn write(&self, name: &str, secret: &Secret) -> Result<(), String> {
            let mut target = wide(name);
            let mut user = wide("github");
            let mut blob = secret.bytes().to_vec();
            let credential = CREDENTIALW {
                Type: CRED_TYPE_GENERIC,
                TargetName: target.as_mut_ptr(),
                CredentialBlobSize: u32::try_from(blob.len()).map_err(|error| error.to_string())?,
                CredentialBlob: blob.as_mut_ptr(),
                Persist: CRED_PERSIST_LOCAL_MACHINE,
                UserName: user.as_mut_ptr(),
                ..Default::default()
            };
            // SAFETY: every pointer in `credential` points into a buffer that outlives the call,
            // the strings are NUL-terminated and the blob's size is its length.
            let written = unsafe { CredWriteW(&credential, 0) };
            let outcome = if written == 0 {
                Err(last_error())
            } else {
                Ok(())
            };
            blob.fill(0);
            outcome
        }

        fn remove(&self, name: &str) -> Result<(), String> {
            let target = wide(name);
            // SAFETY: the name is NUL-terminated.
            if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
                // SAFETY: no call since CredDeleteW.
                if unsafe { GetLastError() } != ERROR_NOT_FOUND {
                    return Err(last_error());
                }
            }
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{Secret, Store};
    use std::ffi::CString;

    // From <linux/keyctl.h>.
    const KEY_SPEC_SESSION_KEYRING: libc::c_long = -3;
    const KEYCTL_SETPERM: libc::c_long = 5;
    const KEYCTL_UNLINK: libc::c_long = 9;
    const KEYCTL_SEARCH: libc::c_long = 10;
    const KEYCTL_READ: libc::c_long = 11;
    const KEYCTL_INVALIDATE: libc::c_long = 21;
    /// Every right for the processes that possess the key and for its user, none for its group or
    /// anyone else: a process of the same user that does not possess it can still find it.
    const PERMISSIONS: libc::c_long = 0x3f3f_0000;

    pub struct SessionKeyring;

    fn text(value: &str) -> Result<CString, String> {
        CString::new(value).map_err(|error| error.to_string())
    }

    /// The key kept under `name` in the session keyring or a keyring it holds, if any.
    fn find(name: &str) -> Result<Option<libc::c_long>, String> {
        let (kind, description) = (text("user")?, text(name)?);
        // SAFETY: both strings are NUL-terminated and outlive the call; no keyring is linked.
        let id = unsafe {
            libc::syscall(
                libc::SYS_keyctl,
                KEYCTL_SEARCH,
                KEY_SPEC_SESSION_KEYRING,
                kind.as_ptr(),
                description.as_ptr(),
                0,
            )
        };
        if id >= 0 {
            return Ok(Some(id));
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ENOKEY) | Some(libc::EKEYEXPIRED) | Some(libc::EKEYREVOKED) => Ok(None),
            _ => Err(error.to_string()),
        }
    }

    impl Store for SessionKeyring {
        fn kind(&self) -> &'static str {
            "linux_session_keyring"
        }

        fn holds(&self, name: &str) -> Result<bool, String> {
            find(name).map(|id| id.is_some())
        }

        fn read(&self, name: &str) -> Result<Option<Secret>, String> {
            let Some(id) = find(name)? else {
                return Ok(None);
            };
            // A token is at most `LONGEST` bytes; the room for more only says that the key holds
            // something else, which is refused rather than cut.
            let mut buffer = Secret(vec![0; super::LONGEST + 1]);
            // SAFETY: the buffer is writable for its whole length, which is the length given.
            let size = unsafe {
                libc::syscall(
                    libc::SYS_keyctl,
                    KEYCTL_READ,
                    id,
                    buffer.0.as_mut_ptr(),
                    buffer.0.len(),
                )
            };
            if size < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let size = usize::try_from(size).map_err(|error| error.to_string())?;
            if size > super::LONGEST {
                return Err("the key holds more than a token".into());
            }
            let secret = Secret(buffer.0[..size].to_vec());
            Ok(Some(secret))
        }

        fn write(&self, name: &str, secret: &Secret) -> Result<(), String> {
            let (kind, description) = (text("user")?, text(name)?);
            let payload = secret.bytes();
            // SAFETY: the strings are NUL-terminated, the payload's length is its own, and all of
            // them outlive the call. A key of the same type and description is updated in place.
            let id = unsafe {
                libc::syscall(
                    libc::SYS_add_key,
                    kind.as_ptr(),
                    description.as_ptr(),
                    payload.as_ptr(),
                    payload.len(),
                    KEY_SPEC_SESSION_KEYRING,
                )
            };
            if id < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            // SAFETY: plain integers.
            if unsafe { libc::syscall(libc::SYS_keyctl, KEYCTL_SETPERM, id, PERMISSIONS) } < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<(), String> {
            let Some(id) = find(name)? else {
                return Ok(());
            };
            // Invalidating destroys the key at once; unlinking it from the session keyring is the
            // fallback for a kernel older than 3.5.
            // SAFETY: plain integers.
            if unsafe { libc::syscall(libc::SYS_keyctl, KEYCTL_INVALIDATE, id) } == 0 {
                return Ok(());
            }
            // SAFETY: plain integers.
            if unsafe {
                libc::syscall(
                    libc::SYS_keyctl,
                    KEYCTL_UNLINK,
                    id,
                    KEY_SPEC_SESSION_KEYRING,
                )
            } < 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(())
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// A store in memory, for the IPC contract and the window's tests: never the system's, which
    /// may hold the person's token.
    #[derive(Default)]
    pub(crate) struct Memory(Mutex<BTreeMap<String, Vec<u8>>>);

    impl Store for Memory {
        fn kind(&self) -> &'static str {
            "windows_credential_manager"
        }
        fn holds(&self, name: &str) -> Result<bool, String> {
            Ok(self.0.lock().unwrap().contains_key(name))
        }
        fn read(&self, name: &str) -> Result<Option<Secret>, String> {
            Ok(self.0.lock().unwrap().get(name).cloned().map(Secret))
        }
        fn write(&self, name: &str, secret: &Secret) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .insert(name.into(), secret.bytes().to_vec());
            Ok(())
        }
        fn remove(&self, name: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(name);
            Ok(())
        }
    }

    pub(crate) fn in_memory() -> Credentials {
        Credentials::with(Box::<Memory>::default(), "com.devhq.desktop.test")
    }

    /// An invented token of the fine-grained shape: never a real one.
    pub(crate) const TOKEN: &str =
        "github_pat_11AAAAAAA0invented0for0tests0only_0000000000000000000000000000000000000000000000";

    #[test]
    fn keeps_a_fine_grained_token_and_says_only_whether_one_is_kept() {
        let credentials = in_memory();
        assert!(!credentials.github().unwrap().stored);
        // What a paste brings around it is left out.
        let saved = credentials.save_github(&format!("  {TOKEN}\r\n")).unwrap();
        assert_eq!(
            saved,
            GithubToken {
                stored: true,
                store: "windows_credential_manager"
            }
        );
        assert!(!serde_json::to_string(&saved)
            .unwrap()
            .contains("github_pat_"));
        // Only the reader gets it back, trimmed as it was kept.
        assert_eq!(
            credentials.github_secret().unwrap().unwrap().bytes(),
            TOKEN.as_bytes()
        );
        assert!(!credentials.remove_github().unwrap().stored);
        assert!(credentials.github_secret().unwrap().is_none());
        // Removing what is not there is not an error.
        assert!(!credentials.remove_github().unwrap().stored);
    }

    #[test]
    fn refuses_what_is_not_a_fine_grained_token_without_repeating_it() {
        let credentials = in_memory();
        let classic = format!("ghp_{}", "a".repeat(36));
        let long = format!("github_pat_{}", "a".repeat(LONGEST));
        for text in [
            "",
            "github_pat_",
            "hello",
            "github_pat_abc def",
            "github_pat_abc-def",
            "github_pat_ñandú",
            classic.as_str(),
            long.as_str(),
        ] {
            let error = credentials.save_github(text).unwrap_err();
            assert_eq!(error.code, "invalid_token", "{text}");
            // The message names the prefix, never what came after it.
            assert!(
                text.len() <= "github_pat_".len() || !error.message.contains(text),
                "{text}"
            );
            assert!(!credentials.github().unwrap().stored, "{text}");
        }
        let error = credentials.save_github(&classic).unwrap_err();
        assert!(error.message.contains("classic"), "{}", error.message);
        // A refusal keeps the token kept before.
        credentials.save_github(TOKEN).unwrap();
        credentials.save_github("hello").unwrap_err();
        assert!(credentials.github().unwrap().stored);
    }

    #[test]
    fn a_secret_never_prints_what_it_holds() {
        let secret = fine_grained(TOKEN).unwrap();
        assert_eq!(format!("{secret:?}"), "Secret(…)");
    }

    #[test]
    fn a_system_whose_store_devhq_does_not_know_keeps_nothing() {
        let credentials = Credentials::with(Box::new(Nowhere), "com.devhq.desktop.test");
        let error = credentials.save_github(TOKEN).unwrap_err();
        assert_eq!(error.code, "credential_store_error");
        assert_eq!(
            credentials.github().unwrap(),
            GithubToken {
                stored: false,
                store: "none"
            }
        );
    }

    /// Removes the test's entry from the system's store when the test ends, failed or not.
    struct Entry<'a>(&'a dyn Store, String);

    impl Drop for Entry<'_> {
        fn drop(&mut self) {
            let _ = self.0.remove(&self.1);
        }
    }

    /// The system's own store, under a name of this test's alone (never the window's), written,
    /// replaced, found and removed. The Windows workflow runs it against the Credential Manager.
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn the_system_store_keeps_replaces_and_removes_a_secret() {
        let store = system_store();
        let entry = Entry(
            store.as_ref(),
            format!("devhq-test-{}:{GITHUB}", uuid::Uuid::new_v4()),
        );
        let name = entry.1.clone();
        assert!(!store.holds(&name).unwrap());
        assert!(store.read(&name).unwrap().is_none());
        store.write(&name, &fine_grained(TOKEN).unwrap()).unwrap();
        assert!(store.holds(&name).unwrap());
        assert_eq!(
            store.read(&name).unwrap().unwrap().bytes(),
            TOKEN.as_bytes()
        );
        store
            .write(&name, &Secret(b"github_pat_replaced".to_vec()))
            .unwrap();
        assert!(store.holds(&name).unwrap());
        assert_eq!(
            store.read(&name).unwrap().unwrap().bytes(),
            b"github_pat_replaced"
        );
        store.remove(&name).unwrap();
        assert!(!store.holds(&name).unwrap());
        assert!(store.read(&name).unwrap().is_none());
        store.remove(&name).unwrap();
        drop(entry);
    }
}
