use std::sync::Arc;

use lumen_config::schemas::modules::ModelUsageConfig;
use lumen_model_usage::{ModelUsageService, ModelUsageStatus, ProviderEntry, UsageSnapshot};
use lumen_widgets::watch;
use relm4::ComponentSender;

use super::{ModelUsageCmd, ModelUsageModule};

const WARNING_THRESHOLD: f64 = 25.0;
const CRITICAL_THRESHOLD: f64 = 10.0;

pub(super) fn spawn_watchers(
    sender: &ComponentSender<ModelUsageModule>,
    config: &ModelUsageConfig,
    model_usage: &Arc<ModelUsageService>,
) {
    spawn_usage_watcher(sender, model_usage);
    spawn_config_watcher(sender, config, model_usage);
}

/// Mirrors the service's snapshot into the bar button label and status class.
fn spawn_usage_watcher(
    sender: &ComponentSender<ModelUsageModule>,
    model_usage: &Arc<ModelUsageService>,
) {
    let usage_prop = model_usage.usage.clone();
    let status_prop = model_usage.status.clone();

    watch!(sender, [usage_prop.watch(), status_prop.watch()], |out| {
        let Some(snapshot) = usage_prop.get() else {
            let _ = out.send(ModelUsageCmd::Update {
                label: String::from("Models --"),
                class: None,
            });
            return;
        };

        let _ = out.send(summarize(&snapshot, &status_prop.get()));
    });
}

/// Pushes runtime config changes into the service.
fn spawn_config_watcher(
    sender: &ComponentSender<ModelUsageModule>,
    config: &ModelUsageConfig,
    model_usage: &Arc<ModelUsageService>,
) {
    let claude_enabled = config.claude_enabled.clone();
    let codex_enabled = config.codex_enabled.clone();
    let refresh_interval = config.refresh_interval_seconds.clone();
    let service = model_usage.clone();
    let mut last = (
        claude_enabled.get(),
        codex_enabled.get(),
        refresh_interval.get(),
    );

    watch!(
        sender,
        [
            claude_enabled.watch(),
            codex_enabled.watch(),
            refresh_interval.watch()
        ],
        |_out| {
            let current = (
                claude_enabled.get(),
                codex_enabled.get(),
                refresh_interval.get(),
            );
            if current == last {
                return;
            }
            if (current.0, current.1) != (last.0, last.1) {
                let mut providers = Vec::new();
                if current.0 {
                    providers.push(lumen_model_usage::ProviderKind::Claude);
                }
                if current.1 {
                    providers.push(lumen_model_usage::ProviderKind::Codex);
                }
                service.set_providers(providers);
            }
            if current.2 != last.2 {
                service.set_poll_interval(std::time::Duration::from_secs(u64::from(current.2)));
            }
            last = current;
        }
    );
}

fn summarize(snapshot: &UsageSnapshot, status: &ModelUsageStatus) -> ModelUsageCmd {
    let parts: Vec<String> = snapshot
        .providers
        .iter()
        .filter_map(provider_summary)
        .collect();

    if parts.is_empty() {
        let label = match status {
            ModelUsageStatus::Loading => String::from("Models --"),
            _ => String::from("Models offline"),
        };
        return ModelUsageCmd::Update {
            label,
            class: Some("offline"),
        };
    }

    let min_remaining = snapshot
        .providers
        .iter()
        .filter_map(|entry| entry.result.as_ref().ok())
        .filter_map(lumen_model_usage::ProviderUsage::min_remaining_percent)
        .min_by(f64::total_cmp);

    ModelUsageCmd::Update {
        label: parts.join(" · "),
        class: Some(status_class(min_remaining)),
    }
}

fn provider_summary(entry: &ProviderEntry) -> Option<String> {
    let usage = entry.result.as_ref().ok()?;
    let remaining = usage.min_remaining_percent()?;
    Some(format!("{} {:.0}%", entry.kind.display_name(), remaining))
}

fn status_class(min_remaining: Option<f64>) -> &'static str {
    match min_remaining {
        Some(pct) if pct <= CRITICAL_THRESHOLD => "critical",
        Some(pct) if pct <= WARNING_THRESHOLD => "warning",
        _ => "ok",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_class_thresholds() {
        assert_eq!(status_class(Some(5.0)), "critical");
        assert_eq!(status_class(Some(10.0)), "critical");
        assert_eq!(status_class(Some(20.0)), "warning");
        assert_eq!(status_class(Some(80.0)), "ok");
        assert_eq!(status_class(None), "ok");
    }
}
