use lumen_derive::lumen_config;
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};

use crate::{
    ClickAction, ConfigProperty,
    docs::{ConfigGroup, GroupDefaults, ModuleInfo, ModuleInfoProvider},
    schemas::styling::{ColorValue, CssToken, ScaleFactor},
};

/// Hermes Agent chat dropdown backed by an external Hermes API server.
#[lumen_config(bar_button, i18n_prefix = "settings-modules-hermes-chat")]
pub struct HermesChatConfig {
    /// Enable the Hermes chat client module.
    #[default(false)]
    pub enabled: ConfigProperty<bool>,

    /// Hermes API server base URL. `/v1` suffix is accepted and normalized.
    #[serde(rename = "endpoint-url")]
    #[default(String::from("http://127.0.0.1:8642"))]
    pub endpoint_url: ConfigProperty<String>,

    /// Hermes API bearer token. `$HERMES_API_SERVER_KEY` is recommended.
    #[serde(rename = "api-key")]
    #[default(String::from("$HERMES_API_SERVER_KEY"))]
    pub api_key: ConfigProperty<String>,

    /// Cosmetic model name sent to OpenAI-compatible endpoints.
    #[default(String::from("hermes-agent"))]
    pub model: ConfigProperty<String>,

    /// Optional `X-Hermes-Session-Key` used by Hermes for server-side memory scoping.
    #[serde(rename = "session-key")]
    #[default(String::new())]
    pub session_key: ConfigProperty<String>,

    /// Preferred API transport mode.
    #[serde(rename = "transport-mode")]
    #[default(HermesChatTransportMode::Auto)]
    pub transport_mode: ConfigProperty<HermesChatTransportMode>,

    /// Local history persistence policy. `full` stores transcripts locally.
    #[serde(rename = "local-history")]
    #[default(HermesChatLocalHistory::Full)]
    pub local_history: ConfigProperty<HermesChatLocalHistory>,

    /// Maximum number of messages kept in the local transcript.
    #[serde(rename = "history-limit")]
    #[schemars(range(min = 1, max = 5000))]
    #[default(200)]
    pub history_limit: ConfigProperty<u32>,

    /// Request timeout in seconds.
    #[serde(rename = "request-timeout-seconds")]
    #[schemars(range(min = 5, max = 1800))]
    #[default(120)]
    pub request_timeout_seconds: ConfigProperty<u32>,

    /// Show Hermes tool progress rows in the transcript.
    #[serde(rename = "show-tool-progress")]
    #[default(true)]
    pub show_tool_progress: ConfigProperty<bool>,

    /// Show a warning that tools execute on the remote Hermes API server.
    #[serde(rename = "show-runtime-warning")]
    #[default(true)]
    pub show_runtime_warning: ConfigProperty<bool>,

    /// Dropdown size scale, applied on top of the global UI scale.
    #[serde(rename = "dropdown-scale")]
    #[default(ScaleFactor::new(0.95))]
    pub dropdown_scale: ConfigProperty<ScaleFactor>,

    /// Icon for the bar button.
    #[serde(rename = "icon-name")]
    #[default(String::from("ld-message-circle-symbolic"))]
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

    /// Display chat status label.
    #[serde(rename = "label-show")]
    #[default(false)]
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
    #[default(ClickAction::Dropdown(String::from("hermes-chat")))]
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

/// Preferred Hermes API transport.
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
pub enum HermesChatTransportMode {
    /// Discover capabilities and prefer native session streaming.
    #[default]
    Auto,
    /// Use Hermes native session resources.
    Sessions,
    /// Use run submission/status/approval endpoints.
    Runs,
    /// Use OpenAI-compatible chat completions.
    ChatCompletions,
}

/// Local transcript persistence policy.
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
pub enum HermesChatLocalHistory {
    /// Do not store transcript on disk.
    Disabled,
    /// Store full transcript locally under Lumen data dir.
    #[default]
    Full,
}

impl ModuleInfoProvider for HermesChatConfig {
    fn module_info() -> ModuleInfo {
        ModuleInfo {
            name: String::from("hermes-chat"),
            schema: || schema_for!(HermesChatConfig),
            layout_id: Some(String::from("hermes-chat")),
            array_entry: false,
        }
    }

    fn groups() -> Vec<ConfigGroup> {
        GroupDefaults::bar_button()
    }
}

crate::register_module!(HermesChatConfig);
