use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::thread;
use std::time::Duration;
use tauri::State;
use uuid::Uuid;

use crate::commands::AppState;
use crate::kernel::artifact_render::render_artifact_file;
use crate::kernel::artifacts::ArtifactDeliveryView;
use crate::kernel::artifacts::{
    ArtifactEngine, ArtifactGenerationRequest, ArtifactInput, ArtifactPhase,
};
use crate::kernel::tool_runtime::ToolExecutionStatus;

pub fn spawn_artifact_recovery_worker(state: AppState) {
    thread::spawn(move || loop {
        run_artifact_recovery_sweep(&state);
        thread::sleep(Duration::from_secs(30));
    });
}

pub(crate) fn run_artifact_recovery_sweep(state: &AppState) {
    reconcile_generation_intents(state);
    let due = {
        let event_store = state.event_store();
        let Ok(store) = event_store.lock() else {
            return;
        };
        let mut valid = Vec::new();
        for (mut record, revision) in store
            .recoverable_artifact_records(64, Utc::now())
            .unwrap_or_default()
        {
            match store.artifact_storage_path(
                &record.storage_ref,
                record.request_id,
                &record.artifact_hash,
            ) {
                Ok(path) if valid.len() < 16 => valid.push((record, revision, path)),
                Ok(_) => {}
                Err(_) => {
                    record.phase = ArtifactPhase::Failed;
                    record.safe_error = Some("artifact_storage_binding_invalid".to_string());
                    record.updated_at = Utc::now();
                    let _ = store.update_artifact_record(&record, revision);
                }
            }
        }
        valid
    };
    for (mut record, row_revision, path) in due {
        let Ok(bytes) = fs::read(&path) else {
            mark_artifact_needs_attention(
                state,
                &mut record,
                row_revision,
                "artifact_file_missing",
            );
            continue;
        };
        if hex::encode(Sha256::digest(&bytes)) != record.artifact_hash {
            mark_artifact_needs_attention(
                state,
                &mut record,
                row_revision,
                "artifact_file_identity_changed",
            );
            continue;
        }
        let mut previews = None;
        let mut new_binding: Option<(String, String, String)> = None;
        let transition = match record.phase {
            ArtifactPhase::Generated => {
                let _ = ArtifactEngine::check_structure(&mut record, &bytes, Utc::now());
                Ok(())
            }
            ArtifactPhase::StructureChecked => {
                render_artifact_file(record.format, Path::new(&path)).and_then(|rendered| {
                    let preview_ref = format!(
                        "artifact-preview:{}:{}",
                        record.id, record.artifact_revision
                    );
                    let visual_result = ArtifactEngine::check_actual_visual(
                        &mut record,
                        &rendered.pages,
                        rendered.renderer_version,
                        preview_ref,
                        Utc::now(),
                    );
                    if visual_result.is_ok() {
                        previews = Some(rendered.pages);
                    }
                    Ok(())
                })
            }
            ArtifactPhase::ReadyForDelivery | ArtifactPhase::VisualChecked => {
                record.complete(Utc::now())
            }
            ArtifactPhase::RevisionRequired => record.request_revision(Utc::now()),
            ArtifactPhase::RevisionPrepared => {
                let regenerated = ArtifactEngine::generate(
                    &ArtifactGenerationRequest {
                        request_id: record.request_id,
                        input: record.input.clone(),
                        template: record.template.clone(),
                        approved_storage_ref: record.storage_ref.clone(),
                    },
                    Utc::now(),
                );
                regenerated.and_then(|regenerated| {
                    if regenerated.record.input_fingerprint != record.input_fingerprint {
                        return Err("artifact revision input binding changed".to_string());
                    }
                    let current = Path::new(&path);
                    let stem = current
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .ok_or_else(|| "artifact revision path is invalid".to_string())?;
                    let extension = current
                        .extension()
                        .and_then(|value| value.to_str())
                        .ok_or_else(|| "artifact revision extension is invalid".to_string())?;
                    let revised_path = current.with_file_name(format!(
                        "{stem}.revision-{}.{}",
                        record.revision_attempts, extension
                    ));
                    {
                        let event_store = state.event_store();
                        let store = event_store.lock().map_err(|_| {
                            "artifact revision authority is unavailable".to_string()
                        })?;
                        store
                            .authorize_artifact_revision_path(
                                &record.storage_ref,
                                record.request_id,
                                &revised_path,
                                record.revision_attempts,
                            )
                            .map_err(|_| "artifact revision is not authorized".to_string())?;
                    }
                    write_revision_atomically(
                        &revised_path,
                        &regenerated.bytes,
                        record.id,
                        record.revision_attempts,
                    )?;
                    let revised_hash = hex::encode(Sha256::digest(&regenerated.bytes));
                    let revised_storage_ref = format!(
                        "artifact-storage:{}:revision:{}",
                        record.id, record.revision_attempts
                    );
                    new_binding = Some((
                        revised_storage_ref.clone(),
                        revised_path.to_string_lossy().to_string(),
                        revised_hash,
                    ));
                    record.storage_ref = revised_storage_ref;
                    let input_fingerprint = record.input_fingerprint.clone();
                    record.replace_revision(&regenerated.bytes, input_fingerprint, Utc::now())
                })
            }
            _ => continue,
        };
        if transition.is_err() && record.phase != ArtifactPhase::Failed {
            mark_artifact_needs_attention(
                state,
                &mut record,
                row_revision,
                "artifact_recovery_transition_failed",
            );
            continue;
        }
        let event_store = state.event_store();
        let Ok(store) = event_store.lock() else {
            continue;
        };
        if let Some(pages) = previews.as_ref() {
            if store
                .store_artifact_visual_previews(&record, pages)
                .is_err()
            {
                continue;
            }
        }
        if let Some((storage_ref, revised_path, revised_hash)) = new_binding.as_ref() {
            if store
                .bind_artifact_storage_path(
                    storage_ref,
                    record.request_id,
                    revised_path,
                    revised_hash,
                    Utc::now(),
                )
                .is_err()
            {
                continue;
            }
        }
        let _ = store.update_artifact_record(&record, row_revision);
    }
}

fn reconcile_generation_intents(state: &AppState) {
    let event_store = state.event_store();
    let Ok(store) = event_store.lock() else {
        return;
    };
    let intents = store
        .pending_artifact_generation_intents()
        .unwrap_or_default();
    if intents.is_empty() {
        return;
    }
    let invocations = store.list_tool_invocations().unwrap_or_default();
    for (fingerprint, input) in intents {
        let Some(invocation) = invocations
            .iter()
            .find(|invocation| invocation.request_fingerprint == fingerprint)
        else {
            continue;
        };
        if invocation.status == ToolExecutionStatus::Succeeded {
            if let ArtifactInput::Office { spec } = input {
                let _ = crate::commands::register_tool_office_artifact_if_succeeded(
                    &store, invocation, &spec,
                );
            }
        } else if matches!(
            invocation.status,
            ToolExecutionStatus::Failed | ToolExecutionStatus::Blocked
        ) {
            let _ = store.fulfill_artifact_generation_intent(&fingerprint, Utc::now());
        }
    }
}

fn mark_artifact_needs_attention(
    state: &AppState,
    record: &mut crate::kernel::artifacts::ArtifactRecord,
    row_revision: u64,
    code: &str,
) {
    record.phase = ArtifactPhase::Failed;
    record.safe_error = Some(code.to_string());
    record.updated_at = Utc::now();
    let event_store = state.event_store();
    if let Ok(store) = event_store.lock() {
        let _ = store.update_artifact_record(record, row_revision);
    };
}

fn write_revision_atomically(
    revised_path: &Path,
    bytes: &[u8],
    artifact_id: Uuid,
    attempt: u32,
) -> Result<(), String> {
    if revised_path.exists() {
        return match fs::read(revised_path) {
            Ok(existing) if existing == bytes => Ok(()),
            _ => Err("artifact revision receipt changed".to_string()),
        };
    }
    let file_name = revised_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "artifact revision filename is invalid".to_string())?;
    let temp_path =
        revised_path.with_file_name(format!(".{file_name}.ds-agent-{artifact_id}-{attempt}.tmp"));
    if temp_path.exists() {
        let existing = fs::read(&temp_path)
            .map_err(|_| "artifact revision temporary receipt is unreadable".to_string())?;
        if existing != bytes {
            return Err("artifact revision temporary receipt changed".to_string());
        }
    } else {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|_| "artifact revision could not be staged".to_string())?;
        file.write_all(bytes)
            .map_err(|_| "artifact revision could not be written".to_string())?;
        file.sync_all()
            .map_err(|_| "artifact revision could not be synced".to_string())?;
    }
    fs::rename(&temp_path, revised_path)
        .map_err(|_| "artifact revision could not be atomically finalized".to_string())
}

#[tauri::command]
pub fn list_artifact_deliveries(
    state: State<'_, AppState>,
) -> Result<Vec<ArtifactDeliveryView>, String> {
    let event_store = state.event_store();
    let store = event_store
        .lock()
        .map_err(|_| "artifact delivery status is unavailable".to_string())?;
    store
        .list_artifact_deliveries(50)
        .map_err(|_| "artifact delivery status is unavailable".to_string())
}

#[tauri::command]
pub fn get_artifact_visual_preview(
    artifact_id: Uuid,
    page_index: u32,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let event_store = state.event_store();
    let store = event_store
        .lock()
        .map_err(|_| "artifact preview is unavailable".to_string())?;
    let bytes = store
        .artifact_visual_preview_page(artifact_id, page_index)
        .map_err(|_| "artifact preview is unavailable".to_string())?;
    Ok(format!(
        "data:image/png;base64,{}",
        general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::artifacts::{ArtifactGenerationRequest, ArtifactInput, ArtifactTemplateRef};
    use crate::kernel::event_store::EventStore;

    #[test]
    fn revision_preparation_survives_restart_and_writes_a_versioned_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("artifact-recovery.sqlite3");
        let original_path = temp.path().join("damaged.pdf");
        let request_id = Uuid::new_v4();
        let mut generated = ArtifactEngine::generate(
            &ArtifactGenerationRequest {
                request_id,
                input: ArtifactInput::Pdf {
                    title: "Recovery fixture".to_string(),
                    paragraphs: vec!["Versioned repair".to_string()],
                },
                template: ArtifactTemplateRef {
                    template_id: "recovery-fixture".to_string(),
                    version: 1,
                    content_hash: "d".repeat(64),
                },
                approved_storage_ref: format!("artifact-storage:{request_id}"),
            },
            Utc::now(),
        )
        .unwrap();
        let damaged = b"damaged artifact bytes".to_vec();
        fs::write(&original_path, &damaged).unwrap();
        generated.record.artifact_hash = hex::encode(Sha256::digest(&damaged));
        generated.record.phase = ArtifactPhase::RevisionRequired;
        let artifact_id = generated.record.id;
        {
            let store = EventStore::open(&db).unwrap();
            store.insert_artifact_record(&generated.record).unwrap();
            store
                .bind_artifact_storage_path(
                    &generated.record.storage_ref,
                    request_id,
                    &original_path.to_string_lossy(),
                    &generated.record.artifact_hash,
                    Utc::now(),
                )
                .unwrap();
            let state = AppState::new(store, temp.path().join("vault-1")).unwrap();
            run_artifact_recovery_sweep(&state);
            assert_eq!(
                state
                    .event_store()
                    .lock()
                    .unwrap()
                    .artifact_record(artifact_id)
                    .unwrap()
                    .0
                    .phase,
                ArtifactPhase::RevisionPrepared
            );
        }
        let store = EventStore::open(&db).unwrap();
        let state = AppState::new(store, temp.path().join("vault-2")).unwrap();
        run_artifact_recovery_sweep(&state);
        let record = state
            .event_store()
            .lock()
            .unwrap()
            .artifact_record(artifact_id)
            .unwrap()
            .0;
        assert_eq!(record.phase, ArtifactPhase::Generated);
        assert_eq!(record.artifact_revision, 1);
        assert_eq!(record.revision_attempts, 1);
        assert_eq!(fs::read(&original_path).unwrap(), damaged);
        assert!(temp.path().join("damaged.revision-1.pdf").is_file());
    }
}
