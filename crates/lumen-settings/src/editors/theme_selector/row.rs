//! Theme preset selector control factory.

use lumen_config::{
    ConfigProperty,
    schemas::styling::{PaletteConfig, ScaleFactor, ThemeEntry},
};
use relm4::{gtk::prelude::*, prelude::*};

use crate::{
    editors::theme_selector::{ThemeSelectorControl, ThemeSelectorInit},
    editors::make_dirty_badge,
    pages::spec::SettingRowInit,
    property_handle::PropertyHandle,
    row::RowBehavior,
};

/// Row rendering the available theme presets as a gallery that applies a preset on click. Uses action behavior, so source and reset UI are suppressed.
pub(crate) fn theme_selector(
    available: &ConfigProperty<Vec<ThemeEntry>>,
    palette: &PaletteConfig,
    scale: &ConfigProperty<ScaleFactor>,
    i18n_key: &'static str,
) -> SettingRowInit {
    let badge = make_dirty_badge();
    let controller = ThemeSelectorControl::builder()
        .launch(ThemeSelectorInit {
            available: available.clone(),
            palette: palette.clone(),
            scale: scale.clone(),
            dirty_badge: Some(badge.clone()),
        })
        .detach();

    let widget = controller.widget().clone();

    SettingRowInit {
        i18n_key: Some(i18n_key),
        handle: PropertyHandle::new(available, |themes| format!("{} themes", themes.len())),
        control: widget.upcast(),
        keepalive: Box::new(controller),
        full_width: false,
        dirty_badge: Some(badge),
        behavior: RowBehavior::Action,
        unit: None,
    }
}
