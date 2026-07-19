use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use uuid::Uuid;

use super::{
    EventStore, EventStoreError, EventStoreResult, CAPABILITY_ACCESS_REQUESTED_EVENT,
    PERMISSION_RESOLUTION_RECORDED_EVENT,
};
use crate::kernel::models::{AccessMode, KernelEvent};
use crate::kernel::policy::{
    exact_tool_preview_hash, request_capability_access, CapabilityAccessRequest,
    CapabilityAccessStatus, PermissionResolution, PolicyDecision,
};
use crate::kernel::task_capability_manifest::{
    compile_task_capability_manifest, task_authorization_preview, TaskAuthorizationPreview,
    TaskCapabilityManifest, TaskCapabilityManifestContext, TaskCapabilityProposal,
};
use crate::kernel::task_grouped_approval::{
    capability_request_event_id_for, event_id_for, item_event_id_for, legacy_consumption_id_for,
    permission_resolution_event_id_for, permission_resolution_id_for, TaskGroupedApproval,
    TaskGroupedApprovalActor, TaskGroupedApprovalError, TaskGroupedApprovalResolutionClaim,
    TaskGroupedApprovalStatus, TaskGroupedAuthorizationIntent, TaskGroupedAuthorizationView,
    TaskGroupedCapabilityAudit, TaskGroupedCapabilityClaim, TaskGroupedCapabilityGrant,
    TASK_GROUPED_APPROVAL_VERSION,
};

const TASK_GROUPED_APPROVAL_PREPARED_EVENT: &str = "task_grouped_approval.prepared";
const TASK_GROUPED_APPROVAL_RESOLVED_EVENT: &str = "task_grouped_approval.resolved";
const TASK_GROUPED_APPROVAL_REVOKED_EVENT: &str = "task_grouped_approval.revoked";
const TASK_GROUPED_APPROVAL_EXPIRED_EVENT: &str = "task_grouped_approval.expired";
const TASK_GROUPED_APPROVAL_SCOPE_CHANGED_EVENT: &str = "task_grouped_approval.scope_changed";

pub(super) fn is_grouped_request(store: &EventStore, request_id: Uuid) -> EventStoreResult<bool> {
    Ok(store
        .conn
        .query_row(
            r#"SELECT 1 FROM task_grouped_approval_item_audit
               WHERE approval_request_id = ?1
               UNION ALL
               SELECT 1 FROM task_grouped_approval_state AS state,
                    json_each(state.projection_json, '$.capability_audits') AS item
               WHERE json_extract(item.value, '$.approval_request_id') = ?1
               LIMIT 1"#,
            params![request_id.to_string()],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

pub(super) fn migrate(store: &EventStore) -> EventStoreResult<()> {
    store.conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS task_grouped_approval_state (
            group_id TEXT PRIMARY KEY NOT NULL,
            task_id TEXT NOT NULL,
            schema_version TEXT NOT NULL,
            manifest_revision TEXT NOT NULL,
            manifest_fingerprint TEXT NOT NULL,
            preview_schema_revision INTEGER NOT NULL,
            preview_renderer_revision INTEGER NOT NULL,
            preview_hash TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            status TEXT NOT NULL,
            row_revision INTEGER NOT NULL,
            projection_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_task_grouped_approval_task_status
            ON task_grouped_approval_state (task_id, status, updated_at);

        CREATE UNIQUE INDEX IF NOT EXISTS idx_task_grouped_approval_one_active
            ON task_grouped_approval_state (task_id)
            WHERE status IN ('pending', 'approved');

        CREATE TABLE IF NOT EXISTS task_grouped_approval_item_audit (
            audit_event_id TEXT PRIMARY KEY NOT NULL,
            group_id TEXT NOT NULL,
            item_id TEXT NOT NULL,
            capability TEXT NOT NULL,
            risk_level TEXT NOT NULL,
            tool_id TEXT NOT NULL,
            approval_request_id TEXT NOT NULL,
            transition TEXT NOT NULL,
            group_revision INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE (group_id, item_id, group_revision)
        );

        CREATE INDEX IF NOT EXISTS idx_task_grouped_approval_item_group
            ON task_grouped_approval_item_audit
               (group_id, group_revision, item_id);
        "#,
    )?;
    validate_all_rows(store)
}

impl EventStore {
    pub fn prepare_task_grouped_approval_from_proposal(
        &self,
        task_id: Uuid,
        proposal: &TaskCapabilityProposal,
        context: &TaskCapabilityManifestContext,
        now: DateTime<Utc>,
    ) -> EventStoreResult<TaskGroupedApproval> {
        let goal = self
            .goal_envelope_projection(task_id)?
            .ok_or_else(|| invalid("task capability proposal requires a frozen goal"))?;
        let bound_proposal = proposal
            .bind_to_frozen_goal(task_id, &goal)
            .map_err(|_| invalid("task capability proposal is stale or invalid"))?;
        let manifest = compile_task_capability_manifest(task_id, &goal, &bound_proposal, context)
            .map_err(|_| {
            invalid("task capability proposal cannot compile for the frozen goal")
        })?;
        let preview = task_authorization_preview(&manifest)
            .map_err(|_| invalid("task authorization preview cannot be derived"))?;
        self.prepare_task_grouped_approval(task_id, &manifest, &preview, now)
    }

    pub fn prepare_task_grouped_approval(
        &self,
        task_id: Uuid,
        manifest: &TaskCapabilityManifest,
        preview: &TaskAuthorizationPreview,
        now: DateTime<Utc>,
    ) -> EventStoreResult<TaskGroupedApproval> {
        let goal = self
            .goal_envelope_projection(task_id)?
            .ok_or_else(|| invalid("task grouped approval requires a frozen goal"))?;
        manifest
            .validate_for_goal(&goal)
            .map_err(|_| invalid("task grouped approval manifest is stale or invalid"))?;
        preview
            .validate_for_manifest(manifest)
            .map_err(|_| invalid("task grouped approval preview is stale or invalid"))?;
        let prepared = TaskGroupedApproval::new(manifest.clone(), preview.clone(), now)
            .map_err(group_error)?;
        if prepared.task_id != task_id {
            return Err(invalid("task grouped approval task binding changed"));
        }

        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if let Some(existing) = load_group(&transaction, prepared.id)? {
            transaction.commit()?;
            return Ok(existing);
        }

        let task_group_ids = {
            let mut statement = transaction.prepare(
                r#"SELECT group_id FROM task_grouped_approval_state
                   WHERE task_id = ?1
                   ORDER BY created_at ASC, group_id ASC"#,
            )?;
            let ids = statement
                .query_map(params![task_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        };
        let mut task_already_resolved = false;
        for existing_id in task_group_ids {
            let existing_id = Uuid::parse_str(&existing_id)?;
            let existing = load_group(&transaction, existing_id)?
                .ok_or_else(|| invalid("task grouped approval projection disappeared"))?;
            task_already_resolved |= existing.resolution.is_some();
            if !matches!(
                existing.status,
                TaskGroupedApprovalStatus::Pending | TaskGroupedApprovalStatus::Approved
            ) {
                continue;
            }
            let existing_expired = now >= existing.manifest.expires_at;
            if existing.status == TaskGroupedApprovalStatus::Pending {
                resolve_item_requests(
                    &transaction,
                    &existing,
                    false,
                    if existing_expired {
                        "Task grouped approval expired before replacement."
                    } else {
                        "Task grouped approval invalidated because the exact scope changed."
                    },
                    now,
                )?;
            }
            let changed = if existing_expired {
                existing.expire(now).map_err(group_error)?
            } else {
                existing.scope_changed(now).map_err(group_error)?
            };
            persist_transition(&transaction, &existing, &changed)?;
        }
        if task_already_resolved {
            transaction.commit()?;
            return Err(invalid(
                "task grouped approval already has a user resolution; a new exact task is required",
            ));
        }

        insert_item_requests(&transaction, &prepared)?;
        persist_new_group(&transaction, &prepared)?;
        transaction.commit()?;
        Ok(prepared)
    }

    pub fn task_grouped_approval(
        &self,
        group_id: Uuid,
    ) -> EventStoreResult<Option<TaskGroupedApproval>> {
        let group = load_group_connection(self, group_id)?;
        if let Some(group) = &group {
            validate_audit_history(self, group)?;
        }
        Ok(group)
    }

    pub fn list_task_grouped_authorizations(
        &self,
        now: DateTime<Utc>,
    ) -> EventStoreResult<Vec<TaskGroupedAuthorizationView>> {
        let group_ids = {
            let mut statement = self.conn.prepare(
                r#"SELECT group_id FROM task_grouped_approval_state
                   ORDER BY updated_at DESC, group_id ASC"#,
            )?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        };
        let mut views = Vec::with_capacity(group_ids.len());
        for group_id in group_ids {
            let group_id = Uuid::parse_str(&group_id)?;
            let group = self.refresh_task_grouped_approval_state(group_id, None, now)?;
            let goal = self
                .goal_envelope_projection(group.task_id)?
                .ok_or_else(|| invalid("task grouped authorization lost its frozen goal"))?;
            views.push(group.authorization_view(&goal).map_err(group_error)?);
        }
        Ok(views)
    }

    pub fn resolve_task_grouped_authorization(
        &self,
        intent: &TaskGroupedAuthorizationIntent,
        approved: bool,
        now: DateTime<Utc>,
    ) -> EventStoreResult<TaskGroupedAuthorizationView> {
        self.refresh_task_grouped_approval_state(intent.group_id, Some(intent.task_id), now)?;
        let resolved =
            self.resolve_task_grouped_approval(&intent.resolution_claim(), approved, now)?;
        let goal = self
            .goal_envelope_projection(resolved.task_id)?
            .ok_or_else(|| invalid("task grouped authorization lost its frozen goal"))?;
        resolved.authorization_view(&goal).map_err(group_error)
    }

    pub fn revoke_task_grouped_authorization(
        &self,
        intent: &TaskGroupedAuthorizationIntent,
        now: DateTime<Utc>,
    ) -> EventStoreResult<TaskGroupedAuthorizationView> {
        self.refresh_task_grouped_approval_state(intent.group_id, Some(intent.task_id), now)?;
        let revoked = self.revoke_task_grouped_approval(&intent.resolution_claim(), now)?;
        let goal = self
            .goal_envelope_projection(revoked.task_id)?
            .ok_or_else(|| invalid("task grouped authorization lost its frozen goal"))?;
        revoked.authorization_view(&goal).map_err(group_error)
    }

    fn refresh_task_grouped_approval_state(
        &self,
        group_id: Uuid,
        expected_task_id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> EventStoreResult<TaskGroupedApproval> {
        let current = self
            .task_grouped_approval(group_id)?
            .ok_or_else(|| EventStoreError::NotFound("task grouped approval".to_string()))?;
        if expected_task_id.is_some_and(|task_id| task_id != current.task_id) {
            return Err(invalid("task grouped authorization task binding changed"));
        }
        if !matches!(
            current.status,
            TaskGroupedApprovalStatus::Pending | TaskGroupedApprovalStatus::Approved
        ) {
            return Ok(current);
        }
        if now >= current.manifest.expires_at {
            return self.expire_task_grouped_approval(group_id, current.task_id, now);
        }
        let goal_is_current = self
            .goal_envelope_projection(current.task_id)?
            .as_ref()
            .is_some_and(|goal| current.manifest.validate_for_goal(goal).is_ok());
        if goal_is_current {
            return Ok(current);
        }

        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let current = load_group(&transaction, group_id)?
            .ok_or_else(|| EventStoreError::NotFound("task grouped approval".to_string()))?;
        if !matches!(
            current.status,
            TaskGroupedApprovalStatus::Pending | TaskGroupedApprovalStatus::Approved
        ) {
            transaction.commit()?;
            return Ok(current);
        }
        if current.status == TaskGroupedApprovalStatus::Pending {
            resolve_item_requests(
                &transaction,
                &current,
                false,
                "Task grouped approval invalidated because the frozen goal changed.",
                now,
            )?;
        }
        let changed = current.scope_changed(now).map_err(group_error)?;
        persist_transition(&transaction, &current, &changed)?;
        transaction.commit()?;
        Ok(changed)
    }

    pub fn resolve_task_grouped_approval(
        &self,
        claim: &TaskGroupedApprovalResolutionClaim,
        approved: bool,
        now: DateTime<Utc>,
    ) -> EventStoreResult<TaskGroupedApproval> {
        if claim.actor != TaskGroupedApprovalActor::User {
            return Err(invalid(
                "DeepSeek and frontend payloads cannot resolve task grouped approval",
            ));
        }
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let current = load_group(&transaction, claim.group_id)?
            .ok_or_else(|| EventStoreError::NotFound("task grouped approval".to_string()))?;
        if current.resolution_replay_matches(claim, approved)
            && matches!(
                current.status,
                TaskGroupedApprovalStatus::Approved | TaskGroupedApprovalStatus::Rejected
            )
        {
            transaction.commit()?;
            return Ok(current);
        }
        if current.status != TaskGroupedApprovalStatus::Pending
            || !current.binding_matches_resolution_claim(claim)
        {
            return Err(invalid(
                "task grouped approval resolution is stale or already terminal",
            ));
        }
        if now >= current.manifest.expires_at {
            resolve_item_requests(
                &transaction,
                &current,
                false,
                "Task grouped approval expired before user resolution.",
                now,
            )?;
            let expired = current.expire(now).map_err(group_error)?;
            persist_transition(&transaction, &current, &expired)?;
            transaction.commit()?;
            return Err(invalid("task grouped approval expired"));
        }

        resolve_item_requests(
            &transaction,
            &current,
            approved,
            if approved {
                "Task grouped approval approved by the user."
            } else {
                "Task grouped approval rejected by the user."
            },
            now,
        )?;
        let resolved = current.resolve(claim, approved, now).map_err(group_error)?;
        persist_transition(&transaction, &current, &resolved)?;
        transaction.commit()?;
        Ok(resolved)
    }

    pub fn revoke_task_grouped_approval(
        &self,
        claim: &TaskGroupedApprovalResolutionClaim,
        now: DateTime<Utc>,
    ) -> EventStoreResult<TaskGroupedApproval> {
        if !matches!(
            claim.actor,
            TaskGroupedApprovalActor::User | TaskGroupedApprovalActor::KernelLifecycle
        ) {
            return Err(invalid(
                "DeepSeek and frontend payloads cannot revoke task grouped approval",
            ));
        }
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let current = load_group(&transaction, claim.group_id)?
            .ok_or_else(|| EventStoreError::NotFound("task grouped approval".to_string()))?;
        if current.status == TaskGroupedApprovalStatus::Revoked
            && terminal_replay_matches(&current, claim)
        {
            transaction.commit()?;
            return Ok(current);
        }
        if current.status != TaskGroupedApprovalStatus::Approved
            || !current.binding_matches_resolution_claim(claim)
        {
            return Err(invalid(
                "task grouped approval revocation is stale or unavailable",
            ));
        }
        let revoked = current.revoke(claim.actor, now).map_err(group_error)?;
        persist_transition(&transaction, &current, &revoked)?;
        transaction.commit()?;
        Ok(revoked)
    }

    pub fn expire_task_grouped_approval(
        &self,
        group_id: Uuid,
        task_id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<TaskGroupedApproval> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let current = load_group(&transaction, group_id)?
            .ok_or_else(|| EventStoreError::NotFound("task grouped approval".to_string()))?;
        if current.task_id != task_id {
            return Err(invalid("task grouped approval task binding changed"));
        }
        if current.status == TaskGroupedApprovalStatus::Expired {
            transaction.commit()?;
            return Ok(current);
        }
        if now < current.manifest.expires_at
            || !matches!(
                current.status,
                TaskGroupedApprovalStatus::Pending | TaskGroupedApprovalStatus::Approved
            )
        {
            return Err(invalid(
                "task grouped approval cannot expire from its current state",
            ));
        }
        if current.status == TaskGroupedApprovalStatus::Pending {
            resolve_item_requests(
                &transaction,
                &current,
                false,
                "Task grouped approval expired before user resolution.",
                now,
            )?;
        }
        let expired = current.expire(now).map_err(group_error)?;
        persist_transition(&transaction, &current, &expired)?;
        transaction.commit()?;
        Ok(expired)
    }

    pub fn authorize_task_grouped_capability(
        &self,
        claim: &TaskGroupedCapabilityClaim,
        now: DateTime<Utc>,
    ) -> EventStoreResult<TaskGroupedCapabilityGrant> {
        let current_goal = self.goal_envelope_projection(claim.task_id)?;
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let current = load_group(&transaction, claim.group_id)?
            .ok_or_else(|| EventStoreError::NotFound("task grouped approval".to_string()))?;

        if now >= current.manifest.expires_at
            && matches!(
                current.status,
                TaskGroupedApprovalStatus::Pending | TaskGroupedApprovalStatus::Approved
            )
        {
            if current.status == TaskGroupedApprovalStatus::Pending {
                resolve_item_requests(
                    &transaction,
                    &current,
                    false,
                    "Task grouped approval expired before capability use.",
                    now,
                )?;
            }
            let expired = current.expire(now).map_err(group_error)?;
            persist_transition(&transaction, &current, &expired)?;
            transaction.commit()?;
            return Err(invalid("task grouped approval expired"));
        }

        let goal_is_current = current_goal
            .as_ref()
            .is_some_and(|goal| current.manifest.validate_for_goal(goal).is_ok());
        if !goal_is_current
            && matches!(
                current.status,
                TaskGroupedApprovalStatus::Pending | TaskGroupedApprovalStatus::Approved
            )
        {
            if current.status == TaskGroupedApprovalStatus::Pending {
                resolve_item_requests(
                    &transaction,
                    &current,
                    false,
                    "Task grouped approval invalidated because the frozen goal changed.",
                    now,
                )?;
            }
            let changed = current.scope_changed(now).map_err(group_error)?;
            persist_transition(&transaction, &current, &changed)?;
            transaction.commit()?;
            return Err(invalid("task grouped approval frozen scope changed"));
        }

        if current.status != TaskGroupedApprovalStatus::Approved {
            return Err(invalid("task grouped approval carries no authority"));
        }
        let item = current
            .capability_item(claim)
            .map_err(|_| invalid("task grouped capability claim is stale or mismatched"))?;
        validate_underlying_item_approved(&transaction, &current, item)?;
        let grant = TaskGroupedCapabilityGrant {
            group_id: current.id,
            task_id: current.task_id,
            projection_revision: current.projection_revision,
            manifest_revision: current.manifest.revision.clone(),
            manifest_fingerprint: current.manifest.fingerprint.clone(),
            preview_renderer_revision: current.preview.renderer_revision,
            preview_hash: current.preview.preview_hash.clone(),
            capability: item.capability,
            tool_id: item.tool_id.clone(),
            request_fingerprint: item.request_fingerprint.clone(),
            approval_request_id: item.approval_request_id,
            expires_at: current.manifest.expires_at,
        };
        transaction.commit()?;
        Ok(grant)
    }
}

fn insert_item_requests(
    transaction: &Transaction<'_>,
    group: &TaskGroupedApproval,
) -> EventStoreResult<()> {
    for item in &group.capability_audits {
        let mut request = request_capability_access(AccessMode::AskEveryStep, item.capability)
            .map_err(invalid)?;
        request.id = item.approval_request_id;
        request.created_at = group.created_at;
        request
            .bind_exact_tool(
                item.tool_id.clone(),
                item.request_fingerprint.clone(),
                item.exact_preview.clone(),
            )
            .map_err(invalid)?;
        if request.status != CapabilityAccessStatus::PendingApproval
            || request.decision != PolicyDecision::Ask
            || request.risk_level != item.risk_level
        {
            return Err(invalid(
                "task grouped approval did not derive a pending exact policy request",
            ));
        }
        let event = KernelEvent {
            id: capability_request_event_id_for(group.id, &item.item_id),
            event_type: CAPABILITY_ACCESS_REQUESTED_EVENT.to_string(),
            payload_json: serde_json::to_string(&request)?,
            created_at: group.created_at,
        };
        EventStore::insert_kernel_event(transaction, &event)?;
    }
    Ok(())
}

fn resolve_item_requests(
    transaction: &Transaction<'_>,
    group: &TaskGroupedApproval,
    approved: bool,
    note: &str,
    now: DateTime<Utc>,
) -> EventStoreResult<()> {
    for item in &group.capability_audits {
        let (request_json, resolution_json, effective_status_json, row_revision) = transaction
            .query_row(
                r#"SELECT request_json, resolution_json, effective_status, row_revision
                   FROM capability_access_state WHERE request_id = ?1"#,
                params![item.approval_request_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, u64>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| invalid("task grouped approval exact item request is missing"))?;
        let request: CapabilityAccessRequest = serde_json::from_str(&request_json)?;
        let effective_status: CapabilityAccessStatus =
            serde_json::from_str(&effective_status_json)?;
        validate_underlying_request(group, item, &request)?;
        if resolution_json.is_some()
            || effective_status != CapabilityAccessStatus::PendingApproval
            || row_revision != 0
        {
            return Err(invalid(
                "task grouped approval exact item request is stale or resolved",
            ));
        }
        let scope = request
            .exact_tool
            .as_ref()
            .ok_or_else(|| invalid("task grouped approval exact preview is missing"))?;
        let mut resolution = PermissionResolution::new_exact(
            request.id,
            approved,
            note.to_string(),
            row_revision,
            scope,
        )
        .map_err(invalid)?;
        resolution.id = permission_resolution_id_for(group.id, &item.item_id, approved);
        resolution.created_at = now;
        let event = KernelEvent {
            id: permission_resolution_event_id_for(resolution.id),
            event_type: PERMISSION_RESOLUTION_RECORDED_EVENT.to_string(),
            payload_json: serde_json::to_string(&resolution)?,
            created_at: now,
        };
        EventStore::insert_kernel_event(transaction, &event)?;
        if approved {
            transaction.execute(
                r#"INSERT INTO capability_approval_consumptions
                   (request_id, capability_invocation_id, consumed_at)
                   VALUES (?1, ?2, ?3)"#,
                params![
                    item.approval_request_id.to_string(),
                    legacy_consumption_id_for(group.id, &item.item_id).to_string(),
                    timestamp(now),
                ],
            )?;
        }
    }
    Ok(())
}

fn validate_underlying_request(
    group: &TaskGroupedApproval,
    item: &TaskGroupedCapabilityAudit,
    request: &CapabilityAccessRequest,
) -> EventStoreResult<()> {
    let scope = request
        .exact_tool
        .as_ref()
        .ok_or_else(|| invalid("task grouped approval exact preview is missing"))?;
    if request.id != item.approval_request_id
        || request.access_mode != AccessMode::AskEveryStep
        || request.capability != item.capability
        || request.risk_level != item.risk_level
        || request.decision != PolicyDecision::Ask
        || request.status != CapabilityAccessStatus::PendingApproval
        || request.created_at != group.created_at
        || scope.tool_id != item.tool_id
        || scope.request_fingerprint != item.request_fingerprint
        || scope.preview != item.exact_preview
        || scope.preview_revision != item.exact_preview_revision
        || scope.preview_hash != item.exact_preview_hash
        || scope.preview_hash != exact_tool_preview_hash(scope.preview_revision, &scope.preview)
    {
        return Err(invalid(
            "task grouped approval exact item binding is invalid",
        ));
    }
    Ok(())
}

fn validate_underlying_item_approved(
    transaction: &Transaction<'_>,
    group: &TaskGroupedApproval,
    item: &TaskGroupedCapabilityAudit,
) -> EventStoreResult<()> {
    let (request_json, resolution_json, effective_status_json, row_revision) = transaction
        .query_row(
            r#"SELECT request_json, resolution_json, effective_status, row_revision
               FROM capability_access_state WHERE request_id = ?1"#,
            params![item.approval_request_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u64>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| invalid("task grouped approval exact item request is missing"))?;
    let request: CapabilityAccessRequest = serde_json::from_str(&request_json)?;
    validate_underlying_request(group, item, &request)?;
    let resolution: PermissionResolution = serde_json::from_str(
        resolution_json
            .as_deref()
            .ok_or_else(|| invalid("task grouped approval exact item resolution is missing"))?,
    )?;
    let effective_status: CapabilityAccessStatus = serde_json::from_str(&effective_status_json)?;
    let scope = request
        .exact_tool
        .as_ref()
        .ok_or_else(|| invalid("task grouped approval exact preview is missing"))?;
    if !resolution.approved
        || effective_status != CapabilityAccessStatus::Approved
        || row_revision != 1
        || resolution.request_id != request.id
        || resolution.expected_request_revision != Some(0)
        || resolution.exact_preview_revision != Some(scope.preview_revision)
        || resolution.exact_preview_hash.as_deref() != Some(scope.preview_hash.as_str())
        || resolution.id != permission_resolution_id_for(group.id, &item.item_id, true)
    {
        return Err(invalid(
            "task grouped approval exact item resolution is invalid",
        ));
    }
    let consumption: Option<String> = transaction
        .query_row(
            r#"SELECT capability_invocation_id FROM capability_approval_consumptions
               WHERE request_id = ?1"#,
            params![item.approval_request_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    if consumption.as_deref()
        != Some(
            legacy_consumption_id_for(group.id, &item.item_id)
                .to_string()
                .as_str(),
        )
    {
        return Err(invalid(
            "task grouped approval item escaped the task-only authority boundary",
        ));
    }
    Ok(())
}

fn persist_new_group(
    transaction: &Transaction<'_>,
    group: &TaskGroupedApproval,
) -> EventStoreResult<()> {
    group.validate_integrity().map_err(group_error)?;
    transaction.execute(
        r#"INSERT INTO task_grouped_approval_state
           (group_id, task_id, schema_version, manifest_revision,
            manifest_fingerprint, preview_schema_revision,
            preview_renderer_revision, preview_hash, expires_at, status,
            row_revision, projection_json, created_at, updated_at)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"#,
        state_params(group)?,
    )?;
    insert_group_event(transaction, group)?;
    insert_item_audit_events(transaction, group)
}

fn persist_transition(
    transaction: &Transaction<'_>,
    previous: &TaskGroupedApproval,
    next: &TaskGroupedApproval,
) -> EventStoreResult<()> {
    previous.validate_integrity().map_err(group_error)?;
    next.validate_integrity().map_err(group_error)?;
    if next.id != previous.id
        || next.task_id != previous.task_id
        || next.projection_revision != previous.projection_revision + 1
        || next.manifest != previous.manifest
        || next.preview != previous.preview
    {
        return Err(invalid("task grouped approval transition binding changed"));
    }
    let changed = transaction.execute(
        r#"UPDATE task_grouped_approval_state SET
             status = ?2, row_revision = ?3, projection_json = ?4, updated_at = ?5
           WHERE group_id = ?1 AND row_revision = ?6
             AND manifest_revision = ?7 AND manifest_fingerprint = ?8
             AND preview_schema_revision = ?9
             AND preview_renderer_revision = ?10 AND preview_hash = ?11"#,
        params![
            next.id.to_string(),
            next.status.as_str(),
            next.projection_revision,
            next.canonical_json().map_err(group_error)?,
            timestamp(next.updated_at),
            previous.projection_revision,
            next.manifest.revision,
            next.manifest.fingerprint,
            next.preview.schema_revision,
            next.preview.renderer_revision,
            next.preview.preview_hash,
        ],
    )?;
    if changed != 1 {
        return Err(invalid("task grouped approval projection revision changed"));
    }
    insert_group_event(transaction, next)?;
    insert_item_audit_events(transaction, next)
}

fn insert_group_event(
    transaction: &Transaction<'_>,
    group: &TaskGroupedApproval,
) -> EventStoreResult<()> {
    let event = KernelEvent {
        id: event_id_for(group),
        event_type: event_type_for(group.status).to_string(),
        payload_json: serde_json::to_string(&group.event_receipt())?,
        created_at: group.updated_at,
    };
    EventStore::insert_kernel_event(transaction, &event)
}

fn insert_item_audit_events(
    transaction: &Transaction<'_>,
    group: &TaskGroupedApproval,
) -> EventStoreResult<()> {
    for item in &group.capability_audits {
        let changed = transaction.execute(
            r#"INSERT OR IGNORE INTO task_grouped_approval_item_audit
               (audit_event_id, group_id, item_id, capability, risk_level,
                tool_id, approval_request_id, transition, group_revision, created_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#,
            params![
                item_event_id_for(group, item).to_string(),
                group.id.to_string(),
                item.item_id,
                item.capability.as_str(),
                risk_name(item.risk_level),
                item.tool_id,
                item.approval_request_id.to_string(),
                group.status.as_str(),
                group.projection_revision,
                timestamp(group.updated_at),
            ],
        )?;
        if changed != 1 {
            return Err(invalid(
                "task grouped approval duplicate item audit transition",
            ));
        }
    }
    Ok(())
}

fn state_params(group: &TaskGroupedApproval) -> EventStoreResult<[rusqlite::types::Value; 14]> {
    Ok([
        group.id.to_string().into(),
        group.task_id.to_string().into(),
        TASK_GROUPED_APPROVAL_VERSION.to_string().into(),
        group.manifest.revision.clone().into(),
        group.manifest.fingerprint.clone().into(),
        i64::from(group.preview.schema_revision).into(),
        i64::from(group.preview.renderer_revision).into(),
        group.preview.preview_hash.clone().into(),
        timestamp(group.manifest.expires_at).into(),
        group.status.as_str().to_string().into(),
        i64::try_from(group.projection_revision)
            .map_err(|_| invalid("task grouped approval revision is too large"))?
            .into(),
        group.canonical_json().map_err(group_error)?.into(),
        timestamp(group.created_at).into(),
        timestamp(group.updated_at).into(),
    ])
}

fn load_group(
    transaction: &Transaction<'_>,
    group_id: Uuid,
) -> EventStoreResult<Option<TaskGroupedApproval>> {
    let row = transaction
        .query_row(
            r#"SELECT task_id, schema_version, manifest_revision,
                      manifest_fingerprint, preview_schema_revision,
                      preview_renderer_revision, preview_hash, expires_at,
                      status, row_revision, projection_json, created_at, updated_at
               FROM task_grouped_approval_state WHERE group_id = ?1"#,
            params![group_id.to_string()],
            read_group_row,
        )
        .optional()?;
    row.map(|row| validate_row(group_id, row)).transpose()
}

fn load_group_connection(
    store: &EventStore,
    group_id: Uuid,
) -> EventStoreResult<Option<TaskGroupedApproval>> {
    let row = store
        .conn
        .query_row(
            r#"SELECT task_id, schema_version, manifest_revision,
                      manifest_fingerprint, preview_schema_revision,
                      preview_renderer_revision, preview_hash, expires_at,
                      status, row_revision, projection_json, created_at, updated_at
               FROM task_grouped_approval_state WHERE group_id = ?1"#,
            params![group_id.to_string()],
            read_group_row,
        )
        .optional()?;
    row.map(|row| validate_row(group_id, row)).transpose()
}

type GroupRow = (
    String,
    String,
    String,
    String,
    u32,
    u32,
    String,
    String,
    String,
    u64,
    String,
    String,
    String,
);

fn read_group_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GroupRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
    ))
}

fn validate_row(group_id: Uuid, row: GroupRow) -> EventStoreResult<TaskGroupedApproval> {
    let (
        task_id,
        schema_version,
        manifest_revision,
        manifest_fingerprint,
        preview_schema_revision,
        preview_renderer_revision,
        preview_hash,
        expires_at,
        status,
        row_revision,
        projection_json,
        created_at,
        updated_at,
    ) = row;
    let group = TaskGroupedApproval::parse_json(&projection_json).map_err(group_error)?;
    if group.id != group_id
        || group.task_id != Uuid::parse_str(&task_id)?
        || schema_version != TASK_GROUPED_APPROVAL_VERSION
        || group.manifest.revision != manifest_revision
        || group.manifest.fingerprint != manifest_fingerprint
        || group.preview.schema_revision != preview_schema_revision
        || group.preview.renderer_revision != preview_renderer_revision
        || group.preview.preview_hash != preview_hash
        || timestamp(group.manifest.expires_at) != expires_at
        || group.status.as_str() != status
        || group.projection_revision != row_revision
        || timestamp(group.created_at) != created_at
        || timestamp(group.updated_at) != updated_at
    {
        return Err(invalid("task grouped approval projection columns drifted"));
    }
    Ok(group)
}

fn validate_all_rows(store: &EventStore) -> EventStoreResult<()> {
    let ids = {
        let mut statement = store
            .conn
            .prepare("SELECT group_id FROM task_grouped_approval_state ORDER BY group_id")?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids
    };
    for id in ids {
        let id = Uuid::parse_str(&id)?;
        let group = load_group_connection(store, id)?
            .ok_or_else(|| invalid("task grouped approval migration lost a projection"))?;
        validate_audit_history(store, &group)?;
    }
    Ok(())
}

fn validate_audit_history(store: &EventStore, group: &TaskGroupedApproval) -> EventStoreResult<()> {
    let mut statement = store.conn.prepare(
        r#"SELECT audit_event_id, item_id, capability, risk_level, tool_id,
                  approval_request_id, transition, group_revision
           FROM task_grouped_approval_item_audit
           WHERE group_id = ?1 ORDER BY group_revision ASC, item_id ASC"#,
    )?;
    let rows = statement
        .query_map(params![group.id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, u64>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let expected_count = group.capability_audits.len().checked_mul(
        usize::try_from(group.projection_revision)
            .map_err(|_| invalid("task grouped approval revision is too large"))?
            + 1,
    );
    if expected_count != Some(rows.len()) {
        return Err(invalid(
            "task grouped approval per-capability audit history is incomplete",
        ));
    }
    for revision in 0..=group.projection_revision {
        let transition = if revision == 0 {
            TaskGroupedApprovalStatus::Pending
        } else if revision == group.projection_revision {
            group.status
        } else {
            TaskGroupedApprovalStatus::Approved
        };
        let mut revision_group = group.clone();
        revision_group.status = transition;
        revision_group.projection_revision = revision;
        for item in &group.capability_audits {
            let expected_event = item_event_id_for(&revision_group, item).to_string();
            let row = rows
                .iter()
                .find(|row| row.1 == item.item_id && row.7 == revision)
                .ok_or_else(|| invalid("task grouped approval item audit transition is missing"))?;
            if row.0 != expected_event
                || row.2 != item.capability.as_str()
                || row.3 != risk_name(item.risk_level)
                || row.4 != item.tool_id
                || row.5 != item.approval_request_id.to_string()
                || row.6 != transition.as_str()
            {
                return Err(invalid(
                    "task grouped approval item audit transition was tampered",
                ));
            }
        }
    }
    Ok(())
}

fn terminal_replay_matches(
    group: &TaskGroupedApproval,
    claim: &TaskGroupedApprovalResolutionClaim,
) -> bool {
    group.id == claim.group_id
        && group.task_id == claim.task_id
        && group.projection_revision == claim.expected_projection_revision.saturating_add(1)
        && group.manifest.revision == claim.manifest_revision
        && group.manifest.fingerprint == claim.manifest_fingerprint
        && group.preview.schema_revision == claim.preview_schema_revision
        && group.preview.renderer_revision == claim.preview_renderer_revision
        && group.preview.preview_hash == claim.preview_hash
}

fn event_type_for(status: TaskGroupedApprovalStatus) -> &'static str {
    match status {
        TaskGroupedApprovalStatus::Pending => TASK_GROUPED_APPROVAL_PREPARED_EVENT,
        TaskGroupedApprovalStatus::Approved | TaskGroupedApprovalStatus::Rejected => {
            TASK_GROUPED_APPROVAL_RESOLVED_EVENT
        }
        TaskGroupedApprovalStatus::Revoked => TASK_GROUPED_APPROVAL_REVOKED_EVENT,
        TaskGroupedApprovalStatus::Expired => TASK_GROUPED_APPROVAL_EXPIRED_EVENT,
        TaskGroupedApprovalStatus::ScopeChanged => TASK_GROUPED_APPROVAL_SCOPE_CHANGED_EVENT,
    }
}

fn risk_name(risk: crate::kernel::policy::RiskLevel) -> &'static str {
    match risk {
        crate::kernel::policy::RiskLevel::Low => "low",
        crate::kernel::policy::RiskLevel::Medium => "medium",
        crate::kernel::policy::RiskLevel::High => "high",
        crate::kernel::policy::RiskLevel::Critical => "critical",
    }
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn group_error(error: TaskGroupedApprovalError) -> EventStoreError {
    invalid(error.to_string())
}

fn invalid(message: impl Into<String>) -> EventStoreError {
    EventStoreError::InvalidState(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::goal_envelope::{
        GoalDoneWhenProposal, GoalEnvelopeProposal, GoalExternalTargetProposal,
        GoalVerifierProposal, GOAL_ENVELOPE_PROPOSAL_VERSION,
    };
    use crate::kernel::goal_lifecycle::{GoalTargetBindingKind, GoalValidationContext};
    use crate::kernel::local_directory::WorkspaceReadinessCode;
    use crate::kernel::policy::{request_capability_access, CapabilityGrantState, CapabilityKind};
    use crate::kernel::task_capability_manifest::{
        compile_task_capability_manifest, task_authorization_preview,
        TaskCapabilityDescriptionProposal, TaskCapabilityManifestContext,
        TaskCapabilityManifestProposal, TaskCapabilityNeedProposal, TaskCapabilityProposal,
        TASK_CAPABILITY_MANIFEST_VERSION, TASK_CAPABILITY_PROPOSAL_VERSION,
    };
    use crate::kernel::task_grouped_approval::TaskGroupedCapabilityAuditStatus;
    use crate::kernel::tool_runtime::{CONNECTOR_MUTATE_TOOL_ID, FILE_WRITE_TOOL_ID};
    use rusqlite::Connection;
    use tempfile::tempdir;

    const TASK_ID: Uuid = Uuid::from_u128(0x33b);
    const OTHER_TASK_ID: Uuid = Uuid::from_u128(0x33c);

    fn fixed_now() -> DateTime<Utc> {
        "2029-01-02T03:04:05Z".parse().unwrap()
    }

    fn fixed_expiry() -> DateTime<Utc> {
        "2030-01-02T03:04:05Z".parse().unwrap()
    }

    fn frozen_goal_for(
        store: &EventStore,
        task_id: Uuid,
        path_authority: &[u8],
    ) -> crate::kernel::goal_lifecycle::GoalLifecycleProjection {
        let proposal = GoalEnvelopeProposal {
            version: GOAL_ENVELOPE_PROPOSAL_VERSION.to_string(),
            user_goal: "Create a verified report and send one approved external update."
                .to_string(),
            assumptions: Vec::new(),
            constraints: vec!["Use only locally bound task scopes.".to_string()],
            done_when: vec![
                GoalDoneWhenProposal {
                    done_when_id: "external-state-verified".to_string(),
                    description: "The external state is independently verified.".to_string(),
                },
                GoalDoneWhenProposal {
                    done_when_id: "report-verified".to_string(),
                    description: "The local report is independently verified.".to_string(),
                },
            ],
            required_artifacts: Vec::new(),
            verifiers: vec![
                GoalVerifierProposal {
                    verifier_id: "external-verifier-v1".to_string(),
                    done_when_id: "external-state-verified".to_string(),
                    description: "Verify the remote object state after the operation.".to_string(),
                    evidence_kind: "connector_remote_state".to_string(),
                },
                GoalVerifierProposal {
                    verifier_id: "report-verifier-v1".to_string(),
                    done_when_id: "report-verified".to_string(),
                    description: "Verify the report hash and rendered output.".to_string(),
                    evidence_kind: "artifact_hash".to_string(),
                },
            ],
            proposed_capabilities: vec![
                CONNECTOR_MUTATE_TOOL_ID.to_string(),
                FILE_WRITE_TOOL_ID.to_string(),
            ],
            external_targets: vec![
                GoalExternalTargetProposal {
                    target_id: "finance-account".to_string(),
                    description: "A locally bound external account.".to_string(),
                },
                GoalExternalTargetProposal {
                    target_id: "finance-recipient".to_string(),
                    description: "A locally bound recipient.".to_string(),
                },
                GoalExternalTargetProposal {
                    target_id: "report-folder".to_string(),
                    description: "A locally bound workspace path scope.".to_string(),
                },
                GoalExternalTargetProposal {
                    target_id: "weekday-window".to_string(),
                    description: "A locally bound schedule window.".to_string(),
                },
            ],
            stop_conditions: vec!["Stop when any exact scope changes.".to_string()],
        };
        let context =
            GoalValidationContext::new(AccessMode::FullAccess, WorkspaceReadinessCode::Ready)
                .with_max_risk(crate::kernel::policy::RiskLevel::Critical)
                .with_enabled_tool(CONNECTOR_MUTATE_TOOL_ID, true)
                .with_enabled_tool(FILE_WRITE_TOOL_ID, true)
                .with_approval_route(CONNECTOR_MUTATE_TOOL_ID)
                .with_approval_route(FILE_WRITE_TOOL_ID)
                .with_verifier_kind("artifact_hash")
                .with_verifier_kind("connector_remote_state")
                .with_target_binding(
                    "finance-account",
                    GoalTargetBindingKind::Account,
                    b"account-authority-v1",
                )
                .with_target_binding(
                    "finance-recipient",
                    GoalTargetBindingKind::Recipient,
                    b"recipient-authority-v1",
                )
                .with_target_binding("report-folder", GoalTargetBindingKind::Path, path_authority)
                .with_target_binding(
                    "weekday-window",
                    GoalTargetBindingKind::TimeWindow,
                    b"time-authority-v1",
                )
                .allowing_local_effects()
                .allowing_external_effects();
        let validated = store
            .submit_goal_proposal(task_id, &proposal, &context)
            .expect("goal validates");
        store
            .freeze_goal_envelope(task_id, validated.revision().expect("goal revision"))
            .expect("goal freezes")
    }

    fn compiled_fixture_for(
        store: &EventStore,
        task_id: Uuid,
        path_authority: &[u8],
        path_label: &str,
        expires_at: DateTime<Utc>,
    ) -> (TaskCapabilityManifest, TaskAuthorizationPreview) {
        let goal = frozen_goal_for(store, task_id, path_authority);
        let frozen = goal.frozen().expect("frozen goal");
        let proposal = TaskCapabilityManifestProposal {
            version: TASK_CAPABILITY_MANIFEST_VERSION.to_string(),
            task_id,
            goal_id: task_id,
            goal_revision: frozen.revision.clone(),
            goal_fingerprint: frozen.fingerprint.clone(),
            expires_at,
            capabilities: vec![
                TaskCapabilityNeedProposal {
                    capability: "connector_write".to_string(),
                    tool_ids: vec![CONNECTOR_MUTATE_TOOL_ID.to_string()],
                    application_ids: vec!["outlook".to_string()],
                    path_target_ids: Vec::new(),
                    account_target_ids: vec!["finance-account".to_string()],
                    recipient_target_ids: vec!["finance-recipient".to_string()],
                    time_window_target_ids: vec!["weekday-window".to_string()],
                    external_target_ids: vec![
                        "finance-account".to_string(),
                        "finance-recipient".to_string(),
                        "weekday-window".to_string(),
                    ],
                    verifier_ids: vec!["external-verifier-v1".to_string()],
                },
                TaskCapabilityNeedProposal {
                    capability: "file_write".to_string(),
                    tool_ids: vec![FILE_WRITE_TOOL_ID.to_string()],
                    application_ids: vec!["excel".to_string()],
                    path_target_ids: vec!["report-folder".to_string()],
                    account_target_ids: Vec::new(),
                    recipient_target_ids: Vec::new(),
                    time_window_target_ids: Vec::new(),
                    external_target_ids: vec!["report-folder".to_string()],
                    verifier_ids: vec!["report-verifier-v1".to_string()],
                },
            ],
        };
        let context = manifest_context(path_label);
        let manifest = compile_task_capability_manifest(task_id, &goal, &proposal, &context)
            .expect("manifest compiles");
        let preview = task_authorization_preview(&manifest).expect("preview renders");
        (manifest, preview)
    }

    fn manifest_context(path_label: &str) -> TaskCapabilityManifestContext {
        TaskCapabilityManifestContext::default()
            .with_application("excel", "Microsoft Excel")
            .with_application("outlook", "Microsoft Outlook")
            .with_target_display("finance-account", "Work mailbox account")
            .with_target_display("finance-recipient", "finance-test@example.com")
            .with_target_display("report-folder", path_label)
            .with_target_display("weekday-window", "Weekdays 09:00-17:00 Asia/Shanghai")
    }

    fn descriptive_proposal(expires_at: DateTime<Utc>) -> TaskCapabilityProposal {
        TaskCapabilityProposal {
            version: TASK_CAPABILITY_PROPOSAL_VERSION.to_string(),
            expires_at,
            capabilities: vec![
                TaskCapabilityDescriptionProposal {
                    capability: "connector_write".to_string(),
                    application_ids: vec!["outlook".to_string()],
                    path_target_ids: Vec::new(),
                    account_target_ids: vec!["finance-account".to_string()],
                    recipient_target_ids: vec!["finance-recipient".to_string()],
                    time_window_target_ids: vec!["weekday-window".to_string()],
                    external_target_ids: vec![
                        "finance-account".to_string(),
                        "finance-recipient".to_string(),
                        "weekday-window".to_string(),
                    ],
                    verifier_ids: vec!["external-verifier-v1".to_string()],
                },
                TaskCapabilityDescriptionProposal {
                    capability: "file_write".to_string(),
                    application_ids: vec!["excel".to_string()],
                    path_target_ids: vec!["report-folder".to_string()],
                    account_target_ids: Vec::new(),
                    recipient_target_ids: Vec::new(),
                    time_window_target_ids: Vec::new(),
                    external_target_ids: vec!["report-folder".to_string()],
                    verifier_ids: vec!["report-verifier-v1".to_string()],
                },
            ],
        }
    }

    fn compiled_fixture(store: &EventStore) -> (TaskCapabilityManifest, TaskAuthorizationPreview) {
        compiled_fixture_for(
            store,
            TASK_ID,
            b"path-authority-v1",
            "Workspace / reports",
            fixed_expiry(),
        )
    }

    fn prepare(store: &EventStore) -> TaskGroupedApproval {
        let (manifest, preview) = compiled_fixture(store);
        store
            .prepare_task_grouped_approval(TASK_ID, &manifest, &preview, fixed_now())
            .expect("group prepares")
    }

    fn approve(store: &EventStore, group: &TaskGroupedApproval) -> TaskGroupedApproval {
        store
            .resolve_task_grouped_approval(
                &TaskGroupedApprovalResolutionClaim::from_group(
                    group,
                    TaskGroupedApprovalActor::User,
                ),
                true,
                fixed_now() + chrono::Duration::minutes(1),
            )
            .expect("group approves")
    }

    fn event_count(store: &EventStore, event_type: &str) -> i64 {
        store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM kernel_events WHERE event_type = ?1",
                params![event_type],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn exact_group_approval_uses_one_user_resolution_and_per_capability_audit() {
        let store = EventStore::open_memory().unwrap();
        let pending = prepare(&store);
        assert_eq!(pending.capability_audits.len(), 2);
        assert_eq!(event_count(&store, TASK_GROUPED_APPROVAL_PREPARED_EVENT), 1);
        assert!(store
            .list_pending_capability_access_records()
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .list_capability_access_records()
                .unwrap()
                .into_iter()
                .filter(|record| {
                    pending
                        .capability_audits
                        .iter()
                        .any(|item| item.approval_request_id == record.request.id)
                })
                .count(),
            2
        );
        for item in &pending.capability_audits {
            assert!(store
                .resolve_capability_access_request(
                    item.approval_request_id,
                    true,
                    "attempted per-item approval".to_string(),
                )
                .is_err());
            assert!(store
                .append_permission_resolution(&PermissionResolution::new(
                    item.approval_request_id,
                    true,
                    "forged model resolution".to_string(),
                ))
                .is_err());
        }
        let connector_item = pending
            .capability_audits
            .iter()
            .find(|item| item.capability == CapabilityKind::ConnectorWrite)
            .unwrap();
        assert!(store
            .resolve_connector_mutation_access_request(
                connector_item.approval_request_id,
                true,
                "attempted connector bypass".to_string(),
                0,
                connector_item.exact_preview_revision,
                &connector_item.exact_preview_hash,
            )
            .is_err());

        let approved = approve(&store, &pending);
        assert_eq!(approved.status, TaskGroupedApprovalStatus::Approved);
        assert!(approved.status.carries_authority());
        assert_eq!(
            store
                .prepare_task_grouped_approval(
                    TASK_ID,
                    &approved.manifest,
                    &approved.preview,
                    fixed_now() + chrono::Duration::minutes(2),
                )
                .unwrap(),
            approved
        );
        assert_eq!(event_count(&store, TASK_GROUPED_APPROVAL_RESOLVED_EVENT), 1);
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM task_grouped_approval_item_audit WHERE group_id = ?1",
                    params![approved.id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            4
        );
        assert_eq!(
            store
                .list_permission_resolutions()
                .unwrap()
                .into_iter()
                .filter(|resolution| {
                    approved
                        .capability_audits
                        .iter()
                        .any(|item| item.approval_request_id == resolution.request_id)
                })
                .count(),
            2
        );
        assert_eq!(
            store
                .available_capability_grant_request_id(CapabilityKind::FileWrite)
                .unwrap(),
            None,
            "group item approvals must never escape through the legacy reusable grant lookup"
        );
        for item in &approved.capability_audits {
            let claim = TaskGroupedCapabilityClaim::from_group_item(&approved, item);
            let grant = store
                .authorize_task_grouped_capability(
                    &claim,
                    fixed_now() + chrono::Duration::minutes(2),
                )
                .expect("exact item authorizes");
            assert_eq!(grant.group_id, approved.id);
            assert_eq!(grant.task_id, TASK_ID);
            assert_eq!(grant.capability, item.capability);
            assert_eq!(grant.tool_id, item.tool_id);
            assert_eq!(grant.request_fingerprint, item.request_fingerprint);
            assert_eq!(grant.approval_request_id, item.approval_request_id);
            assert_eq!(grant.manifest_revision, approved.manifest.revision);
            assert_eq!(grant.manifest_fingerprint, approved.manifest.fingerprint);
            assert_eq!(grant.preview_hash, approved.preview.preview_hash);
            assert_eq!(
                grant.preview_renderer_revision,
                approved.preview.renderer_revision
            );
            assert_eq!(grant.projection_revision, 1);
            assert_eq!(grant.expires_at, fixed_expiry());
        }
    }

    #[test]
    fn exact_group_rejection_and_resolution_replay_are_safe_and_idempotent() {
        let store = EventStore::open_memory().unwrap();
        let pending = prepare(&store);
        let claim = TaskGroupedApprovalResolutionClaim::from_group(
            &pending,
            TaskGroupedApprovalActor::User,
        );
        let rejected = store
            .resolve_task_grouped_approval(
                &claim,
                false,
                fixed_now() + chrono::Duration::minutes(1),
            )
            .unwrap();
        assert_eq!(rejected.status, TaskGroupedApprovalStatus::Rejected);
        assert!(!rejected.status.carries_authority());
        assert_eq!(
            store
                .resolve_task_grouped_approval(
                    &claim,
                    false,
                    fixed_now() + chrono::Duration::minutes(2),
                )
                .unwrap(),
            rejected
        );
        assert!(
            store
                .resolve_task_grouped_approval(
                    &claim,
                    true,
                    fixed_now() + chrono::Duration::minutes(2),
                )
                .is_err()
        );
        let capability_claim =
            TaskGroupedCapabilityClaim::from_group_item(&rejected, &rejected.capability_audits[0]);
        assert!(store
            .authorize_task_grouped_capability(
                &capability_claim,
                fixed_now() + chrono::Duration::minutes(2),
            )
            .is_err());
        assert_eq!(event_count(&store, TASK_GROUPED_APPROVAL_RESOLVED_EVENT), 1);
    }

    #[test]
    fn revoke_is_exact_idempotent_and_model_or_frontend_self_authority_is_rejected() {
        let store = EventStore::open_memory().unwrap();
        let pending = prepare(&store);
        for actor in [
            TaskGroupedApprovalActor::DeepSeekModel,
            TaskGroupedApprovalActor::FrontendPayload,
        ] {
            let claim = TaskGroupedApprovalResolutionClaim::from_group(&pending, actor);
            assert!(store
                .resolve_task_grouped_approval(
                    &claim,
                    true,
                    fixed_now() + chrono::Duration::minutes(1),
                )
                .is_err());
        }
        assert!(store
            .task_grouped_approval(pending.id)
            .unwrap()
            .unwrap()
            .resolution
            .is_none());

        let approved = approve(&store, &pending);
        for actor in [
            TaskGroupedApprovalActor::DeepSeekModel,
            TaskGroupedApprovalActor::FrontendPayload,
        ] {
            let claim = TaskGroupedApprovalResolutionClaim::from_group(&approved, actor);
            assert!(store
                .revoke_task_grouped_approval(&claim, fixed_now() + chrono::Duration::minutes(2),)
                .is_err());
        }
        let revoke_claim = TaskGroupedApprovalResolutionClaim::from_group(
            &approved,
            TaskGroupedApprovalActor::User,
        );
        let revoked = store
            .revoke_task_grouped_approval(&revoke_claim, fixed_now() + chrono::Duration::minutes(2))
            .unwrap();
        assert_eq!(revoked.status, TaskGroupedApprovalStatus::Revoked);
        assert_eq!(
            store
                .revoke_task_grouped_approval(
                    &revoke_claim,
                    fixed_now() + chrono::Duration::minutes(3),
                )
                .unwrap(),
            revoked
        );
        let capability_claim =
            TaskGroupedCapabilityClaim::from_group_item(&approved, &approved.capability_audits[0]);
        assert!(store
            .authorize_task_grouped_capability(
                &capability_claim,
                fixed_now() + chrono::Duration::minutes(3),
            )
            .is_err());
        assert_eq!(event_count(&store, TASK_GROUPED_APPROVAL_REVOKED_EVENT), 1);

        let kernel_store = EventStore::open_memory().unwrap();
        let kernel_pending = prepare(&kernel_store);
        let kernel_approved = approve(&kernel_store, &kernel_pending);
        let kernel_claim = TaskGroupedApprovalResolutionClaim::from_group(
            &kernel_approved,
            TaskGroupedApprovalActor::KernelLifecycle,
        );
        assert_eq!(
            kernel_store
                .revoke_task_grouped_approval(
                    &kernel_claim,
                    fixed_now() + chrono::Duration::minutes(2),
                )
                .unwrap()
                .status,
            TaskGroupedApprovalStatus::Revoked
        );
    }

    #[test]
    fn expiry_is_durable_for_pending_and_approved_groups() {
        let store = EventStore::open_memory().unwrap();
        let pending = prepare(&store);
        let after_expiry = fixed_expiry() + chrono::Duration::seconds(1);
        let resolution_claim = TaskGroupedApprovalResolutionClaim::from_group(
            &pending,
            TaskGroupedApprovalActor::User,
        );
        assert!(store
            .resolve_task_grouped_approval(&resolution_claim, true, after_expiry)
            .is_err());
        let expired = store.task_grouped_approval(pending.id).unwrap().unwrap();
        assert_eq!(expired.status, TaskGroupedApprovalStatus::Expired);
        assert_eq!(
            store
                .expire_task_grouped_approval(pending.id, TASK_ID, after_expiry)
                .unwrap(),
            expired
        );
        assert!(store
            .expire_task_grouped_approval(pending.id, OTHER_TASK_ID, after_expiry)
            .is_err());

        let store = EventStore::open_memory().unwrap();
        let pending = prepare(&store);
        let approved = approve(&store, &pending);
        let claim =
            TaskGroupedCapabilityClaim::from_group_item(&approved, &approved.capability_audits[0]);
        assert!(store
            .authorize_task_grouped_capability(&claim, after_expiry)
            .is_err());
        assert_eq!(
            store
                .task_grouped_approval(approved.id)
                .unwrap()
                .unwrap()
                .status,
            TaskGroupedApprovalStatus::Expired
        );
    }

    #[test]
    fn wrong_and_stale_resolution_or_capability_bindings_have_zero_authority() {
        let store = EventStore::open_memory().unwrap();
        let pending = prepare(&store);
        let base = TaskGroupedApprovalResolutionClaim::from_group(
            &pending,
            TaskGroupedApprovalActor::User,
        );
        let mut wrong_claims = Vec::new();
        let mut claim = base.clone();
        claim.group_id = Uuid::new_v4();
        wrong_claims.push(claim);
        let mut claim = base.clone();
        claim.task_id = OTHER_TASK_ID;
        wrong_claims.push(claim);
        let mut claim = base.clone();
        claim.expected_projection_revision += 1;
        wrong_claims.push(claim);
        let mut claim = base.clone();
        claim.manifest_revision = "a".repeat(64);
        wrong_claims.push(claim);
        let mut claim = base.clone();
        claim.manifest_fingerprint = "b".repeat(64);
        wrong_claims.push(claim);
        let mut claim = base.clone();
        claim.preview_schema_revision += 1;
        wrong_claims.push(claim);
        let mut claim = base.clone();
        claim.preview_renderer_revision += 1;
        wrong_claims.push(claim);
        let mut claim = base.clone();
        claim.preview_hash = "c".repeat(64);
        wrong_claims.push(claim);
        for claim in wrong_claims {
            assert!(store
                .resolve_task_grouped_approval(
                    &claim,
                    true,
                    fixed_now() + chrono::Duration::minutes(1),
                )
                .is_err());
        }
        let approved = approve(&store, &pending);
        let base =
            TaskGroupedCapabilityClaim::from_group_item(&approved, &approved.capability_audits[0]);
        let mut wrong_claims = Vec::new();
        let mut claim = base.clone();
        claim.task_id = OTHER_TASK_ID;
        wrong_claims.push(claim);
        let mut claim = base.clone();
        claim.expected_projection_revision += 1;
        wrong_claims.push(claim);
        let mut claim = base.clone();
        claim.manifest_revision = "d".repeat(64);
        wrong_claims.push(claim);
        let mut claim = base.clone();
        claim.manifest_fingerprint = "e".repeat(64);
        wrong_claims.push(claim);
        let mut claim = base.clone();
        claim.preview_hash = "f".repeat(64);
        wrong_claims.push(claim);
        let mut claim = base.clone();
        claim.capability = CapabilityKind::FileRead;
        wrong_claims.push(claim);
        let mut claim = base.clone();
        claim.tool_id = FILE_WRITE_TOOL_ID.to_string();
        wrong_claims.push(claim);
        let mut claim = base;
        claim.request_fingerprint = "0".repeat(64);
        wrong_claims.push(claim);
        for claim in wrong_claims {
            assert!(store
                .authorize_task_grouped_capability(
                    &claim,
                    fixed_now() + chrono::Duration::minutes(2),
                )
                .is_err());
        }
    }

    #[test]
    fn scope_or_external_target_change_invalidates_old_group_and_cross_task_reuse() {
        let store = EventStore::open_memory().unwrap();
        let pending = prepare(&store);
        let approved = approve(&store, &pending);
        let old_claim =
            TaskGroupedCapabilityClaim::from_group_item(&approved, &approved.capability_audits[0]);

        let (changed_manifest, changed_preview) = compiled_fixture_for(
            &store,
            TASK_ID,
            b"path-authority-v2",
            "Workspace / approved-reports",
            fixed_expiry(),
        );
        assert!(store
            .prepare_task_grouped_approval(
                TASK_ID,
                &changed_manifest,
                &changed_preview,
                fixed_now() + chrono::Duration::minutes(3),
            )
            .is_err());
        assert_eq!(
            store
                .task_grouped_approval(approved.id)
                .unwrap()
                .unwrap()
                .status,
            TaskGroupedApprovalStatus::ScopeChanged
        );
        assert_eq!(
            store
                .list_task_grouped_authorizations(fixed_now() + chrono::Duration::minutes(3))
                .unwrap()
                .into_iter()
                .find(|view| view.intent.group_id == approved.id)
                .unwrap()
                .status,
            TaskGroupedApprovalStatus::ScopeChanged
        );
        assert!(store
            .authorize_task_grouped_capability(
                &old_claim,
                fixed_now() + chrono::Duration::minutes(4),
            )
            .is_err());

        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM task_grouped_approval_state WHERE task_id = ?1",
                    params![TASK_ID.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1,
            "a resolved exact task cannot prompt for a second grouped resolution"
        );

        let pending_store = EventStore::open_memory().unwrap();
        let old_pending = prepare(&pending_store);
        let (changed_manifest, changed_preview) = compiled_fixture_for(
            &pending_store,
            TASK_ID,
            b"path-authority-v2",
            "Workspace / approved-reports",
            fixed_expiry(),
        );
        let replacement = pending_store
            .prepare_task_grouped_approval(
                TASK_ID,
                &changed_manifest,
                &changed_preview,
                fixed_now() + chrono::Duration::minutes(3),
            )
            .unwrap();
        assert_ne!(replacement.id, old_pending.id);
        assert_eq!(replacement.status, TaskGroupedApprovalStatus::Pending);
        assert_eq!(
            pending_store
                .task_grouped_approval(old_pending.id)
                .unwrap()
                .unwrap()
                .status,
            TaskGroupedApprovalStatus::ScopeChanged
        );
        let mut cross_task = old_claim;
        cross_task.task_id = OTHER_TASK_ID;
        assert!(store
            .authorize_task_grouped_capability(
                &cross_task,
                fixed_now() + chrono::Duration::minutes(4),
            )
            .is_err());
    }

    #[test]
    fn duplicate_prepare_and_restart_are_deterministic_without_duplicate_events() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("grouped-approval.db");
        let store = EventStore::open(&path).unwrap();
        let (manifest, preview) = compiled_fixture(&store);
        let first = store
            .prepare_task_grouped_approval(TASK_ID, &manifest, &preview, fixed_now())
            .unwrap();
        let duplicate = store
            .prepare_task_grouped_approval(
                TASK_ID,
                &manifest,
                &preview,
                fixed_now() + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!(first, duplicate);
        assert_eq!(event_count(&store, TASK_GROUPED_APPROVAL_PREPARED_EVENT), 1);
        assert!(store
            .append(
                &KernelEvent::new(TASK_GROUPED_APPROVAL_PREPARED_EVENT, first.event_receipt(),)
                    .unwrap(),
            )
            .is_err());
        assert_eq!(event_count(&store, TASK_GROUPED_APPROVAL_PREPARED_EVENT), 1);
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM task_grouped_approval_item_audit",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            2
        );
        let approved = approve(&store, &first);
        drop(store);

        let reopened = EventStore::open(&path).unwrap();
        let restored = reopened.task_grouped_approval(first.id).unwrap().unwrap();
        assert_eq!(restored, approved);
        let claim =
            TaskGroupedCapabilityClaim::from_group_item(&restored, &restored.capability_audits[0]);
        reopened
            .authorize_task_grouped_capability(&claim, fixed_now() + chrono::Duration::minutes(4))
            .expect("restart preserves exact authority");
    }

    #[test]
    fn c3d_production_producer_is_idempotent_across_reconciliation_restart_and_terminal_state() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("c3d-producer.db");
        let store = EventStore::open(&path).unwrap();
        frozen_goal_for(&store, TASK_ID, b"path-authority-v1");
        let proposal = descriptive_proposal(fixed_expiry());
        let context = manifest_context("Workspace / reports");

        let first = store
            .prepare_task_grouped_approval_from_proposal(TASK_ID, &proposal, &context, fixed_now())
            .unwrap();
        let duplicate = store
            .prepare_task_grouped_approval_from_proposal(
                TASK_ID,
                &proposal,
                &context,
                fixed_now() + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!(first, duplicate);
        assert_eq!(event_count(&store, TASK_GROUPED_APPROVAL_PREPARED_EVENT), 1);
        assert_eq!(
            store
                .list_task_grouped_authorizations(fixed_now())
                .unwrap()
                .len(),
            1
        );
        drop(store);

        let reopened = EventStore::open(&path).unwrap();
        let replay = reopened
            .prepare_task_grouped_approval_from_proposal(
                TASK_ID,
                &proposal,
                &context,
                fixed_now() + chrono::Duration::seconds(2),
            )
            .unwrap();
        assert_eq!(first, replay);
        assert_eq!(
            event_count(&reopened, TASK_GROUPED_APPROVAL_PREPARED_EVENT),
            1
        );

        let approved = approve(&reopened, &replay);
        let terminal_replay = reopened
            .prepare_task_grouped_approval_from_proposal(
                TASK_ID,
                &proposal,
                &context,
                fixed_now() + chrono::Duration::minutes(2),
            )
            .unwrap();
        assert_eq!(terminal_replay, approved);
        assert_eq!(terminal_replay.status, TaskGroupedApprovalStatus::Approved);
        assert_eq!(
            event_count(&reopened, TASK_GROUPED_APPROVAL_PREPARED_EVENT),
            1
        );

        let expired_store = EventStore::open_memory().unwrap();
        frozen_goal_for(&expired_store, TASK_ID, b"path-authority-v1");
        let expired = descriptive_proposal(fixed_now());
        assert!(expired_store
            .prepare_task_grouped_approval_from_proposal(TASK_ID, &expired, &context, fixed_now(),)
            .is_err());
        assert!(expired_store
            .list_task_grouped_authorizations(fixed_now())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn additive_migration_is_idempotent_and_preserves_legacy_exact_tool_approval() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("legacy.db");
        let store = EventStore::open(&path).unwrap();
        let mut request =
            request_capability_access(AccessMode::AskEveryStep, CapabilityKind::FileWrite).unwrap();
        request
            .bind_exact_tool(
                FILE_WRITE_TOOL_ID,
                "1".repeat(64),
                "Legacy exact file write preview",
            )
            .unwrap();
        let request_id = request.id;
        store.append_capability_access_request(&request).unwrap();
        store
            .resolve_capability_access_request(request_id, true, "legacy user approval".to_string())
            .unwrap();
        store
            .conn
            .execute_batch(
                "DROP TABLE task_grouped_approval_item_audit;
                 DROP TABLE task_grouped_approval_state;",
            )
            .unwrap();
        drop(store);

        for _ in 0..2 {
            let reopened = EventStore::open(&path).unwrap();
            let record = reopened
                .list_capability_access_records()
                .unwrap()
                .into_iter()
                .find(|record| record.request.id == request_id)
                .unwrap();
            assert_eq!(record.effective_status, CapabilityAccessStatus::Approved);
            assert_eq!(record.grant_state, CapabilityGrantState::OneShotAvailable);
            assert_eq!(
                reopened
                    .conn
                    .query_row(
                        "SELECT COUNT(*) FROM task_grouped_approval_state",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                0
            );
            drop(reopened);
        }
    }

    #[test]
    fn tampered_projection_or_per_item_audit_fails_closed_on_restart() {
        let directory = tempdir().unwrap();
        let projection_path = directory.path().join("tampered-projection.db");
        let store = EventStore::open(&projection_path).unwrap();
        let group = prepare(&store);
        drop(store);
        let connection = Connection::open(&projection_path).unwrap();
        let json: String = connection
            .query_row(
                "SELECT projection_json FROM task_grouped_approval_state WHERE group_id = ?1",
                params![group.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["preview"]["preview_hash"] = serde_json::Value::String("0".repeat(64));
        connection
            .execute(
                "UPDATE task_grouped_approval_state SET projection_json = ?2 WHERE group_id = ?1",
                params![group.id.to_string(), serde_json::to_string(&value).unwrap()],
            )
            .unwrap();
        drop(connection);
        assert!(EventStore::open(&projection_path).is_err());

        let audit_path = directory.path().join("tampered-audit.db");
        let store = EventStore::open(&audit_path).unwrap();
        let group = prepare(&store);
        drop(store);
        let connection = Connection::open(&audit_path).unwrap();
        connection
            .execute(
                r#"UPDATE task_grouped_approval_item_audit SET transition = 'approved'
                   WHERE group_id = ?1 AND group_revision = 0"#,
                params![group.id.to_string()],
            )
            .unwrap();
        drop(connection);
        assert!(EventStore::open(&audit_path).is_err());
    }

    #[test]
    fn c3c_ui_projection_is_exact_redacted_and_keeps_per_capability_audit_visible() {
        let store = EventStore::open_memory().unwrap();
        let pending = prepare(&store);
        let views = store
            .list_task_grouped_authorizations(fixed_now())
            .expect("authorization views load");
        assert_eq!(views.len(), 1);
        let view = &views[0];
        assert_eq!(view.status, TaskGroupedApprovalStatus::Pending);
        assert_eq!(view.intent.group_id, pending.id);
        assert_eq!(view.intent.task_id, TASK_ID);
        assert_eq!(view.intent.expected_projection_revision, 0);
        assert_eq!(
            view.goal,
            "Create a verified report and send one approved external update."
        );
        assert_eq!(
            view.applications,
            vec!["Microsoft Excel", "Microsoft Outlook"]
        );
        assert_eq!(view.paths, vec!["Workspace / reports"]);
        assert_eq!(view.accounts, vec!["Work mailbox account"]);
        assert_eq!(view.recipients, vec!["finance-test@example.com"]);
        assert_eq!(
            view.time_windows,
            vec!["Weekdays 09:00-17:00 Asia/Shanghai"]
        );
        assert_eq!(view.verifiers.len(), 2);
        assert_eq!(view.capability_audits.len(), 2);
        assert!(view
            .capability_audits
            .iter()
            .all(|audit| audit.status == TaskGroupedCapabilityAuditStatus::Pending));

        let json = serde_json::to_string(view).unwrap().to_ascii_lowercase();
        for forbidden in [
            "approval_request_id",
            "request_fingerprint",
            "authority_fingerprint",
            "exact_preview",
            "tool_id",
            "credential",
            "provider_ref",
            "claim",
            "token",
            "c:\\users\\private\\appdata",
        ] {
            assert!(
                !json.contains(forbidden),
                "UI projection leaked {forbidden}"
            );
        }
    }

    #[test]
    fn c3c_exact_ui_intent_rejects_tamper_replay_and_frontend_authority_fields() {
        let store = EventStore::open_memory().unwrap();
        let pending = prepare(&store);
        let goal = store.goal_envelope_projection(TASK_ID).unwrap().unwrap();
        let intent = pending.authorization_view(&goal).unwrap().intent;

        let mut tampered = Vec::new();
        let mut wrong_group = intent.clone();
        wrong_group.group_id = Uuid::from_u128(0xdead);
        tampered.push(wrong_group);
        let mut wrong_task = intent.clone();
        wrong_task.task_id = OTHER_TASK_ID;
        tampered.push(wrong_task);
        let mut wrong_projection_revision = intent.clone();
        wrong_projection_revision.expected_projection_revision += 1;
        tampered.push(wrong_projection_revision);
        let mut wrong_manifest_revision = intent.clone();
        wrong_manifest_revision.manifest_revision = "0".repeat(64);
        tampered.push(wrong_manifest_revision);
        let mut wrong_manifest_fingerprint = intent.clone();
        wrong_manifest_fingerprint.manifest_fingerprint = "1".repeat(64);
        tampered.push(wrong_manifest_fingerprint);
        let mut wrong_preview_schema = intent.clone();
        wrong_preview_schema.preview_schema_revision += 1;
        tampered.push(wrong_preview_schema);
        let mut wrong_preview_renderer = intent.clone();
        wrong_preview_renderer.preview_renderer_revision += 1;
        tampered.push(wrong_preview_renderer);
        let mut wrong_preview_hash = intent.clone();
        wrong_preview_hash.preview_hash = "2".repeat(64);
        tampered.push(wrong_preview_hash);

        for tampered_intent in tampered {
            assert!(store
                .resolve_task_grouped_authorization(
                    &tampered_intent,
                    true,
                    fixed_now() + chrono::Duration::minutes(1),
                )
                .is_err());
            assert_eq!(
                store
                    .task_grouped_approval(pending.id)
                    .unwrap()
                    .unwrap()
                    .status,
                TaskGroupedApprovalStatus::Pending
            );
        }

        let base_value = serde_json::to_value(&intent).unwrap();
        for forbidden in [
            "capability",
            "risk",
            "scope",
            "target",
            "authority",
            "preview",
            "grant",
            "actor",
            "claim",
            "token",
        ] {
            let mut forged = base_value.clone();
            forged.as_object_mut().unwrap().insert(
                forbidden.to_string(),
                serde_json::Value::String("frontend-forgery".to_string()),
            );
            assert!(serde_json::from_value::<TaskGroupedAuthorizationIntent>(forged).is_err());
        }

        let approved = store
            .resolve_task_grouped_authorization(
                &intent,
                true,
                fixed_now() + chrono::Duration::minutes(1),
            )
            .unwrap();
        assert!(approved
            .capability_audits
            .iter()
            .all(|audit| audit.status == TaskGroupedCapabilityAuditStatus::Approved));
        assert_eq!(event_count(&store, TASK_GROUPED_APPROVAL_RESOLVED_EVENT), 1);
        let duplicate = store
            .resolve_task_grouped_authorization(
                &intent,
                true,
                fixed_now() + chrono::Duration::minutes(2),
            )
            .unwrap();
        assert_eq!(approved, duplicate);
        assert!(store
            .resolve_task_grouped_authorization(
                &intent,
                false,
                fixed_now() + chrono::Duration::minutes(2),
            )
            .is_err());
        for item in &pending.capability_audits {
            assert!(store
                .resolve_capability_access_request(
                    item.approval_request_id,
                    true,
                    "forged per-item approval".to_string(),
                )
                .is_err());
        }

        let revoked = store
            .revoke_task_grouped_authorization(
                &approved.intent,
                fixed_now() + chrono::Duration::minutes(3),
            )
            .unwrap();
        let duplicate_revoke = store
            .revoke_task_grouped_authorization(
                &approved.intent,
                fixed_now() + chrono::Duration::minutes(4),
            )
            .unwrap();
        assert_eq!(revoked, duplicate_revoke);
        assert_eq!(revoked.status, TaskGroupedApprovalStatus::Revoked);

        let reject_store = EventStore::open_memory().unwrap();
        let reject_pending = prepare(&reject_store);
        let reject_goal = reject_store
            .goal_envelope_projection(TASK_ID)
            .unwrap()
            .unwrap();
        let rejected = reject_store
            .resolve_task_grouped_authorization(
                &reject_pending
                    .authorization_view(&reject_goal)
                    .unwrap()
                    .intent,
                false,
                fixed_now() + chrono::Duration::minutes(1),
            )
            .unwrap();
        assert_eq!(rejected.status, TaskGroupedApprovalStatus::Rejected);
        assert!(rejected
            .capability_audits
            .iter()
            .all(|audit| audit.status == TaskGroupedCapabilityAuditStatus::Rejected));
        assert_eq!(
            event_count(&reject_store, TASK_GROUPED_APPROVAL_RESOLVED_EVENT),
            1
        );
    }

    #[test]
    fn c3c_ui_read_refreshes_expiry_and_survives_restart_without_new_authority() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("c3c-ui.db");
        let store = EventStore::open(&path).unwrap();
        let expiry = fixed_now() + chrono::Duration::minutes(1);
        let (manifest, preview) = compiled_fixture_for(
            &store,
            TASK_ID,
            b"path-authority-v1",
            "Workspace / reports",
            expiry,
        );
        let group = store
            .prepare_task_grouped_approval(TASK_ID, &manifest, &preview, fixed_now())
            .unwrap();
        drop(store);

        let reopened = EventStore::open(&path).unwrap();
        let views = reopened
            .list_task_grouped_authorizations(expiry + chrono::Duration::seconds(1))
            .unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].intent.group_id, group.id);
        assert_eq!(views[0].status, TaskGroupedApprovalStatus::Expired);
        assert!(reopened
            .resolve_task_grouped_authorization(
                &views[0].intent,
                true,
                expiry + chrono::Duration::seconds(2),
            )
            .is_err());
        assert_eq!(
            event_count(&reopened, TASK_GROUPED_APPROVAL_RESOLVED_EVENT),
            0
        );
    }

    #[test]
    fn malformed_input_creates_zero_group_permission_or_execution_authority() {
        let store = EventStore::open_memory().unwrap();
        let malformed = r#"{
          "version":"ds-agent.task-capability-manifest/v1",
          "task_id":"00000000-0000-0000-0000-00000000033b",
          "unexpected_authority":true
        }"#;
        assert!(TaskCapabilityManifest::parse_json(malformed).is_err());
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM task_grouped_approval_state",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert!(store.list_capability_access_records().unwrap().is_empty());
        assert!(store.list_tool_invocations().unwrap().is_empty());
        assert_eq!(event_count(&store, TASK_GROUPED_APPROVAL_PREPARED_EVENT), 0);
    }

    #[test]
    fn grouped_projection_and_events_keep_secret_path_and_provider_refs_out() {
        let store = EventStore::open_memory().unwrap();
        let group = prepare(&store);
        let serialized = group.canonical_json().unwrap();
        let event_payloads = {
            let mut statement = store
                .conn
                .prepare("SELECT payload_json FROM kernel_events ORDER BY rowid")
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join("\n")
        };
        let combined = format!("{serialized}\n{event_payloads}").to_ascii_lowercase();
        for forbidden in [
            "sk-live-secret-marker",
            "c:\\users\\private\\appdata",
            "provider-message-id",
            "credential_handle",
            "claim_token",
            "bearer ",
        ] {
            assert!(!combined.contains(forbidden), "leaked {forbidden}");
        }
        for item in &group.capability_audits {
            assert!(!item.exact_preview.contains("finance-test@example.com"));
            assert!(!item.exact_preview.contains("Workspace / reports"));
        }
    }
}
