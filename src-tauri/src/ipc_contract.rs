//! What crosses the IPC boundary, sent through the real command handler on Tauri's mock
//! runtime: no window or WebView, but the same command list, argument parsing and threads, so
//! each command runs where it would in DevHQ.

use super::{commands, InventoryPath};
use crate::monitor::Monitor;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY,
};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager, WebviewWindow, WebviewWindowBuilder};

fn path() -> PathBuf {
    std::env::temp_dir().join(format!("devhq-ipc-test-{}.sqlite3", uuid::Uuid::new_v4()))
}

/// DevHQ's commands and state over the database at `path`, as `run` registers them, without
/// supervision's loop. The app must outlive its window.
fn devhq(path: &Path) -> (App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .invoke_handler(commands())
        .build(mock_context(noop_assets()))
        .unwrap();
    app.manage(InventoryPath(path.to_path_buf()));
    app.manage(Monitor::new(path.to_path_buf()));
    let window = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    (app, window)
}

/// Sends a command with its arguments as the window does, from this thread, which has no
/// async runtime, like the main thread. Returns what the command answered or why it failed.
fn invoke(window: &WebviewWindow<MockRuntime>, command: &str, args: Value) -> Result<Value, Value> {
    get_ipc_response(
        window,
        InvokeRequest {
            cmd: command.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            // The app's own origin, which may call its commands without a capability.
            url: if cfg!(windows) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.into(),
        },
    )
    .map(|body| body.deserialize().unwrap())
}

#[test]
fn the_window_can_watch_supervision_from_a_thread_without_a_runtime() {
    let path = path();
    let (app, window) = devhq(&path);
    // As synchronous, the command spawned its watcher where it ran and panicked: "there is no
    // reactor running, must be called from the context of a Tokio 1.x runtime".
    let answer = invoke(
        &window,
        "watch_http_monitor",
        json!({ "channel": "__CHANNEL__:1" }),
    );
    assert_eq!(answer, Ok(Value::Null));
    drop((window, app));
    let _ = std::fs::remove_file(path);
}
