use std::sync::Arc;

use lumen_audio::AudioService;
use lumen_battery::BatteryService;
use lumen_bluetooth::BluetoothService;
use lumen_brightness::BrightnessService;
use lumen_config::ConfigService;
use lumen_core::DeferredService;
use lumen_hermes::HermesChatService;
use lumen_hyprland::HyprlandService;
use lumen_mango::MangoService;
use lumen_media::MediaService;
use lumen_model_usage::ModelUsageService;
use lumen_network::NetworkService;
use lumen_niri::NiriService;
use lumen_notification::NotificationService;
use lumen_power_profiles::PowerProfilesService;
use lumen_sysinfo::SysinfoService;
use lumen_systray::SystemTrayService;
use lumen_wallpaper::WallpaperService;
use lumen_weather::WeatherService;

use crate::services::{IdleInhibitService, ShellIpcService};

/// Container for services used by shell components.
///
/// Optional services are `None` when hardware, compositor, or D-Bus
/// daemons are unavailable.
#[derive(Clone)]
pub(crate) struct ShellServices {
    pub audio: Option<Arc<AudioService>>,
    pub battery: Option<Arc<BatteryService>>,
    pub bluetooth: DeferredService<BluetoothService>,
    pub brightness: Option<Arc<BrightnessService>>,
    pub config: Arc<ConfigService>,
    pub hermes_chat: Arc<HermesChatService>,
    pub hyprland: Option<Arc<HyprlandService>>,
    pub idle_inhibit: Arc<IdleInhibitService>,
    pub mango: Option<Arc<MangoService>>,
    pub media: Option<Arc<MediaService>>,
    pub model_usage: Arc<ModelUsageService>,
    pub niri: Option<Arc<NiriService>>,
    pub network: Option<Arc<NetworkService>>,
    pub notification: Option<Arc<NotificationService>>,
    pub power_profiles: DeferredService<PowerProfilesService>,
    pub sysinfo: Arc<SysinfoService>,
    pub systray: Option<Arc<SystemTrayService>>,
    pub wallpaper: Option<Arc<WallpaperService>>,
    pub weather: Arc<WeatherService>,
    pub shell_ipc: Arc<ShellIpcService>,
}
