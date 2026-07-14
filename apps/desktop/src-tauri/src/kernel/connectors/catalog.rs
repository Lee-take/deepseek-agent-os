use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::{ConnectorAccount, ConnectorCapability, ConnectorHealth};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorProviderKey {
    Microsoft365,
    GoogleWorkspace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorProviderAvailability {
    Unavailable,
    FakeOnly,
    Configured,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorAbility {
    MailRead,
    MailAttachments,
    MailDraft,
    MailSend,
    MailSync,
    CalendarRead,
    CalendarChange,
    CalendarSync,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorAbilityRisk {
    ReadOnly,
    LocalDraft,
    ExternalWrite,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConnectorAbilityDescriptor {
    pub ability: ConnectorAbility,
    pub risk: ConnectorAbilityRisk,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConnectorProviderDescriptor {
    pub key: ConnectorProviderKey,
    pub availability: ConnectorProviderAvailability,
    pub abilities: Vec<ConnectorAbilityDescriptor>,
    pub repair_available: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorProviderLabelCode {
    Microsoft365,
    GoogleWorkspace,
    WorkspaceConnector,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorHealthReasonCode {
    AuthorizationExpired,
    DisconnectFinishing,
    RevocationUnconfirmed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorSyncHealthState {
    NotEnabled,
    NeverSynced,
    Healthy,
    Delayed,
    Stopped,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConnectorSyncHealthSnapshot {
    pub stream_count: usize,
    pub any_stopped: bool,
    pub any_delayed: bool,
    pub last_successful_sync_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConnectorAccountHealthView {
    pub id: Uuid,
    pub display_name: String,
    pub provider_label: ConnectorProviderLabelCode,
    pub abilities: Vec<ConnectorAbility>,
    pub health: ConnectorHealth,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_reason: Option<ConnectorHealthReasonCode>,
    pub sync_state: ConnectorSyncHealthState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_successful_sync_at: Option<DateTime<Utc>>,
    pub repair_action_available: bool,
    pub connected_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ConnectorAccountHealthView {
    pub fn from_private_account(
        account: &ConnectorAccount,
        sync: ConnectorSyncHealthSnapshot,
        repair_action_available: bool,
    ) -> Self {
        let sync_enabled = account.granted_capabilities.iter().any(|capability| {
            matches!(
                capability,
                ConnectorCapability::MailSyncInbox | ConnectorCapability::CalendarSyncEvents
            )
        });
        let sync_state = if !sync_enabled {
            ConnectorSyncHealthState::NotEnabled
        } else if sync.any_stopped {
            ConnectorSyncHealthState::Stopped
        } else if sync.any_delayed {
            ConnectorSyncHealthState::Delayed
        } else if sync.stream_count == 0 || sync.last_successful_sync_at.is_none() {
            ConnectorSyncHealthState::NeverSynced
        } else {
            ConnectorSyncHealthState::Healthy
        };
        let health_reason = match account.health {
            ConnectorHealth::NeedsRepair => Some(ConnectorHealthReasonCode::AuthorizationExpired),
            ConnectorHealth::DisconnectPending => {
                Some(ConnectorHealthReasonCode::DisconnectFinishing)
            }
            ConnectorHealth::RevocationPending => {
                Some(ConnectorHealthReasonCode::RevocationUnconfirmed)
            }
            ConnectorHealth::Connected | ConnectorHealth::Disconnected => None,
        };
        Self {
            id: account.id,
            display_name: account.display_name.clone(),
            provider_label: provider_label(&account.provider_id),
            abilities: user_abilities(&account.granted_capabilities),
            health: account.health,
            health_reason,
            sync_state,
            last_successful_sync_at: sync.last_successful_sync_at,
            repair_action_available: repair_action_available
                && account.health == ConnectorHealth::NeedsRepair,
            connected_at: account.connected_at,
            updated_at: account.updated_at,
        }
    }
}

pub fn production_connector_catalog() -> Vec<ConnectorProviderDescriptor> {
    [
        ConnectorProviderKey::Microsoft365,
        ConnectorProviderKey::GoogleWorkspace,
    ]
    .into_iter()
    .map(|key| ConnectorProviderDescriptor {
        key,
        availability: ConnectorProviderAvailability::Unavailable,
        abilities: standard_mail_calendar_abilities(),
        repair_available: false,
    })
    .collect()
}

pub fn user_abilities(capabilities: &[ConnectorCapability]) -> Vec<ConnectorAbility> {
    let mut abilities = capabilities
        .iter()
        .map(|capability| match capability {
            ConnectorCapability::MailSearch | ConnectorCapability::MailReadThread => {
                ConnectorAbility::MailRead
            }
            ConnectorCapability::MailReadAttachment => ConnectorAbility::MailAttachments,
            ConnectorCapability::MailCreateDraft => ConnectorAbility::MailDraft,
            ConnectorCapability::MailSendDraft => ConnectorAbility::MailSend,
            ConnectorCapability::MailSyncInbox => ConnectorAbility::MailSync,
            ConnectorCapability::CalendarListEvents | ConnectorCapability::CalendarFindFreeTime => {
                ConnectorAbility::CalendarRead
            }
            ConnectorCapability::CalendarCreateEvent
            | ConnectorCapability::CalendarUpdateEvent
            | ConnectorCapability::CalendarCancelEvent => ConnectorAbility::CalendarChange,
            ConnectorCapability::CalendarSyncEvents => ConnectorAbility::CalendarSync,
        })
        .collect::<Vec<_>>();
    abilities.sort_by_key(|ability| *ability as u8);
    abilities.dedup();
    abilities
}

fn standard_mail_calendar_abilities() -> Vec<ConnectorAbilityDescriptor> {
    use ConnectorAbility::*;
    use ConnectorAbilityRisk::*;
    [
        (MailRead, ReadOnly),
        (MailAttachments, ReadOnly),
        (MailDraft, LocalDraft),
        (MailSend, ExternalWrite),
        (MailSync, ReadOnly),
        (CalendarRead, ReadOnly),
        (CalendarChange, ExternalWrite),
        (CalendarSync, ReadOnly),
    ]
    .into_iter()
    .map(|(ability, risk)| ConnectorAbilityDescriptor { ability, risk })
    .collect()
}

pub(crate) fn provider_label(provider_id: &str) -> ConnectorProviderLabelCode {
    match provider_id {
        "microsoft" => ConnectorProviderLabelCode::Microsoft365,
        "google" => ConnectorProviderLabelCode::GoogleWorkspace,
        _ => ConnectorProviderLabelCode::WorkspaceConnector,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::connectors::ConnectorCredentialHandle;

    #[test]
    fn production_catalog_is_metadata_only_unavailable_and_secret_free() {
        let catalog = production_connector_catalog();
        assert_eq!(catalog.len(), 2);
        assert!(catalog.iter().all(|provider| {
            provider.availability == ConnectorProviderAvailability::Unavailable
                && !provider.repair_available
                && provider
                    .abilities
                    .iter()
                    .any(|ability| ability.risk == ConnectorAbilityRisk::ExternalWrite)
        }));
        let json = serde_json::to_string(&catalog).unwrap();
        for forbidden in [
            "client_id",
            "authority",
            "scope",
            "redirect_uri",
            "credential",
            "tenant",
            "continuation",
            "provider_id",
        ] {
            assert!(!json.contains(forbidden));
        }
    }

    #[test]
    fn internal_capabilities_collapse_to_bounded_user_abilities() {
        let abilities = user_abilities(&[
            ConnectorCapability::MailSearch,
            ConnectorCapability::MailReadThread,
            ConnectorCapability::CalendarUpdateEvent,
            ConnectorCapability::CalendarCancelEvent,
        ]);
        assert_eq!(
            abilities,
            vec![ConnectorAbility::MailRead, ConnectorAbility::CalendarChange]
        );
        let json = serde_json::to_string(&abilities).unwrap();
        assert!(!json.contains("mail_search"));
        assert!(!json.contains("calendar_update_event"));
    }

    #[test]
    fn account_health_view_hides_private_authority_and_uses_committed_sync_time() {
        let now = Utc::now();
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "microsoft".to_string(),
            display_name: "Work account".to_string(),
            tenant_ref: Some("tenant-marker".to_string()),
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: vec![
                ConnectorCapability::MailSearch,
                ConnectorCapability::MailSyncInbox,
            ],
            health: ConnectorHealth::NeedsRepair,
            connected_at: now,
            updated_at: now,
        };
        let committed_at = now - chrono::Duration::minutes(2);
        let view = ConnectorAccountHealthView::from_private_account(
            &account,
            ConnectorSyncHealthSnapshot {
                stream_count: 1,
                any_stopped: false,
                any_delayed: true,
                last_successful_sync_at: Some(committed_at),
            },
            false,
        );
        assert_eq!(view.sync_state, ConnectorSyncHealthState::Delayed);
        assert_eq!(view.last_successful_sync_at, Some(committed_at));
        assert!(!view.repair_action_available);
        let json = serde_json::to_string(&view).unwrap();
        for forbidden in [
            "tenant-marker",
            "credential_handle",
            "provider_id",
            "mail_search",
            "mail_sync_inbox",
            "stream_fingerprint",
            "scope",
            "continuation",
        ] {
            assert!(!json.contains(forbidden));
        }
    }
}
