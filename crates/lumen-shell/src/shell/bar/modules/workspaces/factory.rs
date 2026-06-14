use std::rc::Rc;

use lumen_widgets::prelude::BarSettings;
use tracing::warn;

use crate::shell::{
    bar::{
        dropdowns::DropdownRegistry,
        modules::{
            compositor::Compositor,
            hyprland_workspaces, mango_workspaces, niri_workspaces,
            registry::{ModuleFactory, ModuleInstance},
        },
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
        match Compositor::detect() {
            Compositor::Hyprland => {
                hyprland_workspaces::Factory::create(settings, services, dropdowns, class)
            }
            Compositor::Mango => {
                mango_workspaces::Factory::create(settings, services, dropdowns, class)
            }
            Compositor::Niri => {
                niri_workspaces::Factory::create(settings, services, dropdowns, class)
            }
            Compositor::Unknown(desktop) => {
                warn!(
                    module = "workspaces",
                    compositor = %desktop,
                    "workspace module requires Hyprland, Mango, or niri, skipping"
                );
                None
            }
        }
    }
}
