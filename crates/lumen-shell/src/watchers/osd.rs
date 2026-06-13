use lumen_widgets::watch;
use relm4::ComponentSender;

use crate::shell::{Shell, ShellCmd, ShellServices};

pub(crate) fn spawn(sender: &ComponentSender<Shell>, services: &ShellServices) {
    let enabled = services.config.config().osd.enabled.clone();

    watch!(sender, [enabled.watch()], |out| {
        let _ = out.send(ShellCmd::OsdEnabledChanged(enabled.get()));
    });
}
