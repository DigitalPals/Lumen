use relm4::prelude::*;

use super::{VpnDropdown, messages::VpnDropdownInit};
use crate::shell::{
    bar::dropdowns::{DropdownFactory, DropdownInstance, require_service},
    services::ShellServices,
};

pub(crate) struct Factory;

impl DropdownFactory for Factory {
    fn create(services: &ShellServices) -> Option<DropdownInstance> {
        let network = require_service("vpn", "network", services.network.clone())?;
        let config = services.config.clone();

        let init = VpnDropdownInit { network, config };
        let controller = VpnDropdown::builder().launch(init).detach();

        let popover = controller.widget().clone();
        Some(DropdownInstance::new(popover, Box::new(controller)))
    }
}
