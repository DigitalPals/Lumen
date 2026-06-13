use std::{sync::Arc, time::Duration};

use lumen_config::schemas::modules::ModulesConfig;
use lumen_model_usage::{ModelUsageService, ProviderKind};

pub(super) fn build_model_usage_service(modules: &ModulesConfig) -> Arc<ModelUsageService> {
    let cfg = &modules.model_usage;

    Arc::new(
        ModelUsageService::builder()
            .poll_interval(Duration::from_secs(u64::from(
                cfg.refresh_interval_seconds.get(),
            )))
            .providers(enabled_providers(modules))
            .build(),
    )
}

pub(crate) fn enabled_providers(modules: &ModulesConfig) -> Vec<ProviderKind> {
    let cfg = &modules.model_usage;
    let mut providers = Vec::new();
    if cfg.claude_enabled.get() {
        providers.push(ProviderKind::Claude);
    }
    if cfg.codex_enabled.get() {
        providers.push(ProviderKind::Codex);
    }
    providers
}
