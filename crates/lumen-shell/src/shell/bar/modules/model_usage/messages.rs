use std::{rc::Rc, sync::Arc};

use lumen_config::ConfigService;
use lumen_model_usage::ModelUsageService;
use lumen_widgets::prelude::BarSettings;

use crate::shell::bar::dropdowns::DropdownRegistry;

pub(crate) struct ModelUsageInit {
    pub(crate) settings: BarSettings,
    pub(crate) config: Arc<ConfigService>,
    pub(crate) dropdowns: Rc<DropdownRegistry>,
    pub(crate) model_usage: Arc<ModelUsageService>,
}

#[derive(Debug)]
pub(crate) enum ModelUsageMsg {
    LeftClick,
    RightClick,
    MiddleClick,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug)]
pub(crate) enum ModelUsageCmd {
    Update {
        label: String,
        class: Option<&'static str>,
    },
}
