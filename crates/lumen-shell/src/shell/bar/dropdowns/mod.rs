mod audio;
mod battery;
mod bluetooth;
mod brightness;
mod calendar;
mod dashboard;
mod hermes_chat;
mod media;
mod model_usage;
mod network;
mod notification;
mod registry;
mod vpn;
mod weather;

pub(crate) use self::registry::{
    DropdownFactory, DropdownInstance, DropdownRegistry, dispatch_click, dispatch_click_widget,
    require_service,
};
use crate::shell::services::ShellServices;

pub(crate) fn scaled_dimension(base: f32, scale: f32) -> i32 {
    (base * scale).round() as i32
}

macro_rules! register_dropdowns {
    ($($name:literal => $factory:ty),+ $(,)?) => {
        pub(crate) const DROPDOWN_NAMES: &[&str] = &[$($name),+];

        pub(crate) fn create(
            name: &str,
            services: &ShellServices,
        ) -> Option<DropdownInstance> {
            match name {
                $($name => <$factory as DropdownFactory>::create(services),)+
                _ => {
                    tracing::warn!(dropdown = name, "unknown dropdown type");
                    None
                }
            }
        }
    };
}

register_dropdowns! {
    "audio" => audio::Factory,
    "battery" => battery::Factory,
    "bluetooth" => bluetooth::Factory,
    "brightness" => brightness::Factory,
    "calendar" => calendar::Factory,
    "dashboard" => dashboard::Factory,
    "hermes-chat" => hermes_chat::Factory,
    "media" => media::Factory,
    "model-usage" => model_usage::Factory,
    "network" => network::Factory,
    "notification" => notification::Factory,
    "vpn" => vpn::Factory,
    "weather" => weather::Factory,
}
