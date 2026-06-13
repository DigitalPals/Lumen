//! Raw wire types for the Claude Code OAuth usage endpoint.

use serde::Deserialize;

/// Response from `GET /api/oauth/usage`.
#[derive(Debug, Deserialize)]
pub(crate) struct UsageResponse {
    pub five_hour: Option<RawWindow>,
    pub seven_day: Option<RawWindow>,
    pub seven_day_opus: Option<RawWindow>,
    pub seven_day_sonnet: Option<RawWindow>,
    pub extra_usage: Option<RawExtraUsage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawWindow {
    /// Percentage of the window's quota used, `0..=100`.
    pub utilization: f64,
    /// ISO 8601 timestamp of the window reset.
    pub resets_at: Option<String>,
}

/// Pay-as-you-go credit usage on top of the subscription.
#[derive(Debug, Deserialize)]
pub(crate) struct RawExtraUsage {
    #[serde(default)]
    pub is_enabled: bool,
    pub used_credits: Option<f64>,
    pub monthly_limit: Option<f64>,
    pub currency: Option<String>,
}
