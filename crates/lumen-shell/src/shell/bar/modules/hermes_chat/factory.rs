use std::rc::Rc;

use lumen_widgets::prelude::BarSettings;
use relm4::prelude::*;

use super::{HermesChatInit, HermesChatModule};
use crate::shell::{
    bar::{
        dropdowns::DropdownRegistry,
        modules::registry::{ModuleFactory, ModuleInstance, dynamic_controller},
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
        let init = HermesChatInit {
            settings: settings.clone(),
            config: services.config.clone(),
            dropdowns: dropdowns.clone(),
            hermes_chat: services.hermes_chat.clone(),
        };
        let controller = dynamic_controller(HermesChatModule::builder().launch(init).detach());
        Some(ModuleInstance { controller, class })
    }
}
