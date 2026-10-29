//! Whether a DevHQ window is open on an inventory, so that the agent gate can say whether
//! anything will notice what it writes and supervise the checks. The window's process holds a
//! lock on `inventory.sqlite3.window-lock`, next to the inventory, for as long as it lives, and
//! the system releases it when the process ends, however it ends: closed, crashed or killed. No
//! file says «open» and could outlive the process that wrote it.
//!
//! The window holds a shared lock and the gate asks for an exclusive one without waiting:
//! refused means that a window holds it. Shared, so that two DevHQ windows on one inventory
//! both hold it, and the answer stays right while either is open.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// The lock file of the inventory at `database`.
pub(crate) fn lock_path(database: &Path) -> PathBuf {
    let mut name = database.as_os_str().to_owned();
    name.push(".window-lock");
    PathBuf::from(name)
}

/// Held by the window's process for as long as it lives: the open file holds the lock.
pub(crate) struct WindowLock {
    _file: File,
}

impl WindowLock {
    /// Takes the lock. It only waits for a gate asking at that very moment, which holds its
    /// exclusive lock for no longer than the asking.
    pub(crate) fn hold(database: &Path) -> std::io::Result<Self> {
        if let Some(folder) = database.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path(database))?;
        file.lock_shared()?;
        Ok(Self { _file: file })
    }
}

/// Whether a DevHQ window is open on the inventory at `database`; None when the system cannot
/// say. Without a lock file, no window has opened this inventory since DevHQ began keeping one.
/// It never creates the file.
pub(crate) fn window_open(database: &Path) -> Option<bool> {
    let file = match File::open(lock_path(database)) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Some(false),
        Err(_) => return None,
    };
    // Taken only to ask: closing the file at the end of this function releases it.
    match file.try_lock() {
        Ok(()) => Some(false),
        Err(TryLockError::WouldBlock) => Some(true),
        Err(TryLockError::Error(_)) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    /// An inventory path in a folder of its own.
    fn database() -> PathBuf {
        crate::inventory::tests::disposable(
            std::env::temp_dir().join(format!("devhq-window-lock-test-{}", uuid::Uuid::new_v4())),
        )
        .join("inventory.sqlite3")
    }

    fn remove(database: &Path) {
        crate::inventory::tests::remove_now(database.parent().unwrap());
    }

    /// Asks until the answer is `expected`: the system may release a dead process's lock a moment
    /// after the process is gone (Windows does it as resources allow).
    fn until(database: &Path, expected: bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while window_open(database) != Some(expected) {
            assert!(
                Instant::now() < deadline,
                "the gate never saw windowOpen {expected}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn says_whether_a_window_holds_the_lock() {
        let database = database();
        assert_eq!(window_open(&database), Some(false), "no window ever opened");
        assert!(
            !lock_path(&database).exists(),
            "asking creates nothing, not even the folder's file"
        );
        let first = WindowLock::hold(&database).unwrap();
        assert_eq!(window_open(&database), Some(true));
        // Asking does not take the lock from the window.
        assert_eq!(window_open(&database), Some(true));
        // A second window on the same inventory holds it too.
        let second = WindowLock::hold(&database).unwrap();
        drop(first);
        assert_eq!(
            window_open(&database),
            Some(true),
            "one window is still open"
        );
        drop(second);
        assert_eq!(window_open(&database), Some(false));
        remove(&database);
    }

    /// The environment variable that makes `holds_the_lock_until_killed` hold a lock.
    const HOLD: &str = "DEVHQ_TEST_HOLD_WINDOW_LOCK";

    /// Run by `the_system_releases_the_lock_of_a_process_that_died` in a process of its own,
    /// which it then kills; on its own, it does nothing.
    #[test]
    #[ignore = "a helper that another test runs in a process of its own"]
    fn holds_the_lock_until_killed() {
        let Some(database) = std::env::var_os(HOLD) else {
            return;
        };
        let _lock = WindowLock::hold(Path::new(&database)).unwrap();
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }

    #[test]
    fn the_system_releases_the_lock_of_a_process_that_died() {
        let database = database();
        // This same test executable, running only the helper above.
        let mut holder = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "window_lock::tests::holds_the_lock_until_killed",
                "--nocapture",
            ])
            .env(HOLD, &database)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        until(&database, true);
        // Killed, as a crash ends it: nothing in the process gets to release anything.
        holder.kill().unwrap();
        holder.wait().unwrap();
        until(&database, false);
        remove(&database);
    }
}
