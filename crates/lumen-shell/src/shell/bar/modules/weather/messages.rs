use std::{rc::Rc, sync::Arc};

use lumen_config::ConfigService;
use lumen_weather::WeatherService;
use lumen_widgets::prelude::BarSettings;

use crate::shell::bar::dropdowns::DropdownRegistry;

pub(crate) struct WeatherInit {
    pub settings: BarSettings,
    pub weather: Arc<WeatherService>,
    pub config: Arc<ConfigService>,
    pub dropdowns: Rc<DropdownRegistry>,
}

#[derive(Debug)]
pub(crate) enum WeatherMsg {
    LeftClick,
    RightClick,
    MiddleClick,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug)]
pub(crate) enum WeatherCmd {
    UpdateLabel(String),
    UpdateIcon(String),
}
