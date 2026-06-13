use std::{rc::Rc, sync::Arc};

use lumen_config::ConfigService;
use lumen_sysinfo::SysinfoService;
use lumen_widgets::prelude::BarSettings;

use crate::shell::bar::dropdowns::DropdownRegistry;

pub(crate) struct NetstatInit {
    pub settings: BarSettings,
    pub sysinfo: Arc<SysinfoService>,
    pub config: Arc<ConfigService>,
    pub dropdowns: Rc<DropdownRegistry>,
}

#[derive(Debug)]
pub(crate) enum NetstatMsg {
    LeftClick,
    RightClick,
    MiddleClick,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug)]
pub(crate) enum NetstatCmd {
    UpdateLabel(String),
    UpdateIcon(String),
}
