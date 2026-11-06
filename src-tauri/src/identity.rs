//! Which data a DevHQ executable keeps, and where: the installed DevHQ's, or development data apart
//! from it.
//!
//! The data lives in a folder of the person's home, outside AppData: `DevHQ` for the installed
//! DevHQ, `DevHQ-desarrollo` for development data. It holds the inventory, its backups, the window
//! lock and `this-workstation.json`. Windows redirects AppData for every process a packaged app
//! (MSIX) starts, such as the Claude or Codex desktop app, even one without the package's identity:
//! a gate started there saw the package's own copy of AppData, never the inventory the window
//! opened. The home folder is not redirected, so the window, a gate started from a terminal and a
//! gate started from a packaged app open the same file. Only the WebView2 profile stays in Tauri's
//! local data folder (`%LOCALAPPDATA%\<identifier>`): it is the browser's own, not the person's.
//!
//! One decision, made here, names those folders for the window and for the agent gate alike: only
//! the executable DevHQ's installer packed keeps the installed DevHQ's data; every other one
//! (`tauri dev`, a debug build, the release executable left in `target/`) keeps development data.
//! A development build used to open the inventory a person uses every day, and would have upgraded
//! it to a schema no release has.
//!
//! Tauri's bundler tells them apart: before it packs the executable into the NSIS installer it
//! patches a marker in it, which `tauri::utils::platform::bundle_type` reads; the executable in
//! `target/` keeps the marker unpatched. That marker says how the file was built, nothing more. It
//! does not prove that the file is installed anywhere: a copy of the installed executable, run from
//! any folder, keeps the installed DevHQ's data. `--database` still opens whatever file it names. It
//! keeps development builds away from the everyday inventory by default; it is not a barrier.
//!
//! Before this, DevHQ kept its data in Tauri's data folders (`%APPDATA%\<identifier>` and
//! `%LOCALAPPDATA%\<identifier>`). `previous_database` names where that inventory was, so that a
//! DevHQ whose data has not been moved yet never starts an empty inventory in its place (see
//! `location`). DevHQ never moves it: the person copies it, once, as the README says.

use std::path::PathBuf;
use tauri::utils::config::BundleType;

/// The identifier of the installed DevHQ, as `tauri.conf.json` gives it to the installer. It names
/// the WebView2 profile's folder, and the folders where DevHQ used to keep its data.
pub const INSTALLED: &str = "com.devhq.desktop";

/// The identifier of every executable the installer did not pack.
pub const DEVELOPMENT: &str = "com.devhq.desktop.dev";

/// The folder of the person's home that holds the installed DevHQ's data.
pub const INSTALLED_FOLDER: &str = "DevHQ";

/// The folder of the person's home that holds development data.
pub const DEVELOPMENT_FOLDER: &str = "DevHQ-desarrollo";

/// The inventory's file name, in the data folder.
pub const DATABASE_FILE: &str = "inventory.sqlite3";

/// What a development build's window title adds, so that a person can tell the windows apart.
pub const DEVELOPMENT_TITLE: &str = " (development data)";

/// The identifier that names this executable's data folders.
pub fn identifier() -> &'static str {
    identifier_of(tauri::utils::platform::bundle_type())
}

/// Only NSIS, the one installer DevHQ ships, keeps the installed DevHQ's data. Any other package
/// gets development data until DevHQ ships it and checks the marker its bundler leaves: on macOS
/// `bundle_type` answers `App` for every executable, packed or not.
fn identifier_of(bundle: Option<BundleType>) -> &'static str {
    match bundle {
        Some(BundleType::Nsis) => INSTALLED,
        _ => DEVELOPMENT,
    }
}

/// Whether this executable keeps development data.
pub fn development() -> bool {
    identifier() == DEVELOPMENT
}

/// Names the window's data folders and, for development data, marks its title.
pub fn apply(config: &mut tauri::Config) {
    config.identifier = identifier().to_owned();
    if development() {
        for window in &mut config.app.windows {
            window.title.push_str(DEVELOPMENT_TITLE);
        }
    }
}

/// The folder that holds this executable's data: the inventory, its backups, the window lock and
/// `this-workstation.json`. None when the system names no home folder.
pub fn data_folder() -> Option<PathBuf> {
    data_folder_of(identifier())
}

fn data_folder_of(identifier: &str) -> Option<PathBuf> {
    let folder = if identifier == INSTALLED {
        INSTALLED_FOLDER
    } else {
        DEVELOPMENT_FOLDER
    };
    dirs::home_dir().map(|home| home.join(folder))
}

/// The inventory the window opens, and the agent gate unless told `--database`.
pub fn database() -> Option<PathBuf> {
    data_folder().map(|folder| folder.join(DATABASE_FILE))
}

/// Where DevHQ kept the inventory before its data left AppData: `inventory.sqlite3` in the
/// system's data directory followed by the identifier (Tauri's `app_data_dir`).
pub fn previous_database() -> Option<PathBuf> {
    previous_database_of(identifier())
}

fn previous_database_of(identifier: &str) -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join(identifier).join(DATABASE_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;

    #[test]
    fn only_the_nsis_installer_keeps_the_installed_devhq_data() {
        assert_eq!(identifier_of(Some(BundleType::Nsis)), INSTALLED);
        for other in [
            None,
            Some(BundleType::Deb),
            Some(BundleType::Rpm),
            Some(BundleType::AppImage),
            Some(BundleType::Msi),
            Some(BundleType::App),
            Some(BundleType::Dmg),
        ] {
            assert_eq!(identifier_of(other.clone()), DEVELOPMENT, "{other:?}");
        }
    }

    /// The test executable is built like `target/`'s, never packed: its marker is unpatched.
    #[test]
    fn an_executable_the_installer_did_not_pack_keeps_development_data() {
        assert_eq!(tauri::utils::platform::bundle_type(), None);
        assert_eq!(identifier(), DEVELOPMENT);
        assert!(development());
        let folder = dirs::home_dir().unwrap().join("DevHQ-desarrollo");
        assert_eq!(data_folder().unwrap(), folder);
        assert_eq!(database().unwrap(), folder.join("inventory.sqlite3"));
    }

    /// Each kind of data has a folder of its own in the home, outside the folders the system (and,
    /// on Windows, a packaged app's redirection) keeps for applications.
    #[test]
    fn each_kind_of_data_keeps_its_own_folder_of_the_home() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(data_folder_of(INSTALLED).unwrap(), home.join("DevHQ"));
        assert_eq!(
            data_folder_of(DEVELOPMENT).unwrap(),
            home.join("DevHQ-desarrollo")
        );
        let system = [dirs::data_dir(), dirs::data_local_dir()];
        for identifier in [INSTALLED, DEVELOPMENT] {
            let folder = data_folder_of(identifier).unwrap();
            for system in system.iter().flatten() {
                assert!(!folder.starts_with(system), "{folder:?} is in {system:?}");
            }
        }
    }

    /// The installer names the WebView2 profile's folder after `tauri.conf.json`; the native walk
    /// derives development's from it.
    #[test]
    fn the_installed_identifier_is_the_one_the_installer_gets() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], INSTALLED);
        assert_eq!(DEVELOPMENT, format!("{INSTALLED}.dev"));
    }

    /// For either identifier, the previous location is where an older DevHQ's window kept its
    /// inventory: Tauri's data folder, as Tauri itself resolves it.
    #[test]
    fn the_previous_location_is_tauris_data_folder() {
        for identifier in [INSTALLED, DEVELOPMENT] {
            let mut context = tauri::test::mock_context(tauri::test::noop_assets());
            context.config_mut().identifier = identifier.into();
            let app = tauri::test::mock_builder().build(context).unwrap();
            assert_eq!(
                previous_database_of(identifier).unwrap(),
                app.path().app_data_dir().unwrap().join("inventory.sqlite3")
            );
        }
    }

    #[test]
    fn development_data_marks_the_window_title() {
        let mut config: tauri::Config =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        apply(&mut config);
        assert_eq!(config.identifier, DEVELOPMENT);
        assert_eq!(config.app.windows[0].title, "DevHQ (development data)");
    }
}
