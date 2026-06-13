//! Raw wire types for the Codex CLI usage endpoint.

use serde::Deserialize;

/// Response from `GET /backend-api/wham/usage`.
#[derive(Debug, Deserialize)]
pub(crate) struct UsageResponse {
    pub email: Option<String>,
    pub plan_type: Option<String>,
    pub rate_limit: Option<RawRateLimit>,
    #[serde(default)]
    pub additional_rate_limits: Vec<RawAdditionalLimit>,
    pub credits: Option<RawCredits>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawRateLimit {
    pub primary_window: Option<RawWindow>,
    pub secondary_window: Option<RawWindow>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawWindow {
    /// Percentage of the window's quota used, `0..=100`.
    pub used_percent: f64,
    /// Length of the rolling window in seconds.
    pub limit_window_seconds: Option<u64>,
    /// Window reset time; observed as epoch seconds, kept lenient for ISO 8601.
    pub reset_at: Option<ResetAt>,
}

/// Reset timestamp in either wire format.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ResetAt {
    Epoch(i64),
    Iso(String),
}

/// Model-scoped limits (e.g. per-model session/weekly caps).
#[derive(Debug, Deserialize)]
pub(crate) struct RawAdditionalLimit {
    /// Display name of the limited model (e.g. "GPT-5.3-Codex-Spark").
    pub limit_name: Option<String>,
    pub rate_limit: Option<RawRateLimit>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawCredits {
    #[serde(default)]
    pub has_credits: bool,
    #[serde(default)]
    pub unlimited: bool,
    /// Observed as a string ("0"); kept lenient for a plain number.
    pub balance: Option<Balance>,
}

/// Credit balance in either wire format.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Balance {
    Number(f64),
    Text(String),
}

impl Balance {
    pub(crate) fn value(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Text(text) => text.trim().parse().ok(),
        }
    }
}
