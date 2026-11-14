use std::env;

// Which commit this build is made from, read from `.git` as files: shared with the crate's tests.
include!("src/build_version.rs");

/// Makes the version resource of a Windows build declare the commit it was made from, after the
/// version: «0.2.0+3f9c2ab01d4e». tauri-build writes `ProductVersion` and `FileVersion` from the
/// version in Tauri's configuration, and merges `TAURI_CONFIG` into that configuration when it
/// runs, so the version with its commit goes there, for this build script alone: the application's
/// own version, which `generate_context!` reads in another process, and the installer's, which the
/// bundler reads, stay the plain one. Without a `.git` to read, nothing is added.
fn declare_the_commit(manifest_dir: &Path) {
    println!("cargo:rerun-if-env-changed=DEVHQ_BUILD_DIRTY");
    let Some(checkout) = manifest_dir.parent() else {
        return;
    };
    let Some(head) = head_of(checkout) else {
        return;
    };
    // Made again when HEAD names another commit: each of these files changes when it does.
    for file in &head.watch {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    // Only whoever builds knows whether the tree had uncommitted changes: this script does not
    // run `git status`, so it says «dirty» when told and otherwise claims nothing of the tree.
    let dirty = env::var("DEVHQ_BUILD_DIRTY").is_ok_and(|value| !value.is_empty() && value != "0");
    let version = declared_version(
        &env::var("CARGO_PKG_VERSION").expect("set by Cargo"),
        Some(&head),
        dirty,
    );
    // On top of whatever the builder already merges into the configuration.
    let mut config: serde_json::Value = env::var("TAURI_CONFIG")
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    config["version"] = serde_json::Value::String(version);
    env::set_var("TAURI_CONFIG", config.to_string());
}

fn main() {
    // tao, muda and Tauri's own dialogs import TaskDialogIndirect, which only exists in Common
    // Controls v6, and a process gets v6 only through its manifest. tauri-build embeds that
    // manifest as a resource that Cargo links into binaries alone, so the executables `cargo test`
    // builds had none and Windows refused to start them (STATUS_ENTRYPOINT_NOT_FOUND) before a
    // single test ran. With the MSVC linker, the same manifest goes into every executable this
    // package links instead, the app included; tauri-build then embeds none, so there is one.
    let windows = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let windows_msvc = windows && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("set by Cargo");
    // Only a Windows executable has a version resource to declare it in.
    if windows {
        declare_the_commit(Path::new(&manifest_dir));
    }
    if !windows_msvc {
        tauri_build::build();
        return;
    }
    let manifest = Path::new(&manifest_dir).join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    // Only what the file says, as before: no trustInfo section added by the linker.
    println!("cargo:rustc-link-arg=/MANIFESTUAC:NO");
    let attributes = tauri_build::Attributes::new()
        .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    tauri_build::try_build(attributes).expect("error while building DevHQ");
}
