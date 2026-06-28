use std::sync::Mutex;

use tauri::State;

use crate::kernel::event_store::EventStore;
use crate::kernel::models::AccessMode;
use crate::kernel::models::FoundationState;
use crate::kernel::models::MemoryRecord;
use crate::kernel::models::TaskRecord;
use crate::kernel::policy::{CapabilityKind, PermissionAuditEntry};
use crate::kernel::work_package::{
    export_work_package as build_work_package, parse_work_package_json, WorkPackage,
    WorkPackageImportSummary,
};

pub struct AppState {
    event_store: Mutex<EventStore>,
}

impl AppState {
    pub fn new(event_store: EventStore) -> Self {
        Self {
            event_store: Mutex::new(event_store),
        }
    }
}

fn event_store_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn lock_error() -> String {
    "event store lock is unavailable".to_string()
}

#[tauri::command]
pub fn get_foundation_state() -> FoundationState {
    FoundationState::default()
}

#[tauri::command]
pub fn list_task_records(state: State<'_, AppState>) -> Result<Vec<TaskRecord>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store.list_task_records().map_err(event_store_error)
}

#[tauri::command]
pub fn list_memory_records(state: State<'_, AppState>) -> Result<Vec<MemoryRecord>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store.list_memory_records().map_err(event_store_error)
}

#[tauri::command]
pub fn search_memory_records(
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<MemoryRecord>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .search_memory_records(&query)
        .map_err(event_store_error)
}

#[tauri::command]
pub fn list_permission_audit_entries(
    state: State<'_, AppState>,
) -> Result<Vec<PermissionAuditEntry>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .list_permission_audit_entries()
        .map_err(event_store_error)
}

#[tauri::command]
pub fn record_permission_audit(
    access_mode: AccessMode,
    capability: CapabilityKind,
    state: State<'_, AppState>,
) -> Result<PermissionAuditEntry, String> {
    let entry = PermissionAuditEntry::evaluate(access_mode, capability);
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    Ok(entry)
}

#[tauri::command]
pub fn create_task_record(
    title: String,
    summary: String,
    state: State<'_, AppState>,
) -> Result<TaskRecord, String> {
    let record = TaskRecord::new(title, summary)?;
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .append_task_record(&record)
        .map_err(event_store_error)?;
    let memory = MemoryRecord::from_task_record(&record);
    store
        .append_memory_record(&memory)
        .map_err(event_store_error)?;
    Ok(record)
}

#[tauri::command]
pub fn export_work_package(state: State<'_, AppState>) -> Result<WorkPackage, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    let task_records = store.list_task_records().map_err(event_store_error)?;
    Ok(build_work_package(FoundationState::default(), task_records))
}

#[tauri::command]
pub fn import_work_package(
    package_json: String,
    state: State<'_, AppState>,
) -> Result<WorkPackageImportSummary, String> {
    let package = parse_work_package_json(&package_json).map_err(event_store_error)?;
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .import_task_records(&package.task_records)
        .map_err(event_store_error)
}
