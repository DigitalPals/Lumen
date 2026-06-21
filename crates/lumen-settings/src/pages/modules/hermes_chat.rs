//! Hermes Chat module settings.

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
    let module = &config.modules.hermes_chat;

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
        id: "hermes-chat",
        i18n_key: "settings-nav-hermes-chat",
        icon: "ld-message-circle-symbolic",
        spec: page_spec(
            "settings-page-hermes-chat",
            vec![
                SectionSpec {
                    title_key: "settings-section-general",
                    items: vec![
                        toggle(&module.enabled),
                        text(&module.endpoint_url),
                        text(&module.api_key),
                        text(&module.dashboard_token),
                        text(&module.model),
                        text(&module.session_key),
                    ],
                },
                SectionSpec {
                    title_key: "settings-section-behavior",
                    items: vec![
                        enum_select(&module.transport_mode),
                        enum_select(&module.local_history),
                        number_u32(&module.history_limit),
                        number_u32(&module.request_timeout_seconds),
                        toggle(&module.show_tool_progress),
                        toggle(&module.show_runtime_warning),
                        scale(&module.dropdown_scale),
                    ],
                },
                SectionSpec {
                    title_key: "settings-section-display",
                    items: vec![text(&module.icon_name)],
                },
                bar_display_section(&fields),
                colors_section(&fields),
                actions_section(&fields),
            ],
        ),
    }
}
