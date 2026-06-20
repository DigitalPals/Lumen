use std::{
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};

use futures::StreamExt;
use lumen_core::Property;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::{
    ApprovalRequest, ConnectionConfig, Error, HermesClient, HermesMessage, HermesRole,
    HermesSessionSummary, HermesStatus, LocalHistoryMode, MessageStatus, Result, SseEvent,
    ToolEvent, TransportMode, store::LocalHistoryStore,
};

/// Builder for [`HermesChatService`].
#[derive(Debug)]
pub struct HermesChatServiceBuilder {
    config: ConnectionConfig,
    history_path: Option<PathBuf>,
}

impl HermesChatServiceBuilder {
    /// Creates a builder with default disabled config.
    pub fn new() -> Self {
        Self {
            config: ConnectionConfig::default(),
            history_path: None,
        }
    }

    /// Sets runtime connection config.
    pub fn config(mut self, config: ConnectionConfig) -> Self {
        self.config = config;
        self
    }

    /// Sets local-only history path.
    pub fn history_path(mut self, path: PathBuf) -> Self {
        self.history_path = Some(path);
        self
    }

    /// Builds the service.
    pub fn build(self) -> HermesChatService {
        HermesChatService::new(self.config, self.history_path)
    }
}

impl Default for HermesChatServiceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Reactive Hermes chat service consumed by shell widgets.
#[derive(Debug)]
pub struct HermesChatService {
    config: RwLock<ConnectionConfig>,
    store: Option<Arc<LocalHistoryStore>>,
    stream_token: Mutex<Option<CancellationToken>>,
    active_run_id: Arc<RwLock<Option<String>>>,

    /// Connection status.
    pub status: Property<HermesStatus>,
    /// Raw capabilities JSON advertised by Hermes.
    pub capabilities: Property<Option<Arc<Value>>>,
    /// Server-side sessions, when available.
    pub sessions: Property<Vec<HermesSessionSummary>>,
    /// Active session id.
    pub active_session_id: Property<Option<String>>,
    /// Local-only transcript shown by Lumen.
    pub messages: Property<Vec<HermesMessage>>,
    /// Pending approval/clarification prompt.
    pub approval: Property<Option<ApprovalRequest>>,
    /// Last user-facing error.
    pub last_error: Property<Option<String>>,
}

impl HermesChatService {
    /// Returns a builder.
    pub fn builder() -> HermesChatServiceBuilder {
        HermesChatServiceBuilder::new()
    }

    fn new(config: ConnectionConfig, history_path: Option<PathBuf>) -> Self {
        let store = history_path.map(LocalHistoryStore::new).map(Arc::new);
        let (active_session_id, messages) = store
            .as_ref()
            .and_then(|store| store.load().ok())
            .unwrap_or((None, Vec::new()));
        let status = if config.enabled {
            HermesStatus::Connecting
        } else {
            HermesStatus::Disabled
        };
        let service = Self {
            config: RwLock::new(config),
            store,
            stream_token: Mutex::new(None),
            active_run_id: Arc::new(RwLock::new(None)),
            status: Property::new(status),
            capabilities: Property::new(None),
            sessions: Property::new(Vec::new()),
            active_session_id: Property::new(active_session_id),
            messages: Property::new(messages),
            approval: Property::new(None),
            last_error: Property::new(None),
        };
        service.connect();
        service
    }

    /// Replaces runtime config, preserving transcript, and reconnects.
    pub fn update_config(&self, config: ConnectionConfig) {
        if let Ok(mut guard) = self.config.write() {
            if *guard == config {
                return;
            }
            *guard = config;
        }
        self.connect();
    }

    /// Connects/refreshes capabilities and sessions in the background.
    pub fn connect(&self) {
        let config = self.config();
        if !config.enabled {
            self.status.set(HermesStatus::Disabled);
            return;
        }
        if config.api_key.as_deref().unwrap_or_default().is_empty()
            || config
                .api_key
                .as_deref()
                .is_some_and(|key| key.starts_with('$'))
        {
            self.status.set(HermesStatus::MissingApiKey);
            return;
        }

        self.status.set(HermesStatus::Connecting);
        let status = self.status.clone();
        let capabilities_prop = self.capabilities.clone();
        let sessions_prop = self.sessions.clone();
        let active_prop = self.active_session_id.clone();
        let error_prop = self.last_error.clone();
        tokio::spawn(async move {
            match connect_inner(config).await {
                Ok((capabilities, sessions)) => {
                    capabilities_prop.set(Some(Arc::new(capabilities)));
                    if active_prop.get().is_none() {
                        active_prop.set(sessions.first().map(|session| session.id.clone()));
                    }
                    sessions_prop.set(sessions);
                    error_prop.set(None);
                    status.set(HermesStatus::Connected);
                }
                Err(err) => {
                    let message = err.short_message();
                    error_prop.set(Some(message.clone()));
                    status.set(status_from_error(&err, message));
                }
            }
        });
    }

    /// Creates a new server session and selects it.
    pub fn new_session(&self, title: Option<String>) {
        let config = self.config();
        let status = self.status.clone();
        let sessions_prop = self.sessions.clone();
        let active_prop = self.active_session_id.clone();
        let messages_prop = self.messages.clone();
        let store = self.store.clone();
        tokio::spawn(async move {
            status.set(HermesStatus::Connecting);
            match HermesClient::new(config) {
                Ok(client) => match client.create_session(title.as_deref()).await {
                    Ok(session) => {
                        let mut sessions = sessions_prop.get();
                        sessions.insert(0, session.clone());
                        sessions_prop.set(sessions);
                        active_prop.set(Some(session.id.clone()));
                        messages_prop.set(Vec::new());
                        if let Some(store) = store.as_ref() {
                            store.set_active_session_id(Some(session.id));
                            if let Err(err) = store.save(&[]) {
                                warn!(error = %err, "could not save Hermes local history");
                            }
                        }
                        status.set(HermesStatus::Connected);
                    }
                    Err(err) => status.set(status_from_error(&err, err.short_message())),
                },
                Err(err) => status.set(status_from_error(&err, err.short_message())),
            }
        });
    }

    /// Selects a session and loads its server messages while preserving local history on disk.
    pub fn select_session(&self, session_id: String) {
        self.active_session_id.set(Some(session_id.clone()));
        if let Some(store) = self.store.as_ref() {
            store.set_active_session_id(Some(session_id.clone()));
        }
        let config = self.config();
        let messages = self.messages.clone();
        let status = self.status.clone();
        let store = self.store.clone();
        tokio::spawn(async move {
            match HermesClient::new(config) {
                Ok(client) => match client.session_messages(&session_id).await {
                    Ok(remote_messages) if !remote_messages.is_empty() => {
                        messages.set(remote_messages.clone());
                        if let Some(store) = store.as_ref()
                            && let Err(err) = store.save(&remote_messages)
                        {
                            warn!(error = %err, "could not save Hermes local history");
                        }
                    }
                    Ok(_) => {}
                    Err(err) => status.set(status_from_error(&err, err.short_message())),
                },
                Err(err) => status.set(status_from_error(&err, err.short_message())),
            }
        });
    }

    /// Sends a message and streams the answer.
    pub fn send_message(&self, content: String) {
        let content = content.trim().to_owned();
        if content.is_empty() {
            return;
        }
        self.stop_current();
        let config = self.config();
        let token = CancellationToken::new();
        if let Ok(mut guard) = self.stream_token.lock() {
            *guard = Some(token.clone());
        }

        let user_id = format!("local-user-{}", chrono::Utc::now().timestamp_millis());
        let assistant_id = format!("local-assistant-{}", chrono::Utc::now().timestamp_millis());
        let mut current = self.messages.get();
        current.push(HermesMessage::new(
            user_id,
            HermesRole::User,
            content.clone(),
        ));
        let mut placeholder = HermesMessage::new(&assistant_id, HermesRole::Assistant, "");
        placeholder.status = MessageStatus::Streaming;
        current.push(placeholder);
        trim_history(&mut current, config.history_limit);
        self.messages.set(current.clone());
        self.persist(&config, &current);

        let status = self.status.clone();
        let messages_prop = self.messages.clone();
        let active_prop = self.active_session_id.clone();
        let sessions_prop = self.sessions.clone();
        let approval_prop = self.approval.clone();
        let last_error = self.last_error.clone();
        let store = self.store.clone();
        let active_run_id = self.active_run_id.clone();
        tokio::spawn(async move {
            status.set(HermesStatus::Busy);
            let result = stream_message(StreamContext {
                config: config.clone(),
                content,
                assistant_id: assistant_id.clone(),
                token: token.clone(),
                messages: messages_prop.clone(),
                active_session_id: active_prop.clone(),
                sessions: sessions_prop.clone(),
                approval: approval_prop,
                active_run_id: active_run_id.clone(),
            })
            .await;
            if token.is_cancelled() {
                mark_message(
                    &messages_prop,
                    &assistant_id,
                    MessageStatus::Stopped,
                    None,
                    config.history_limit,
                );
                status.set(HermesStatus::Connected);
            } else if let Err(err) = result {
                let message = err.short_message();
                append_error(&messages_prop, message.clone(), config.history_limit);
                last_error.set(Some(message.clone()));
                status.set(status_from_error(&err, message));
            } else {
                mark_message(
                    &messages_prop,
                    &assistant_id,
                    MessageStatus::Complete,
                    None,
                    config.history_limit,
                );
                status.set(HermesStatus::Connected);
            }
            if let Some(store) = store.as_ref()
                && config.local_history == LocalHistoryMode::Full
                && let Err(err) = store.save(&messages_prop.get())
            {
                warn!(error = %err, "could not save Hermes local history");
            }
            if let Ok(mut guard) = active_run_id.write() {
                *guard = None;
            }
        });
    }

    /// Cancels the active stream and asks Hermes to stop any tracked run.
    pub fn stop_current(&self) {
        if let Ok(mut guard) = self.stream_token.lock()
            && let Some(token) = guard.take()
        {
            token.cancel();
        }
        let run_id = self
            .active_run_id
            .read()
            .ok()
            .and_then(|guard| guard.clone());
        if let Some(run_id) = run_id {
            let config = self.config();
            tokio::spawn(async move {
                match HermesClient::new(config) {
                    Ok(client) => {
                        if let Err(err) = client.stop_run(&run_id).await {
                            debug!(error = %err, "Hermes run stop failed");
                        }
                    }
                    Err(err) => debug!(error = %err, "Hermes run stop client build failed"),
                }
            });
        }
    }

    /// Responds to a pending approval request.
    pub fn submit_approval(&self, approved: bool, message: Option<String>) {
        let Some(approval) = self.approval.get() else {
            return;
        };
        let config = self.config();
        let approval_prop = self.approval.clone();
        let last_error = self.last_error.clone();
        tokio::spawn(async move {
            match HermesClient::new(config) {
                Ok(client) => match client
                    .submit_approval(&approval.run_id, approved, message.as_deref())
                    .await
                {
                    Ok(()) => approval_prop.set(None),
                    Err(err) => last_error.set(Some(err.short_message())),
                },
                Err(err) => last_error.set(Some(err.short_message())),
            }
        });
    }

    fn config(&self) -> ConnectionConfig {
        self.config
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn persist(&self, config: &ConnectionConfig, messages: &[HermesMessage]) {
        if config.local_history != LocalHistoryMode::Full {
            return;
        }
        if let Some(store) = self.store.as_ref()
            && let Err(err) = store.save(messages)
        {
            warn!(error = %err, "could not save Hermes local history");
        }
    }
}

async fn connect_inner(config: ConnectionConfig) -> Result<(Value, Vec<HermesSessionSummary>)> {
    let client = HermesClient::new(config)?;
    client.health().await?;
    let capabilities = client.capabilities().await?;
    let sessions = client.list_sessions().await.unwrap_or_default();
    Ok((capabilities, sessions))
}

struct StreamContext {
    config: ConnectionConfig,
    content: String,
    assistant_id: String,
    token: CancellationToken,
    messages: Property<Vec<HermesMessage>>,
    active_session_id: Property<Option<String>>,
    sessions: Property<Vec<HermesSessionSummary>>,
    approval: Property<Option<ApprovalRequest>>,
    active_run_id: Arc<RwLock<Option<String>>>,
}

async fn stream_message(ctx: StreamContext) -> Result<()> {
    let client = HermesClient::new(ctx.config.clone())?;
    let mut session_id = ctx.active_session_id.get();
    if session_id.is_none()
        && matches!(
            ctx.config.transport_mode,
            TransportMode::Auto | TransportMode::Sessions
        )
    {
        match client.create_session(Some("Lumen Chat")).await {
            Ok(session) => {
                session_id = Some(session.id.clone());
                ctx.active_session_id.set(Some(session.id.clone()));
                let mut current_sessions = ctx.sessions.get();
                current_sessions.insert(0, session);
                ctx.sessions.set(current_sessions);
            }
            Err(err) if matches!(ctx.config.transport_mode, TransportMode::Sessions) => {
                return Err(err);
            }
            Err(_) => {}
        }
    }

    let mut stream = if matches!(ctx.config.transport_mode, TransportMode::Runs) {
        let run_id = client
            .start_run(&ctx.content)
            .await?
            .ok_or_else(|| Error::UnsupportedEvent(String::from("missing run id")))?;
        if let Ok(mut guard) = ctx.active_run_id.write() {
            *guard = Some(run_id.clone());
        }
        client.stream_run_events(&run_id).await?
    } else if matches!(ctx.config.transport_mode, TransportMode::ChatCompletions) {
        client.stream_chat_completions(&ctx.messages.get()).await?
    } else if let Some(session_id) = session_id.as_deref() {
        client.stream_session_chat(session_id, &ctx.content).await?
    } else if matches!(ctx.config.transport_mode, TransportMode::Auto) {
        client.stream_chat_completions(&ctx.messages.get()).await?
    } else {
        return Err(Error::UnsupportedEvent(String::from("missing session")));
    };

    while let Some(event) = stream.next().await {
        if ctx.token.is_cancelled() {
            break;
        }
        apply_event(
            &ctx.messages,
            &ctx.assistant_id,
            event?,
            ctx.config.history_limit,
            &ctx.approval,
        )?;
    }
    Ok(())
}

fn apply_event(
    messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    event: SseEvent,
    limit: usize,
    approval: &Property<Option<ApprovalRequest>>,
) -> Result<()> {
    if event.data.trim() == "[DONE]" || event.event.as_deref() == Some("done") {
        return Ok(());
    }
    let event_name = event.event.as_deref().unwrap_or("message");
    match event_name {
        "assistant.delta" | "message.delta" | "response.output_text.delta" => {
            let delta = json_text_delta(&event.data)?;
            append_delta(messages, assistant_id, &delta, limit);
        }
        "assistant.completed" | "run.completed" | "message.completed" => {
            mark_message(messages, assistant_id, MessageStatus::Complete, None, limit);
        }
        "tool.progress" | "tool.started" | "tool.completed" | "hermes.tool.progress" => {
            if let Some(tool_event) = parse_tool_event(&event.data, event_name)? {
                push_tool_event(messages, assistant_id, tool_event, limit);
            }
        }
        "approval.required" | "run.requires_approval" | "clarify.required" => {
            let value: Value = serde_json::from_str(&event.data)?;
            let run_id = value
                .get("run_id")
                .or_else(|| value.get("runId"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let prompt = value
                .get("prompt")
                .or_else(|| value.get("message"))
                .or_else(|| value.get("question"))
                .and_then(Value::as_str)
                .unwrap_or("Hermes is requesting approval")
                .to_owned();
            approval.set(Some(ApprovalRequest {
                run_id,
                approval_id: value
                    .get("approval_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                prompt,
            }));
        }
        "message" => {
            let value: Value = serde_json::from_str(&event.data)?;
            if let Some(delta) = openai_delta(&value) {
                append_delta(messages, assistant_id, &delta, limit);
            } else if value.get("object").and_then(Value::as_str) == Some("chat.completion.chunk") {
                return Ok(());
            } else if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                append_delta(messages, assistant_id, delta, limit);
            }
        }
        "run.started" | "message.started" => {}
        "error" => {
            let value: Value =
                serde_json::from_str(&event.data).unwrap_or(Value::String(event.data));
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| value.as_str())
                .unwrap_or("Hermes stream error")
                .to_owned();
            append_error(messages, message, limit);
        }
        other => {
            debug!(event = other, "Ignoring Hermes SSE event");
        }
    }
    Ok(())
}

fn json_text_delta(data: &str) -> Result<String> {
    let value: Value = serde_json::from_str(data)?;
    Ok(value
        .get("delta")
        .or_else(|| value.get("content"))
        .or_else(|| value.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned())
}

fn openai_delta(value: &Value) -> Option<String> {
    value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn parse_tool_event(data: &str, event_name: &str) -> Result<Option<ToolEvent>> {
    let value: Value = serde_json::from_str(data)?;
    let tool = value
        .get("tool")
        .or_else(|| value.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_owned();
    let label = value
        .get("label")
        .or_else(|| value.get("message"))
        .or_else(|| value.get("content"))
        .and_then(Value::as_str)
        .unwrap_or(&tool)
        .to_owned();
    let id = value
        .get("toolCallId")
        .or_else(|| value.get("tool_call_id"))
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .unwrap_or(&tool)
        .to_owned();
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or(match event_name {
            "tool.started" => "running",
            "tool.completed" => "completed",
            _ => "running",
        })
        .to_owned();
    Ok(Some(ToolEvent {
        id,
        tool,
        label,
        status,
    }))
}

fn append_delta(
    messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    delta: &str,
    limit: usize,
) {
    if delta.is_empty() {
        return;
    }
    let mut current = messages.get();
    if let Some(message) = current
        .iter_mut()
        .find(|message| message.id == assistant_id)
    {
        message.content.push_str(delta);
        message.status = MessageStatus::Streaming;
    }
    trim_history(&mut current, limit);
    messages.replace(current);
}

fn push_tool_event(
    messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    event: ToolEvent,
    limit: usize,
) {
    let mut current = messages.get();
    if let Some(message) = current
        .iter_mut()
        .find(|message| message.id == assistant_id)
    {
        if let Some(existing) = message
            .tool_events
            .iter_mut()
            .find(|existing| existing.id == event.id)
        {
            *existing = event;
        } else {
            message.tool_events.push(event);
        }
    }
    trim_history(&mut current, limit);
    messages.replace(current);
}

fn mark_message(
    messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    status: MessageStatus,
    content: Option<String>,
    limit: usize,
) {
    let mut current = messages.get();
    if let Some(message) = current
        .iter_mut()
        .find(|message| message.id == assistant_id)
    {
        message.status = status;
        if let Some(content) = content {
            message.content = content;
        }
    }
    trim_history(&mut current, limit);
    messages.replace(current);
}

fn append_error(messages: &Property<Vec<HermesMessage>>, message: String, limit: usize) {
    let mut current = messages.get();
    let mut row = HermesMessage::new(
        format!("local-error-{}", chrono::Utc::now().timestamp_millis()),
        HermesRole::Error,
        message,
    );
    row.status = MessageStatus::Error;
    current.push(row);
    trim_history(&mut current, limit);
    messages.replace(current);
}

fn trim_history(messages: &mut Vec<HermesMessage>, limit: usize) {
    if limit == 0 || messages.len() <= limit {
        return;
    }
    let remove = messages.len() - limit;
    messages.drain(0..remove);
}

fn status_from_error(err: &Error, message: String) -> HermesStatus {
    match err {
        Error::MissingApiKey => HermesStatus::MissingApiKey,
        Error::Api { status: 401, .. } => HermesStatus::AuthFailed,
        Error::Http(_) => HermesStatus::Offline(message),
        _ => HermesStatus::Error(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SseEvent;

    #[test]
    fn applies_openai_delta() {
        let messages = Property::new(vec![HermesMessage::new("a", HermesRole::Assistant, "")]);
        apply_event(
            &messages,
            "a",
            SseEvent {
                event: None,
                data: String::from(
                    r#"{"object":"chat.completion.chunk","choices":[{"delta":{"content":"hi"}}]}"#,
                ),
            },
            10,
            &Property::new(None),
        )
        .expect("event applies");
        assert_eq!(messages.get()[0].content, "hi");
    }

    #[test]
    fn updates_tool_event_by_id() {
        let messages = Property::new(vec![HermesMessage::new("a", HermesRole::Assistant, "")]);
        let approval = Property::new(None);
        apply_event(
            &messages,
            "a",
            SseEvent {
                event: Some(String::from("tool.progress")),
                data: String::from(r#"{"tool":"terminal","toolCallId":"t1","status":"running"}"#),
            },
            10,
            &approval,
        )
        .expect("first applies");
        apply_event(
            &messages,
            "a",
            SseEvent {
                event: Some(String::from("tool.completed")),
                data: String::from(r#"{"tool":"terminal","toolCallId":"t1","status":"completed"}"#),
            },
            10,
            &approval,
        )
        .expect("second applies");
        let message = messages.get().remove(0);
        assert_eq!(message.tool_events.len(), 1);
        assert_eq!(message.tool_events[0].status, "completed");
    }
}
