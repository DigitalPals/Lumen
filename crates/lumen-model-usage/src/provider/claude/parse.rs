use std::time::Duration;

use chrono::{DateTime, Utc};

use super::types::{RawExtraUsage, RawWindow, UsageResponse};
use crate::model::{Credits, ProviderUsage, UsageWindow, WindowKind};

const FIVE_HOURS: Duration = Duration::from_secs(5 * 60 * 60);
const SEVEN_DAYS: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub(crate) fn to_usage(response: UsageResponse, plan: Option<String>) -> ProviderUsage {
    let mut windows = Vec::new();
    push_window(
        &mut windows,
        response.five_hour,
        WindowKind::Session,
        "5 hour limit",
        FIVE_HOURS,
    );
    push_window(
        &mut windows,
        response.seven_day,
        WindowKind::Weekly,
        "Weekly limit",
        SEVEN_DAYS,
    );
    push_window(
        &mut windows,
        response.seven_day_opus,
        WindowKind::Model("opus".to_owned()),
        "Weekly (Opus)",
        SEVEN_DAYS,
    );
    push_window(
        &mut windows,
        response.seven_day_sonnet,
        WindowKind::Model("sonnet".to_owned()),
        "Weekly (Sonnet)",
        SEVEN_DAYS,
    );

    ProviderUsage {
        plan,
        account: None,
        windows,
        credits: response.extra_usage.and_then(to_credits),
    }
}

fn push_window(
    windows: &mut Vec<UsageWindow>,
    raw: Option<RawWindow>,
    kind: WindowKind,
    label: &str,
    duration: Duration,
) {
    let Some(raw) = raw else {
        return;
    };
    windows.push(UsageWindow {
        kind,
        label: label.to_owned(),
        used_percent: raw.utilization.clamp(0.0, 100.0),
        window_duration: Some(duration),
        resets_at: raw.resets_at.as_deref().and_then(parse_timestamp),
    });
}

fn to_credits(extra: RawExtraUsage) -> Option<Credits> {
    if !extra.is_enabled {
        return None;
    }
    // The endpoint reports monetary amounts in cents (observed live:
    // a €42.50 extra-usage cap arrives as monthly_limit = 4250).
    let used = extra.used_credits.map(|value| value / 100.0);
    let limit = extra.monthly_limit.map(|value| value / 100.0);
    let remaining = match (limit, used) {
        (Some(limit), Some(used)) => Some((limit - used).max(0.0)),
        _ => None,
    };
    Some(Credits {
        used,
        limit,
        remaining,
        currency: extra.currency,
        unlimited: false,
    })
}

fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_response_maps_all_windows() {
        let response: UsageResponse = serde_json::from_str(
            r#"{
                "five_hour": { "utilization": 42.0, "resets_at": "2026-06-12T18:00:00Z" },
                "seven_day": { "utilization": 13.5, "resets_at": "2026-06-15T09:00:00Z" },
                "seven_day_opus": { "utilization": 5.0, "resets_at": null },
                "seven_day_sonnet": { "utilization": 9.0, "resets_at": null },
                "extra_usage": {
                    "is_enabled": true,
                    "used_credits": 350,
                    "monthly_limit": 2000,
                    "currency": "USD"
                }
            }"#,
        )
        .unwrap();

        let usage = to_usage(response, Some("Claude Max".to_owned()));
        assert_eq!(usage.windows.len(), 4);
        assert_eq!(usage.windows[0].label, "5 hour limit");
        assert_eq!(usage.windows[0].used_percent, 42.0);
        assert!(usage.windows[0].resets_at.is_some());
        assert_eq!(usage.windows[1].kind, WindowKind::Weekly);
        assert_eq!(usage.windows[2].kind, WindowKind::Model("opus".to_owned()));

        let credits = usage.credits.unwrap();
        assert_eq!(credits.used, Some(3.5));
        assert_eq!(credits.remaining, Some(16.5));
        assert_eq!(credits.currency.as_deref(), Some("USD"));
    }

    #[test]
    fn absent_windows_are_skipped() {
        let response: UsageResponse =
            serde_json::from_str(r#"{ "five_hour": { "utilization": 80.0 } }"#).unwrap();
        let usage = to_usage(response, None);
        assert_eq!(usage.windows.len(), 1);
        assert_eq!(usage.windows[0].kind, WindowKind::Session);
    }

    #[test]
    fn disabled_extra_usage_is_no_credits() {
        let response: UsageResponse = serde_json::from_str(
            r#"{ "extra_usage": { "is_enabled": false, "used_credits": 1.0 } }"#,
        )
        .unwrap();
        let usage = to_usage(response, None);
        assert!(usage.credits.is_none());
    }

    #[test]
    fn utilization_is_clamped() {
        let response: UsageResponse =
            serde_json::from_str(r#"{ "five_hour": { "utilization": 250.0 } }"#).unwrap();
        let usage = to_usage(response, None);
        assert_eq!(usage.windows[0].used_percent, 100.0);
    }
}
