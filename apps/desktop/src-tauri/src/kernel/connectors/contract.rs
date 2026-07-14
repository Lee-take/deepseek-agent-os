use std::collections::HashSet;

use super::provider::{
    collect_calendar_events, collect_mail_search, CalendarConnectorProvider, CalendarListRequest,
    MailConnectorProvider, MailSearchRequest, MailThreadRequest,
};
use super::sync::{
    CalendarSyncProvider, CalendarSyncRequest, ConnectorSyncChange, ConnectorSyncContinuation,
    ConnectorSyncPage, MailSyncProvider, MailSyncRequest,
};
use super::{
    ConnectorAccount, ConnectorCapability, ConnectorDraftProvider, ConnectorEvidenceRef,
    ConnectorHealth, ConnectorInvocation, ConnectorMutationApplyOutcome, ConnectorMutationProvider,
    ConnectorMutationReceipt, ConnectorMutationReconciler, ConnectorProvider,
    ConnectorReconciliationOutcome,
};

#[must_use = "provider contract coverage must be finished"]
pub struct ProviderContractCoverage {
    advertised: HashSet<ConnectorCapability>,
    covered: HashSet<ConnectorCapability>,
}

impl ProviderContractCoverage {
    fn cover(&mut self, capabilities: &[ConnectorCapability]) -> Result<(), String> {
        for capability in capabilities {
            if !self.advertised.contains(capability) {
                return Err("provider contract exercised an unadvertised capability".to_string());
            }
            if !self.covered.insert(*capability) {
                return Err("provider contract exercised a capability more than once".to_string());
            }
        }
        Ok(())
    }

    pub fn finish(self) -> Result<(), String> {
        if self.covered == self.advertised {
            Ok(())
        } else {
            Err("provider contract did not exercise every advertised capability".to_string())
        }
    }
}

pub fn validate_provider_contract(
    provider: &dyn ConnectorProvider,
    account: &ConnectorAccount,
) -> Result<ProviderContractCoverage, String> {
    if provider.provider_id().trim().is_empty() || account.provider_id != provider.provider_id() {
        return Err("provider contract requires a stable matching provider id".to_string());
    }
    if account.health != ConnectorHealth::Connected {
        return Err("provider contract fixture must be connected".to_string());
    }
    let mut advertised = HashSet::new();
    for capability in provider.capabilities() {
        if !advertised.insert(*capability) {
            return Err("provider contract contains a duplicate capability".to_string());
        }
        if !account.granted_capabilities.contains(capability) {
            return Err("provider contract fixture is missing a granted capability".to_string());
        }
    }
    Ok(ProviderContractCoverage {
        advertised,
        covered: HashSet::new(),
    })
}

pub fn validate_mail_read_contract(
    provider: &dyn MailConnectorProvider,
    account: &ConnectorAccount,
    mail_search: &MailSearchRequest,
    thread_read: &MailThreadRequest,
    coverage: &mut ProviderContractCoverage,
) -> Result<(), String> {
    let threads = collect_mail_search(provider, account, mail_search)
        .map_err(|_| "provider failed the typed mail search contract".to_string())?;
    if threads.len() > usize::from(mail_search.max_results()) {
        return Err("provider exceeded the bounded mail search total limit".to_string());
    }
    for thread in &threads {
        thread.validate()?;
    }
    let thread = provider
        .read_thread(account, thread_read)
        .map_err(|_| "provider failed the typed thread contract".to_string())?;
    thread.validate()?;
    if thread.remote_ref != thread_read.thread_ref()
        || thread.messages.len() > usize::from(thread_read.max_messages())
    {
        return Err("provider returned the wrong or oversized normalized mail thread".to_string());
    }
    coverage.cover(&[
        ConnectorCapability::MailSearch,
        ConnectorCapability::MailReadThread,
    ])
}

pub fn validate_calendar_read_contract(
    provider: &dyn CalendarConnectorProvider,
    account: &ConnectorAccount,
    calendar_list: &CalendarListRequest,
    coverage: &mut ProviderContractCoverage,
) -> Result<(), String> {
    let events = collect_calendar_events(provider, account, calendar_list)
        .map_err(|_| "provider failed the typed calendar contract".to_string())?;
    if events.len() > usize::from(calendar_list.max_results()) {
        return Err("provider exceeded the bounded calendar total limit".to_string());
    }
    for event in &events {
        event.validate()?;
        if event.ends_at <= calendar_list.starts_at() || event.starts_at >= calendar_list.ends_at()
        {
            return Err("provider returned an event outside the requested range".to_string());
        }
    }
    coverage.cover(&[ConnectorCapability::CalendarListEvents])
}

pub fn validate_mail_sync_contract(
    provider: &dyn MailSyncProvider,
    account: &ConnectorAccount,
    request: &MailSyncRequest,
    coverage: &mut ProviderContractCoverage,
) -> Result<(), String> {
    let page = provider
        .sync_mail_page(account, request, None)
        .map_err(|_| "provider failed the typed mail sync contract".to_string())?;
    validate_mail_sync_page(&page, request.max_changes())?;
    coverage.cover(&[ConnectorCapability::MailSyncInbox])
}

pub fn validate_calendar_sync_contract(
    provider: &dyn CalendarSyncProvider,
    account: &ConnectorAccount,
    request: &CalendarSyncRequest,
    coverage: &mut ProviderContractCoverage,
) -> Result<(), String> {
    let page = provider
        .sync_calendar_page(account, request, None)
        .map_err(|_| "provider failed the typed calendar sync contract".to_string())?;
    validate_calendar_sync_page(&page, request.max_changes())?;
    coverage.cover(&[ConnectorCapability::CalendarSyncEvents])
}

pub fn validate_draft_contract(
    provider: &dyn ConnectorDraftProvider,
    account: &ConnectorAccount,
    coverage: &mut ProviderContractCoverage,
) -> Result<ConnectorEvidenceRef, String> {
    let evidence = provider
        .create_draft(account, "Provider contract draft")
        .map_err(|_| "provider failed the explicit draft contract".to_string())?;
    validate_evidence(&evidence, provider.provider_id(), account.id)?;
    coverage.cover(&[ConnectorCapability::MailCreateDraft])?;
    Ok(evidence)
}

pub fn validate_mutation_contract(
    provider: &dyn ConnectorMutationProvider,
    reconciler: &dyn ConnectorMutationReconciler,
    account: &ConnectorAccount,
    invocation: &ConnectorInvocation,
    coverage: &mut ProviderContractCoverage,
) -> Result<(), String> {
    if provider.provider_id() != reconciler.provider_id()
        || !invocation.capability.external_mutation()
    {
        return Err("provider mutation contract fixture is invalid".to_string());
    }
    let ConnectorMutationApplyOutcome::Applied(applied) = provider
        .apply_mutation(account, invocation)
        .map_err(|_| "provider failed the explicit mutation contract".to_string())?
    else {
        return Err("provider mutation contract returned an uncertain first result".to_string());
    };
    validate_mutation_receipt(&applied, provider.provider_id(), account, invocation, false)?;
    let ConnectorReconciliationOutcome::Applied(reconciled) = reconciler
        .reconcile_mutation(account, invocation)
        .map_err(|_| "provider failed the read-only reconciliation contract".to_string())?
    else {
        return Err("provider could not reconcile its applied contract fixture".to_string());
    };
    validate_mutation_receipt(
        &reconciled,
        provider.provider_id(),
        account,
        invocation,
        true,
    )?;
    coverage.cover(&[invocation.capability])
}

fn validate_mail_sync_page(
    page: &ConnectorSyncPage<super::domain::MailMessage>,
    maximum: u16,
) -> Result<(), String> {
    if page.changes().len() > usize::from(maximum) {
        return Err("provider exceeded the typed mail sync page limit".to_string());
    }
    for change in page.changes() {
        match change {
            ConnectorSyncChange::Upsert(message) => message.validate()?,
            ConnectorSyncChange::Deleted { remote_ref } => validate_remote_ref(remote_ref)?,
        }
    }
    validate_continuation(page.continuation())
}

fn validate_calendar_sync_page(
    page: &ConnectorSyncPage<super::domain::CalendarEvent>,
    maximum: u16,
) -> Result<(), String> {
    if page.changes().len() > usize::from(maximum) {
        return Err("provider exceeded the typed calendar sync page limit".to_string());
    }
    for change in page.changes() {
        match change {
            ConnectorSyncChange::Upsert(event) => event.validate()?,
            ConnectorSyncChange::Deleted { remote_ref } => validate_remote_ref(remote_ref)?,
        }
    }
    validate_continuation(page.continuation())
}

fn validate_continuation(continuation: &ConnectorSyncContinuation) -> Result<(), String> {
    let value = match continuation {
        ConnectorSyncContinuation::Next(value) | ConnectorSyncContinuation::Delta(value) => {
            value.expose()
        }
    };
    if value.trim().is_empty() {
        Err("provider returned an invalid typed sync continuation".to_string())
    } else {
        Ok(())
    }
}

fn validate_mutation_receipt(
    receipt: &ConnectorMutationReceipt,
    provider_id: &str,
    account: &ConnectorAccount,
    invocation: &ConnectorInvocation,
    reconciled: bool,
) -> Result<(), String> {
    let mutation = invocation
        .mutation
        .as_ref()
        .ok_or_else(|| "provider mutation contract requires a frozen envelope".to_string())?;
    if receipt.provider_id != provider_id
        || receipt.account_id != account.id
        || receipt.capability != invocation.capability
        || receipt.target_ref != mutation.target_ref
        || receipt.request_fingerprint != invocation.request_fingerprint
        || receipt.idempotency_key != invocation.idempotency_key
        || receipt.reconciled != reconciled
    {
        return Err("provider returned a mutation receipt outside the frozen contract".to_string());
    }
    validate_evidence(&receipt.evidence, provider_id, account.id)
}

fn validate_evidence(
    evidence: &ConnectorEvidenceRef,
    provider_id: &str,
    account_id: uuid::Uuid,
) -> Result<(), String> {
    if evidence.provider_id != provider_id
        || evidence.account_id != account_id
        || evidence.remote_object_ref.trim().is_empty()
        || evidence.remote_object_ref.chars().count() > 1024
        || evidence
            .bounded_summary
            .as_ref()
            .is_some_and(|value| value.chars().count() > 2000)
    {
        return Err("provider returned invalid bounded evidence".to_string());
    }
    Ok(())
}

fn validate_remote_ref(remote_ref: &str) -> Result<(), String> {
    if remote_ref.trim().is_empty() || remote_ref.chars().count() > 1024 {
        Err("provider returned an invalid deleted remote reference".to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::{Duration, Utc};
    use uuid::Uuid;

    use super::*;
    use crate::kernel::connectors::domain::{CalendarEvent, MailAddress, MailMessage, MailThread};
    use crate::kernel::connectors::provider::ConnectorProviderResult;
    use crate::kernel::connectors::{
        ConnectorCredentialHandle, ConnectorInvocationStatus, ConnectorMutationEnvelope,
        FakeConnectorProvider,
    };
    use crate::kernel::models::AccessMode;

    fn account(provider: &dyn ConnectorProvider) -> ConnectorAccount {
        let now = Utc::now();
        ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: provider.provider_id().to_string(),
            display_name: "Contract account".to_string(),
            tenant_ref: None,
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: provider.capabilities().to_vec(),
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        }
    }

    fn requests() -> (
        MailSearchRequest,
        MailThreadRequest,
        CalendarListRequest,
        MailSyncRequest,
        CalendarSyncRequest,
    ) {
        let now = Utc::now();
        (
            MailSearchRequest::new("contract".to_string(), 10).unwrap(),
            MailThreadRequest::new("fake:thread:contract".to_string(), 10).unwrap(),
            CalendarListRequest::new(now, now + Duration::days(1), 10).unwrap(),
            MailSyncRequest::inbox(10).unwrap(),
            CalendarSyncRequest::new(now, now + Duration::days(1), 10).unwrap(),
        )
    }

    fn running_invocation(
        account: &ConnectorAccount,
        capability: ConnectorCapability,
    ) -> ConnectorInvocation {
        let now = Utc::now();
        let automation_run_id = Uuid::new_v4();
        let idempotency_key = format!("contract:{}", capability.contract_name());
        ConnectorInvocation {
            id: Uuid::new_v4(),
            provider_id: account.provider_id.clone(),
            account_id: account.id,
            account_generation: Some(0),
            capability,
            automation_run_id: Some(automation_run_id),
            tool_invocation_id: Some(Uuid::new_v4()),
            request_fingerprint: format!("sha256:{}", Uuid::new_v4()),
            idempotency_key: idempotency_key.clone(),
            mutation: Some(ConnectorMutationEnvelope {
                provider_id: account.provider_id.clone(),
                account_id: account.id,
                account_generation: Some(0),
                capability,
                target_ref: format!("contract-target:{}", capability.contract_name()),
                preview_hash: "sha256:contract-preview".to_string(),
                idempotency_key,
                automation_run_id,
                agent_run_id: None,
                access_mode: AccessMode::FullAccess,
            }),
            status: ConnectorInvocationStatus::Running,
            evidence: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn fake_provider_passes_every_advertised_contract_exactly_once() {
        let provider = FakeConnectorProvider::default();
        let account = account(&provider);
        let (mail_search, thread_read, calendar_list, mail_sync, calendar_sync) = requests();
        let mut coverage = validate_provider_contract(&provider, &account).unwrap();
        validate_mail_read_contract(
            &provider,
            &account,
            &mail_search,
            &thread_read,
            &mut coverage,
        )
        .unwrap();
        validate_calendar_read_contract(&provider, &account, &calendar_list, &mut coverage)
            .unwrap();
        validate_mail_sync_contract(&provider, &account, &mail_sync, &mut coverage).unwrap();
        validate_calendar_sync_contract(&provider, &account, &calendar_sync, &mut coverage)
            .unwrap();
        validate_draft_contract(&provider, &account, &mut coverage).unwrap();
        for capability in [
            ConnectorCapability::MailSendDraft,
            ConnectorCapability::CalendarCreateEvent,
        ] {
            validate_mutation_contract(
                &provider,
                &provider,
                &account,
                &running_invocation(&account, capability),
                &mut coverage,
            )
            .unwrap();
        }
        coverage.finish().unwrap();
        assert_eq!(provider.applied_count(), 2);
    }

    const READ_CAPABILITIES: &[ConnectorCapability] = &[
        ConnectorCapability::MailSearch,
        ConnectorCapability::MailReadThread,
        ConnectorCapability::CalendarListEvents,
    ];

    struct EmptyReadProvider;

    impl ConnectorProvider for EmptyReadProvider {
        fn provider_id(&self) -> &'static str {
            "empty"
        }

        fn capabilities(&self) -> &'static [ConnectorCapability] {
            READ_CAPABILITIES
        }
    }

    impl MailConnectorProvider for EmptyReadProvider {
        fn search_mail_page(
            &self,
            _account: &ConnectorAccount,
            _request: &MailSearchRequest,
            _continuation: Option<&super::super::provider::ConnectorReadContinuation>,
        ) -> ConnectorProviderResult<super::super::provider::ConnectorReadPage<MailThread>>
        {
            Ok(super::super::provider::ConnectorReadPage::new(
                Vec::new(),
                None,
            ))
        }

        fn read_thread(
            &self,
            _account: &ConnectorAccount,
            request: &MailThreadRequest,
        ) -> ConnectorProviderResult<MailThread> {
            let address = MailAddress {
                display_name: None,
                address: "sender@example.com".to_string(),
            };
            Ok(MailThread {
                remote_ref: request.thread_ref().to_string(),
                messages: vec![MailMessage {
                    remote_ref: "message:1".to_string(),
                    thread_ref: request.thread_ref().to_string(),
                    from: address.clone(),
                    to: vec![address],
                    subject: String::new(),
                    received_at: Utc::now(),
                    bounded_body_summary: None,
                    attachments: Vec::new(),
                    has_attachments: false,
                    untrusted_evidence: true,
                }],
            })
        }
    }

    impl CalendarConnectorProvider for EmptyReadProvider {
        fn list_events_page(
            &self,
            _account: &ConnectorAccount,
            _request: &CalendarListRequest,
            _continuation: Option<&super::super::provider::ConnectorReadContinuation>,
        ) -> ConnectorProviderResult<super::super::provider::ConnectorReadPage<CalendarEvent>>
        {
            Ok(super::super::provider::ConnectorReadPage::new(
                Vec::new(),
                None,
            ))
        }
    }

    #[test]
    fn typed_read_contract_allows_legitimate_empty_search_and_calendar_results() {
        let provider = EmptyReadProvider;
        let account = account(&provider);
        let (mail_search, thread_read, calendar_list, _, _) = requests();
        let mut coverage = validate_provider_contract(&provider, &account).unwrap();
        validate_mail_read_contract(
            &provider,
            &account,
            &mail_search,
            &thread_read,
            &mut coverage,
        )
        .unwrap();
        validate_calendar_read_contract(&provider, &account, &calendar_list, &mut coverage)
            .unwrap();
        coverage.finish().unwrap();
    }

    struct CountingDraftProvider {
        draft_calls: AtomicUsize,
    }

    impl ConnectorProvider for CountingDraftProvider {
        fn provider_id(&self) -> &'static str {
            "counting"
        }

        fn capabilities(&self) -> &'static [ConnectorCapability] {
            &[ConnectorCapability::MailCreateDraft]
        }
    }

    impl ConnectorDraftProvider for CountingDraftProvider {
        fn create_draft(
            &self,
            account: &ConnectorAccount,
            _title: &str,
        ) -> Result<ConnectorEvidenceRef, String> {
            self.draft_calls.fetch_add(1, Ordering::SeqCst);
            Ok(ConnectorEvidenceRef {
                provider_id: self.provider_id().to_string(),
                account_id: account.id,
                remote_object_ref: "draft:1".to_string(),
                retrieved_at: Utc::now(),
                bounded_summary: None,
            })
        }
    }

    #[test]
    fn metadata_contract_is_pure_and_uncovered_capabilities_fail_finish() {
        let provider = CountingDraftProvider {
            draft_calls: AtomicUsize::new(0),
        };
        let account = account(&provider);
        let coverage = validate_provider_contract(&provider, &account).unwrap();
        assert_eq!(provider.draft_calls.load(Ordering::SeqCst), 0);
        assert!(coverage.finish().is_err());
        assert_eq!(provider.draft_calls.load(Ordering::SeqCst), 0);
    }
}
