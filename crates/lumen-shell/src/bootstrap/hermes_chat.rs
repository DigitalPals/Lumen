use std::{path::PathBuf, sync::Arc};

use lumen_config::{
    infrastructure::secrets,
    schemas::modules::{HermesChatLocalHistory, HermesChatTransportMode, ModulesConfig},
};
use lumen_core::paths::ConfigPaths;
use lumen_hermes::{ConnectionConfig, HermesChatService, LocalHistoryMode, TransportMode};

pub(super) fn build_hermes_chat_service(modules: &ModulesConfig) -> Arc<HermesChatService> {
    let cfg = &modules.hermes_chat;
    let history_path = ConfigPaths::data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("hermes-chat")
        .join("history.json");

    Arc::new(
        HermesChatService::builder()
            .config(ConnectionConfig {
                enabled: cfg.enabled.get(),
                endpoint_url: cfg.endpoint_url.get(),
                api_key: secrets::resolve(Some(cfg.api_key.get())),
                dashboard_token: secrets::resolve(Some(cfg.dashboard_token.get())),
                model: cfg.model.get(),
                session_key: secrets::resolve(Some(cfg.session_key.get()))
                    .filter(|value| !value.is_empty()),
                timeout_seconds: u64::from(cfg.request_timeout_seconds.get()),
                transport_mode: transport_mode(cfg.transport_mode.get()),
                local_history: local_history(cfg.local_history.get()),
                history_limit: cfg.history_limit.get() as usize,
                show_tool_progress: cfg.show_tool_progress.get(),
            })
            .history_path(history_path)
            .build(),
    )
}

pub(crate) fn connection_config(modules: &ModulesConfig) -> ConnectionConfig {
    let cfg = &modules.hermes_chat;
    ConnectionConfig {
        enabled: cfg.enabled.get(),
        endpoint_url: cfg.endpoint_url.get(),
        api_key: secrets::resolve(Some(cfg.api_key.get())),
        dashboard_token: secrets::resolve(Some(cfg.dashboard_token.get())),
        model: cfg.model.get(),
        session_key: secrets::resolve(Some(cfg.session_key.get()))
            .filter(|value| !value.is_empty()),
        timeout_seconds: u64::from(cfg.request_timeout_seconds.get()),
        transport_mode: transport_mode(cfg.transport_mode.get()),
        local_history: local_history(cfg.local_history.get()),
        history_limit: cfg.history_limit.get() as usize,
        show_tool_progress: cfg.show_tool_progress.get(),
    }
}

fn transport_mode(mode: HermesChatTransportMode) -> TransportMode {
    match mode {
        HermesChatTransportMode::Auto => TransportMode::Auto,
        HermesChatTransportMode::Sessions => TransportMode::Sessions,
        HermesChatTransportMode::Runs => TransportMode::Runs,
        HermesChatTransportMode::ChatCompletions => TransportMode::ChatCompletions,
        HermesChatTransportMode::DashboardWs => TransportMode::DashboardWs,
    }
}

fn local_history(mode: HermesChatLocalHistory) -> LocalHistoryMode {
    match mode {
        HermesChatLocalHistory::Disabled => LocalHistoryMode::Disabled,
        HermesChatLocalHistory::Full => LocalHistoryMode::Full,
    }
}
