use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Which Hermes API surface the client should prefer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransportMode {
    /// Discover capabilities and prefer native session streaming.
    #[default]
    Auto,
    /// Use `/api/sessions` and `/api/sessions/{id}/chat/stream`.
    Sessions,
    /// Use `/v1/runs` plus event streams for approval/stop-heavy flows.
    Runs,
    /// Use OpenAI-compatible `/v1/chat/completions`.
    ChatCompletions,
}

/// Local persistence policy for Lumen-side history.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalHistoryMode {
    /// Do not write transcripts to disk.
    Disabled,
    /// Store full transcript locally for the selected endpoint.
    #[default]
    Full,
}

/// Runtime connection configuration. Secrets must already be resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionConfig {
    /// Whether the module should connect.
    pub enabled: bool,
    /// Hermes API server base URL, with or without a trailing `/v1`.
    pub endpoint_url: String,
    /// Resolved bearer token. Never log this value.
    pub api_key: Option<String>,
    /// Cosmetic model name sent to OpenAI-compatible endpoints.
    pub model: String,
    /// Optional `X-Hermes-Session-Key` for server-side memory scoping.
    pub session_key: Option<String>,
    /// Request timeout in seconds.
    pub timeout_seconds: u64,
    /// Preferred transport mode.
    pub transport_mode: TransportMode,
    /// Local persistence mode.
    pub local_history: LocalHistoryMode,
    /// Number of messages to keep in memory and on disk.
    pub history_limit: usize,
    /// Whether tool progress events should be displayed.
    pub show_tool_progress: bool,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint_url: String::from("http://127.0.0.1:8642"),
            api_key: Some(String::from("$HERMES_API_SERVER_KEY")),
            model: String::from("hermes-agent"),
            session_key: None,
            timeout_seconds: 120,
            transport_mode: TransportMode::Auto,
            local_history: LocalHistoryMode::Full,
            history_limit: 200,
            show_tool_progress: true,
        }
    }
}

/// Bar/dropdown connection status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HermesStatus {
    /// Module disabled in config.
    Disabled,
    /// API key is absent or unresolved.
    MissingApiKey,
    /// Connecting or refreshing server state.
    Connecting,
    /// Server is reachable and authenticated.
    Connected,
    /// A response stream is active.
    Busy,
    /// Bearer token was rejected.
    AuthFailed,
    /// Network/server problem.
    Offline(String),
    /// Other error.
    Error(String),
}

impl HermesStatus {
    /// Short label for compact bar UI.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Disabled => "Hermes off",
            Self::MissingApiKey => "Hermes key",
            Self::Connecting => "Hermes…",
            Self::Connected => "Hermes",
            Self::Busy => "Hermes typing",
            Self::AuthFailed => "Hermes auth",
            Self::Offline(_) => "Hermes offline",
            Self::Error(_) => "Hermes error",
        }
    }

    /// Stable CSS class for status styling.
    pub fn css_class(&self) -> &'static str {
        match self {
            Self::Connected => "ok",
            Self::Busy | Self::Connecting => "busy",
            Self::Disabled => "disabled",
            Self::MissingApiKey | Self::AuthFailed | Self::Offline(_) | Self::Error(_) => "error",
        }
    }
}

/// Session summary returned by Hermes or synthesized locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesSessionSummary {
    /// Server/session identifier.
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Last update time if known.
    pub updated_at: Option<DateTime<Utc>>,
}

/// Chat message role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HermesRole {
    /// User-authored message.
    User,
    /// Assistant response.
    Assistant,
    /// System/context message.
    System,
    /// Tool progress or result.
    Tool,
    /// Client/server error row.
    Error,
}

/// Message lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    /// Complete/stable message.
    Complete,
    /// Message is currently streaming.
    Streaming,
    /// Generation was stopped.
    Stopped,
    /// Message failed.
    Error,
}

/// Tool progress row attached to an assistant response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolEvent {
    /// Stable server tool-call id if provided.
    pub id: String,
    /// Tool name.
    pub tool: String,
    /// Human label/progress text.
    pub label: String,
    /// Status such as running/completed/failed.
    pub status: String,
}

/// Chat message shown by the dropdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesMessage {
    /// Local/server message id.
    pub id: String,
    /// Role.
    pub role: HermesRole,
    /// Text content.
    pub content: String,
    /// Lifecycle status.
    pub status: MessageStatus,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Attached tool progress.
    pub tool_events: Vec<ToolEvent>,
}

impl HermesMessage {
    /// Builds a new text message.
    pub fn new(id: impl Into<String>, role: HermesRole, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role,
            content: content.into(),
            status: MessageStatus::Complete,
            created_at: Utc::now(),
            tool_events: Vec::new(),
        }
    }
}

/// Pending approval/clarification surfaced by a Hermes run/event stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Run identifier used by `/v1/runs/{id}/approval`.
    pub run_id: String,
    /// Approval id if Hermes provided one.
    pub approval_id: Option<String>,
    /// Prompt/question to show the user.
    pub prompt: String,
}
