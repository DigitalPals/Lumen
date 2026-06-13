//! Claude Code usage provider.
//!
//! Fetches subscription rate-limit windows from Anthropic's OAuth usage
//! endpoint using the access token Claude Code stores locally. This is the
//! same endpoint Claude Code's own `/usage` screen reads; it is not part of
//! the public API surface and may change without notice.

mod parse;
mod types;

use std::path::PathBuf;

use async_trait::async_trait;
use reqwest::StatusCode;

use super::UsageProvider;
use crate::{
    credentials,
    error::{Error, Result},
    model::{ProviderKind, ProviderUsage},
};

const PROVIDER: &str = "Claude";
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const ANTHROPIC_BETA: &str = "oauth-2025-04-20";

/// Identifying as Claude Code is required: other user agents land in an
/// aggressively rate-limited bucket on this endpoint.
const USER_AGENT: &str = "claude-code/2.1.0 (external, cli)";

/// Usage provider for Claude Code (Anthropic Pro/Max subscriptions).
pub struct ClaudeProvider {
    credentials_path: Option<PathBuf>,
}

impl ClaudeProvider {
    /// Creates a provider reading credentials from the given path, or the
    /// default Claude Code location when `None`.
    pub fn new(credentials_path: Option<PathBuf>) -> Self {
        Self { credentials_path }
    }
}

#[async_trait]
impl UsageProvider for ClaudeProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Claude
    }

    async fn fetch(&self, client: &reqwest::Client) -> Result<ProviderUsage> {
        let creds = credentials::load_claude(self.credentials_path.as_deref()).await?;

        let response = client
            .get(USAGE_URL)
            .bearer_auth(&creds.access_token)
            .header("anthropic-beta", ANTHROPIC_BETA)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
            .map_err(|err| Error::http(PROVIDER, err))?;

        match response.status() {
            StatusCode::TOO_MANY_REQUESTS => return Err(Error::RateLimited { provider: PROVIDER }),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(Error::TokenExpired { provider: PROVIDER });
            }
            status if !status.is_success() => return Err(Error::status(PROVIDER, status)),
            _ => {}
        }

        let usage: types::UsageResponse = response
            .json()
            .await
            .map_err(|err| Error::parse(PROVIDER, err.to_string()))?;

        Ok(parse::to_usage(usage, creds.plan))
    }
}
