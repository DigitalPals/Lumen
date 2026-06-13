use std::{rc::Rc, sync::Arc};

use lumen_config::ConfigService;
use lumen_widgets::prelude::BarSettings;

use super::helpers::HyprsunsetState;
use crate::shell::bar::dropdowns::DropdownRegistry;

pub(crate) struct HyprsunsetInit {
    pub settings: BarSettings,
    pub config: Arc<ConfigService>,
    pub dropdowns: Rc<DropdownRegistry>,
}

#[derive(Debug)]
pub(crate) enum HyprsunsetMsg {
    LeftClick,
    RightClick,
    MiddleClick,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug)]
pub(crate) enum HyprsunsetCmd {
    ConfigChanged,
    StateChanged(Option<HyprsunsetState>),
}
