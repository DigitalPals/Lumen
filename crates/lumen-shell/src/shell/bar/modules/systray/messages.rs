use std::sync::Arc;

use lumen_config::{ConfigProperty, ConfigService};
use lumen_systray::{SystemTrayService, core::item::TrayItem};

pub(crate) struct SystrayInit {
    pub is_vertical: ConfigProperty<bool>,
    pub systray: Arc<SystemTrayService>,
    pub config: Arc<ConfigService>,
}

#[derive(Debug)]
pub(crate) enum SystrayMsg {}

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum SystrayCmd {
    ItemsChanged(Vec<Arc<TrayItem>>),
    StylingChanged,
    OrientationChanged(bool),
}
