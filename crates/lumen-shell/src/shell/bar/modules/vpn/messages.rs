use std::{rc::Rc, sync::Arc};

use lumen_config::ConfigService;
use lumen_network::NetworkService;
use lumen_widgets::prelude::BarSettings;

use crate::shell::bar::dropdowns::DropdownRegistry;

pub(crate) struct VpnInit {
    pub settings: BarSettings,
    pub network: Arc<NetworkService>,
    pub config: Arc<ConfigService>,
    pub dropdowns: Rc<DropdownRegistry>,
}

#[derive(Debug)]
pub(crate) enum VpnMsg {
    LeftClick,
    RightClick,
    MiddleClick,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug)]
pub(crate) enum VpnCmd {
    StateChanged,
}
