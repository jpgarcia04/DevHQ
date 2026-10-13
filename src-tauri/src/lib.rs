mod access;
mod attention;
mod backup;
mod change_log;
pub mod gate;
mod http_check;
mod identity;
mod inventory;
#[cfg(test)]
mod ipc_contract;
pub mod local_copies;
mod monitor;
mod records;
mod references;
mod window_lock;
mod workstation;

use local_copies::readings::{self, ReadOutcome, ReadingPage, Which};
use local_copies::Reader;
use monitor::{Monitor, MonitorStatus};
use std::path::{Path, PathBuf};
use tauri::ipc::Channel;
use tauri::{Manager, RunEvent};
use workstation::{Computer, ThisWorkstation};

/// The inventory's path, and what the window found there when it started (`inventory::start`).
struct InventoryPath {
    path: PathBuf,
    ready: Result<(), inventory::ApiError>,
}

impl InventoryPath {
    /// The path to open, when the window started on an inventory in this build's schema; otherwise
    /// why it serves nothing from the file, which it does not open again until DevHQ starts again.
    fn current(&self) -> Result<&Path, inventory::ApiError> {
        self.ready.clone().map(|()| self.path.as_path())
    }
}

#[tauri::command]
fn list_inventory(
    path: tauri::State<'_, InventoryPath>,
) -> Result<inventory::Inventory, inventory::ApiError> {
    inventory::list(path.current()?)
}

/// Where the inventory file is, so that it can be backed up. It reads nothing from it.
#[tauri::command]
fn data_location(path: tauri::State<'_, InventoryPath>) -> inventory::DataLocation {
    inventory::DataLocation::of(&path.path)
}

/// Copies the inventory into its backups folder while DevHQ stays open, one backup at a time and
/// off the main thread: the copy reads the whole file.
#[tauri::command]
async fn backup_inventory(
    path: tauri::State<'_, InventoryPath>,
    backups: tauri::State<'_, backup::Backups>,
) -> Result<backup::Backup, inventory::ApiError> {
    backups.back_up(path.current()?.to_owned()).await
}

/// The files in the backups folder, as found there. Async: a folder can be slow to read.
#[tauri::command]
async fn list_backups(
    path: tauri::State<'_, InventoryPath>,
) -> Result<backup::BackupFolder, inventory::ApiError> {
    backup::list(&path.path)
}

/// Whether agents may use the agent gate, and the executable that registers it.
#[tauri::command]
fn agent_access(
    path: tauri::State<'_, InventoryPath>,
) -> Result<access::AgentAccess, inventory::ApiError> {
    access::agent_access(path.current()?)
}

#[tauri::command]
fn set_agent_access(
    path: tauri::State<'_, InventoryPath>,
    mode: String,
) -> Result<access::AgentAccess, inventory::ApiError> {
    access::set_agent_access(path.current()?, &mode)
}

#[tauri::command]
fn save_resource(
    path: tauri::State<'_, InventoryPath>,
    input: inventory::ResourceInput,
) -> Result<inventory::Resource, inventory::ApiError> {
    inventory::save_resource(path.current()?, input)
}

#[tauri::command]
fn save_application(
    path: tauri::State<'_, InventoryPath>,
    input: inventory::ApplicationInput,
) -> Result<inventory::Application, inventory::ApiError> {
    inventory::save_application(path.current()?, input)
}

/// Deleting a subject takes its HTTP check with it, so supervision forgets it at once.
#[tauri::command]
async fn delete_application(
    path: tauri::State<'_, InventoryPath>,
    monitor: tauri::State<'_, Monitor>,
    id: String,
) -> Result<(), inventory::ApiError> {
    inventory::delete_application(path.current()?, &id)?;
    monitor.refresh();
    Ok(())
}

#[tauri::command]
async fn delete_resource(
    path: tauri::State<'_, InventoryPath>,
    monitor: tauri::State<'_, Monitor>,
    id: String,
) -> Result<(), inventory::ApiError> {
    inventory::delete_resource(path.current()?, &id)?;
    monitor.refresh();
    Ok(())
}

#[tauri::command]
fn record_observation(
    path: tauri::State<'_, InventoryPath>,
    input: records::ObservationInput,
) -> Result<records::Observation, inventory::ApiError> {
    records::record_observation(path.current()?, input)
}

/// Automated observations older than the snapshot carries, a page at a time.
#[tauri::command]
fn older_observations(
    path: tauri::State<'_, InventoryPath>,
    subject: records::Subject,
    before: String,
) -> Result<records::ObservationPage, inventory::ApiError> {
    records::older_observations(path.current()?, &subject, &before)
}

/// Automated events older than a timeline's snapshot carries, for several systems at once.
#[tauri::command]
fn older_events(
    path: tauri::State<'_, InventoryPath>,
    subjects: Vec<records::Subject>,
    before: String,
) -> Result<records::EventPage, inventory::ApiError> {
    records::older_events(path.current()?, &subjects, &before)
}

#[tauri::command]
fn open_incident(
    path: tauri::State<'_, InventoryPath>,
    input: records::IncidentInput,
) -> Result<records::Incident, inventory::ApiError> {
    records::open_incident(path.current()?, input)
}

#[tauri::command]
fn update_incident_status(
    path: tauri::State<'_, InventoryPath>,
    input: records::IncidentStatusInput,
) -> Result<records::Incident, inventory::ApiError> {
    records::update_incident_status(path.current()?, input)
}

#[tauri::command]
fn record_event(
    path: tauri::State<'_, InventoryPath>,
    input: records::EventInput,
) -> Result<records::Event, inventory::ApiError> {
    records::record_event(path.current()?, input)
}

// Commands that change a check tell supervision right away, so pausing, resuming and removing
// take effect before they return. They are async to keep that work off the main thread.

#[tauri::command]
async fn save_http_check(
    path: tauri::State<'_, InventoryPath>,
    monitor: tauri::State<'_, Monitor>,
    input: http_check::HttpCheckInput,
) -> Result<http_check::HttpCheck, inventory::ApiError> {
    let check = http_check::save_http_check(path.current()?, input)?;
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
    let check = http_check::set_http_check_paused(path.current()?, &subject, paused)?;
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
    http_check::remove_http_check(path.current()?, subject)?;
    monitor.refresh();
    Ok(())
}

/// Runs the check now, or waits for the run already in flight, so asking never duplicates
/// evidence. Async, so the request runs off the main thread and the window stays responsive.
#[tauri::command]
async fn run_http_check(
    path: tauri::State<'_, InventoryPath>,
    monitor: tauri::State<'_, Monitor>,
    subject: records::Subject,
) -> Result<records::Observation, inventory::ApiError> {
    // Supervision opens the inventory itself: never one the window does not serve.
    path.current()?;
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

/// Which workstation this computer is, from its local association (never from the inventory).
#[tauri::command]
fn this_workstation(
    path: tauri::State<'_, InventoryPath>,
    computer: tauri::State<'_, Computer>,
) -> Result<ThisWorkstation, inventory::ApiError> {
    Ok(computer.resolve(&inventory::list(path.current()?)?.resources))
}

/// Chooses which workstation this computer is, or forgets it with null. Only the window can.
#[tauri::command]
fn set_this_workstation(
    path: tauri::State<'_, InventoryPath>,
    computer: tauri::State<'_, Computer>,
    resource_id: Option<String>,
) -> Result<ThisWorkstation, inventory::ApiError> {
    computer.choose(
        &inventory::list(path.current()?)?.resources,
        resource_id.as_deref(),
    )
}

/// Reads local copies of this workstation, the ones named or (null) all but those of archived
/// applications, each in a confined reader of its own. Async, and waiting on a thread of its own:
/// one copy can take up to its deadline.
#[tauri::command]
async fn read_copies(
    path: tauri::State<'_, InventoryPath>,
    computer: tauri::State<'_, Computer>,
    reader: tauri::State<'_, Reader>,
    reference_ids: Option<Vec<String>>,
) -> Result<ReadOutcome, inventory::ApiError> {
    let which = reference_ids.map_or(Which::ThisWorkstation, Which::References);
    let (path, computer, reader) = (
        path.current()?.to_owned(),
        computer.inner().clone(),
        reader.inner().clone(),
    );
    tauri::async_runtime::spawn_blocking(move || {
        readings::read_copies(&path, &computer, &reader, &which, &records::Actor::Person)
    })
    .await
    .map_err(|error| {
        inventory::ApiError::read(format!("DevHQ could not wait for its readers: {error}"))
    })?
}

/// A system's readings older than one shown (or its newest), a page at a time.
#[tauri::command]
fn older_readings(
    path: tauri::State<'_, InventoryPath>,
    subject: records::Subject,
    before: Option<String>,
) -> Result<ReadingPage, inventory::ApiError> {
    readings::older_readings(path.current()?, &subject, before.as_deref())
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
        watch_http_monitor,
        this_workstation,
        set_this_workstation,
        read_copies,
        older_readings
    ]
}

/// The state the commands find, over the inventory at `path`, on `computer`, reading copies with
/// `reader`, once the window has done what it does when it starts (`inventory::start`): creating a
/// missing inventory, recovering a current one, and nothing to any other. The app and the IPC
/// contract tests manage this same state. Returns supervision, whose loops are left to the caller,
/// and whether the inventory is current, without which they must not run.
fn manage<R: tauri::Runtime>(
    app: &impl Manager<R>,
    path: PathBuf,
    computer: Computer,
    reader: Reader,
) -> (Monitor, bool) {
    let ready = inventory::start(&path);
    if let Err(error) = &ready {
        eprintln!(
            "DevHQ serves nothing from its inventory: {} ({})",
            error.message, error.code
        );
    }
    let current = ready.is_ok();
    let monitor = Monitor::new(path.clone());
    app.manage(InventoryPath { path, ready });
    app.manage(monitor.clone());
    app.manage(backup::Backups::default());
    app.manage(computer);
    app.manage(reader);
    (monitor, current)
}

pub fn run() {
    // Which data folders this executable keeps: the installed DevHQ's, or development data apart
    // from them (see `identity`). The agent gate makes the same decision.
    let mut context = tauri::generate_context!();
    identity::apply(context.config_mut());
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
            // Which workstation this computer is lives in the local data folder, outside the
            // inventory, which a roaming profile or a copied inventory does not carry.
            let computer = Computer::this_one(app.path().app_local_data_dir()?);
            let reader = Reader::this_executable().map_err(|error| error.message)?;
            // Supervision, and noticing what other processes change, live only as long as this
            // process: no service, no system task. Only over an inventory the window serves: one
            // it refused is never opened again until DevHQ starts again.
            let (monitor, current) = manage(app, path, computer, reader);
            if current {
                tauri::async_runtime::spawn(monitor.clone().run());
                tauri::async_runtime::spawn(monitor.notice_changes());
            }
            Ok(())
        })
        .invoke_handler(commands())
        .build(context)
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
