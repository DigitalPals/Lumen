use chrono::{DateTime, Utc};
use std::time::Duration;

use crate::service::ModelUsageErrorKind;

/// Supported AI coding agent providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    /// Claude Code (Anthropic Pro/Max subscriptions).
    Claude,
    /// Codex CLI (ChatGPT Plus/Pro/Team subscriptions).
    Codex,
}

impl ProviderKind {
    /// Stable lowercase identifier (e.g. for CSS classes or config keys).
    pub fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// Human-readable display name.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }

    /// All supported providers, in default display order.
    pub fn all() -> [Self; 2] {
        [Self::Claude, Self::Codex]
    }
}

/// Categorization of a usage window's time span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowKind {
    /// Short rolling window (e.g. the 5-hour session limit).
    Session,
    /// Weekly rolling window covering all models.
    Weekly,
    /// Weekly rolling window scoped to a specific model family.
    Model(String),
    /// Any other window the provider reports.
    Other,
}

/// A single rate-limit window reported by a provider.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageWindow {
    /// Categorization of this window's time span.
    pub kind: WindowKind,
    /// Human-readable label (e.g. "5 hour limit", "Weekly limit").
    pub label: String,
    /// Percentage of the window's quota already used, clamped to `0..=100`.
    pub used_percent: f64,
    /// Length of the rolling window, when reported.
    pub window_duration: Option<Duration>,
    /// When the window resets, when reported.
    pub resets_at: Option<DateTime<Utc>>,
}

impl UsageWindow {
    /// Percentage of the window's quota still available, clamped to `0..=100`.
    pub fn remaining_percent(&self) -> f64 {
        (100.0 - self.used_percent).clamp(0.0, 100.0)
    }
}

/// Pay-as-you-go credit balance attached to a subscription, when reported.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Credits {
    /// Credits consumed in the current period.
    pub used: Option<f64>,
    /// Credit limit for the current period.
    pub limit: Option<f64>,
    /// Credits still available.
    pub remaining: Option<f64>,
    /// ISO 4217 currency code (e.g. "USD"), when credits are monetary.
    pub currency: Option<String>,
    /// Whether the account has unlimited credits.
    pub unlimited: bool,
}

/// Usage data successfully fetched for one provider.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderUsage {
    /// Subscription plan display name (e.g. "Claude Max", "ChatGPT Plus").
    pub plan: Option<String>,
    /// Account identifier (e.g. email address), when available.
    pub account: Option<String>,
    /// Rate-limit windows, most constrained windows first.
    pub windows: Vec<UsageWindow>,
    /// Credit balance, when the provider reports one.
    pub credits: Option<Credits>,
}

impl ProviderUsage {
    /// The lowest remaining percentage across all windows.
    ///
    /// This is the number to surface at a glance: the window closest to
    /// exhaustion. `None` when the provider reported no windows.
    pub fn min_remaining_percent(&self) -> Option<f64> {
        self.windows
            .iter()
            .map(UsageWindow::remaining_percent)
            .min_by(f64::total_cmp)
    }
}

/// Per-provider fetch outcome within a snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderEntry {
    /// Which provider this entry describes.
    pub kind: ProviderKind,
    /// Usage data, or the reason it could not be fetched.
    pub result: Result<ProviderUsage, ModelUsageErrorKind>,
}

/// A point-in-time view of usage across all enabled providers.
///
/// Published after every poll cycle, even when every provider failed —
/// per-provider errors are carried in [`ProviderEntry::result`] so UIs can
/// render partial data alongside failure notices.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageSnapshot {
    /// When this snapshot was assembled.
    pub updated_at: DateTime<Utc>,
    /// One entry per enabled provider, in the order they were configured.
    pub providers: Vec<ProviderEntry>,
}

impl UsageSnapshot {
    /// Returns the entry for the given provider, if it was polled.
    pub fn provider(&self, kind: ProviderKind) -> Option<&ProviderEntry> {
        self.providers.iter().find(|entry| entry.kind == kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_percent_clamps_overflow() {
        let window = UsageWindow {
            kind: WindowKind::Session,
            label: "5 hour limit".to_owned(),
            used_percent: 130.0,
            window_duration: None,
            resets_at: None,
        };
        assert_eq!(window.remaining_percent(), 0.0);
    }

    #[test]
    fn remaining_percent_clamps_negative_usage() {
        let window = UsageWindow {
            kind: WindowKind::Session,
            label: "5 hour limit".to_owned(),
            used_percent: -5.0,
            window_duration: None,
            resets_at: None,
        };
        assert_eq!(window.remaining_percent(), 100.0);
    }

    #[test]
    fn min_remaining_picks_most_constrained_window() {
        let usage = ProviderUsage {
            plan: None,
            account: None,
            windows: vec![
                UsageWindow {
                    kind: WindowKind::Session,
                    label: "5 hour limit".to_owned(),
                    used_percent: 20.0,
                    window_duration: None,
                    resets_at: None,
                },
                UsageWindow {
                    kind: WindowKind::Weekly,
                    label: "Weekly limit".to_owned(),
                    used_percent: 91.0,
                    window_duration: None,
                    resets_at: None,
                },
            ],
            credits: None,
        };
        assert_eq!(usage.min_remaining_percent(), Some(9.0));
    }

    #[test]
    fn min_remaining_empty_windows_is_none() {
        let usage = ProviderUsage {
            plan: None,
            account: None,
            windows: vec![],
            credits: None,
        };
        assert_eq!(usage.min_remaining_percent(), None);
    }
}
