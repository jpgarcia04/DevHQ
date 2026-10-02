use std::env;
use std::path::Path;

fn main() {
    // tao, muda and Tauri's own dialogs import TaskDialogIndirect, which only exists in Common
    // Controls v6, and a process gets v6 only through its manifest. tauri-build embeds that
    // manifest as a resource that Cargo links into binaries alone, so the executables `cargo test`
    // builds had none and Windows refused to start them (STATUS_ENTRYPOINT_NOT_FOUND) before a
    // single test ran. With the MSVC linker, the same manifest goes into every executable this
    // package links instead, the app included; tauri-build then embeds none, so there is one.
    let windows_msvc = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !windows_msvc {
        tauri_build::build();
        return;
    }
    let manifest = Path::new(&env::var("CARGO_MANIFEST_DIR").expect("set by Cargo"))
        .join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    // Only what the file says, as before: no trustInfo section added by the linker.
    println!("cargo:rustc-link-arg=/MANIFESTUAC:NO");
    let attributes = tauri_build::Attributes::new()
        .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    tauri_build::try_build(attributes).expect("error while building DevHQ");
}
