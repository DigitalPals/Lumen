use std::sync::Arc;

use lumen_config::ConfigService;
use lumen_widgets::watch;
use relm4::ComponentSender;

use super::{MediaDropdown, messages::MediaDropdownCmd};

pub(super) fn spawn(sender: &ComponentSender<MediaDropdown>, config: &Arc<ConfigService>) {
    let scale = config.config().styling.scale.clone();
    watch!(sender, [scale.watch()], |out| {
        let _ = out.send(MediaDropdownCmd::ScaleChanged(scale.get().value()));
    });
}
