//! Usage data provider implementations.

mod claude;
mod codex;

use std::path::PathBuf;

use async_trait::async_trait;
pub use claude::ClaudeProvider;
pub use codex::CodexProvider;

use crate::{
    error::Result,
    model::{ProviderKind, ProviderUsage},
};

/// Trait for AI agent usage providers.
///
/// Each implementation reads the credentials its CLI stores locally and
/// fetches that provider's rate-limit windows, normalizing them into the
/// common [`ProviderUsage`] model.
#[async_trait]
pub trait UsageProvider: Send + Sync {
    /// Returns the provider kind.
    fn kind(&self) -> ProviderKind;

    /// Fetches current usage for this provider.
    ///
    /// # Errors
    ///
    /// Returns an error when credentials are missing or expired, on network
    /// failure, or when the provider's response cannot be parsed.
    async fn fetch(&self, client: &reqwest::Client) -> Result<ProviderUsage>;
}

/// Credential file path overrides for provider construction.
#[derive(Debug, Clone, Default)]
pub(crate) struct CredentialPaths {
    pub claude: Option<PathBuf>,
    pub codex: Option<PathBuf>,
}

/// Creates the provider implementation for the given kind.
pub(crate) fn create_provider(
    kind: ProviderKind,
    paths: &CredentialPaths,
) -> Box<dyn UsageProvider> {
    match kind {
        ProviderKind::Claude => Box::new(ClaudeProvider::new(paths.claude.clone())),
        ProviderKind::Codex => Box::new(CodexProvider::new(paths.codex.clone())),
    }
}
