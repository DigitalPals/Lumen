use std::sync::Arc;

use lumen_config::ConfigService;
use lumen_widgets::watch;
use relm4::ComponentSender;

use super::{BatteryDropdown, messages::BatteryDropdownCmd};

pub(super) fn spawn(sender: &ComponentSender<BatteryDropdown>, config: &Arc<ConfigService>) {
    let scale = config.config().styling.scale.clone();
    watch!(sender, [scale.watch()], |out| {
        let _ = out.send(BatteryDropdownCmd::ScaleChanged(scale.get().value()));
    });
}
