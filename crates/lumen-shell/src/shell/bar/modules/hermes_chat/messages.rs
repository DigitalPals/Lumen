use std::{rc::Rc, sync::Arc};

use lumen_config::ConfigService;
use lumen_hermes::HermesChatService;
use lumen_widgets::prelude::BarSettings;

use crate::shell::bar::dropdowns::DropdownRegistry;

pub(crate) struct HermesChatInit {
    pub(crate) settings: BarSettings,
    pub(crate) config: Arc<ConfigService>,
    pub(crate) dropdowns: Rc<DropdownRegistry>,
    pub(crate) hermes_chat: Arc<HermesChatService>,
}

#[derive(Debug)]
pub(crate) enum HermesChatMsg {
    LeftClick,
    RightClick,
    MiddleClick,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug)]
pub(crate) enum HermesChatCmd {
    Update {
        label: String,
        class: Option<&'static str>,
    },
    ConfigChanged,
}
