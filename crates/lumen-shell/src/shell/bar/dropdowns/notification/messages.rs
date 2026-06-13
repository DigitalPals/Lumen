use std::sync::Arc;

use lumen_config::ConfigService;
use lumen_notification::NotificationService;

pub(crate) struct NotificationDropdownInit {
    pub notification: Arc<NotificationService>,
    pub config: Arc<ConfigService>,
}

#[derive(Debug)]
pub(crate) enum NotificationDropdownMsg {
    DndToggled(bool),
    ClearAll,
    NotificationDismissed,
}

#[derive(Debug)]
pub(crate) enum NotificationDropdownCmd {
    NotificationsChanged,
    DndChanged(bool),
    ScaleChanged(f32),
    IconSourceChanged,
    TimeTick,
}
