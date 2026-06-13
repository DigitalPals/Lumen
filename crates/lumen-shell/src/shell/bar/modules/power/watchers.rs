use lumen_config::schemas::modules::PowerConfig;
use lumen_widgets::watch;
use relm4::ComponentSender;

use super::{PowerModule, messages::PowerCmd};

pub(super) fn spawn_watchers(sender: &ComponentSender<PowerModule>, config: &PowerConfig) {
    let icon_name = config.icon_name.clone();

    watch!(sender, [icon_name.watch()], |out| {
        let _ = out.send(PowerCmd::IconConfigChanged);
    });
}
