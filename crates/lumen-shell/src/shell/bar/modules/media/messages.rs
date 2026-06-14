use std::{rc::Rc, sync::Arc};

use lumen_config::ConfigService;
use lumen_media::{MediaService, core::player::Player};
use lumen_widgets::prelude::BarSettings;

use crate::shell::bar::dropdowns::DropdownRegistry;

pub(crate) struct MediaInit {
    pub settings: BarSettings,
    pub media: Arc<MediaService>,
    pub config: Arc<ConfigService>,
    pub dropdowns: Rc<DropdownRegistry>,
}

#[derive(Debug)]
pub(crate) enum MediaMsg {
    LeftClick,
    RightClick,
    MiddleClick,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug)]
pub(crate) enum MediaCmd {
    PlayerChanged(Option<Arc<Player>>),
    MetadataChanged,
    PlaybackStateChanged,
    UpdateIcon(String),
    IconTypeChanged,
    HideWhenNothingPlayingChanged,
}
