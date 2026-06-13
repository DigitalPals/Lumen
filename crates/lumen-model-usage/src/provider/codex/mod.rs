//! Codex CLI usage provider.
//!
//! Fetches subscription rate-limit windows from the ChatGPT backend using
//! the tokens Codex CLI stores locally. This mirrors what `codex` itself
//! polls for its status display; the endpoint is internal and may change
//! without notice (its shape is observable in the open-source codex client).

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

const PROVIDER: &str = "Codex";
const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const ACCOUNT_ID_HEADER: &str = "ChatGPT-Account-Id";
const USER_AGENT: &str = "codex-cli";

/// Usage provider for Codex CLI (ChatGPT Plus/Pro/Team subscriptions).
pub struct CodexProvider {
    auth_path: Option<PathBuf>,
}

impl CodexProvider {
    /// Creates a provider reading credentials from the given path, or the
    /// default Codex CLI location when `None`.
    pub fn new(auth_path: Option<PathBuf>) -> Self {
        Self { auth_path }
    }
}

#[async_trait]
impl UsageProvider for CodexProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Codex
    }

    async fn fetch(&self, client: &reqwest::Client) -> Result<ProviderUsage> {
        let creds = credentials::load_codex(self.auth_path.as_deref()).await?;

        let mut request = client
            .get(USAGE_URL)
            .bearer_auth(&creds.access_token)
            .header(reqwest::header::USER_AGENT, USER_AGENT);
        if let Some(account_id) = &creds.account_id {
            request = request.header(ACCOUNT_ID_HEADER, account_id);
        }

        let response = request
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

        Ok(parse::to_usage(usage, &creds))
    }
}
