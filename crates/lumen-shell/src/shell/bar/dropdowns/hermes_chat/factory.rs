use relm4::prelude::*;

use super::{HermesChatDropdown, HermesChatDropdownInit};
use crate::shell::{
    bar::dropdowns::{DropdownFactory, DropdownInstance},
    services::ShellServices,
};

pub(crate) struct Factory;

impl DropdownFactory for Factory {
    fn create(services: &ShellServices) -> Option<DropdownInstance> {
        let controller = HermesChatDropdown::builder()
            .launch(HermesChatDropdownInit {
                config: services.config.clone(),
                hermes_chat: services.hermes_chat.clone(),
            })
            .detach();

        let popover = controller.widget().clone();
        Some(DropdownInstance::new(popover, Box::new(controller)))
    }
}
