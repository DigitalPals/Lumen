use std::rc::Rc;

use lumen_widgets::prelude::BarSettings;
use relm4::prelude::*;

use super::{ModelUsageInit, ModelUsageModule};
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
        let init = ModelUsageInit {
            settings: settings.clone(),
            config: services.config.clone(),
            dropdowns: dropdowns.clone(),
            model_usage: services.model_usage.clone(),
        };
        let controller = dynamic_controller(ModelUsageModule::builder().launch(init).detach());
        Some(ModuleInstance { controller, class })
    }
}
