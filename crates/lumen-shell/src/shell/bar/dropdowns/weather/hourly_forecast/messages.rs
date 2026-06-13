use std::sync::Arc;

use lumen_config::ConfigService;
use lumen_weather::WeatherService;

pub(crate) struct HourlyForecastInit {
    pub weather: Arc<WeatherService>,
    pub config: Arc<ConfigService>,
}

#[derive(Debug)]
pub(crate) enum HourlyForecastCmd {
    WeatherChanged,
}
