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
use crate::access::AgentAccess;
use crate::backup::{Backup, BackupCounts, BackupFile, BackupFolder};
use crate::change_log::{Change, FieldChange};
use crate::http_check::{HttpCheck, HttpCheckInput};
use crate::inventory::{
    self, Application, ApplicationInput, Component, DataLocation, Inventory, Resource,
    ResourceInput,
};
use crate::local_copies::readings::{
    CopyReading, LocalCopy, ReadFailure, ReadOutcome, ReadingPage,
};
use crate::local_copies::Remote;
use crate::monitor::{CheckStatus, MonitorStatus, RunError};
use crate::records::{
    AutomatedHistory, Event, EventInput, EventPage, HttpEvidence, Incident, IncidentInput,
    IncidentStatusInput, Observation, ObservationInput, ObservationPage, Redirect, Subject,
    TruncatedHistory,
};
use crate::references::{Reference, SharedRepository};
use crate::upgrade::{BeforeUpgrade, Inspection, Step, TableRows, Upgraded};
use crate::workstation::{Computer, ThisWorkstation};
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
    let folder = crate::inventory::tests::disposable(
        std::env::temp_dir().join(format!("devhq-ipc-test-{}", uuid::Uuid::new_v4())),
    );
    std::fs::create_dir_all(&folder).unwrap();
    folder.join("inventory.sqlite3")
}

fn remove(path: &Path) {
    crate::inventory::tests::remove_now(path.parent().unwrap());
}

/// The computer the contract runs on: its local folder beside the inventory and a fingerprint of
/// its own, never this machine's.
fn computer(path: &Path) -> Computer {
    Computer {
        local: path.parent().unwrap().join("local"),
        fingerprint: Some("contract-computer".into()),
    }
}

/// DevHQ's commands and state over the database at `path`, as `run` registers them, without
/// supervision's loop, reading copies with a confined reader of the test executable. The app must
/// outlive its window.
fn devhq(path: &Path) -> (App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .invoke_handler(commands())
        .build(mock_context(noop_assets()))
        .unwrap();
    manage(
        &app,
        path.to_path_buf(),
        computer(path),
        crate::local_copies::tests::test_reader("confined", &[]),
    );
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
pub(crate) fn fixtures(kind: &str) -> BTreeMap<String, Value> {
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
        "list_inventory" | "data_location" | "backup_inventory" | "list_backups"
        | "agent_access" | "inspect_upgrade" => vec![],
        "set_agent_access" => vec![("mode", text)],
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
        "this_workstation" => vec![],
        "set_this_workstation" => vec![("resourceId", read_completely::<Option<String>>)],
        "read_copies" => vec![("referenceIds", read_completely::<Option<Vec<String>>>)],
        "older_readings" => vec![
            ("subject", subject),
            ("before", read_completely::<Option<String>>),
        ],
        "upgrade_inventory" => vec![
            ("from", read_completely::<i64>),
            ("fingerprint", text),
            ("progress", text),
        ],
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
               VALUES ('host', 'Host', 'server', '', 'Linux'), ('spare', 'Spare', 'workstation', '', ''),
                      ('laptop', 'Laptop', 'workstation', '', 'Windows 11');
             INSERT INTO applications (id, name, lifecycle, description, version)
               VALUES ('shop', 'Shop', 'production', 'Store', '2.0'), ('draft', 'Draft', 'development', '', NULL);
             INSERT INTO components (id, application_id, name, kind, resource_id)
               VALUES ('api', 'shop', 'API', 'backend', 'host');
             -- The reference an edit of the shop keeps by its ID.
             INSERT INTO system_references (id, application_id, kind, target)
               VALUES ('shop-repo', 'shop', 'repository', 'https://github.com/me/shop');
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
               VALUES ('incident-1', 'shop', 'Checkout fails', '', 'critical', 'open', 'manual', 1767225600000, 1767225600000);
             -- A local copy of the shop on the laptop, this computer, at a path where nothing is:
             -- reading it keeps a reading that says so. And an earlier reading, the paging anchor.
             INSERT INTO system_references (id, application_id, kind, target, machine_id)
               VALUES ('shop-copy', 'shop', 'local_copy', '/devhq-contract/shop', 'laptop');
             INSERT INTO copy_readings (id, application_id, reference_id, machine_id, path, source, reader, observed_at, recorded_at, outcome, detail)
               VALUES ('reading-1', 'shop', 'shop-copy', 'laptop', '/devhq-contract/shop', 'read', 'libgit2 1.9.7', 1767225600000, 1767225600000, 'missing', 'Nothing is at this path.');",
        )
        .unwrap();
    // This computer is the laptop.
    computer(&path)
        .choose(&inventory::list(&path).unwrap().resources, Some("laptop"))
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

/// Every file in `folder` and below, by path, with what it holds.
fn files(folder: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    for entry in std::fs::read_dir(folder).unwrap() {
        let entry = entry.unwrap().path();
        if entry.is_dir() {
            found.extend(files(&entry));
        } else {
            found.insert(entry.clone(), std::fs::read(&entry).unwrap());
        }
    }
    found
}

/// Commands that never open the inventory: they say where it is, list its backups folder, or
/// stream supervision's status.
const OPEN_NOTHING: [&str; 3] = ["data_location", "list_backups", "watch_http_monitor"];

/// The window serves nothing from a file that is not an inventory in this build's schema: started
/// on it, every command that would open it refuses with what was found, and no file in its folder
/// changes, appears or goes, whatever was asked (backups and supervision included). An outdated one
/// is looked at, without changing it, for the person to decide on upgrading it; the upgrade the
/// fixture asks for is not what is there (`UPGRADE`), so it is refused before it writes anything.
#[test]
fn no_command_changes_an_inventory_the_window_does_not_serve() {
    use crate::inventory::tests::{database_at, kill, writer};
    const ROWS: &str = "INSERT INTO resources (id, name, kind, description, operating_system)
                          VALUES ('host', 'Host', 'server', '', 'Linux');";
    // What the upgrade the fixture asks for answers for each outdated inventory.
    const UPGRADE: [(&str, &str); 4] = [
        ("v6, rollback journal", "upgrade_changed"),
        ("v6, write-ahead log", "upgrade_changed"),
        ("v6, with a write that did not finish", "needs_recovery"),
        ("v11, Read and write", "upgrade_changed"),
    ];
    type Case = (&'static str, fn(&Path), &'static str);
    let cases: [Case; 9] = [
        (
            "v6, rollback journal",
            |path| database_at(path, 6).execute_batch(ROWS).unwrap(),
            "schema_outdated",
        ),
        (
            "v6, write-ahead log",
            |path| {
                let db = database_at(path, 6);
                db.execute_batch(ROWS).unwrap();
                db.pragma_update(None, "journal_mode", "WAL").unwrap();
            },
            "schema_outdated",
        ),
        (
            "v6, with a write that did not finish",
            |path| {
                database_at(path, 6).execute_batch(ROWS).unwrap();
                kill(writer(path, "journal"), path);
            },
            "schema_outdated",
        ),
        (
            "v11, Read and write",
            |path| {
                database_at(path, 11)
                    .execute_batch("INSERT INTO agent_access VALUES (1, 'write', 0);")
                    .unwrap()
            },
            "schema_outdated",
        ),
        (
            "v14, rollback journal",
            |path| {
                let db = database_at(path, 13);
                db.execute_batch(ROWS).unwrap();
                db.pragma_update(None, "user_version", 14).unwrap();
            },
            "schema_newer",
        ),
        (
            "v14, write-ahead log",
            |path| {
                let db = database_at(path, 13);
                db.pragma_update(None, "user_version", 14).unwrap();
                db.pragma_update(None, "journal_mode", "WAL").unwrap();
            },
            "schema_newer",
        ),
        (
            "an empty file",
            |path| std::fs::write(path, b"").unwrap(),
            "not_an_inventory",
        ),
        (
            "another program's SQLite database",
            |path| {
                rusqlite::Connection::open(path)
                    .unwrap()
                    .execute_batch("CREATE TABLE notes (body TEXT);")
                    .unwrap()
            },
            "not_an_inventory",
        ),
        (
            "stamped 13 without the schema's structure",
            |path| {
                database_at(path, 13)
                    .execute_batch("DROP TABLE agent_access;")
                    .unwrap()
            },
            "schema_mismatch",
        ),
    ];
    let fixtures = fixtures("commands");
    for (case, make, code) in cases {
        let path = path();
        make(&path);
        let folder = path.parent().unwrap().to_owned();
        let before = files(&folder);
        let (app, window) = devhq(&path);
        let upgrade = UPGRADE.iter().find(|(outdated, _)| *outdated == case);
        for (command, args) in &fixtures {
            let answer = invoke(&window, command, args.clone());
            if OPEN_NOTHING.contains(&command.as_str())
                || (command == "inspect_upgrade" && upgrade.is_some())
            {
                assert!(answer.is_ok(), "{case}: {command}: {answer:?}");
            } else if let (Some((_, refused)), "upgrade_inventory") = (upgrade, command.as_str()) {
                let answer = answer.expect_err(&format!("{case}: {command} was served"));
                assert_eq!(answer["code"], *refused, "{case}: {command}: {answer}");
            } else {
                let refusal = answer.expect_err(&format!("{case}: {command} was served"));
                assert_eq!(refusal["code"], code, "{case}: {command}: {refusal}");
            }
        }
        drop((window, app));
        assert_eq!(
            files(&folder),
            before,
            "{case}: the files changed, appeared or went"
        );
        remove(&path);
    }
}

/// A window started where there is no inventory creates a new one in this build's schema, and
/// writes ahead; one whose inventory goes while it is open never creates another in its place.
#[test]
fn creates_an_inventory_only_where_none_is_when_it_starts() {
    let path = path();
    assert!(!path.exists());
    let (app, window) = devhq(&path);
    let listed = invoke(&window, "list_inventory", json!({})).unwrap();
    assert_eq!(listed["applications"], json!([]));
    let header = std::fs::read(&path).unwrap();
    assert_eq!(header[18..20], [2, 2], "the new inventory writes ahead");
    // Only the inventory itself was made: what it was built as is gone.
    let names: BTreeSet<String> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| !name.ends_with("-wal") && !name.ends_with("-shm"))
        .collect();
    assert_eq!(names, BTreeSet::from(["inventory.sqlite3".to_owned()]));

    std::fs::remove_file(&path).unwrap();
    for (command, args) in [
        ("list_inventory", json!({})),
        ("agent_access", json!({})),
        (
            "save_resource",
            fixtures("commands")["save_resource"].clone(),
        ),
        ("backup_inventory", json!({})),
    ] {
        let refusal = invoke(&window, command, args).unwrap_err();
        let message = refusal["message"].as_str().unwrap();
        assert!(
            message.contains("There is no DevHQ inventory at")
                && message.contains("never while it is open"),
            "{command}: {refusal}"
        );
        assert!(!path.exists(), "{command} created an inventory");
    }
    drop((window, app));
    remove(&path);
}

/// A window started on an outdated inventory serves nothing from it until the person upgrades it;
/// then it serves it at once, without starting again: the inventory is current, written ahead, and
/// every command and supervision use it. Asked again, the upgrade has nothing to do.
#[test]
fn serves_an_outdated_inventory_once_the_person_upgrades_it() {
    use crate::inventory::tests::database_at;
    use tauri::Manager;
    let path = path();
    database_at(&path, 6)
        .execute_batch(
            "INSERT INTO resources (id, name, kind, description, operating_system)
               VALUES ('host', 'Host', 'server', 'Rack 2', 'Linux');
             INSERT INTO applications (id, name, lifecycle, description, version)
               VALUES ('shop', 'Shop', 'production', 'Store', '2.0');
             INSERT INTO components (id, application_id, name, kind, resource_id)
               VALUES ('api', 'shop', 'API', 'backend', 'host');",
        )
        .unwrap();
    let (app, window) = devhq(&path);
    let refused = invoke(&window, "list_inventory", json!({})).unwrap_err();
    assert_eq!(refused["code"], "schema_outdated");

    let inspection = invoke(&window, "inspect_upgrade", json!({})).unwrap();
    assert_eq!(inspection["state"], "outdated");
    assert_eq!(inspection["upgradable"], true);
    let consent = json!({
        "from": inspection["storedVersion"],
        "fingerprint": inspection["fingerprint"],
        "progress": "__CHANNEL__:1",
    });
    let upgraded = invoke(&window, "upgrade_inventory", consent.clone()).unwrap();
    assert_eq!(
        (&upgraded["from"], &upgraded["to"]),
        (&json!(6), &json!(13))
    );
    let backup = PathBuf::from(upgraded["backup"]["path"].as_str().unwrap());
    assert!(
        backup.exists() && PathBuf::from(upgraded["backup"]["manifest"].as_str().unwrap()).exists()
    );

    // Served at once: as if the window had started on it.
    let listed = invoke(&window, "list_inventory", json!({})).unwrap();
    assert_eq!(
        listed["applications"][0]["components"][0]["resourceId"],
        "host"
    );
    assert_eq!(
        std::fs::read(&path).unwrap()[18..20],
        [2, 2],
        "written ahead"
    );
    assert!(invoke(&window, "agent_access", json!({})).is_ok());
    let inspection = invoke(&window, "inspect_upgrade", json!({})).unwrap();
    assert_eq!(inspection["state"], "current");
    // Nothing more to do: no second backup.
    let again = invoke(&window, "upgrade_inventory", consent).unwrap();
    assert_eq!(
        (&again["from"], &again["backup"]),
        (&json!(13), &Value::Null)
    );
    assert_eq!(
        std::fs::read_dir(backup.parent().unwrap()).unwrap().count(),
        2
    );

    app.state::<crate::monitor::Monitor>().shutdown();
    drop((window, app));
    remove(&path);
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
    // A reading of the shop's copy on the laptop: commits on no remote, a stash and changes.
    let read_on_laptop = CopyReading {
        id: "reading-2".into(),
        subject: shop(),
        reference_id: "shop-copy".into(),
        machine_id: "laptop".into(),
        path: r"C:\src\platform".into(),
        source: "read".into(),
        reader: "libgit2 1.9.7".into(),
        agent: None,
        observed_at: at("10:00"),
        recorded_at: at("10:00"),
        outcome: "read".into(),
        detail: None,
        branch: Some("main".into()),
        head: Some("0123456789abcdef0123456789abcdef01234567".into()),
        last_commit_at: Some(at("09:00")),
        unpublished_on_head: Some(2),
        unpublished_elsewhere: Some(0),
        stashes: Some(1),
        synced_at: Some(at("08:00")),
        remotes: Some(vec![
            Remote {
                name: "mirror".into(),
                url: None,
            },
            Remote {
                name: "origin".into(),
                url: Some("git@github.com:me/platform.git".into()),
            },
        ]),
        operation: None,
        changed: Some(3),
        untracked: Some(1),
        unverifiable: Some(0),
        worktree_unknown: None,
    };
    let laptop = || ThisWorkstation {
        resource_id: Some("laptop".into()),
        chosen_at: Some(at("08:00")),
        problem: None,
        message: None,
        file: r"C:\Users\Juan Pablo\AppData\Local\com.devhq.desktop\this-workstation.json".into(),
        stored_at: None,
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
            references: vec![
                Reference {
                    id: "shop-repo".into(),
                    kind: "repository".into(),
                    target: "https://github.com/me/platform".into(),
                    label: None,
                    machine_id: None,
                },
                Reference {
                    id: "shop-logs".into(),
                    kind: "logs".into(),
                    target: "/var/log/shop".into(),
                    label: Some("API".into()),
                    machine_id: Some("host".into()),
                },
                Reference {
                    id: "shop-copy".into(),
                    kind: "local_copy".into(),
                    target: r"C:\src\platform".into(),
                    label: None,
                    machine_id: Some("laptop".into()),
                },
            ],
        }],
        resources: vec![
            Resource {
                id: "host".into(),
                name: "Host".into(),
                kind: "server".into(),
                description: "Rack 2".into(),
                operating_system: "Debian 13".into(),
                references: vec![
                    Reference {
                        id: "host-repo".into(),
                        kind: "repository".into(),
                        target: "https://github.com/me/platform".into(),
                        label: None,
                        machine_id: None,
                    },
                    Reference {
                        id: "host-dashboard".into(),
                        kind: "dashboard".into(),
                        target: "https://grafana.example/d/host".into(),
                        label: None,
                        machine_id: None,
                    },
                ],
            },
            Resource {
                id: "laptop".into(),
                name: "Laptop".into(),
                kind: "workstation".into(),
                description: String::new(),
                operating_system: "Windows 11".into(),
                references: vec![],
            },
        ],
        shared_repositories: vec![SharedRepository {
            target: "https://github.com/me/platform".into(),
            systems: vec![shop(), host()],
        }],
        local_copies: vec![LocalCopy {
            reference_id: "shop-copy".into(),
            subject: shop(),
            machine_id: "laptop".into(),
            path: r"C:\src\platform".into(),
            latest: Some(read_on_laptop.clone()),
            moved_since_read: false,
            repository: Some("matches".into()),
            local_work: true,
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
        changes: vec![
            Change {
                id: "change-2".into(),
                subject: shop(),
                subject_name: "Shop".into(),
                source: "agent".into(),
                agent: Some("claude-code".into()),
                action: "updated".into(),
                summary: "Updated application «Shop»: version «1.9» → «2.0»".into(),
                changes: Some(vec![FieldChange {
                    field: "version".into(),
                    before: "1.9".into(),
                    after: "2.0".into(),
                }]),
                occurred_at: at("09:35"),
            },
            Change {
                id: "change-1".into(),
                subject: host(),
                subject_name: "Host".into(),
                source: "manual".into(),
                agent: None,
                action: "check_removed".into(),
                summary: "Removed the HTTP check of «Host» (http://host.example/old); the evidence it recorded stays".into(),
                changes: Some(vec![FieldChange {
                    field: "url".into(),
                    before: "http://host.example/old".into(),
                    after: serde_json::Value::Null,
                }]),
                occurred_at: at("08:45"),
            },
            Change {
                id: "change-0".into(),
                subject: shop(),
                subject_name: "Shop".into(),
                source: "agent".into(),
                agent: Some("codex".into()),
                action: "created".into(),
                summary: "Added application «Shop» (development)".into(),
                changes: None,
                occurred_at: at("08:40"),
            },
        ],
        changes_since: Some(at("08:30")),
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
        changes: 7,
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
        (
            "agent_access".to_owned(),
            value(&AgentAccess {
                mode: "read".into(),
                changed_at: Some(at("09:15")),
                executable: Some(r"C:\Users\Juan Pablo\AppData\Local\DevHQ\devhq.exe".into()),
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
        ("this_workstation".to_owned(), value(laptop())),
        (
            "read_copies".to_owned(),
            value(&ReadOutcome {
                readings: vec![read_on_laptop.clone()],
                failures: vec![ReadFailure {
                    reference_id: "docs-copy".into(),
                    subject: host(),
                    path: r"C:\src\infra".into(),
                    message: r"Reading C:\src\infra took longer than 30 s, so DevHQ stopped it. Nothing was kept.".into(),
                }],
                this_workstation: laptop(),
            }),
        ),
        (
            "older_readings".to_owned(),
            value(&ReadingPage {
                readings: vec![
                    CopyReading {
                        id: "reading-1".into(),
                        agent: Some("claude-code".into()),
                        observed_at: at("07:00"),
                        recorded_at: at("07:00"),
                        last_commit_at: Some(at("06:30")),
                        synced_at: Some(at("06:00")),
                        changed: None,
                        untracked: None,
                        unverifiable: None,
                        worktree_unknown: Some(
                            "Reading the working tree took longer than 30 s, so DevHQ stopped it."
                                .into(),
                        ),
                        ..read_on_laptop.clone()
                    },
                    CopyReading {
                        id: "reading-0".into(),
                        observed_at: at("06:00"),
                        recorded_at: at("06:00"),
                        outcome: "missing".into(),
                        detail: Some("Nothing is at this path.".into()),
                        branch: None,
                        head: None,
                        last_commit_at: None,
                        unpublished_on_head: None,
                        unpublished_elsewhere: None,
                        stashes: None,
                        synced_at: None,
                        remotes: None,
                        changed: None,
                        untracked: None,
                        unverifiable: None,
                        ..read_on_laptop.clone()
                    },
                ],
                more: true,
            }),
        ),
    ]);
    let table = |table: &str, rows: i64| TableRows {
        table: table.into(),
        rows,
    };
    let before_upgrade = BeforeUpgrade {
        name: "inventory-20260101T100500.123Z-v6.sqlite3".into(),
        path: format!(r"{folder}\backups\inventory-20260101T100500.123Z-v6.sqlite3"),
        stored_at: None,
        manifest: format!(r"{folder}\backups\inventory-20260101T100500.123Z-v6.sqlite3.json"),
        taken_at: "2026-01-01T10:05:00.123Z".into(),
        size_bytes: 98_304,
        sha256: "9f2c55b5e2a3c3f0f0d8f1f0d7b3b0a1c2e4f6a8b0c2d4e6f8a0b2c4d6e8f0a2".into(),
        schema_version: 6,
        integrity_check: "ok".into(),
        tables: vec![table("applications", 1), table("resources", 1)],
    };
    let mut answers = answers;
    answers.extend([
        (
            "inspect_upgrade".to_owned(),
            value(&Inspection {
                database: format!(r"{folder}\inventory.sqlite3"),
                stored_at: None,
                state: "outdated",
                stored_version: Some(6),
                version: 12,
                message: "A DevHQ inventory in schema version 6, older than this DevHQ's (12). DevHQ read it without changing it, and serves nothing from it until it is upgraded.".into(),
                upgradable: true,
                problems: vec![],
                tables: vec![
                    table("applications", 1),
                    table("components", 2),
                    table("events", 3),
                    table("http_checks", 1),
                    table("incidents", 0),
                    table("observations", 2),
                    table("resources", 1),
                ],
                fingerprint: Some(
                    "3a7bd3e2360a3d29eea436fcfb7e44c735d117c42d1c1835420b6b9942dd4f1b".into(),
                ),
                backups: format!(r"{folder}\backups"),
            }),
        ),
        (
            "upgrade_inventory".to_owned(),
            value(&Upgraded {
                from: 6,
                to: 12,
                backup: Some(before_upgrade.clone()),
                tables_kept: 7,
                rows_kept: 10,
            }),
        ),
        // What the progress channel of `upgrade_inventory` delivers, once the backup is made.
        (
            "upgrade_step".to_owned(),
            value(&Step::BackedUp {
                backup: Box::new(BeforeUpgrade {
                    stored_at: Some(format!(
                        r"{packaged}\backups\inventory-20260101T100500.123Z-v6.sqlite3"
                    )),
                    ..before_upgrade
                }),
            }),
        ),
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
