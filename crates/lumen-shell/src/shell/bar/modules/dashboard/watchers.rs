use lumen_config::schemas::modules::DashboardConfig;
use lumen_widgets::watch;
use relm4::ComponentSender;

use super::{DashboardModule, messages::DashboardCmd};

pub(super) fn spawn_watchers(sender: &ComponentSender<DashboardModule>, config: &DashboardConfig) {
    let icon_override = config.icon_override.clone();

    watch!(sender, [icon_override.watch()], |out| {
        let _ = out.send(DashboardCmd::IconConfigChanged);
    });
}
