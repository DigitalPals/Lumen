use std::{rc::Rc, sync::Arc};

use lumen_config::{ConfigService, schemas::styling::ThresholdColors};
use lumen_notification::NotificationService;
use lumen_widgets::prelude::BarSettings;

use crate::shell::bar::dropdowns::DropdownRegistry;

pub(crate) struct NotificationInit {
    pub settings: BarSettings,
    pub notification: Arc<NotificationService>,
    pub config: Arc<ConfigService>,
    pub dropdowns: Rc<DropdownRegistry>,
}

#[derive(Debug)]
pub(crate) enum NotificationMsg {
    LeftClick,
    RightClick,
    MiddleClick,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum NotificationCmd {
    NotificationsChanged(usize),
    DndChanged(bool),
    IconConfigChanged,
    UpdateThresholdColors(ThresholdColors),
}
