use std::sync::Arc;

use lumen_config::schemas::modules::HermesChatConfig;
use lumen_hermes::{HermesChatService, HermesStatus};
use lumen_widgets::watch;
use relm4::ComponentSender;

use super::{HermesChatCmd, HermesChatModule};

pub(super) fn spawn_watchers(
    sender: &ComponentSender<HermesChatModule>,
    config: &HermesChatConfig,
    hermes_chat: &Arc<HermesChatService>,
) {
    spawn_status_watcher(sender, hermes_chat);
    spawn_config_watcher(sender, config);
}

fn spawn_status_watcher(
    sender: &ComponentSender<HermesChatModule>,
    hermes_chat: &Arc<HermesChatService>,
) {
    let status_prop = hermes_chat.status.clone();
    watch!(sender, [status_prop.watch()], |out| {
        let status = status_prop.get();
        let _ = out.send(HermesChatCmd::Update {
            label: label_for_status(&status),
            class: Some(status.css_class()),
        });
    });
}

fn spawn_config_watcher(sender: &ComponentSender<HermesChatModule>, config: &HermesChatConfig) {
    let enabled = config.enabled.clone();
    let endpoint = config.endpoint_url.clone();
    let api_key = config.api_key.clone();
    let model = config.model.clone();
    let session_key = config.session_key.clone();
    let transport_mode = config.transport_mode.clone();
    let local_history = config.local_history.clone();
    let history_limit = config.history_limit.clone();
    let timeout = config.request_timeout_seconds.clone();
    let show_tool_progress = config.show_tool_progress.clone();

    watch!(
        sender,
        [
            enabled.watch(),
            endpoint.watch(),
            api_key.watch(),
            model.watch(),
            session_key.watch(),
            transport_mode.watch(),
            local_history.watch(),
            history_limit.watch(),
            timeout.watch(),
            show_tool_progress.watch()
        ],
        |out| {
            let _ = out.send(HermesChatCmd::ConfigChanged);
        }
    );
}

fn label_for_status(status: &HermesStatus) -> String {
    status.label().to_owned()
}
