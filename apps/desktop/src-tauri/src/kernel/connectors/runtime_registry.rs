use std::sync::Arc;

use super::oauth::ConnectorOAuthProvider;
use super::provider::{CalendarConnectorProvider, MailConnectorProvider};
use super::reconciliation::{ConnectorReconcilerRegistry, EmptyConnectorReconcilerRegistry};
use super::revocation::{ConnectorRevocationProvider, ConnectorRevocationRegistry};
use super::sync::{CalendarSyncProvider, MailSyncProvider};
use super::{ConnectorCapability, ConnectorMutationProvider};

pub(crate) trait ConnectorOAuthRegistry: Send + Sync {
    fn provider(&self, provider_key: &str) -> Option<&dyn ConnectorOAuthProvider>;
}

pub(crate) trait ConnectorReadRegistry: Send + Sync {
    fn mail_provider(&self, provider_key: &str) -> Option<&dyn MailConnectorProvider>;
    fn calendar_provider(&self, provider_key: &str) -> Option<&dyn CalendarConnectorProvider>;

    fn execution_enabled(&self) -> bool {
        false
    }
}

pub(crate) trait ConnectorSyncRegistry: Send + Sync {
    fn mail_provider(&self, provider_key: &str) -> Option<&dyn MailSyncProvider>;
    fn calendar_provider(&self, provider_key: &str) -> Option<&dyn CalendarSyncProvider>;

    fn execution_enabled(&self) -> bool {
        false
    }
}

pub(crate) trait ConnectorMutationRegistry: Send + Sync {
    fn provider(&self, provider_key: &str) -> Option<&dyn ConnectorMutationProvider>;

    fn execution_enabled(&self) -> bool {
        false
    }

    fn supports(&self, provider_key: &str, capability: ConnectorCapability) -> bool {
        self.provider(provider_key).is_some_and(|provider| {
            provider.provider_id() == provider_key && provider.capabilities().contains(&capability)
        })
    }
}

#[derive(Default)]
struct EmptyConnectorOAuthRegistry;

impl ConnectorOAuthRegistry for EmptyConnectorOAuthRegistry {
    fn provider(&self, _provider_key: &str) -> Option<&dyn ConnectorOAuthProvider> {
        None
    }
}

#[derive(Default)]
struct EmptyConnectorReadRegistry;

impl ConnectorReadRegistry for EmptyConnectorReadRegistry {
    fn mail_provider(&self, _provider_key: &str) -> Option<&dyn MailConnectorProvider> {
        None
    }

    fn calendar_provider(&self, _provider_key: &str) -> Option<&dyn CalendarConnectorProvider> {
        None
    }
}

#[derive(Default)]
struct EmptyConnectorSyncRegistry;

impl ConnectorSyncRegistry for EmptyConnectorSyncRegistry {
    fn mail_provider(&self, _provider_key: &str) -> Option<&dyn MailSyncProvider> {
        None
    }

    fn calendar_provider(&self, _provider_key: &str) -> Option<&dyn CalendarSyncProvider> {
        None
    }
}

#[derive(Default)]
struct EmptyConnectorMutationRegistry;

impl ConnectorMutationRegistry for EmptyConnectorMutationRegistry {
    fn provider(&self, _provider_key: &str) -> Option<&dyn ConnectorMutationProvider> {
        None
    }
}

#[derive(Default)]
struct EmptyConnectorRevocationRegistry;

impl ConnectorRevocationRegistry for EmptyConnectorRevocationRegistry {
    fn provider(&self, _provider_id: &str) -> Option<&dyn ConnectorRevocationProvider> {
        None
    }
}

pub(crate) struct ConnectorRuntimeRegistries {
    oauth: Arc<dyn ConnectorOAuthRegistry>,
    reads: Arc<dyn ConnectorReadRegistry>,
    syncs: Arc<dyn ConnectorSyncRegistry>,
    mutations: Arc<dyn ConnectorMutationRegistry>,
    reconcilers: Arc<dyn ConnectorReconcilerRegistry>,
    revocations: Arc<dyn ConnectorRevocationRegistry>,
}

impl ConnectorRuntimeRegistries {
    pub(crate) fn empty() -> Self {
        Self {
            oauth: Arc::new(EmptyConnectorOAuthRegistry),
            reads: Arc::new(EmptyConnectorReadRegistry),
            syncs: Arc::new(EmptyConnectorSyncRegistry),
            mutations: Arc::new(EmptyConnectorMutationRegistry),
            reconcilers: Arc::new(EmptyConnectorReconcilerRegistry),
            revocations: Arc::new(EmptyConnectorRevocationRegistry),
        }
    }

    pub(crate) fn reconcilers(&self) -> Arc<dyn ConnectorReconcilerRegistry> {
        Arc::clone(&self.reconcilers)
    }

    pub(crate) fn reads(&self) -> Arc<dyn ConnectorReadRegistry> {
        Arc::clone(&self.reads)
    }

    pub(crate) fn syncs(&self) -> Arc<dyn ConnectorSyncRegistry> {
        Arc::clone(&self.syncs)
    }

    pub(crate) fn oauth(&self) -> Arc<dyn ConnectorOAuthRegistry> {
        Arc::clone(&self.oauth)
    }

    pub(crate) fn mutations(&self) -> Arc<dyn ConnectorMutationRegistry> {
        Arc::clone(&self.mutations)
    }

    #[cfg(test)]
    pub(crate) fn with_reconciler_for_test(
        reconcilers: Arc<dyn ConnectorReconcilerRegistry>,
    ) -> Self {
        Self {
            oauth: Arc::new(EmptyConnectorOAuthRegistry),
            reads: Arc::new(EmptyConnectorReadRegistry),
            syncs: Arc::new(EmptyConnectorSyncRegistry),
            mutations: Arc::new(EmptyConnectorMutationRegistry),
            reconcilers,
            revocations: Arc::new(EmptyConnectorRevocationRegistry),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_oauth_for_test(oauth: Arc<dyn ConnectorOAuthRegistry>) -> Self {
        Self {
            oauth,
            reads: Arc::new(EmptyConnectorReadRegistry),
            syncs: Arc::new(EmptyConnectorSyncRegistry),
            mutations: Arc::new(EmptyConnectorMutationRegistry),
            reconcilers: Arc::new(EmptyConnectorReconcilerRegistry),
            revocations: Arc::new(EmptyConnectorRevocationRegistry),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_sync_for_test(syncs: Arc<dyn ConnectorSyncRegistry>) -> Self {
        Self {
            oauth: Arc::new(EmptyConnectorOAuthRegistry),
            reads: Arc::new(EmptyConnectorReadRegistry),
            syncs,
            mutations: Arc::new(EmptyConnectorMutationRegistry),
            reconcilers: Arc::new(EmptyConnectorReconcilerRegistry),
            revocations: Arc::new(EmptyConnectorRevocationRegistry),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_read_for_test(reads: Arc<dyn ConnectorReadRegistry>) -> Self {
        Self {
            oauth: Arc::new(EmptyConnectorOAuthRegistry),
            reads,
            syncs: Arc::new(EmptyConnectorSyncRegistry),
            mutations: Arc::new(EmptyConnectorMutationRegistry),
            reconcilers: Arc::new(EmptyConnectorReconcilerRegistry),
            revocations: Arc::new(EmptyConnectorRevocationRegistry),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_mutation_for_test(mutations: Arc<dyn ConnectorMutationRegistry>) -> Self {
        Self {
            oauth: Arc::new(EmptyConnectorOAuthRegistry),
            reads: Arc::new(EmptyConnectorReadRegistry),
            syncs: Arc::new(EmptyConnectorSyncRegistry),
            mutations,
            reconcilers: Arc::new(EmptyConnectorReconcilerRegistry),
            revocations: Arc::new(EmptyConnectorRevocationRegistry),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_registry_set_has_no_execution_authority() {
        let registries = ConnectorRuntimeRegistries::empty();

        assert!(registries.oauth.provider("microsoft").is_none());
        assert!(registries.oauth.provider("google").is_none());
        assert!(registries.reads.mail_provider("microsoft").is_none());
        assert!(registries.reads.calendar_provider("google").is_none());
        assert!(!registries.reads.execution_enabled());
        assert!(registries.syncs.mail_provider("microsoft").is_none());
        assert!(registries.syncs.calendar_provider("google").is_none());
        assert!(!registries.syncs.execution_enabled());
        assert!(registries.mutations.provider("microsoft").is_none());
        assert!(registries.mutations.provider("google").is_none());
        assert!(!registries.mutations.execution_enabled());
        assert!(registries.reconcilers.reconciler("microsoft").is_none());
        assert!(registries.revocations.provider("google").is_none());
    }
}
