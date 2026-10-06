mod access;
mod agent_log;
mod attention;
mod backup;
pub mod gate;
mod http_check;
mod inventory;
#[cfg(test)]
mod ipc_contract;
mod monitor;
mod records;
mod window_lock;

use monitor::{Monitor, MonitorStatus};
use std::path::PathBuf;
use tauri::ipc::Channel;
use tauri::{Manager, RunEvent};

struct InventoryPath(PathBuf);

#[tauri::command]
fn list_inventory(
    path: tauri::State<'_, InventoryPath>,
) -> Result<inventory::Inventory, inventory::ApiError> {
    inventory::list(&path.0)
}

/// Where the inventory file is, so that it can be backed up. It reads nothing from it.
#[tauri::command]
fn data_location(path: tauri::State<'_, InventoryPath>) -> inventory::DataLocation {
    inventory::DataLocation::of(&path.0)
}

/// Copies the inventory into its backups folder while DevHQ stays open, one backup at a time and
/// off the main thread: the copy reads the whole file.
#[tauri::command]
async fn backup_inventory(
    path: tauri::State<'_, InventoryPath>,
    backups: tauri::State<'_, backup::Backups>,
) -> Result<backup::Backup, inventory::ApiError> {
    backups.back_up(path.0.clone()).await
}

/// The files in the backups folder, as found there. Async: a folder can be slow to read.
#[tauri::command]
async fn list_backups(
    path: tauri::State<'_, InventoryPath>,
) -> Result<backup::BackupFolder, inventory::ApiError> {
    backup::list(&path.0)
}

/// Whether agents may use the agent gate, and the executable that registers it.
#[tauri::command]
fn agent_access(
    path: tauri::State<'_, InventoryPath>,
) -> Result<access::AgentAccess, inventory::ApiError> {
    access::agent_access(&path.0)
}

#[tauri::command]
fn set_agent_access(
    path: tauri::State<'_, InventoryPath>,
    mode: String,
) -> Result<access::AgentAccess, inventory::ApiError> {
    access::set_agent_access(&path.0, &mode)
}

#[tauri::command]
fn save_resource(
    path: tauri::State<'_, InventoryPath>,
    input: inventory::ResourceInput,
) -> Result<inventory::Resource, inventory::ApiError> {
    inventory::save_resource(&path.0, input)
}

#[tauri::command]
fn save_application(
    path: tauri::State<'_, InventoryPath>,
    input: inventory::ApplicationInput,
) -> Result<inventory::Application, inventory::ApiError> {
    inventory::save_application(&path.0, input)
}

/// Deleting a subject takes its HTTP check with it, so supervision forgets it at once.
#[tauri::command]
async fn delete_application(
    path: tauri::State<'_, InventoryPath>,
    monitor: tauri::State<'_, Monitor>,
    id: String,
) -> Result<(), inventory::ApiError> {
    inventory::delete_application(&path.0, &id)?;
    monitor.refresh();
    Ok(())
}

#[tauri::command]
async fn delete_resource(
    path: tauri::State<'_, InventoryPath>,
    monitor: tauri::State<'_, Monitor>,
    id: String,
) -> Result<(), inventory::ApiError> {
    inventory::delete_resource(&path.0, &id)?;
    monitor.refresh();
    Ok(())
}

#[tauri::command]
fn record_observation(
    path: tauri::State<'_, InventoryPath>,
    input: records::ObservationInput,
) -> Result<records::Observation, inventory::ApiError> {
    records::record_observation(&path.0, input)
}

/// Automated observations older than the snapshot carries, a page at a time.
#[tauri::command]
fn older_observations(
    path: tauri::State<'_, InventoryPath>,
    subject: records::Subject,
    before: String,
) -> Result<records::ObservationPage, inventory::ApiError> {
    records::older_observations(&path.0, &subject, &before)
}

/// Automated events older than a timeline's snapshot carries, for several systems at once.
#[tauri::command]
fn older_events(
    path: tauri::State<'_, InventoryPath>,
    subjects: Vec<records::Subject>,
    before: String,
) -> Result<records::EventPage, inventory::ApiError> {
    records::older_events(&path.0, &subjects, &before)
}

#[tauri::command]
fn open_incident(
    path: tauri::State<'_, InventoryPath>,
    input: records::IncidentInput,
) -> Result<records::Incident, inventory::ApiError> {
    records::open_incident(&path.0, input)
}

#[tauri::command]
fn update_incident_status(
    path: tauri::State<'_, InventoryPath>,
    input: records::IncidentStatusInput,
) -> Result<records::Incident, inventory::ApiError> {
    records::update_incident_status(&path.0, input)
}

#[tauri::command]
fn record_event(
    path: tauri::State<'_, InventoryPath>,
    input: records::EventInput,
) -> Result<records::Event, inventory::ApiError> {
    records::record_event(&path.0, input)
}

// Commands that change a check tell supervision right away, so pausing, resuming and removing
// take effect before they return. They are async to keep that work off the main thread.

#[tauri::command]
async fn save_http_check(
    path: tauri::State<'_, InventoryPath>,
    monitor: tauri::State<'_, Monitor>,
    input: http_check::HttpCheckInput,
) -> Result<http_check::HttpCheck, inventory::ApiError> {
    let check = http_check::save_http_check(&path.0, input)?;
    monitor.refresh();
    Ok(check)
}

#[tauri::command]
async fn set_http_check_paused(
    path: tauri::State<'_, InventoryPath>,
    monitor: tauri::State<'_, Monitor>,
    subject: records::Subject,
    paused: bool,
) -> Result<http_check::HttpCheck, inventory::ApiError> {
    let check = http_check::set_http_check_paused(&path.0, &subject, paused)?;
    monitor.refresh();
    Ok(check)
}

/// Also cancels the check's run in flight; nothing more is recorded for it.
#[tauri::command]
async fn remove_http_check(
    path: tauri::State<'_, InventoryPath>,
    monitor: tauri::State<'_, Monitor>,
    subject: records::Subject,
) -> Result<(), inventory::ApiError> {
    http_check::remove_http_check(&path.0, subject)?;
    monitor.refresh();
    Ok(())
}

/// Runs the check now, or waits for the run already in flight, so asking never duplicates
/// evidence. Async, so the request runs off the main thread and the window stays responsive.
#[tauri::command]
async fn run_http_check(
    monitor: tauri::State<'_, Monitor>,
    subject: records::Subject,
) -> Result<records::Observation, inventory::ApiError> {
    monitor.run_now(subject).await
}

/// Streams supervision status to the window: now, and after every change. The watcher is a
/// task on the async runtime, so the command must run there too: Tauri runs a synchronous
/// command on the thread that received the request (the main thread), which has no runtime,
/// and spawning the task from there panicked.
#[tauri::command]
async fn watch_http_monitor(
    monitor: tauri::State<'_, Monitor>,
    channel: Channel<MonitorStatus>,
) -> Result<(), inventory::ApiError> {
    monitor.watch(move |status| channel.send(status).is_ok());
    Ok(())
}

/// Every command the window can call. The app and the IPC contract tests register this list.
fn commands<R: tauri::Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        list_inventory,
        data_location,
        backup_inventory,
        list_backups,
        agent_access,
        set_agent_access,
        save_resource,
        save_application,
        delete_application,
        delete_resource,
        record_observation,
        older_observations,
        older_events,
        open_incident,
        update_incident_status,
        record_event,
        save_http_check,
        set_http_check_paused,
        remove_http_check,
        run_http_check,
        watch_http_monitor
    ]
}

/// The state the commands find, over the inventory at `path`. The app and the IPC contract tests
/// manage this same state. Returns supervision, whose loop is left to the caller.
fn manage<R: tauri::Runtime>(app: &impl Manager<R>, path: PathBuf) -> Monitor {
    let monitor = Monitor::new(path.clone());
    app.manage(InventoryPath(path));
    app.manage(monitor.clone());
    app.manage(backup::Backups::default());
    monitor
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let path = app.path().app_data_dir()?.join("inventory.sqlite3");
            // Tells the agent gate that a window is open on this inventory, for as long as this
            // process lives. Without it the gate cannot know; the window works the same.
            match window_lock::WindowLock::hold(&path) {
                Ok(lock) => {
                    app.manage(lock);
                }
                Err(error) => eprintln!("DevHQ could not hold its window lock: {error}"),
            }
            // Supervision, and noticing what other processes change, live only as long as this
            // process: no service, no system task.
            let monitor = manage(app, path);
            tauri::async_runtime::spawn(monitor.clone().run());
            tauri::async_runtime::spawn(monitor.notice_changes());
            Ok(())
        })
        .invoke_handler(commands())
        .build(tauri::generate_context!())
        .expect("error while building DevHQ")
        .run(|app, event| {
            // Closing the window ends supervision: runs in flight are cancelled, none start, and
            // nothing is recorded once shutdown returns (it waits for a commit in progress). It
            // also stops noticing changes and closes the connection that did.
            if let RunEvent::ExitRequested { .. } | RunEvent::Exit = event {
                if let Some(monitor) = app.try_state::<Monitor>() {
                    monitor.shutdown();
                }
            }
        });
}
