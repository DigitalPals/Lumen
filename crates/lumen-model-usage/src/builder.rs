use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use lumen_core::Property;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

use crate::{
    model::ProviderKind,
    polling::{self, PollingConfig},
    provider::CredentialPaths,
    service::{ModelUsageService, ModelUsageStatus},
};

pub(crate) const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Lower bound for the polling interval.
///
/// The Claude usage endpoint aggressively rate-limits frequent pollers;
/// staying at or above two minutes keeps the service in the benign bucket.
pub(crate) const MIN_POLL_INTERVAL: Duration = Duration::from_secs(2 * 60);

/// Builder for configuring a [`ModelUsageService`].
pub struct ModelUsageServiceBuilder {
    poll_interval: Duration,
    providers: Vec<ProviderKind>,
    claude_credentials_path: Option<PathBuf>,
    codex_auth_path: Option<PathBuf>,
}

impl ModelUsageServiceBuilder {
    /// Creates a new builder with default configuration.
    ///
    /// Defaults to polling all supported providers every 5 minutes.
    pub fn new() -> Self {
        Self {
            poll_interval: DEFAULT_POLL_INTERVAL,
            providers: ProviderKind::all().to_vec(),
            claude_credentials_path: None,
            codex_auth_path: None,
        }
    }

    /// Sets the polling interval for usage updates.
    ///
    /// Values below 2 minutes are clamped up: the usage endpoints
    /// rate-limit aggressive pollers, which would leave the service stuck
    /// in an error state.
    pub fn poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Sets which providers to poll.
    pub fn providers(mut self, providers: Vec<ProviderKind>) -> Self {
        self.providers = providers;
        self
    }

    /// Overrides the Claude Code credential file path.
    ///
    /// Defaults to `$CLAUDE_CONFIG_DIR/.credentials.json`, falling back to
    /// `~/.claude/.credentials.json`.
    pub fn claude_credentials_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.claude_credentials_path = Some(path.into());
        self
    }

    /// Overrides the Codex CLI auth file path.
    ///
    /// Defaults to `$CODEX_HOME/auth.json`, falling back to
    /// `~/.codex/auth.json`.
    pub fn codex_auth_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.codex_auth_path = Some(path.into());
        self
    }

    /// Builds the service and starts the background polling task.
    ///
    /// Missing or expired credentials are not an error here: the poll loop
    /// reports them through the service's `status` and per-provider results,
    /// and recovers automatically once the user signs in with the CLI.
    #[instrument(skip_all, name = "ModelUsageService::build")]
    pub fn build(self) -> ModelUsageService {
        let cancellation_token = CancellationToken::new();
        let usage = Property::new(None);
        let status = Property::new(ModelUsageStatus::Loading);
        let refresh = Arc::new(Notify::new());
        let poll_interval = self.poll_interval.max(MIN_POLL_INTERVAL);
        let credential_paths = CredentialPaths {
            claude: self.claude_credentials_path,
            codex: self.codex_auth_path,
        };

        let config = PollingConfig {
            poll_interval,
            providers: self.providers.clone(),
            credential_paths: credential_paths.clone(),
            refresh: refresh.clone(),
        };

        let polling_token = cancellation_token.child_token();
        polling::spawn(polling_token.clone(), usage.clone(), status.clone(), config);

        ModelUsageService {
            cancellation_token,
            polling_token: RwLock::new(polling_token),
            poll_interval: RwLock::new(poll_interval),
            providers: RwLock::new(self.providers),
            credential_paths,
            refresh,
            usage,
            status,
        }
    }
}

impl Default for ModelUsageServiceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn poll_interval_is_clamped() {
        let service = ModelUsageService::builder()
            .poll_interval(Duration::from_secs(10))
            .providers(vec![])
            .build();
        let stored = service.poll_interval.read().map(|guard| *guard).unwrap();
        assert_eq!(stored, MIN_POLL_INTERVAL);
    }
}
