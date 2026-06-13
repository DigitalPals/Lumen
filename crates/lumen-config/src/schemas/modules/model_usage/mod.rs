use lumen_derive::lumen_config;
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};

use crate::{
    ClickAction, ConfigProperty,
    docs::{ConfigGroup, GroupDefaults, ModuleInfo, ModuleInfoProvider},
    schemas::styling::{ColorValue, CssToken, ScaleFactor},
};

/// Claude Code and Codex usage limits with a detailed dropdown.
#[lumen_config(bar_button, i18n_prefix = "settings-modules-model-usage")]
pub struct ModelUsageConfig {
    /// Show Claude Code usage.
    #[serde(rename = "claude-enabled")]
    #[default(true)]
    pub claude_enabled: ConfigProperty<bool>,

    /// Show Codex CLI usage.
    #[serde(rename = "codex-enabled")]
    #[default(true)]
    pub codex_enabled: ConfigProperty<bool>,

    /// Provider tab order in the dropdown.
    #[serde(rename = "provider-order")]
    #[default(ProviderOrder::default())]
    pub provider_order: ConfigProperty<ProviderOrder>,

    /// Polling interval in seconds. Minimum 120 seconds.
    #[serde(rename = "refresh-interval-seconds")]
    #[schemars(range(min = 120))]
    #[default(300)]
    pub refresh_interval_seconds: ConfigProperty<u32>,

    /// Dropdown size scale, applied on top of the global UI scale.
    #[serde(rename = "dropdown-scale")]
    #[default(ScaleFactor::new(0.9))]
    pub dropdown_scale: ConfigProperty<ScaleFactor>,

    /// Icon for the bar button.
    #[serde(rename = "icon-name")]
    #[default(String::from("ld-code-symbolic"))]
    pub icon_name: ConfigProperty<String>,

    /// Display border around button.
    #[serde(rename = "border-show")]
    #[default(false)]
    pub border_show: ConfigProperty<bool>,

    /// Border color token.
    #[serde(rename = "border-color")]
    #[default(ColorValue::Token(CssToken::BorderAccent))]
    pub border_color: ConfigProperty<ColorValue>,

    /// Display module icon.
    #[serde(rename = "icon-show")]
    #[default(false)]
    pub icon_show: ConfigProperty<bool>,

    /// Icon foreground color. Auto selects based on variant for contrast.
    #[serde(rename = "icon-color")]
    #[default(ColorValue::Auto)]
    pub icon_color: ConfigProperty<ColorValue>,

    /// Icon container background color token.
    #[serde(rename = "icon-bg-color")]
    #[default(ColorValue::Token(CssToken::Accent))]
    pub icon_bg_color: ConfigProperty<ColorValue>,

    /// Display usage label.
    #[serde(rename = "label-show")]
    #[default(true)]
    pub label_show: ConfigProperty<bool>,

    /// Label text color token.
    #[serde(rename = "label-color")]
    #[default(ColorValue::Token(CssToken::FgDefault))]
    pub label_color: ConfigProperty<ColorValue>,

    /// Max label characters before truncation with ellipsis. Set to 0 to disable.
    #[serde(rename = "label-max-length")]
    #[default(32)]
    pub label_max_length: ConfigProperty<u32>,

    /// Button background color token.
    #[serde(rename = "button-bg-color")]
    #[default(ColorValue::Token(CssToken::BgSurfaceElevated))]
    pub button_bg_color: ConfigProperty<ColorValue>,

    /// Action on left click.
    #[serde(rename = "left-click")]
    #[default(ClickAction::Dropdown(String::from("model-usage")))]
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

/// Provider tab order in the Model Usage dropdown.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
    lumen_derive::EnumVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderOrder {
    /// Claude tab first.
    #[default]
    ClaudeFirst,
    /// Codex tab first.
    CodexFirst,
}

impl ModuleInfoProvider for ModelUsageConfig {
    fn module_info() -> ModuleInfo {
        ModuleInfo {
            name: String::from("model-usage"),
            schema: || schema_for!(ModelUsageConfig),
            layout_id: Some(String::from("model-usage")),
            array_entry: false,
        }
    }

    fn groups() -> Vec<ConfigGroup> {
        GroupDefaults::bar_button()
    }
}

crate::register_module!(ModelUsageConfig);
