//! Model Usage module settings.

use lumen_config::Config;

use crate::{
    editors::{
        enum_select::enum_select,
        number::{number_u32, scale},
        text::text,
        toggle::toggle,
    },
    pages::{
        nav::LeafEntry,
        sections::bar_button::{
            BarButtonFields, actions_section, bar_display_section, colors_section,
        },
        spec::{SectionSpec, page_spec},
    },
};

pub(crate) fn entry(config: &Config) -> LeafEntry {
    let module = &config.modules.model_usage;

    let fields = BarButtonFields {
        icon_show: &module.icon_show,
        label_show: &module.label_show,
        label_max_length: &module.label_max_length,
        border_show: &module.border_show,
        icon_color: &module.icon_color,
        icon_bg_color: &module.icon_bg_color,
        label_color: &module.label_color,
        button_bg_color: &module.button_bg_color,
        border_color: &module.border_color,
        left_click: &module.left_click,
        right_click: &module.right_click,
        middle_click: &module.middle_click,
        scroll_up: &module.scroll_up,
        scroll_down: &module.scroll_down,
    };

    LeafEntry {
        id: "model-usage",
        i18n_key: "settings-nav-model-usage",
        icon: "ld-code-symbolic",
        spec: page_spec(
            "settings-page-model-usage",
            vec![
                SectionSpec {
                    title_key: "settings-section-general",
                    items: vec![
                        toggle(&module.claude_enabled),
                        toggle(&module.codex_enabled),
                        enum_select(&module.provider_order),
                        number_u32(&module.refresh_interval_seconds),
                        scale(&module.dropdown_scale),
                        text(&module.icon_name),
                    ],
                },
                bar_display_section(&fields),
                colors_section(&fields),
                actions_section(&fields),
            ],
        ),
    }
}
