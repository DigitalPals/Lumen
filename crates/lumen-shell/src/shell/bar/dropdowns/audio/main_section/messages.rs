use std::sync::Arc;

use lumen_audio::AudioService;
use lumen_config::ConfigService;

use super::default_devices::DefaultDevicesOutput;

pub(crate) struct MainSectionInit {
    pub audio: Arc<AudioService>,
    pub config: Arc<ConfigService>,
}

#[derive(Debug)]
pub(crate) enum MainSectionInput {
    DefaultDevices(DefaultDevicesOutput),
}

#[derive(Debug)]
pub(crate) enum MainSectionOutput {
    ShowOutputDevices,
    ShowInputDevices,
}
