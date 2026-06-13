use relm4::prelude::*;

use super::{ModelUsageDropdown, ModelUsageDropdownInit};
use crate::shell::{
    bar::dropdowns::{DropdownFactory, DropdownInstance},
    services::ShellServices,
};

pub(crate) struct Factory;

impl DropdownFactory for Factory {
    fn create(services: &ShellServices) -> Option<DropdownInstance> {
        let controller = ModelUsageDropdown::builder()
            .launch(ModelUsageDropdownInit {
                config: services.config.clone(),
                model_usage: services.model_usage.clone(),
            })
            .detach();

        let popover = controller.widget().clone();
        Some(DropdownInstance::new(popover, Box::new(controller)))
    }
}
