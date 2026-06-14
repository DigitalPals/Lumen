use lumen_media::types::PlaybackState;
use lumen_widgets::utils::force_window_resize;
use relm4::{gtk, gtk::prelude::*};

use super::{MediaModule, helpers};

impl MediaModule {
    pub(super) fn refresh_visibility(&self, root: &gtk::Box) {
        let state = self
            .media
            .active_player()
            .map(|player| player.playback_state.get());
        self.update_visibility(root, state);
    }

    pub(super) fn update_visibility(&self, root: &gtk::Box, state: Option<PlaybackState>) {
        let hide_when_nothing_playing = self
            .config
            .config()
            .modules
            .media
            .hide_when_nothing_playing
            .get();
        let visible = helpers::compute_visibility(state, hide_when_nothing_playing);
        self.visible.set(visible);
        if let Some(parent) = root.parent() {
            parent.set_visible(visible);
        }
        force_window_resize(root);
    }

    pub(super) fn update_disc_mode(root: &gtk::Box, enabled: bool) {
        if enabled {
            root.add_css_class("media-disc");
        } else {
            root.remove_css_class("media-disc");
        }
    }

    pub(super) fn update_spinning_state(root: &gtk::Box, state: PlaybackState) {
        match state {
            PlaybackState::Playing => {
                root.add_css_class("media-spinning");
            }
            PlaybackState::Paused | PlaybackState::Stopped => {
                root.remove_css_class("media-spinning");
            }
        }
    }
}
