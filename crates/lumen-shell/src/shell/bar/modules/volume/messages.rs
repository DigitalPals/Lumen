use std::{rc::Rc, sync::Arc};

use lumen_audio::{AudioService, core::device::output::OutputDevice};
use lumen_config::{ConfigService, schemas::styling::ThresholdColors};
use lumen_widgets::prelude::BarSettings;

use crate::shell::bar::dropdowns::DropdownRegistry;

pub(crate) struct VolumeInit {
    pub settings: BarSettings,
    pub audio: Arc<AudioService>,
    pub config: Arc<ConfigService>,
    pub dropdowns: Rc<DropdownRegistry>,
}

#[derive(Debug)]
pub(crate) enum VolumeMsg {
    LeftClick,
    RightClick,
    MiddleClick,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum VolumeCmd {
    DeviceChanged(Option<Arc<OutputDevice>>),
    VolumeOrMuteChanged,
    ConfigChanged,
    UpdateThresholdColors(ThresholdColors),
}
