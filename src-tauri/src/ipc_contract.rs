//! What crosses the IPC boundary, sent through the real command handler on Tauri's mock
//! runtime: no window or WebView, but the same command list, argument parsing and threads, so
//! each command runs where it would in DevHQ.
//!
//! The window's side is `inventoryApi` (`src/features/inventory/inventory-api.ts`). Both test
//! suites check their side against the same JSON fixtures in `contract/`, so a name changed on
//! one side breaks a test until the other side and the fixture agree again:
//! - `contract/commands/<command>.json` holds the arguments `inventoryApi` sends to a command.
//!   Vitest checks that it sends exactly them. Here every argument must be one the command
//!   reads, read completely, and the command must accept them and succeed.
//! - `contract/responses/<name>.json` holds what a command answers (the supervision channel's
//!   message for `watch_http_monitor`, and any refusal for `error`). Here Rust's serialization
//!   must equal it; Vitest checks that a value of the type the window expects equals it too.

use super::{commands, manage};
use crate::backup::{Backup, BackupCounts, BackupFile, BackupFolder};
use crate::http_check::{HttpCheck, HttpCheckInput};
use crate::inventory::{
    self, Application, ApplicationInput, Component, DataLocation, Inventory, Resource,
    ResourceInput,
};
use crate::monitor::{CheckStatus, MonitorStatus, RunError};
use crate::records::{
    AutomatedHistory, Event, EventInput, EventPage, HttpEvidence, Incident, IncidentInput,
    IncidentStatusInput, Observation, ObservationInput, ObservationPage, Redirect, Subject,
    TruncatedHistory,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY,
};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewWindow, WebviewWindowBuilder};

/// An inventory path in a folder of its own, which also holds its log and its backups.
fn path() -> PathBuf {
    let folder = std::env::temp_dir().join(format!("devhq-ipc-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&folder).unwrap();
    folder.join("inventory.sqlite3")
}

fn remove(path: &Path) {
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// DevHQ's commands and state over the database at `path`, as `run` registers them, without
/// supervision's loop. The app must outlive its window.
fn devhq(path: &Path) -> (App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .invoke_handler(commands())
        .build(mock_context(noop_assets()))
        .unwrap();
    manage(&app, path.to_path_buf());
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
    remove(&path);
}

/// The fixtures in `contract/<kind>/`, by file name without `.json`.
fn fixtures(kind: &str) -> BTreeMap<String, Value> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../contract")
        .join(kind);
    std::fs::read_dir(&folder)
        .unwrap_or_else(|error| panic!("{}: {error}", folder.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|file| {
            file.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|file| {
            let text = std::fs::read_to_string(&file).unwrap();
            let value = serde_json::from_str(&text)
                .unwrap_or_else(|error| panic!("{}: {error}", file.display()));
            let name = file.file_stem().unwrap().to_string_lossy().into_owned();
            (name, value)
        })
        .collect()
}

fn without_nulls(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(_, item)| !item.is_null())
                .map(|(key, item)| (key.clone(), without_nulls(item)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_nulls).collect()),
        other => other.clone(),
    }
}

/// Reads `value` as `T` and writes it back. A key Rust does not know is dropped on the way, and
/// so is an optional field sent under another name, which Rust would otherwise read as absent
/// without complaint: a new resource instead of an edit, the time of recording instead of the
/// time observed. Rust reads null and absent alike, so nulls are left out of the comparison.
fn read_completely<T: DeserializeOwned + Serialize>(value: &Value) -> Result<(), String> {
    let read: T = serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
    let written = serde_json::to_value(read).map_err(|error| error.to_string())?;
    if without_nulls(&written) == without_nulls(value) {
        Ok(())
    } else {
        Err(format!("Rust reads {written} out of {value}"))
    }
}

type Reader = fn(&Value) -> Result<(), String>;

/// Each command's arguments, by the name the window gives them, and what Rust reads them as.
fn arguments(command: &str) -> Vec<(&'static str, Reader)> {
    let subject: Reader = read_completely::<Subject>;
    let text: Reader = read_completely::<String>;
    match command {
        "list_inventory" | "data_location" | "backup_inventory" | "list_backups" => vec![],
        "save_resource" => vec![("input", read_completely::<ResourceInput>)],
        "save_application" => vec![("input", read_completely::<ApplicationInput>)],
        "delete_application" | "delete_resource" => vec![("id", text)],
        "record_observation" => vec![("input", read_completely::<ObservationInput>)],
        "older_observations" => vec![("subject", subject), ("before", text)],
        "older_events" => vec![
            ("subjects", read_completely::<Vec<Subject>>),
            ("before", text),
        ],
        "open_incident" => vec![("input", read_completely::<IncidentInput>)],
        "update_incident_status" => vec![("input", read_completely::<IncidentStatusInput>)],
        "record_event" => vec![("input", read_completely::<EventInput>)],
        "save_http_check" => vec![("input", read_completely::<HttpCheckInput>)],
        "set_http_check_paused" => vec![("subject", subject), ("paused", read_completely::<bool>)],
        "remove_http_check" | "run_http_check" => vec![("subject", subject)],
        // A channel crosses as `__CHANNEL__:<id>`.
        "watch_http_monitor" => vec![("channel", text)],
        _ => panic!("contract/commands/{command}.json names no command of DevHQ's"),
    }
}

/// A database holding what the command fixtures refer to, so that each of them can succeed.
fn seeded() -> PathBuf {
    let path = path();
    inventory::open(&path)
        .unwrap()
        .execute_batch(
            "INSERT INTO resources (id, name, kind, description, operating_system)
               VALUES ('host', 'Host', 'server', '', 'Linux'), ('spare', 'Spare', 'workstation', '', '');
             INSERT INTO applications (id, name, lifecycle, description, version)
               VALUES ('shop', 'Shop', 'production', 'Store', '2.0'), ('draft', 'Draft', 'development', '', NULL);
             INSERT INTO components (id, application_id, name, kind, resource_id)
               VALUES ('api', 'shop', 'API', 'backend', 'host');
             -- Supervised, and pointed at a reserved domain: running it records a DNS failure.
             INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at, monitor, monitor_interval_minutes)
               VALUES ('host', 'http://devhq-contract.invalid/health', 200, 1, 15, 1767225600000, 'active', 5);
             -- Two automated results of the host, with their events: the paging anchors.
             INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at,
                                       http_url, http_expected_status, http_status, http_latency_ms, http_error, http_redirects)
               VALUES ('obs-http-1', 'host', 'healthy', 'HTTP 200 as expected', 'http', 1767225600000, 1767226500000, 1767225600000,
                       'http://devhq-contract.invalid/health', 200, 200, 5, NULL, '[]'),
                      ('obs-http-2', 'host', 'healthy', 'HTTP 200 as expected', 'http', 1767225900000, 1767226800000, 1767225900000,
                       'http://devhq-contract.invalid/health', 200, 200, 5, NULL, '[]');
             INSERT INTO events (id, resource_id, kind, outcome, title, detail, source, observation_id, occurred_at, recorded_at)
               VALUES ('evt-http-1', 'host', 'observation', 'success', 'HTTP check: Healthy', 'HTTP 200 as expected', 'http', 'obs-http-1', 1767225600000, 1767225600000),
                      ('evt-http-2', 'host', 'observation', 'success', 'HTTP check: Healthy', 'HTTP 200 as expected', 'http', 'obs-http-2', 1767225900000, 1767225900000);
             INSERT INTO incidents (id, application_id, title, description, severity, status, source, opened_at, updated_at)
               VALUES ('incident-1', 'shop', 'Checkout fails', '', 'critical', 'open', 'manual', 1767225600000, 1767225600000);",
        )
        .unwrap();
    path
}

#[test]
fn every_command_reads_and_accepts_exactly_what_the_window_sends() {
    let fixtures = fixtures("commands");
    assert!(!fixtures.is_empty());
    for (command, args) in &fixtures {
        let expected = arguments(command);
        let names: BTreeSet<&str> = args
            .as_object()
            .unwrap_or_else(|| panic!("{command}: the arguments are not an object"))
            .keys()
            .map(String::as_str)
            .collect();
        // Tauri ignores an argument the command does not take.
        assert_eq!(
            names,
            expected.iter().map(|(name, _)| *name).collect(),
            "{command}: argument names"
        );
        for (name, read) in expected {
            if let Err(error) = read(&args[name]) {
                panic!("{command}: `{name}`: {error}");
            }
        }
        // Through the real handler, on a database of its own: the command's name and its
        // arguments' names are DevHQ's, and their values pass its validation.
        let path = seeded();
        let (app, window) = devhq(&path);
        let answer = invoke(&window, command, args.clone());
        assert!(answer.is_ok(), "{command}: {answer:?}");
        drop((window, app));
        remove(&path);
    }
}

fn value(answer: impl Serialize) -> Value {
    serde_json::to_value(answer).unwrap()
}

#[test]
fn every_answer_has_the_shape_the_window_expects() {
    let host = || Subject::Resource { id: "host".into() };
    let shop = || Subject::Application { id: "shop".into() };
    let at = |time: &str| format!("2026-01-01T{time}:00Z");
    let checked = HttpEvidence {
        url: "http://host.example/health".into(),
        expected_status: 200,
        status: Some(200),
        latency_ms: 42,
        error: None,
        redirects: vec![Redirect {
            status: 301,
            location: "https://host.example/health".into(),
        }],
    };
    let snapshot = Inventory {
        applications: vec![Application {
            id: "shop".into(),
            name: "Shop".into(),
            description: "Store front".into(),
            lifecycle: "production".into(),
            version: Some("2.0".into()),
            components: vec![
                Component {
                    id: "api".into(),
                    name: "API".into(),
                    kind: "backend".into(),
                    resource_id: Some("host".into()),
                },
                Component {
                    id: "db".into(),
                    name: "Database".into(),
                    kind: "database".into(),
                    resource_id: None,
                },
            ],
        }],
        resources: vec![Resource {
            id: "host".into(),
            name: "Host".into(),
            kind: "server".into(),
            description: "Rack 2".into(),
            operating_system: "Debian 13".into(),
        }],
        observations: vec![
            Observation {
                id: "obs-http".into(),
                subject: host(),
                result: "healthy".into(),
                summary: "HTTP 200 as expected, in 42 ms".into(),
                source: "http".into(),
                observed_at: at("10:05"),
                valid_until: at("10:20"),
                recorded_at: at("10:05"),
                http: Some(checked),
                agent: None,
            },
            Observation {
                id: "obs-manual".into(),
                subject: shop(),
                result: "degraded".into(),
                summary: "Checkout is slow".into(),
                source: "manual".into(),
                observed_at: at("09:30"),
                valid_until: at("10:30"),
                recorded_at: at("09:45"),
                http: None,
                agent: None,
            },
        ],
        incidents: vec![Incident {
            id: "incident-1".into(),
            subject: shop(),
            title: "Checkout fails".into(),
            description: "Payments time out".into(),
            severity: "critical".into(),
            status: "resolved".into(),
            source: "manual".into(),
            opened_at: at("09:00"),
            updated_at: at("09:50"),
            resolved_at: Some(at("09:50")),
            agent: None,
        }],
        events: vec![
            Event {
                id: "evt-http".into(),
                subject: host(),
                kind: "observation".into(),
                outcome: "success".into(),
                title: "HTTP check: Healthy".into(),
                detail: "HTTP 200 as expected, in 42 ms".into(),
                source: "http".into(),
                observation_id: Some("obs-http".into()),
                incident_id: None,
                occurred_at: at("10:05"),
                recorded_at: at("10:05"),
                agent: None,
            },
            Event {
                id: "evt-incident".into(),
                subject: shop(),
                kind: "incident".into(),
                outcome: "success".into(),
                title: "Incident resolved: Checkout fails".into(),
                detail: "Rolled back".into(),
                source: "manual".into(),
                observation_id: None,
                incident_id: Some("incident-1".into()),
                occurred_at: at("09:50"),
                recorded_at: at("09:50"),
                agent: None,
            },
            Event {
                id: "evt-agent".into(),
                subject: shop(),
                kind: "deployment".into(),
                outcome: "success".into(),
                title: "Deployed 2.4.1".into(),
                detail: "Noted while the person watched".into(),
                source: "agent".into(),
                observation_id: None,
                incident_id: None,
                occurred_at: at("09:40"),
                recorded_at: at("09:41"),
                agent: Some("claude-code".into()),
            },
        ],
        http_checks: vec![HttpCheck {
            subject: host(),
            url: "http://host.example/health".into(),
            expected_status: 200,
            timeout_seconds: 5,
            valid_for_minutes: 15,
            updated_at: at("08:00"),
            monitor: "active".into(),
            monitor_interval_minutes: Some(5),
        }],
        automated_history: AutomatedHistory {
            limit: 200,
            truncated: vec![TruncatedHistory {
                subject: host(),
                since: at("10:05"),
            }],
        },
    };
    let timed_out = Observation {
        id: "obs-timeout".into(),
        subject: host(),
        result: "unhealthy".into(),
        summary: "Timed out after 5 s".into(),
        source: "http".into(),
        observed_at: at("10:00"),
        valid_until: at("10:15"),
        recorded_at: at("10:00"),
        http: Some(HttpEvidence {
            url: "http://host.example/health".into(),
            expected_status: 200,
            status: None,
            latency_ms: 5000,
            error: Some("Timed out after 5 s".into()),
            redirects: vec![],
        }),
        agent: None,
    };
    let its_event = Event {
        id: "evt-timeout".into(),
        subject: host(),
        kind: "observation".into(),
        outcome: "failure".into(),
        title: "HTTP check: Unhealthy".into(),
        detail: "Timed out after 5 s".into(),
        source: "http".into(),
        observation_id: Some("obs-timeout".into()),
        incident_id: None,
        occurred_at: at("10:00"),
        recorded_at: at("10:00"),
        agent: None,
    };
    let folder = r"C:\Users\Juan Pablo\AppData\Roaming\com.devhq.desktop";
    // Where Windows keeps that folder's files for a DevHQ started from a packaged app.
    let packaged = r"C:\Users\Juan Pablo\AppData\Local\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\com.devhq.desktop";
    let supervision = MonitorStatus {
        checks: vec![CheckStatus {
            subject: host(),
            running: false,
            last_run_at: Some(at("10:05")),
            next_run_at: Some(at("10:10")),
            last_error: Some(RunError {
                message: "Inventory database error: disk I/O error".into(),
                at: at("09:55"),
            }),
        }],
        error: None,
        recorded: 3,
    };
    // A refusal, as the window receives it through the real handler.
    let path = path();
    let (app, window) = devhq(&path);
    let refused = invoke(&window, "delete_resource", json!({ "id": "ghost" })).unwrap_err();
    drop((window, app));
    remove(&path);

    let answers = BTreeMap::from([
        ("list_inventory".to_owned(), value(&snapshot)),
        (
            "older_observations".to_owned(),
            value(&ObservationPage {
                observations: vec![timed_out],
                more: true,
            }),
        ),
        (
            "older_events".to_owned(),
            value(&EventPage {
                events: vec![its_event],
                more: false,
            }),
        ),
        (
            "data_location".to_owned(),
            value(&DataLocation {
                database: format!(r"{folder}\inventory.sqlite3"),
                directory: folder.into(),
                stored_at: Some(format!(r"{packaged}\inventory.sqlite3")),
            }),
        ),
        ("watch_http_monitor".to_owned(), value(&supervision)),
        (
            "backup_inventory".to_owned(),
            value(&Backup {
                name: "inventory-20260101T100500.123Z.sqlite3".into(),
                path: format!(r"{folder}\backups\inventory-20260101T100500.123Z.sqlite3"),
                stored_at: None,
                taken_at: "2026-01-01T10:05:00.123Z".into(),
                size_bytes: 2_097_152,
                quick_check: "ok".into(),
                schema_version: 6,
                counts: BackupCounts {
                    applications: 1,
                    components: 2,
                    resources: 1,
                    observations: 2,
                    incidents: 1,
                    events: 2,
                    http_checks: 1,
                },
            }),
        ),
        (
            "list_backups".to_owned(),
            value(&BackupFolder {
                directory: format!(r"{folder}\backups"),
                files: vec![
                    BackupFile {
                        name: "inventory-20260101T100500.123Z.sqlite3".into(),
                        size_bytes: 2_097_152,
                        modified_at: Some("2026-01-01T10:05:01Z".into()),
                        partial: false,
                        stored_at: None,
                    },
                    BackupFile {
                        name: "inventory-20260101T090000.000Z.sqlite3.partial".into(),
                        size_bytes: 1_048_576,
                        modified_at: None,
                        partial: true,
                        stored_at: Some(format!(
                            r"{packaged}\backups\inventory-20260101T090000.000Z.sqlite3.partial"
                        )),
                    },
                ],
            }),
        ),
        ("error".to_owned(), refused),
    ]);
    let fixtures = fixtures("responses");
    assert_eq!(
        fixtures.keys().collect::<Vec<_>>(),
        answers.keys().collect::<Vec<_>>()
    );
    for (name, answer) in &answers {
        assert_eq!(answer, &fixtures[name], "contract/responses/{name}.json");
    }
}
