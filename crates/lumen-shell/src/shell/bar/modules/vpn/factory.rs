use std::rc::Rc;

use lumen_widgets::prelude::BarSettings;
use relm4::prelude::*;

use super::{VpnInit, VpnModule};
use crate::shell::{
    bar::{
        dropdowns::DropdownRegistry,
        modules::registry::{ModuleFactory, ModuleInstance, dynamic_controller, require_service},
    },
    services::ShellServices,
};

pub(crate) struct Factory;

impl ModuleFactory for Factory {
    fn create(
        settings: &BarSettings,
        services: &ShellServices,
        dropdowns: &Rc<DropdownRegistry>,
        class: Option<String>,
    ) -> Option<ModuleInstance> {
        let network = require_service("vpn", "network", services.network.clone())?;

        let init = VpnInit {
            settings: settings.clone(),
            network,
            config: services.config.clone(),
            dropdowns: dropdowns.clone(),
        };
        let controller = dynamic_controller(VpnModule::builder().launch(init).detach());
        Some(ModuleInstance { controller, class })
    }
}
