use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use lumen_core::Property;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::{
    builder::ModelUsageServiceBuilder,
    error::Error,
    model::{ProviderKind, UsageSnapshot},
    polling::{self, PollingConfig},
    provider::CredentialPaths,
};

/// Categorized error for UI display without implementation details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelUsageErrorKind {
    /// No credentials found — the user hasn't signed in with the CLI.
    CredentialsNotFound,
    /// Stored access token expired; running the CLI refreshes it.
    TokenExpired,
    /// Network-level failure (DNS, timeout, connection refused).
    Network,
    /// Usage endpoint rate limit exceeded.
    RateLimited,
    /// Anything else (parse errors, unexpected status codes, etc.).
    Other,
}

impl From<&Error> for ModelUsageErrorKind {
    fn from(err: &Error) -> Self {
        match err {
            Error::CredentialsNotFound { .. } | Error::CredentialsInvalid { .. } => {
                Self::CredentialsNotFound
            }
            Error::TokenExpired { .. } => Self::TokenExpired,
            Error::Http { .. } => Self::Network,
            Error::RateLimited { .. } => Self::RateLimited,
            Error::ProviderStatus { .. } | Error::Parse { .. } => Self::Other,
        }
    }
}

/// Fetch lifecycle state exposed to UI consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelUsageStatus {
    /// Initial state or a manual refresh in progress.
    Loading,
    /// The latest poll produced usage data for at least one provider.
    Loaded,
    /// The latest poll failed for every enabled provider.
    Error(ModelUsageErrorKind),
}

/// Subscription usage monitoring service for AI coding agents.
///
/// Polls the usage endpoints of locally signed-in agent CLIs (Claude Code,
/// Codex CLI) and exposes the results through reactive properties. Reads
/// each CLI's stored credentials before every poll; it never writes them
/// and never refreshes tokens (that's the CLI's job — see crate docs).
///
/// Configuration can be changed at runtime:
/// - [`set_poll_interval`](Self::set_poll_interval) - Change polling frequency
/// - [`set_providers`](Self::set_providers) - Change which providers are polled
/// - [`refresh`](Self::refresh) - Trigger an immediate poll
#[derive(Debug)]
pub struct ModelUsageService {
    pub(crate) cancellation_token: CancellationToken,
    pub(crate) polling_token: RwLock<CancellationToken>,
    pub(crate) poll_interval: RwLock<Duration>,
    pub(crate) providers: RwLock<Vec<ProviderKind>>,
    pub(crate) credential_paths: CredentialPaths,
    pub(crate) refresh: Arc<Notify>,

    /// Latest usage snapshot. `None` until the first poll completes.
    ///
    /// Snapshots carry per-provider results, so partial failures are
    /// observable even while `status` reports [`ModelUsageStatus::Loaded`].
    pub usage: Property<Option<Arc<UsageSnapshot>>>,

    /// Current fetch lifecycle state.
    pub status: Property<ModelUsageStatus>,
}

impl ModelUsageService {
    /// Returns a builder for configuring the service.
    pub fn builder() -> ModelUsageServiceBuilder {
        ModelUsageServiceBuilder::new()
    }

    /// Updates the polling interval.
    ///
    /// Clamped to the same minimum as the builder (see
    /// [`ModelUsageServiceBuilder::poll_interval`]).
    pub fn set_poll_interval(&self, interval: Duration) {
        let interval = interval.max(crate::builder::MIN_POLL_INTERVAL);
        debug!(?interval, "Updating model usage polling interval");
        if let Ok(mut guard) = self.poll_interval.write() {
            *guard = interval;
        }
        self.restart_polling();
    }

    /// Updates which providers are polled.
    pub fn set_providers(&self, providers: Vec<ProviderKind>) {
        debug!(?providers, "Updating model usage providers");
        if let Ok(mut guard) = self.providers.write() {
            *guard = providers;
        }
        self.restart_polling();
    }

    /// Triggers an immediate poll without waiting for the next interval.
    ///
    /// Concurrent calls coalesce into a single poll.
    pub fn refresh(&self) {
        self.refresh.notify_one();
    }

    fn restart_polling(&self) {
        self.status.set(ModelUsageStatus::Loading);

        let config = PollingConfig {
            poll_interval: self
                .poll_interval
                .read()
                .map(|guard| *guard)
                .unwrap_or(crate::builder::DEFAULT_POLL_INTERVAL),
            providers: self
                .providers
                .read()
                .map(|guard| guard.clone())
                .unwrap_or_else(|_| ProviderKind::all().to_vec()),
            credential_paths: self.credential_paths.clone(),
            refresh: self.refresh.clone(),
        };

        let new_token = self.cancellation_token.child_token();
        if let Ok(mut guard) = self.polling_token.write() {
            guard.cancel();
            polling::spawn(
                new_token.clone(),
                self.usage.clone(),
                self.status.clone(),
                config,
            );
            *guard = new_token;
        }
    }
}

impl Drop for ModelUsageService {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
    }
}
