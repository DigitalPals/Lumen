//! Formatting and view-model helpers for the model usage dropdown.

use chrono::{DateTime, Local, Utc};
use lumen_model_usage::{Credits, ModelUsageErrorKind, ProviderKind, ProviderUsage};

pub(super) fn format_local_time(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%-I:%M:%S %p")
        .to_string()
}

pub(super) fn fmt_countdown(secs: i64) -> String {
    if secs <= 0 {
        return "now".to_owned();
    }
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if days > 0 {
        format!("{days}d {hours:02}h")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub(super) fn fmt_reset_abs(resets_at: DateTime<Utc>) -> String {
    let local = resets_at.with_timezone(&Local);
    if local.date_naive() == Local::now().date_naive() {
        local.format("%-I:%M %p").to_string()
    } else {
        local.format("%b %-d %-I:%M %p").to_string()
    }
}

/// "Claude Max · user@example.com · claude-oauth" style meta line.
pub(super) fn subtitle(kind: ProviderKind, usage: &ProviderUsage) -> String {
    let mut parts = Vec::new();
    if let Some(plan) = usage.plan.as_ref().filter(|value| !value.is_empty()) {
        parts.push(plan.clone());
    }
    if let Some(account) = usage.account.as_ref().filter(|value| !value.is_empty()) {
        parts.push(account.clone());
    }
    parts.push(format!("{}-oauth", kind.id()));
    parts.join(" · ")
}

/// Optional credits card as (title, value, detail).
pub(super) fn credit_card(credits: &Credits) -> Option<(String, String, String)> {
    if credits.unlimited {
        return Some((
            "Credits remaining".to_owned(),
            "Unlimited".to_owned(),
            "No credit cap reported".to_owned(),
        ));
    }
    if let (Some(used), Some(limit)) = (credits.used, credits.limit)
        && used >= 0.0
        && limit >= 0.0
    {
        let currency = credits.currency.as_deref();
        return Some((
            "Extra usage".to_owned(),
            format!(
                "{} / {}",
                format_money(used, currency),
                format_money(limit, currency)
            ),
            "Pay-as-you-go on top of the plan limits".to_owned(),
        ));
    }
    let remaining = credits.remaining?;
    if remaining >= 0.0 {
        return Some((
            "Credits remaining".to_owned(),
            if remaining.fract() == 0.0 {
                format!("{remaining:.0}")
            } else {
                format!("{remaining:.2}")
            },
            "Use credits to continue beyond your plan limits".to_owned(),
        ));
    }
    None
}

/// Title and body for the notice shown when a provider's fetch failed.
pub(super) fn unavailable_notice(
    kind: ProviderKind,
    error: &ModelUsageErrorKind,
) -> (String, String) {
    let name = kind.display_name();
    let sign_in = match kind {
        ProviderKind::Claude => "claude",
        ProviderKind::Codex => "codex login",
    };
    match error {
        ModelUsageErrorKind::CredentialsNotFound => (
            format!("{name} sign-in required"),
            format!(
                "No CLI credentials found. Run `{sign_in}` in a terminal, complete the sign-in, then press Refresh."
            ),
        ),
        ModelUsageErrorKind::TokenExpired => (
            format!("{name} session expired"),
            format!(
                "The stored access token has expired. Run `{sign_in}` in a terminal to refresh it, then press Refresh."
            ),
        ),
        ModelUsageErrorKind::RateLimited => (
            "Usage temporarily unavailable".to_owned(),
            format!(
                "The {name} usage endpoint is rate limiting requests. It recovers on its own; the next poll retries automatically."
            ),
        ),
        ModelUsageErrorKind::Network => (
            "Usage unavailable".to_owned(),
            format!(
                "Cannot reach the {name} usage endpoint. Check the network connection; the next poll retries automatically."
            ),
        ),
        ModelUsageErrorKind::Other => (
            "Usage unavailable".to_owned(),
            format!("The {name} usage endpoint returned an unexpected response."),
        ),
    }
}

fn format_money(value: f64, currency: Option<&str>) -> String {
    let amount = if value.abs() >= 1000.0 {
        format!("{:.1}K", value / 1000.0)
    } else {
        format!("{value:.2}")
    };
    match currency.unwrap_or("").to_uppercase().as_str() {
        "USD" => format!("${amount}"),
        "EUR" => format!("€{amount}"),
        "GBP" => format!("£{amount}"),
        code if !code.is_empty() => format!("{amount} {code}"),
        _ => amount,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_formats() {
        assert_eq!(fmt_countdown(-5), "now");
        assert_eq!(fmt_countdown(95), "1:35");
        assert_eq!(fmt_countdown(2 * 3600 + 16 * 60), "2h 16m");
        assert_eq!(fmt_countdown(2 * 86400 + 5 * 3600), "2d 05h");
    }

    #[test]
    fn credit_card_formats_currency_usage() {
        let credits = Credits {
            used: Some(3.5),
            limit: Some(20.0),
            remaining: Some(16.5),
            currency: Some("USD".to_owned()),
            unlimited: false,
        };
        let (title, value, _) = credit_card(&credits).unwrap();
        assert_eq!(title, "Extra usage");
        assert_eq!(value, "$3.50 / $20.00");
    }

    #[test]
    fn credit_card_uses_symbols_for_eur_and_gbp() {
        assert_eq!(format_money(0.0, Some("EUR")), "€0.00");
        assert_eq!(format_money(50.0, Some("eur")), "€50.00");
        assert_eq!(format_money(12.5, Some("GBP")), "£12.50");
        assert_eq!(format_money(7.0, Some("CHF")), "7.00 CHF");
    }

    #[test]
    fn credit_card_unlimited() {
        let credits = Credits {
            unlimited: true,
            ..Credits::default()
        };
        let (_, value, _) = credit_card(&credits).unwrap();
        assert_eq!(value, "Unlimited");
    }

    #[test]
    fn subtitle_includes_plan_account_source() {
        let usage = ProviderUsage {
            plan: Some("Claude Max".to_owned()),
            account: Some("user@example.com".to_owned()),
            windows: vec![],
            credits: None,
        };
        assert_eq!(
            subtitle(ProviderKind::Claude, &usage),
            "Claude Max · user@example.com · claude-oauth"
        );
    }
}
