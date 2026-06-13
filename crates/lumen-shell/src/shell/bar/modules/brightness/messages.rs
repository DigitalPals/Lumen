use std::{rc::Rc, sync::Arc};

use lumen_brightness::{BacklightDevice, BrightnessService};
use lumen_config::{ConfigService, schemas::styling::ThresholdColors};
use lumen_widgets::prelude::BarSettings;

use crate::shell::bar::dropdowns::DropdownRegistry;

pub(crate) struct BrightnessInit {
    pub settings: BarSettings,
    pub brightness: Arc<BrightnessService>,
    pub config: Arc<ConfigService>,
    pub dropdowns: Rc<DropdownRegistry>,
}

#[derive(Debug)]
pub(crate) enum BrightnessMsg {
    LeftClick,
    RightClick,
    MiddleClick,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum BrightnessCmd {
    DeviceChanged(Option<Arc<BacklightDevice>>),
    BrightnessChanged,
    ConfigChanged,
    UpdateThresholdColors(ThresholdColors),
}
