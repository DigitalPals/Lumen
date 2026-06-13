use std::sync::Arc;

use lumen_battery::BatteryService;
use lumen_config::ConfigService;
use lumen_core::Property;
use lumen_power_profiles::PowerProfilesService;

pub(crate) struct BatteryDropdownInit {
    pub battery: Arc<BatteryService>,
    pub power_profiles: Property<Option<Arc<PowerProfilesService>>>,
    pub config: Arc<ConfigService>,
}

#[derive(Debug)]
pub(crate) enum BatteryDropdownCmd {
    ScaleChanged(f32),
}
