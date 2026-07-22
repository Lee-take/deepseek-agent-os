use chrono::{DateTime, Utc};
use tauri::State;
use uuid::Uuid;

use crate::commands::AppState;
use crate::kernel::automation::{
    next_scheduled_at, AutomationDefinition, AutomationDefinitionStatus, AutomationRun,
    AutomationSchedule, ReviewQueueItemView,
};

fn store_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[tauri::command]
pub fn list_automation_definitions(
    state: State<'_, AppState>,
) -> Result<Vec<AutomationDefinition>, String> {
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    store.list_automation_definitions().map_err(store_error)
}

#[tauri::command]
pub fn list_automation_runs(state: State<'_, AppState>) -> Result<Vec<AutomationRun>, String> {
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    store.list_automation_runs().map_err(store_error)
}

#[tauri::command]
pub fn list_automation_review_items(
    state: State<'_, AppState>,
) -> Result<Vec<ReviewQueueItemView>, String> {
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    store
        .list_review_queue_items()
        .map(|items| items.into_iter().map(|item| item.public_view()).collect())
        .map_err(store_error)
}

#[tauri::command]
pub fn create_once_automation(
    goal: String,
    timezone: String,
    run_at: String,
    state: State<'_, AppState>,
) -> Result<AutomationDefinition, String> {
    let run_at = DateTime::parse_from_rfc3339(run_at.trim())
        .map_err(|_| "run time must include a valid timezone offset".to_string())?
        .with_timezone(&Utc);
    let definition = AutomationDefinition::once(goal, timezone, run_at)?;
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    let definition = store
        .upsert_automation_definition(&definition)
        .map_err(store_error)?;
    Ok(definition)
}

#[expect(
    clippy::too_many_arguments,
    reason = "These argument names are the registered Tauri recurring-automation payload spanning schedule variants; remove only through a versioned frontend/command migration that preserves schedule validation, serde shape, and EventStore persistence."
)]
#[tauri::command]
pub fn create_recurring_automation(
    goal: String,
    timezone: String,
    frequency: String,
    hour: u32,
    minute: u32,
    weekday: Option<u32>,
    day: Option<u32>,
    weekdays: Option<Vec<u32>>,
    state: State<'_, AppState>,
) -> Result<AutomationDefinition, String> {
    let mut definition = AutomationDefinition::once(goal, timezone, Utc::now())?;
    definition.schedule = match frequency.trim() {
        "daily" => AutomationSchedule::Daily { hour, minute },
        "weekly" => AutomationSchedule::Weekly {
            weekday: weekday.ok_or_else(|| "weekly schedule requires a weekday".to_string())?,
            hour,
            minute,
        },
        "monthly" => AutomationSchedule::Monthly {
            day: day.ok_or_else(|| "monthly schedule requires a day".to_string())?,
            hour,
            minute,
        },
        "restricted" => AutomationSchedule::RestrictedCron {
            weekdays: weekdays.unwrap_or_default(),
            hour,
            minute,
        },
        _ => return Err("automation frequency is unsupported".to_string()),
    };
    next_scheduled_at(&definition, Utc::now())?
        .ok_or_else(|| "automation schedule has no future run".to_string())?;
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    let definition = store
        .upsert_automation_definition(&definition)
        .map_err(store_error)?;
    Ok(definition)
}

#[tauri::command]
pub fn set_automation_enabled(
    definition_id: Uuid,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<AutomationDefinition, String> {
    let status = if enabled {
        AutomationDefinitionStatus::Enabled
    } else {
        AutomationDefinitionStatus::Paused
    };
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    store
        .set_automation_definition_status(definition_id, status, Utc::now())
        .map_err(store_error)
}

#[tauri::command]
pub fn update_automation_goal(
    definition_id: Uuid,
    goal: String,
    state: State<'_, AppState>,
) -> Result<AutomationDefinition, String> {
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    store
        .update_automation_goal(definition_id, goal, Utc::now())
        .map_err(store_error)
}

#[tauri::command]
pub fn delete_automation(
    definition_id: Uuid,
    state: State<'_, AppState>,
) -> Result<AutomationDefinition, String> {
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    store
        .set_automation_definition_status(
            definition_id,
            AutomationDefinitionStatus::Deleted,
            Utc::now(),
        )
        .map_err(store_error)
}

#[tauri::command]
pub fn run_automation_now(
    definition_id: Uuid,
    manual_invocation_id: Uuid,
    state: State<'_, AppState>,
) -> Result<AutomationRun, String> {
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    store
        .enqueue_manual_automation_agent_run(
            definition_id,
            manual_invocation_id,
            Utc::now(),
            format!("automation:{definition_id}"),
        )
        .map(|(run, _)| run)
        .map_err(store_error)
}

#[tauri::command]
pub fn edit_automation_review_item(
    item_id: Uuid,
    action_revision: String,
    title: String,
    preview_fingerprint: Option<String>,
    state: State<'_, AppState>,
) -> Result<ReviewQueueItemView, String> {
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    store
        .edit_review_queue_item(
            item_id,
            &action_revision,
            title,
            preview_fingerprint,
            Utc::now(),
        )
        .map(|item| item.public_view())
        .map_err(store_error)
}

#[tauri::command]
pub fn resolve_automation_review_item(
    item_id: Uuid,
    action_revision: String,
    accepted: bool,
    state: State<'_, AppState>,
) -> Result<ReviewQueueItemView, String> {
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    store
        .resolve_review_queue_item(item_id, &action_revision, accepted, Utc::now())
        .map(|item| item.public_view())
        .map_err(store_error)
}

#[tauri::command]
pub fn enqueue_due_automation(
    definition_id: Uuid,
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Option<AutomationRun>, String> {
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    store
        .enqueue_due_automation_agent_run(
            definition_id,
            Utc::now(),
            "desktop-automation-scheduler".to_string(),
            conversation_id,
        )
        .map(|result| result.map(|(run, _)| run))
        .map_err(store_error)
}

#[tauri::command]
pub fn reconcile_automation_runs(state: State<'_, AppState>) -> Result<usize, String> {
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    store
        .reconcile_automation_agent_runs(Utc::now())
        .map_err(store_error)
}

#[tauri::command]
pub fn run_due_automation_sweep(state: State<'_, AppState>) -> Result<usize, String> {
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    let now = Utc::now();
    let mut queued = 0;
    for definition in store.list_automation_definitions().map_err(store_error)? {
        let conversation_id = format!("automation:{}", definition.id);
        if store
            .enqueue_due_automation_agent_run(
                definition.id,
                now,
                "desktop-automation-scheduler".to_string(),
                conversation_id,
            )
            .map_err(store_error)?
            .is_some()
        {
            queued += 1;
        }
    }
    Ok(queued)
}
