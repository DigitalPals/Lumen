use std::sync::Arc;

use lumen_config::ConfigService;
use lumen_widgets::watch;
use relm4::ComponentSender;

use super::{DashboardDropdown, messages::DashboardDropdownCmd};

pub(super) fn spawn(sender: &ComponentSender<DashboardDropdown>, config: &Arc<ConfigService>) {
    let scale = config.config().styling.scale.clone();
    watch!(sender, [scale.watch()], |out| {
        let _ = out.send(DashboardDropdownCmd::ScaleChanged(scale.get().value()));
    });
}
