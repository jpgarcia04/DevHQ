mod http_check;
mod inventory;
mod monitor;
mod records;

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

/// Streams supervision status to the window: now, and after every change.
#[tauri::command]
fn watch_http_monitor(monitor: tauri::State<'_, Monitor>, channel: Channel<MonitorStatus>) {
    monitor.watch(move |status| channel.send(status).is_ok());
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let path = app.path().app_data_dir()?.join("inventory.sqlite3");
            // Supervision lives only as long as this process: no service, no system task.
            let monitor = Monitor::new(path.clone());
            tauri::async_runtime::spawn(monitor.clone().run());
            app.manage(InventoryPath(path));
            app.manage(monitor);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_inventory,
            save_resource,
            save_application,
            delete_application,
            delete_resource,
            record_observation,
            older_observations,
            open_incident,
            update_incident_status,
            record_event,
            save_http_check,
            set_http_check_paused,
            remove_http_check,
            run_http_check,
            watch_http_monitor
        ])
        .build(tauri::generate_context!())
        .expect("error while building DevHQ")
        .run(|app, event| {
            // Closing the window ends supervision: runs in flight are cancelled, none start.
            if let RunEvent::ExitRequested { .. } | RunEvent::Exit = event {
                if let Some(monitor) = app.try_state::<Monitor>() {
                    monitor.shutdown();
                }
            }
        });
}
