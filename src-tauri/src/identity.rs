//! Which data a DevHQ executable keeps: the installed DevHQ's, or development data apart from it.
//!
//! Tauri names an app's data folders after its identifier: the inventory, its backups and the
//! window lock in `app_data_dir`, this computer's workstation file and the WebView2 profile in
//! `app_local_data_dir`. Every executable built from this repository used to have the same one,
//! so a development build opened the inventory a person uses every day, and would have upgraded
//! it to a schema no release has. Now one decision, made here, names those folders for the
//! window and for the agent gate alike: only the executable DevHQ's installer packed keeps
//! `com.devhq.desktop`; every other one (`tauri dev`, a debug build, the release executable left
//! in `target/`) keeps `com.devhq.desktop.dev`.
//!
//! Tauri's bundler tells them apart: before it packs the executable into the NSIS installer it
//! patches a marker in it, which `tauri::utils::platform::bundle_type` reads; the executable in
//! `target/` keeps the marker unpatched. That marker says how the file was built, nothing more. It
//! does not prove that the file is installed anywhere: a copy of the installed executable, run from
//! any folder, keeps the installed DevHQ's data. And it does not choose where the system keeps the
//! files: an executable started from a packaged app (MSIX), such as the Claude desktop app, sees
//! AppData redirected whichever identifier it has. `--database` still opens whatever file it names.
//! It keeps development builds away from the everyday inventory by default; it is not a barrier.

use std::path::PathBuf;
use tauri::utils::config::BundleType;

/// The identifier of the installed DevHQ, as `tauri.conf.json` gives it to the installer, whose
/// uninstaller removes the folders it names. Its data never moves.
pub const INSTALLED: &str = "com.devhq.desktop";

/// The identifier of every executable the installer did not pack.
pub const DEVELOPMENT: &str = "com.devhq.desktop.dev";

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

/// The inventory the window opens: `inventory.sqlite3` in the system's data directory followed by
/// the identifier (Tauri's `app_data_dir`). The agent gate opens it unless told `--database`.
pub fn database() -> Option<PathBuf> {
    database_of(identifier())
}

fn database_of(identifier: &str) -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join(identifier).join("inventory.sqlite3"))
}

/// The local data folder (Tauri's `app_local_data_dir`): where the window says which workstation
/// this computer is, and where WebView2 keeps its profile.
pub fn local_data() -> Option<PathBuf> {
    local_data_of(identifier())
}

fn local_data_of(identifier: &str) -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join(identifier))
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
        assert_eq!(
            database().unwrap(),
            dirs::data_dir()
                .unwrap()
                .join(DEVELOPMENT)
                .join("inventory.sqlite3")
        );
    }

    /// The installer and its uninstaller name the folders after `tauri.conf.json`; the native walk
    /// derives development's from it.
    #[test]
    fn the_installed_identifier_is_the_one_the_installer_gets() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], INSTALLED);
        assert_eq!(DEVELOPMENT, format!("{INSTALLED}.dev"));
    }

    /// For either identifier, the gate opens the inventory the window opens, and knows the
    /// window's local folder, as Tauri itself resolves them.
    #[test]
    fn the_window_and_the_gate_resolve_the_same_folders() {
        for identifier in [INSTALLED, DEVELOPMENT] {
            let mut context = tauri::test::mock_context(tauri::test::noop_assets());
            context.config_mut().identifier = identifier.into();
            let app = tauri::test::mock_builder().build(context).unwrap();
            assert_eq!(
                database_of(identifier).unwrap(),
                app.path().app_data_dir().unwrap().join("inventory.sqlite3")
            );
            assert_eq!(
                local_data_of(identifier).unwrap(),
                app.path().app_local_data_dir().unwrap()
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
