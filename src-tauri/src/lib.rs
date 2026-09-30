mod inventory;

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
            save_application
        ])
        .run(tauri::generate_context!())
        .expect("error while running DevHQ");
}
