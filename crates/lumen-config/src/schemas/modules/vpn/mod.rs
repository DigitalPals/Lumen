use lumen_derive::{lumen_config, lumen_enum};
use schemars::schema_for;

use crate::{
    ClickAction, ConfigProperty,
    docs::{ConfigGroup, GroupDefaults, ModuleInfo, ModuleInfoProvider},
    schemas::styling::{ColorValue, CssToken},
};

/// VPN connection status with a dropdown for switching VPN profiles.
#[lumen_config(bar_button, i18n_prefix = "settings-modules-vpn")]
pub struct VpnConfig {
    /// Icon when a VPN is connected.
    #[serde(rename = "connected-icon")]
    #[default(String::from("ld-shield-check-symbolic"))]
    pub connected_icon: ConfigProperty<String>,

    /// Icon when a VPN is connecting.
    #[serde(rename = "connecting-icon")]
    #[default(String::from("ld-refresh-cw-symbolic"))]
    pub connecting_icon: ConfigProperty<String>,

    /// Icon when no VPN is connected.
    #[serde(rename = "disconnected-icon")]
    #[default(String::from("ld-shield-symbolic"))]
    pub disconnected_icon: ConfigProperty<String>,

    /// Display border around button.
    #[serde(rename = "border-show")]
    #[default(false)]
    pub border_show: ConfigProperty<bool>,

    /// Border color token.
    #[serde(rename = "border-color")]
    #[default(ColorValue::Token(CssToken::Accent))]
    pub border_color: ConfigProperty<ColorValue>,

    /// Display module icon.
    #[serde(rename = "icon-show")]
    #[default(true)]
    pub icon_show: ConfigProperty<bool>,

    /// Icon foreground color. Auto selects based on variant for contrast.
    #[serde(rename = "icon-color")]
    #[default(ColorValue::Auto)]
    pub icon_color: ConfigProperty<ColorValue>,

    /// Icon container background color token.
    #[serde(rename = "icon-bg-color")]
    #[default(ColorValue::Token(CssToken::Accent))]
    pub icon_bg_color: ConfigProperty<ColorValue>,

    /// Display VPN label.
    #[serde(rename = "label-show")]
    #[default(true)]
    pub label_show: ConfigProperty<bool>,

    /// Label style for connected Tailscale.
    #[serde(rename = "tailscale-label")]
    #[default(TailscaleLabel::ServiceName)]
    pub tailscale_label: ConfigProperty<TailscaleLabel>,

    /// Label text color token.
    #[serde(rename = "label-color")]
    #[default(ColorValue::Token(CssToken::Accent))]
    pub label_color: ConfigProperty<ColorValue>,

    /// Max label characters before truncation with ellipsis. Set to 0 to disable.
    #[serde(rename = "label-max-length")]
    #[default(18)]
    pub label_max_length: ConfigProperty<u32>,

    /// Button background color token.
    #[serde(rename = "button-bg-color")]
    #[default(ColorValue::Token(CssToken::BgSurfaceElevated))]
    pub button_bg_color: ConfigProperty<ColorValue>,

    /// Action on left click.
    #[serde(rename = "left-click")]
    #[default(ClickAction::Dropdown(String::from("vpn")))]
    pub left_click: ConfigProperty<ClickAction>,

    /// Action on right click.
    #[serde(rename = "right-click")]
    #[default(ClickAction::None)]
    pub right_click: ConfigProperty<ClickAction>,

    /// Action on middle click.
    #[serde(rename = "middle-click")]
    #[default(ClickAction::None)]
    pub middle_click: ConfigProperty<ClickAction>,

    /// Action on scroll up.
    #[serde(rename = "scroll-up")]
    #[default(ClickAction::None)]
    pub scroll_up: ConfigProperty<ClickAction>,

    /// Action on scroll down.
    #[serde(rename = "scroll-down")]
    #[default(ClickAction::None)]
    pub scroll_down: ConfigProperty<ClickAction>,
}

/// Label shown in the bar when Tailscale is connected.
#[lumen_enum(default)]
pub enum TailscaleLabel {
    /// Show "Tailscale".
    #[default]
    ServiceName,
    /// Show this machine's Tailscale node name when available.
    Hostname,
    /// Show connected/disconnected state.
    Status,
}

impl ModuleInfoProvider for VpnConfig {
    fn module_info() -> ModuleInfo {
        ModuleInfo {
            name: String::from("vpn"),
            schema: || schema_for!(VpnConfig),
            layout_id: Some(String::from("vpn")),
            array_entry: false,
        }
    }

    fn groups() -> Vec<ConfigGroup> {
        GroupDefaults::bar_button()
    }
}

crate::register_module!(VpnConfig);
