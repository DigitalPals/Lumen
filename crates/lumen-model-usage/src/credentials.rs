//! Read-only access to the credential files written by provider CLIs.
//!
//! Tokens are never refreshed here: refresh tokens rotate on use, so a
//! background service refreshing them would race the CLI and invalidate its
//! session. Expired tokens surface as [`Error::TokenExpired`] and recover
//! automatically once the user runs the CLI again (files are re-read on
//! every poll cycle).

use std::path::{Path, PathBuf};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use serde::Deserialize;

use crate::error::{Error, Result};

const CLAUDE: &str = "Claude";
const CODEX: &str = "Codex";

/// Credentials extracted from Claude Code's local OAuth store.
#[derive(Debug, Clone)]
pub(crate) struct ClaudeCredentials {
    pub access_token: String,
    pub plan: Option<String>,
}

/// Credentials extracted from Codex CLI's local auth store.
#[derive(Debug, Clone)]
pub(crate) struct CodexCredentials {
    pub access_token: String,
    pub account_id: Option<String>,
    pub plan: Option<String>,
    pub email: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeCredentialFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: ClaudeOauth,
}

#[derive(Deserialize)]
struct ClaudeOauth {
    #[serde(rename = "accessToken")]
    access_token: String,
    /// Expiry as epoch milliseconds.
    #[serde(rename = "expiresAt")]
    expires_at: Option<i64>,
    #[serde(rename = "subscriptionType")]
    subscription_type: Option<String>,
}

#[derive(Deserialize)]
struct CodexAuthFile {
    tokens: Option<CodexTokens>,
}

#[derive(Deserialize)]
struct CodexTokens {
    access_token: String,
    account_id: Option<String>,
    id_token: Option<String>,
}

/// Resolves the Claude Code credential file path.
///
/// Order: explicit override, `$CLAUDE_CONFIG_DIR/.credentials.json`,
/// `~/.claude/.credentials.json`.
pub(crate) fn claude_credentials_path(path_override: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = path_override {
        return Some(path.to_path_buf());
    }
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(dir).join(".credentials.json"));
    }
    std::env::home_dir().map(|home| home.join(".claude").join(".credentials.json"))
}

/// Resolves the Codex CLI auth file path.
///
/// Order: explicit override, `$CODEX_HOME/auth.json`, `~/.codex/auth.json`.
pub(crate) fn codex_auth_path(path_override: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = path_override {
        return Some(path.to_path_buf());
    }
    if let Ok(dir) = std::env::var("CODEX_HOME") {
        return Some(PathBuf::from(dir).join("auth.json"));
    }
    std::env::home_dir().map(|home| home.join(".codex").join("auth.json"))
}

/// Loads and validates Claude Code credentials.
pub(crate) async fn load_claude(path_override: Option<&Path>) -> Result<ClaudeCredentials> {
    let path = claude_credentials_path(path_override).ok_or(Error::CredentialsNotFound {
        provider: CLAUDE,
        path: "~/.claude/.credentials.json".to_owned(),
    })?;
    let contents = read_credential_file(CLAUDE, &path).await?;
    parse_claude(&contents, Utc::now().timestamp_millis())
}

pub(crate) fn parse_claude(contents: &str, now_ms: i64) -> Result<ClaudeCredentials> {
    let file: ClaudeCredentialFile = serde_json::from_str(contents)
        .map_err(|err| Error::credentials_invalid(CLAUDE, err.to_string()))?;
    let oauth = file.claude_ai_oauth;

    if let Some(expires_at) = oauth.expires_at
        && expires_at <= now_ms
    {
        return Err(Error::TokenExpired { provider: CLAUDE });
    }

    Ok(ClaudeCredentials {
        access_token: oauth.access_token,
        plan: oauth.subscription_type.as_deref().map(claude_plan_label),
    })
}

/// Loads Codex CLI credentials, extracting plan and email from the ID token.
pub(crate) async fn load_codex(path_override: Option<&Path>) -> Result<CodexCredentials> {
    let path = codex_auth_path(path_override).ok_or(Error::CredentialsNotFound {
        provider: CODEX,
        path: "~/.codex/auth.json".to_owned(),
    })?;
    let contents = read_credential_file(CODEX, &path).await?;
    parse_codex(&contents)
}

pub(crate) fn parse_codex(contents: &str) -> Result<CodexCredentials> {
    let file: CodexAuthFile = serde_json::from_str(contents)
        .map_err(|err| Error::credentials_invalid(CODEX, err.to_string()))?;
    let tokens = file.tokens.ok_or_else(|| {
        Error::credentials_invalid(CODEX, "no ChatGPT tokens (API-key auth is not supported)")
    })?;

    let claims = tokens
        .id_token
        .as_deref()
        .and_then(decode_jwt_claims)
        .unwrap_or(serde_json::Value::Null);
    let plan = claims
        .pointer("/https:~1~1api.openai.com~1auth/chatgpt_plan_type")
        .and_then(serde_json::Value::as_str)
        .map(codex_plan_label);
    let email = claims
        .get("email")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);

    Ok(CodexCredentials {
        access_token: tokens.access_token,
        account_id: tokens.account_id,
        plan,
        email,
    })
}

async fn read_credential_file(provider: &'static str, path: &Path) -> Result<String> {
    if !path.is_file() {
        return Err(Error::CredentialsNotFound {
            provider,
            path: path.display().to_string(),
        });
    }
    tokio::fs::read_to_string(path)
        .await
        .map_err(|err| Error::credentials_invalid(provider, err.to_string()))
}

/// Decodes the payload of a JWT without verifying its signature.
///
/// Verification is unnecessary here: the token comes from a local file the
/// user controls and is only mined for display metadata (plan, email).
fn decode_jwt_claims(token: &str) -> Option<serde_json::Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn claude_plan_label(subscription_type: &str) -> String {
    match subscription_type {
        "pro" => "Claude Pro".to_owned(),
        "max" => "Claude Max".to_owned(),
        "team" => "Claude Team".to_owned(),
        "enterprise" => "Claude Enterprise".to_owned(),
        other => other.to_owned(),
    }
}

fn codex_plan_label(plan_type: &str) -> String {
    match plan_type {
        "plus" => "ChatGPT Plus".to_owned(),
        "pro" => "ChatGPT Pro".to_owned(),
        "team" => "ChatGPT Team".to_owned(),
        "business" => "ChatGPT Business".to_owned(),
        "enterprise" => "ChatGPT Enterprise".to_owned(),
        "free" => "ChatGPT Free".to_owned(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_jwt(claims: &serde_json::Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(claims.to_string().as_bytes());
        format!("{header}.{payload}.signature")
    }

    #[test]
    fn parse_claude_valid_token() {
        let contents = r#"{
            "claudeAiOauth": {
                "accessToken": "tok-123",
                "refreshToken": "ref-456",
                "expiresAt": 2000,
                "scopes": ["user:inference"],
                "subscriptionType": "max"
            }
        }"#;
        let creds = parse_claude(contents, 1000).unwrap();
        assert_eq!(creds.access_token, "tok-123");
        assert_eq!(creds.plan.as_deref(), Some("Claude Max"));
    }

    #[test]
    fn parse_claude_expired_token() {
        let contents = r#"{"claudeAiOauth": {"accessToken": "tok", "expiresAt": 500}}"#;
        let err = parse_claude(contents, 1000).unwrap_err();
        assert!(matches!(err, Error::TokenExpired { .. }));
    }

    #[test]
    fn parse_claude_no_expiry_is_valid() {
        let contents = r#"{"claudeAiOauth": {"accessToken": "tok"}}"#;
        let creds = parse_claude(contents, 1000).unwrap();
        assert_eq!(creds.access_token, "tok");
        assert_eq!(creds.plan, None);
    }

    #[test]
    fn parse_claude_malformed_json() {
        let err = parse_claude("not json", 0).unwrap_err();
        assert!(matches!(err, Error::CredentialsInvalid { .. }));
    }

    #[test]
    fn parse_codex_extracts_plan_and_email() {
        let claims = serde_json::json!({
            "email": "user@example.com",
            "https://api.openai.com/auth": { "chatgpt_plan_type": "pro" }
        });
        let contents = serde_json::json!({
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": make_jwt(&claims),
                "access_token": "acc-1",
                "refresh_token": "ref-1",
                "account_id": "acct-uuid"
            },
            "last_refresh": "2026-01-28T08:05:37Z"
        })
        .to_string();

        let creds = parse_codex(&contents).unwrap();
        assert_eq!(creds.access_token, "acc-1");
        assert_eq!(creds.account_id.as_deref(), Some("acct-uuid"));
        assert_eq!(creds.plan.as_deref(), Some("ChatGPT Pro"));
        assert_eq!(creds.email.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn parse_codex_api_key_only_is_invalid() {
        let contents = r#"{"OPENAI_API_KEY": "sk-123"}"#;
        let err = parse_codex(contents).unwrap_err();
        assert!(matches!(err, Error::CredentialsInvalid { .. }));
    }

    #[test]
    fn parse_codex_malformed_id_token_still_works() {
        let contents = serde_json::json!({
            "tokens": {
                "id_token": "garbage",
                "access_token": "acc-1",
                "account_id": "acct"
            }
        })
        .to_string();
        let creds = parse_codex(&contents).unwrap();
        assert_eq!(creds.plan, None);
        assert_eq!(creds.email, None);
    }
}
