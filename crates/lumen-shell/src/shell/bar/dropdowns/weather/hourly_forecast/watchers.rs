use std::sync::Arc;

use lumen_config::ConfigService;
use lumen_weather::WeatherService;
use lumen_widgets::watch;
use relm4::ComponentSender;

use super::{HourlyForecast, messages::HourlyForecastCmd};

pub(super) fn spawn(
    sender: &ComponentSender<HourlyForecast>,
    weather: &Arc<WeatherService>,
    config: &Arc<ConfigService>,
) {
    let weather_prop = weather.weather.clone();
    let units_config = config.config().modules.weather.units.clone();
    let time_format = config.config().modules.weather.time_format.clone();

    watch!(
        sender,
        [
            weather_prop.watch(),
            units_config.watch(),
            time_format.watch()
        ],
        |out| {
            let _ = out.send(HourlyForecastCmd::WeatherChanged);
        }
    );
}
