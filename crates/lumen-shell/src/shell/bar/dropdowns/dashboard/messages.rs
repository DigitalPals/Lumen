use std::sync::Arc;

use lumen_audio::AudioService;
use lumen_battery::BatteryService;
use lumen_bluetooth::BluetoothService;
use lumen_config::ConfigService;
use lumen_core::DeferredService;
use lumen_media::MediaService;
use lumen_network::NetworkService;
use lumen_notification::NotificationService;
use lumen_power_profiles::PowerProfilesService;
use lumen_sysinfo::SysinfoService;

use crate::services::IdleInhibitService;

pub(crate) struct DashboardDropdownInit {
    pub audio: Option<Arc<AudioService>>,
    pub battery: Option<Arc<BatteryService>>,
    pub bluetooth: DeferredService<BluetoothService>,
    pub config: Arc<ConfigService>,
    pub media: Option<Arc<MediaService>>,
    pub network: Option<Arc<NetworkService>>,
    pub notification: Option<Arc<NotificationService>>,
    pub power_profiles: DeferredService<PowerProfilesService>,
    pub sysinfo: Arc<SysinfoService>,
    pub idle_inhibit: Arc<IdleInhibitService>,
}

#[derive(Debug)]
pub(crate) enum DashboardDropdownMsg {
    VisibilityChanged(bool),
    OpenSettings,
}

#[derive(Debug)]
pub(crate) enum DashboardDropdownCmd {
    ScaleChanged(f32),
}
