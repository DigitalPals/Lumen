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
    /// Use Hermes Desktop/TUI dashboard `/api/ws` JSON-RPC events.
    DashboardWs,
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
    /// Resolved dashboard session token. Never log this value.
    pub dashboard_token: Option<String>,
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
            dashboard_token: Some(String::from("$HERMES_DESKTOP_REMOTE_TOKEN")),
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
            Self::Busy => "Hermes busy",
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
    /// Whether Hermes reports that this session is currently active/running.
    #[serde(default)]
    pub is_active: bool,
    /// Whether Hermes reports that this session is blocked waiting for user input.
    #[serde(default)]
    pub needs_input: bool,
    /// Message count reported by Hermes, when available.
    #[serde(default)]
    pub message_count: Option<u64>,
    /// Recent message/session preview shown by Hermes Desktop session pickers.
    #[serde(default)]
    pub preview: Option<String>,
    /// Source surface reported by Hermes, such as desktop, tui, telegram, or cron.
    #[serde(default)]
    pub source: Option<String>,
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
    /// Command text, when the tool payload exposes one.
    #[serde(default)]
    pub command: Option<String>,
    /// Query/path/input summary, when distinct from the display label.
    #[serde(default)]
    pub input: Option<String>,
    /// Output or result snippet, when Hermes provides one.
    #[serde(default)]
    pub output: Option<String>,
    /// Error snippet, when the tool failed.
    #[serde(default)]
    pub error: Option<String>,
    /// File path or target path associated with the tool.
    #[serde(default)]
    pub path: Option<String>,
    /// URL associated with browser/web tools.
    #[serde(default)]
    pub url: Option<String>,
    /// Whether Hermes reported an inline diff for this tool call.
    #[serde(default)]
    pub has_inline_diff: bool,
    /// Compact JSON representation of unmodeled tool payload fields.
    #[serde(default)]
    pub raw: Option<String>,
}

/// Todo item state reported by Hermes' `todo` tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    /// Not started.
    Pending,
    /// Currently in progress.
    InProgress,
    /// Finished.
    Completed,
    /// Cancelled/skipped.
    Cancelled,
}

/// Todo item reported by Hermes' `todo` tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    /// Stable todo id.
    pub id: String,
    /// User-facing todo text.
    pub content: String,
    /// Current status.
    pub status: TodoStatus,
}

/// Status for a Hermes subagent or delegated task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    /// Waiting to start.
    Queued,
    /// Currently running.
    Running,
    /// Finished successfully.
    Completed,
    /// Failed.
    Failed,
    /// Interrupted or cancelled.
    Interrupted,
}

/// Live status for a Hermes subagent or delegated task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentItem {
    /// Stable subagent id.
    pub id: String,
    /// User-facing task goal.
    pub goal: String,
    /// Current status.
    pub status: SubagentStatus,
    /// Current tool name, when Hermes reports one.
    #[serde(default)]
    pub current_tool: Option<String>,
    /// Child Hermes session id, when available.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Total tasks in a delegated batch.
    #[serde(default)]
    pub task_count: Option<u64>,
    /// Zero-based task index in a delegated batch.
    #[serde(default)]
    pub task_index: Option<u64>,
    /// Latest summary or progress text.
    #[serde(default)]
    pub summary: Option<String>,
}

/// Status for a Hermes background process spawned by a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundProcessStatus {
    /// Process is still running.
    Running,
    /// Process exited successfully.
    Completed,
    /// Process exited with an error.
    Failed,
}

/// Live background process status reported by the Hermes dashboard gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundProcessItem {
    /// Stable process/session id.
    pub id: String,
    /// First line of the command or a human label.
    pub title: String,
    /// Current process state.
    pub status: BackgroundProcessStatus,
    /// Exit code, when the process has exited.
    #[serde(default)]
    pub exit_code: Option<i64>,
    /// Captured output tail, when available.
    #[serde(default)]
    pub output: Option<String>,
}

/// Slash command completion shown by the chat composer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlashCommandSuggestion {
    /// Text inserted into the composer when selected.
    pub insert_text: String,
    /// Short label shown in the suggestion list.
    pub display: String,
    /// Optional command metadata or description.
    #[serde(default)]
    pub description: String,
    /// Optional grouping label, such as Commands, Skills, or Sessions.
    #[serde(default)]
    pub group: String,
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
    /// Reasoning/thinking text exposed by Hermes Agent, when available.
    #[serde(default)]
    pub reasoning: String,
    /// Lifecycle status.
    pub status: MessageStatus,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Attached tool progress.
    #[serde(default)]
    pub tool_events: Vec<ToolEvent>,
}

impl HermesMessage {
    /// Builds a new text message.
    pub fn new(id: impl Into<String>, role: HermesRole, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role,
            content: content.into(),
            reasoning: String::new(),
            status: MessageStatus::Complete,
            created_at: Utc::now(),
            tool_events: Vec::new(),
        }
    }
}

/// Type of pending server prompt surfaced to the user.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    /// Permission request for a tool/action.
    #[default]
    Approval,
    /// Clarifying question from Hermes.
    Clarification,
    /// Sudo password request from Hermes.
    Sudo,
    /// Secret or credential value request from Hermes.
    Secret,
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
    /// Request type.
    #[serde(default)]
    pub kind: ApprovalKind,
}
