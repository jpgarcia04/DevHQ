mod inventory;
mod records;

use std::path::PathBuf;
use tauri::Manager;

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

#[tauri::command]
fn record_observation(
    path: tauri::State<'_, InventoryPath>,
    input: records::ObservationInput,
) -> Result<records::Observation, inventory::ApiError> {
    records::record_observation(&path.0, input)
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

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            app.manage(InventoryPath(directory.join("inventory.sqlite3")));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_inventory,
            save_resource,
            save_application,
            record_observation,
            open_incident,
            update_incident_status,
            record_event
        ])
        .run(tauri::generate_context!())
        .expect("error while running DevHQ");
}
