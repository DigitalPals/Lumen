use std::time::Duration;

use chrono::{DateTime, Utc};

use super::types::{RawCredits, RawRateLimit, RawWindow, ResetAt, UsageResponse};
use crate::{
    credentials::CodexCredentials,
    model::{Credits, ProviderUsage, UsageWindow, WindowKind},
};

const FIVE_HOURS_SECS: u64 = 5 * 60 * 60;
const SEVEN_DAYS_SECS: u64 = 7 * 24 * 60 * 60;

pub(crate) fn to_usage(response: UsageResponse, creds: &CodexCredentials) -> ProviderUsage {
    let mut windows = Vec::new();

    if let Some(rate_limit) = &response.rate_limit {
        push_rate_limit(&mut windows, rate_limit, None);
    }
    for additional in &response.additional_rate_limits {
        if let Some(rate_limit) = &additional.rate_limit {
            push_rate_limit(&mut windows, rate_limit, additional.limit_name.as_deref());
        }
    }

    let plan = creds
        .plan
        .clone()
        .or_else(|| response.plan_type.map(|plan| format!("ChatGPT {plan}")));
    let account = creds.email.clone().or(response.email);

    ProviderUsage {
        plan,
        account,
        windows,
        credits: response.credits.and_then(to_credits),
    }
}

fn push_rate_limit(windows: &mut Vec<UsageWindow>, rate_limit: &RawRateLimit, model: Option<&str>) {
    if let Some(raw) = &rate_limit.primary_window {
        windows.push(to_window(raw, model));
    }
    if let Some(raw) = &rate_limit.secondary_window {
        windows.push(to_window(raw, model));
    }
}

fn to_window(raw: &RawWindow, model: Option<&str>) -> UsageWindow {
    let (kind, label) = classify(raw.limit_window_seconds, model);
    UsageWindow {
        kind,
        label,
        used_percent: raw.used_percent.clamp(0.0, 100.0),
        window_duration: raw.limit_window_seconds.map(Duration::from_secs),
        resets_at: raw.reset_at.as_ref().and_then(parse_reset_at),
    }
}

fn classify(window_seconds: Option<u64>, model: Option<&str>) -> (WindowKind, String) {
    let span = match window_seconds {
        Some(secs) if secs <= FIVE_HOURS_SECS => "5 hour",
        Some(SEVEN_DAYS_SECS) => "weekly",
        Some(secs) => return span_only(model, &humanize_window(secs)),
        None => return span_only(model, "usage"),
    };
    match model {
        Some(model) => (
            WindowKind::Model(model.to_owned()),
            format!("{model} ({span})"),
        ),
        None if span == "5 hour" => (WindowKind::Session, "5 hour limit".to_owned()),
        None => (WindowKind::Weekly, "Weekly limit".to_owned()),
    }
}

fn span_only(model: Option<&str>, span: &str) -> (WindowKind, String) {
    match model {
        Some(model) => (
            WindowKind::Model(model.to_owned()),
            format!("{model} ({span})"),
        ),
        None => (WindowKind::Other, format!("{span} limit")),
    }
}

fn humanize_window(secs: u64) -> String {
    let hours = secs / 3600;
    if hours.is_multiple_of(24) && hours >= 24 {
        format!("{} day", hours / 24)
    } else {
        format!("{hours} hour")
    }
}

fn to_credits(raw: RawCredits) -> Option<Credits> {
    if !raw.has_credits && !raw.unlimited {
        return None;
    }
    Some(Credits {
        used: None,
        limit: None,
        remaining: raw.balance.as_ref().and_then(super::types::Balance::value),
        currency: None,
        unlimited: raw.unlimited,
    })
}

fn parse_reset_at(value: &ResetAt) -> Option<DateTime<Utc>> {
    match value {
        ResetAt::Epoch(secs) => DateTime::from_timestamp(*secs, 0),
        ResetAt::Iso(text) => DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|dt| dt.with_timezone(&Utc)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds() -> CodexCredentials {
        CodexCredentials {
            access_token: "tok".to_owned(),
            account_id: Some("acct".to_owned()),
            plan: Some("ChatGPT Pro".to_owned()),
            email: Some("user@example.com".to_owned()),
        }
    }

    /// Shape captured from a live response (2026-06).
    const LIVE_FIXTURE: &str = r#"{
        "user_id": "user-abc",
        "account_id": "user-abc",
        "email": "live@example.com",
        "plan_type": "pro",
        "rate_limit": {
            "allowed": true,
            "limit_reached": false,
            "primary_window": {
                "used_percent": 3,
                "limit_window_seconds": 18000,
                "reset_after_seconds": 9625,
                "reset_at": 1781296139
            },
            "secondary_window": {
                "used_percent": 75,
                "limit_window_seconds": 604800,
                "reset_after_seconds": 57948,
                "reset_at": 1781344462
            }
        },
        "code_review_rate_limit": null,
        "additional_rate_limits": [
            {
                "limit_name": "GPT-5.3-Codex-Spark",
                "metered_feature": "codex_bengalfox",
                "rate_limit": {
                    "allowed": true,
                    "limit_reached": false,
                    "primary_window": {
                        "used_percent": 0,
                        "limit_window_seconds": 18000,
                        "reset_at": 1781304514
                    },
                    "secondary_window": {
                        "used_percent": 0,
                        "limit_window_seconds": 604800,
                        "reset_at": 1781891314
                    }
                }
            }
        ],
        "credits": {
            "has_credits": false,
            "unlimited": false,
            "overage_limit_reached": false,
            "balance": "0",
            "approx_local_messages": [0, 0],
            "approx_cloud_messages": [0, 0]
        },
        "spend_control": { "reached": false, "individual_limit": null },
        "rate_limit_reached_type": null
    }"#;

    #[test]
    fn live_fixture_parses() {
        let response: UsageResponse = serde_json::from_str(LIVE_FIXTURE).unwrap();
        let usage = to_usage(response, &creds());

        assert_eq!(usage.plan.as_deref(), Some("ChatGPT Pro"));
        assert_eq!(usage.account.as_deref(), Some("user@example.com"));
        assert_eq!(usage.windows.len(), 4);
        assert_eq!(usage.windows[0].kind, WindowKind::Session);
        assert_eq!(usage.windows[0].label, "5 hour limit");
        assert_eq!(usage.windows[0].used_percent, 3.0);
        assert!(usage.windows[0].resets_at.is_some());
        assert_eq!(usage.windows[1].kind, WindowKind::Weekly);
        assert_eq!(usage.windows[1].used_percent, 75.0);
        assert_eq!(
            usage.windows[2].kind,
            WindowKind::Model("GPT-5.3-Codex-Spark".to_owned())
        );
        assert_eq!(usage.windows[2].label, "GPT-5.3-Codex-Spark (5 hour)");
        assert_eq!(usage.windows[3].label, "GPT-5.3-Codex-Spark (weekly)");
        // has_credits is false, so no credit card.
        assert!(usage.credits.is_none());
    }

    #[test]
    fn email_falls_back_to_response() {
        let response: UsageResponse = serde_json::from_str(LIVE_FIXTURE).unwrap();
        let mut anonymous = creds();
        anonymous.email = None;
        let usage = to_usage(response, &anonymous);
        assert_eq!(usage.account.as_deref(), Some("live@example.com"));
    }

    #[test]
    fn epoch_and_iso_reset_at_both_parse() {
        let epoch = parse_reset_at(&ResetAt::Epoch(1_781_300_000)).unwrap();
        let iso = parse_reset_at(&ResetAt::Iso("2026-06-12T18:30:00+02:00".to_owned())).unwrap();
        assert_eq!(epoch.timestamp(), 1_781_300_000);
        assert_eq!(iso.timestamp(), 1_781_281_800);
    }

    #[test]
    fn plan_falls_back_to_response_plan_type() {
        let response: UsageResponse = serde_json::from_str(r#"{ "plan_type": "plus" }"#).unwrap();
        let mut anonymous = creds();
        anonymous.plan = None;
        let usage = to_usage(response, &anonymous);
        assert_eq!(usage.plan.as_deref(), Some("ChatGPT plus"));
    }

    #[test]
    fn credits_with_balance_parse() {
        let response: UsageResponse = serde_json::from_str(
            r#"{ "credits": { "has_credits": true, "unlimited": false, "balance": "41.5" } }"#,
        )
        .unwrap();
        let usage = to_usage(response, &creds());
        let credits = usage.credits.unwrap();
        assert_eq!(credits.remaining, Some(41.5));
        assert!(!credits.unlimited);
    }

    #[test]
    fn no_credits_means_no_card() {
        let response: UsageResponse = serde_json::from_str(
            r#"{ "credits": { "has_credits": false, "unlimited": false, "balance": "0" } }"#,
        )
        .unwrap();
        let usage = to_usage(response, &creds());
        assert!(usage.credits.is_none());
    }
}
