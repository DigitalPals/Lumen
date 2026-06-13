use std::{rc::Rc, sync::Arc};

use lumen_cava::CavaService;
use lumen_config::ConfigService;
use lumen_wallpaper::WallpaperService;
use lumen_widgets::prelude::BarSettings;

use crate::shell::bar::dropdowns::DropdownRegistry;

pub(crate) struct CavaInit {
    pub settings: BarSettings,
    pub config: Arc<ConfigService>,
    pub wallpaper: Option<Arc<WallpaperService>>,
    pub dropdowns: Rc<DropdownRegistry>,
}

#[derive(Debug)]
pub(crate) enum CavaCmd {
    ServiceReady(Arc<CavaService>),
    ServiceFailed,
    Frame(Vec<f64>),
    StylingChanged,
    ServiceConfigChanged,
    OrientationChanged(bool),
}

#[derive(Debug)]
pub(crate) enum CavaMsg {
    LeftClick,
    RightClick,
    MiddleClick,
    ScrollUp,
    ScrollDown,
}
