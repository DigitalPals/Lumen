use std::{collections::HashMap, pin::Pin, time::Duration};

use async_stream::try_stream;
use futures::{Stream, StreamExt};
use reqwest::{Client, Method, header};
use serde_json::{Value, json};

use crate::{
    ChatAttachment, ConnectionConfig, Error, HermesMessage, HermesRole, HermesSessionSummary,
    MessageStatus, Result, SseDecoder, SseEvent, ToolEvent,
};

/// Stream type used for decoded SSE events.
pub type EventStream = Pin<Box<dyn Stream<Item = Result<SseEvent>> + Send>>;

/// HTTP client for Hermes Agent API server.
#[derive(Debug, Clone)]
pub struct HermesClient {
    http: Client,
    config: ConnectionConfig,
    base_url: String,
}

impl HermesClient {
    /// Builds a client from resolved connection config.
    ///
    /// # Errors
    /// Returns an error if the endpoint URL is empty or invalid.
    pub fn new(config: ConnectionConfig) -> Result<Self> {
        if !config.enabled {
            return Err(Error::InvalidEndpoint(String::from("module disabled")));
        }
        let base_url = normalize_endpoint_url(&config.endpoint_url)?;
        let timeout = Duration::from_secs(config.timeout_seconds.max(1));
        // Do not use a client-wide timeout: reqwest applies it to the whole
        // SSE response lifetime, which can cancel long-running Hermes turns.
        let http = Client::builder().connect_timeout(timeout).build()?;
        Ok(Self {
            http,
            config,
            base_url,
        })
    }

    /// Returns redaction-safe base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn request_timeout(&self) -> Duration {
        Duration::from_secs(self.config.timeout_seconds.max(1))
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let mut request = self.http.request(method, self.url(path));
        if let Some(key) = self.config.api_key.as_ref().filter(|key| !key.is_empty()) {
            request = request.bearer_auth(key);
        }
        if let Some(session_key) = self
            .config
            .session_key
            .as_ref()
            .filter(|key| !key.is_empty())
        {
            request = request.header("X-Hermes-Session-Key", session_key);
        }
        request
    }

    /// Checks unauthenticated server health.
    ///
    /// # Errors
    /// Returns an error if the server is unreachable or reports an error status.
    pub async fn health(&self) -> Result<()> {
        let response = self
            .http
            .get(self.url("/health"))
            .timeout(self.request_timeout())
            .send()
            .await?;
        check_status(response).await.map(|_| ())
    }

    /// Fetches API capabilities.
    ///
    /// # Errors
    /// Returns an error on HTTP or JSON failure.
    pub async fn capabilities(&self) -> Result<Value> {
        self.get_json("/v1/capabilities").await
    }

    /// Fetches advertised models.
    ///
    /// # Errors
    /// Returns an error on HTTP or JSON failure.
    pub async fn models(&self) -> Result<Value> {
        self.get_json("/v1/models").await
    }

    /// Lists server-side sessions.
    ///
    /// # Errors
    /// Returns an error on HTTP or JSON failure.
    pub async fn list_sessions(&self) -> Result<Vec<HermesSessionSummary>> {
        let value = self
            .get_json("/api/sessions?limit=50&offset=0&order=recent")
            .await?;
        Ok(parse_sessions(&value))
    }

    /// Creates a server-side session.
    ///
    /// # Errors
    /// Returns an error on HTTP or JSON failure.
    pub async fn create_session(&self, title: Option<&str>) -> Result<HermesSessionSummary> {
        let mut body = json!({});
        if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
            body["title"] = json!(title);
        }
        let value = self.post_json("/api/sessions", &body).await?;
        parse_session(&value).ok_or_else(|| Error::UnsupportedEvent(String::from("session.create")))
    }

    /// Gets messages for a server-side session.
    ///
    /// # Errors
    /// Returns an error on HTTP or JSON failure.
    pub async fn session_messages(&self, session_id: &str) -> Result<Vec<HermesMessage>> {
        let value = self
            .get_json(&format!("/api/sessions/{session_id}/messages"))
            .await?;
        Ok(parse_messages(&value))
    }

    /// Opens a native session chat SSE stream.
    ///
    /// # Errors
    /// Returns an error if the request cannot be started.
    pub async fn stream_session_chat(
        &self,
        session_id: &str,
        content: &str,
    ) -> Result<EventStream> {
        self.stream_session_chat_with_attachments(session_id, content, &[])
            .await
    }

    /// Opens a native session chat SSE stream with inline attachments.
    ///
    /// # Errors
    /// Returns an error if the request cannot be started.
    pub async fn stream_session_chat_with_attachments(
        &self,
        session_id: &str,
        content: &str,
        attachments: &[ChatAttachment],
    ) -> Result<EventStream> {
        let body = json!({
            "message": content,
            "content": chat_content_value(content, attachments),
            "stream": true,
        });
        self.post_sse(&format!("/api/sessions/{session_id}/chat/stream"), &body)
            .await
    }

    /// Sends a native session chat turn and waits for the completed answer.
    ///
    /// # Errors
    /// Returns an error if the request fails.
    pub async fn session_chat(&self, session_id: &str, content: &str) -> Result<Value> {
        let body = json!({"message": content, "content": content, "stream": false});
        self.post_json(&format!("/api/sessions/{session_id}/chat"), &body)
            .await
    }

    /// Opens an OpenAI-compatible chat completion SSE stream.
    ///
    /// # Errors
    /// Returns an error if the request cannot be started.
    pub async fn stream_chat_completions(&self, messages: &[HermesMessage]) -> Result<EventStream> {
        self.stream_chat_completions_with_attachments(messages, &[])
            .await
    }

    /// Opens an OpenAI-compatible chat completion SSE stream with inline attachments
    /// on the latest user message.
    ///
    /// # Errors
    /// Returns an error if the request cannot be started.
    pub async fn stream_chat_completions_with_attachments(
        &self,
        messages: &[HermesMessage],
        attachments: &[ChatAttachment],
    ) -> Result<EventStream> {
        let api_messages = chat_completion_messages(messages, attachments);
        let body = json!({"model": self.config.model, "messages": api_messages, "stream": true});
        self.post_sse("/v1/chat/completions", &body).await
    }

    /// Sends an OpenAI-compatible chat completion request and waits for the completed answer.
    ///
    /// # Errors
    /// Returns an error if the request fails.
    pub async fn chat_completions(&self, messages: &[HermesMessage]) -> Result<Value> {
        let api_messages = chat_completion_messages(messages, &[]);
        let body = json!({"model": self.config.model, "messages": api_messages, "stream": false});
        self.post_json("/v1/chat/completions", &body).await
    }

    /// Starts a run and returns the run id when the server supports it.
    ///
    /// # Errors
    /// Returns an error on HTTP or JSON failure.
    pub async fn start_run(&self, prompt: &str) -> Result<Option<String>> {
        let value = self
            .post_json("/v1/runs", &json!({"input": prompt}))
            .await?;
        Ok(value
            .get("run_id")
            .or_else(|| value.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned))
    }

    /// Opens the SSE event stream for a submitted run.
    ///
    /// # Errors
    /// Returns an error if the request cannot be started.
    pub async fn stream_run_events(&self, run_id: &str) -> Result<EventStream> {
        self.get_sse(&format!("/v1/runs/{run_id}/events")).await
    }

    /// Requests a remote run stop.
    ///
    /// # Errors
    /// Returns an error if the stop request fails.
    pub async fn stop_run(&self, run_id: &str) -> Result<()> {
        let response = self
            .request(Method::POST, &format!("/v1/runs/{run_id}/stop"))
            .timeout(self.request_timeout())
            .json(&json!({}))
            .send()
            .await?;
        check_status(response).await.map(|_| ())
    }

    /// Responds to a pending approval.
    ///
    /// # Errors
    /// Returns an error if Hermes rejects the approval response.
    pub async fn submit_approval(
        &self,
        run_id: &str,
        approval_id: Option<&str>,
        approved: bool,
        text: Option<&str>,
    ) -> Result<()> {
        let response = self
            .request(Method::POST, &format!("/v1/runs/{run_id}/approval"))
            .timeout(self.request_timeout())
            .json(&approval_body(approved, text, approval_id))
            .send()
            .await?;
        check_status(response).await.map(|_| ())
    }

    async fn get_json(&self, path: &str) -> Result<Value> {
        let response = self
            .request(Method::GET, path)
            .timeout(self.request_timeout())
            .send()
            .await?;
        let response = check_status(response).await?;
        Ok(response.json().await?)
    }

    async fn post_json(&self, path: &str, body: &Value) -> Result<Value> {
        let response = self
            .request(Method::POST, path)
            .timeout(self.request_timeout())
            .json(body)
            .send()
            .await?;
        let response = check_status(response).await?;
        Ok(response.json().await?)
    }

    async fn get_sse(&self, path: &str) -> Result<EventStream> {
        let response = self
            .request(Method::GET, path)
            .header(header::ACCEPT, "text/event-stream")
            .send()
            .await?;
        let response = check_status(response).await?;
        let mut bytes = response.bytes_stream();
        let stream = try_stream! {
            let mut decoder = SseDecoder::default();
            while let Some(chunk) = bytes.next().await {
                let chunk = chunk?;
                let text = String::from_utf8_lossy(&chunk);
                for event in decoder.push(&text) {
                    yield event;
                }
            }
            for event in decoder.finish() {
                yield event;
            }
        };
        Ok(Box::pin(stream))
    }

    async fn post_sse(&self, path: &str, body: &Value) -> Result<EventStream> {
        let response = self
            .request(Method::POST, path)
            .header(header::ACCEPT, "text/event-stream")
            .json(body)
            .send()
            .await?;
        let response = check_status(response).await?;
        let mut bytes = response.bytes_stream();
        let stream = try_stream! {
            let mut decoder = SseDecoder::default();
            while let Some(chunk) = bytes.next().await {
                let chunk = chunk?;
                let text = String::from_utf8_lossy(&chunk);
                for event in decoder.push(&text) {
                    yield event;
                }
            }
            for event in decoder.finish() {
                yield event;
            }
        };
        Ok(Box::pin(stream))
    }
}

fn approval_body(approved: bool, text: Option<&str>, approval_id: Option<&str>) -> Value {
    let mut body = json!({"approved": approved, "message": text.unwrap_or_default()});
    if let Some(approval_id) = approval_id.filter(|id| !id.trim().is_empty()) {
        body["approval_id"] = json!(approval_id);
    }
    body
}

fn chat_completion_messages(
    messages: &[HermesMessage],
    attachments: &[ChatAttachment],
) -> Vec<Value> {
    let latest_user_index = messages
        .iter()
        .rposition(|message| message.role == HermesRole::User);
    messages
        .iter()
        .enumerate()
        .filter(|(_, message)| {
            matches!(
                message.role,
                HermesRole::User | HermesRole::Assistant | HermesRole::System
            ) && !message.content.trim().is_empty()
        })
        .map(|(index, message)| {
            let content = if Some(index) == latest_user_index {
                chat_content_value(&message.content, attachments)
            } else {
                json!(message.content)
            };
            json!({
                "role": match message.role {
                    HermesRole::User => "user",
                    HermesRole::Assistant => "assistant",
                    HermesRole::System => "system",
                    HermesRole::Tool | HermesRole::Error => "system",
                },
                "content": content,
            })
        })
        .collect()
}

fn chat_content_value(content: &str, attachments: &[ChatAttachment]) -> Value {
    if attachments.is_empty() {
        return json!(content);
    }

    let text = content_with_text_attachments(content, attachments);
    let mut parts = vec![json!({"type": "text", "text": text})];
    for attachment in attachments
        .iter()
        .filter(|attachment| attachment.image_url.is_some())
    {
        if let Some(image_url) = &attachment.image_url {
            parts.push(json!({
                "type": "image_url",
                "image_url": {"url": image_url},
            }));
        }
    }
    json!(parts)
}

fn content_with_text_attachments(content: &str, attachments: &[ChatAttachment]) -> String {
    let mut text = content.to_owned();
    for attachment in attachments
        .iter()
        .filter(|attachment| attachment.text.is_some())
    {
        if let Some(body) = &attachment.text {
            text.push_str("\n\nAttached document: ");
            text.push_str(&attachment.name);
            text.push_str(" (");
            text.push_str(&attachment.mime_type);
            text.push_str(")\n```\n");
            text.push_str(body);
            text.push_str("\n```");
        }
    }
    text
}

/// Normalizes a Hermes endpoint to scheme/host/base path without a trailing `/v1`.
///
/// # Errors
/// Returns an error for empty or unsupported URLs.
pub fn normalize_endpoint_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() || !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err(Error::InvalidEndpoint(raw.to_owned()));
    }
    Ok(trimmed.strip_suffix("/v1").unwrap_or(trimmed).to_owned())
}

pub(crate) async fn check_status(response: reqwest::Response) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let status_code = status.as_u16();
    let text = response.text().await.unwrap_or_default();
    let message = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| text.chars().take(240).collect());
    Err(Error::Api {
        status: status_code,
        message,
    })
}

pub(crate) fn parse_sessions(value: &Value) -> Vec<HermesSessionSummary> {
    let array = value
        .get("sessions")
        .or_else(|| value.get("data"))
        .and_then(Value::as_array)
        .or_else(|| value.as_array());
    array
        .into_iter()
        .flatten()
        .filter_map(parse_session)
        .collect()
}

pub(crate) fn parse_session(value: &Value) -> Option<HermesSessionSummary> {
    if let Some(session) = value.get("session") {
        return parse_session(session);
    }

    let id = value
        .get("id")
        .or_else(|| value.get("stored_session_id"))
        .or_else(|| value.get("storedSessionId"))
        .or_else(|| value.get("session_key"))
        .or_else(|| value.get("sessionKey"))
        .or_else(|| value.get("session_id"))
        .and_then(Value::as_str)?;
    let title = session_label(value).unwrap_or_else(|| String::from("Hermes Chat"));
    let updated_at = value
        .get("updated_at")
        .or_else(|| value.get("updatedAt"))
        .and_then(Value::as_str)
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        .map(|time| time.with_timezone(&chrono::Utc))
        .or_else(|| parse_unix_timestamp(value.get("updated_at")))
        .or_else(|| parse_unix_timestamp(value.get("updatedAt")))
        .or_else(|| parse_unix_timestamp(value.get("started_at")))
        .or_else(|| parse_unix_timestamp(value.get("startedAt")));
    Some(HermesSessionSummary {
        id: id.to_owned(),
        title,
        updated_at,
        is_active: value
            .get("is_active")
            .or_else(|| value.get("isActive"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        needs_input: value
            .get("needs_input")
            .or_else(|| value.get("needsInput"))
            .or_else(|| value.get("awaiting_input"))
            .or_else(|| value.get("awaitingInput"))
            .or_else(|| value.get("blocked"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        message_count: value
            .get("message_count")
            .or_else(|| value.get("messageCount"))
            .and_then(Value::as_u64),
        preview: session_preview(value),
        source: value
            .get("source")
            .and_then(Value::as_str)
            .filter(|source| !source.trim().is_empty())
            .map(str::to_owned),
    })
}

fn session_label(value: &Value) -> Option<String> {
    ["title", "name", "preview", "message_preview"]
        .iter()
        .filter_map(|field| value.get(*field).and_then(Value::as_str))
        .map(str::trim)
        .find(|label| !label.is_empty())
        .map(str::to_owned)
}

fn session_preview(value: &Value) -> Option<String> {
    [
        "preview",
        "message_preview",
        "messagePreview",
        "last_message",
        "lastMessage",
    ]
    .iter()
    .filter_map(|field| value.get(*field).and_then(Value::as_str))
    .map(str::trim)
    .find(|preview| !preview.is_empty())
    .map(str::to_owned)
}

fn parse_unix_timestamp(value: Option<&Value>) -> Option<chrono::DateTime<chrono::Utc>> {
    let seconds = value.and_then(Value::as_f64)?;
    if !seconds.is_finite() {
        return None;
    }
    let whole = seconds.trunc() as i64;
    let nanos = ((seconds.fract().abs()) * 1_000_000_000.0).round() as u32;
    chrono::DateTime::from_timestamp(whole, nanos)
}

pub(crate) fn parse_messages(value: &Value) -> Vec<HermesMessage> {
    let Some(array) = value
        .get("messages")
        .or_else(|| value.get("data"))
        .and_then(Value::as_array)
        .or_else(|| value.as_array())
    else {
        return Vec::new();
    };

    let mut messages = Vec::new();
    let mut tool_targets = HashMap::new();
    let mut last_assistant_index = None;

    for (index, item) in array.iter().enumerate() {
        if message_role(item) == HermesRole::Tool {
            if let Some(event) = tool_row_event(index, item) {
                attach_tool_event(
                    &mut messages,
                    &mut tool_targets,
                    last_assistant_index,
                    index,
                    event,
                );
            }
            continue;
        }

        let Some(mut message) = parse_message(index, item) else {
            continue;
        };
        if message.role == HermesRole::Assistant {
            let message_index = messages.len();
            for event in tool_call_events(item) {
                tool_targets.insert(event.id.clone(), message_index);
                message.tool_events.push(event);
            }
            last_assistant_index = Some(message_index);
        }
        messages.push(message);
    }

    messages
        .into_iter()
        .filter(should_show_loaded_history_message)
        .collect()
}

fn should_show_loaded_history_message(message: &HermesMessage) -> bool {
    match message.role {
        HermesRole::User => !is_context_compaction_message(&message.content),
        HermesRole::Assistant => !message.content.trim().is_empty(),
        HermesRole::System | HermesRole::Tool | HermesRole::Error => {
            !message.content.trim().is_empty()
        }
    }
}

fn is_context_compaction_message(content: &str) -> bool {
    content.trim_start().starts_with("[CONTEXT COMPACTION")
}

fn parse_message(index: usize, value: &Value) -> Option<HermesMessage> {
    let content = message_text(value);
    let role = message_role(value);
    let id = string_field(value, &["id"]).unwrap_or_else(|| format!("remote-{index}"));
    let mut message = HermesMessage::new(id, role, content);
    message.reasoning = value
        .get("reasoning")
        .or_else(|| value.get("reasoning_content"))
        .or_else(|| value.get("reasoningContent"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    message.status = MessageStatus::Complete;
    Some(message)
}

fn message_role(value: &Value) -> HermesRole {
    match value
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("assistant")
    {
        "user" => HermesRole::User,
        "system" => HermesRole::System,
        "tool" => HermesRole::Tool,
        "error" => HermesRole::Error,
        _ => HermesRole::Assistant,
    }
}

fn message_text(value: &Value) -> String {
    value
        .get("content")
        .and_then(content_text)
        .or_else(|| value.get("text").and_then(content_text))
        .unwrap_or_default()
}

fn content_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_owned());
    }

    if let Some(array) = value.as_array() {
        let text = array
            .iter()
            .filter_map(content_text)
            .collect::<Vec<_>>()
            .join("");
        return (!text.is_empty()).then_some(text);
    }

    if let Some(object) = value.as_object() {
        for key in ["text", "content", "value"] {
            if let Some(text) = object.get(key).and_then(content_text) {
                return Some(text);
            }
        }
    }

    None
}

fn tool_call_events(value: &Value) -> Vec<ToolEvent> {
    value
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(tool_call_event)
        .collect()
}

fn tool_call_event(value: &Value) -> Option<ToolEvent> {
    let id = string_field(
        value,
        &[
            "id",
            "call_id",
            "tool_call_id",
            "toolCallId",
            "tool_use_id",
            "toolUseId",
        ],
    )?;
    let function = value.get("function");
    let tool = function
        .and_then(|function| string_field(function, &["name", "tool_name", "toolName"]))
        .or_else(|| string_field(value, &["name", "tool_name", "toolName", "function"]))
        .unwrap_or_else(|| String::from("tool"));
    let arguments = function
        .and_then(|function| function.get("arguments"))
        .or_else(|| value.get("arguments"))
        .or_else(|| value.get("args"))
        .and_then(argument_value);
    let command = arguments
        .as_ref()
        .and_then(|args| string_field(args, &["command"]));
    let path = arguments
        .as_ref()
        .and_then(|args| string_field(args, &["path", "file", "file_path", "target_path"]));
    let url = arguments
        .as_ref()
        .and_then(|args| string_field(args, &["url", "href"]));
    let input = arguments
        .as_ref()
        .and_then(|args| string_field(args, &["input", "query", "pattern", "preview"]));
    let label = command
        .clone()
        .or_else(|| input.clone())
        .or_else(|| path.clone())
        .or_else(|| url.clone())
        .unwrap_or_else(|| tool.clone());

    Some(ToolEvent {
        id,
        tool,
        label,
        status: String::from("completed"),
        command,
        input,
        output: None,
        error: None,
        path,
        url,
        has_inline_diff: false,
        raw: compact_json(value),
    })
}

fn tool_row_event(index: usize, value: &Value) -> Option<ToolEvent> {
    let content = value.get("content");
    let payload = content.and_then(tool_payload_value);
    let id = string_field(
        value,
        &[
            "tool_call_id",
            "toolCallId",
            "tool_use_id",
            "toolUseId",
            "call_id",
            "id",
        ],
    )
    .unwrap_or_else(|| format!("tool-row-{index}"));
    let tool = string_field(value, &["tool_name", "toolName", "name", "tool"])
        .or_else(|| {
            payload.as_ref().and_then(|payload| {
                string_field(
                    payload,
                    &["tool", "tool_name", "toolName", "name", "function"],
                )
            })
        })
        .unwrap_or_else(|| String::from("tool"));
    let command = payload
        .as_ref()
        .and_then(|payload| string_field(payload, &["command"]));
    let path = payload
        .as_ref()
        .and_then(|payload| string_field(payload, &["path", "file", "file_path", "target_path"]));
    let url = payload
        .as_ref()
        .and_then(|payload| string_field(payload, &["url", "href"]));
    let input = payload
        .as_ref()
        .and_then(|payload| string_field(payload, &["input", "query", "pattern", "preview"]));
    let output = payload
        .as_ref()
        .and_then(tool_output)
        .or_else(|| content.and_then(non_json_text));
    let error = payload
        .as_ref()
        .and_then(|payload| string_field(payload, &["error", "message", "description"]));
    let label = command
        .clone()
        .or_else(|| input.clone())
        .or_else(|| path.clone())
        .or_else(|| url.clone())
        .or_else(|| output.as_ref().map(|text| summarize_text(text, 96)))
        .unwrap_or_else(|| tool.clone());

    Some(ToolEvent {
        id,
        tool,
        label,
        status: if error.is_some() {
            String::from("failed")
        } else {
            String::from("completed")
        },
        command,
        input,
        output,
        error,
        path,
        url,
        has_inline_diff: payload
            .as_ref()
            .and_then(|payload| payload.get("inline_diff"))
            .and_then(Value::as_str)
            .is_some_and(|diff| !diff.trim().is_empty()),
        raw: payload
            .as_ref()
            .and_then(compact_json)
            .or_else(|| content.and_then(compact_json)),
    })
}

fn attach_tool_event(
    messages: &mut Vec<HermesMessage>,
    tool_targets: &mut HashMap<String, usize>,
    last_assistant_index: Option<usize>,
    index: usize,
    event: ToolEvent,
) {
    let target_index = tool_targets
        .get(&event.id)
        .copied()
        .or(last_assistant_index);
    if let Some(target_index) = target_index
        && let Some(message) = messages.get_mut(target_index)
    {
        upsert_tool_event(message, event);
        return;
    }

    let mut message = HermesMessage::new(
        format!("remote-tool-activity-{index}"),
        HermesRole::Assistant,
        String::new(),
    );
    tool_targets.insert(event.id.clone(), messages.len());
    message.tool_events.push(event);
    messages.push(message);
}

fn upsert_tool_event(message: &mut HermesMessage, event: ToolEvent) {
    if let Some(existing) = message
        .tool_events
        .iter_mut()
        .find(|existing| existing.id == event.id)
    {
        merge_tool_event(existing, event);
    } else {
        message.tool_events.push(event);
    }
}

fn merge_tool_event(existing: &mut ToolEvent, event: ToolEvent) {
    let event_label_is_specific = !event.label.trim().is_empty() && event.label != event.tool;
    if existing.tool == "tool" || event.tool != "tool" {
        existing.tool = event.tool;
    }
    if existing.label.trim().is_empty()
        || existing.label == existing.tool
        || event_label_is_specific
    {
        existing.label = event.label;
    }
    existing.status = event.status;
    existing.command = event.command.or_else(|| existing.command.clone());
    existing.input = event.input.or_else(|| existing.input.clone());
    existing.output = event.output.or_else(|| existing.output.clone());
    existing.error = event.error.or_else(|| existing.error.clone());
    existing.path = event.path.or_else(|| existing.path.clone());
    existing.url = event.url.or_else(|| existing.url.clone());
    existing.has_inline_diff |= event.has_inline_diff;
    existing.raw = event.raw.or_else(|| existing.raw.clone());
}

fn argument_value(value: &Value) -> Option<Value> {
    if let Some(text) = value.as_str() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        return serde_json::from_str(trimmed)
            .ok()
            .or_else(|| Some(Value::String(text.to_owned())));
    }
    Some(value.clone())
}

fn tool_payload_value(value: &Value) -> Option<Value> {
    if let Some(text) = value.as_str() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            serde_json::from_str(trimmed).ok()
        } else {
            Some(Value::String(text.to_owned()))
        }
    } else {
        Some(value.clone())
    }
}

fn tool_output(value: &Value) -> Option<String> {
    string_field(
        value,
        &[
            "output",
            "output_tail",
            "content",
            "result",
            "stdout",
            "stderr",
            "summary",
            "matches_text",
        ],
    )
    .or_else(|| {
        if value.is_array() {
            compact_json(value)
        } else {
            None
        }
    })
}

fn non_json_text(value: &Value) -> Option<String> {
    let text = value.as_str()?.trim();
    if text.starts_with('{') || text.starts_with('[') || text.is_empty() {
        None
    } else {
        Some(value.as_str()?.to_owned())
    }
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| scalar_text(value.get(*key)?))
}

fn scalar_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str().filter(|text| !text.trim().is_empty()) {
        Some(text.to_owned())
    } else if value.is_number() || value.is_boolean() {
        Some(value.to_string())
    } else {
        None
    }
}

fn compact_json(value: &Value) -> Option<String> {
    if value.is_null() {
        return None;
    }
    Some(summarize_text(&value.to_string(), 2_000))
}

fn summarize_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() > max_chars {
        format!("{}...", text.chars().take(max_chars).collect::<String>())
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_normalization_accepts_v1() {
        assert_eq!(
            normalize_endpoint_url("http://localhost:8642/v1").expect("valid"),
            "http://localhost:8642"
        );
        assert_eq!(
            normalize_endpoint_url("https://example.test/root/").expect("valid"),
            "https://example.test/root"
        );
    }

    #[test]
    fn parse_sessions_supports_object_wrapper() {
        let sessions = parse_sessions(&json!({"sessions": [{"id": "s1", "title": "One"}]}));
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "s1");
    }

    #[test]
    fn parse_session_supports_create_response_wrapper() {
        let session = parse_session(&json!({
            "object": "hermes.session",
            "session": {
                "id": "api_1782033567_9140bfe6",
                "title": null,
                "started_at": 1782033567.500297,
            }
        }))
        .expect("wrapped session parses");

        assert_eq!(session.id, "api_1782033567_9140bfe6");
        assert_eq!(session.title, "Hermes Chat");
        assert!(session.updated_at.is_some());
    }

    #[test]
    fn parse_session_prefers_dashboard_stored_id() {
        let session = parse_session(&json!({
            "session_id": "runtime-1",
            "stored_session_id": "stored-1",
            "title": "Project setup",
        }))
        .expect("dashboard session parses");

        assert_eq!(session.id, "stored-1");
        assert_eq!(session.title, "Project setup");
    }

    #[test]
    fn parse_session_uses_preview_when_title_is_missing() {
        let session = parse_session(&json!({
            "id": "api-1",
            "title": null,
            "preview": "Who is the president of the US",
        }))
        .expect("session parses");

        assert_eq!(session.id, "api-1");
        assert_eq!(session.title, "Who is the president of the US");
        assert_eq!(
            session.preview.as_deref(),
            Some("Who is the president of the US")
        );
    }

    #[test]
    fn parse_session_preserves_desktop_activity_metadata() {
        let session = parse_session(&json!({
            "id": "desktop-1",
            "title": "Active work",
            "is_active": true,
            "needsInput": true,
            "message_count": 12,
            "messagePreview": "Installing packages",
            "source": "desktop",
        }))
        .expect("session parses");

        assert!(session.is_active);
        assert!(session.needs_input);
        assert_eq!(session.message_count, Some(12));
        assert_eq!(session.preview.as_deref(), Some("Installing packages"));
        assert_eq!(session.source.as_deref(), Some("desktop"));
    }

    #[test]
    fn parse_message_accepts_desktop_content_parts_and_reasoning() {
        let messages = parse_messages(&json!({
            "messages": [
                {
                    "id": "m1",
                    "role": "assistant",
                    "content": [{"text": "hello"}, {"content": " world"}],
                    "reasoning_content": "because"
                }
            ]
        }));

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "hello world");
        assert_eq!(messages[0].reasoning, "because");
    }

    #[test]
    fn parse_messages_reconstructs_tool_rows_as_assistant_tool_activity() {
        let messages = parse_messages(&json!({
            "messages": [
                {"id": "u1", "role": "user", "content": "read this file"},
                {
                    "id": "a1",
                    "role": "assistant",
                    "content": "Reading the file...",
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "read_file",
                                "arguments": "{\"path\":\"/tmp/example.txt\"}"
                            }
                        }
                    ]
                },
                {
                    "id": "t1",
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "tool_name": "read_file",
                    "content": "{\"content\":\"1|hello\\n\",\"total_lines\":1}"
                },
                {"id": "a2", "role": "assistant", "content": "Done."}
            ]
        }));

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, HermesRole::User);
        assert_eq!(messages[1].role, HermesRole::Assistant);
        assert_eq!(messages[1].content, "Reading the file...");
        assert_eq!(messages[1].tool_events.len(), 1);
        assert_eq!(messages[1].tool_events[0].id, "call_1");
        assert_eq!(messages[1].tool_events[0].tool, "read_file");
        assert_eq!(messages[1].tool_events[0].status, "completed");
        assert_eq!(
            messages[1].tool_events[0].path.as_deref(),
            Some("/tmp/example.txt")
        );
        assert_eq!(
            messages[1].tool_events[0].output.as_deref(),
            Some("1|hello\n")
        );
        assert_eq!(messages[2].content, "Done.");
    }

    #[test]
    fn parse_messages_hides_loaded_history_internal_steps() {
        let messages = parse_messages(&json!({
            "messages": [
                {"id": "u1", "role": "user", "content": "Build this feature"},
                {
                    "id": "a1",
                    "role": "assistant",
                    "content": "",
                    "finish_reason": "tool_calls",
                    "reasoning": "I need to inspect the files first.",
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "read_file",
                                "arguments": "{\"path\":\"/tmp/example.txt\"}"
                            }
                        }
                    ]
                },
                {
                    "id": "t1",
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "tool_name": "read_file",
                    "content": "{\"content\":\"1|hello\\n\",\"total_lines\":1}"
                },
                {
                    "id": "u-compaction",
                    "role": "user",
                    "content": "[CONTEXT COMPACTION — REFERENCE ONLY] Earlier turns were compacted into the summary below."
                },
                {
                    "id": "a-reasoning",
                    "role": "assistant",
                    "content": "",
                    "reasoning_content": "Thinking through the next tool call."
                },
                {"id": "a2", "role": "assistant", "content": "Implemented."},
                {"id": "u2", "role": "user", "content": "commit"},
                {"id": "a3", "role": "assistant", "content": "Committed."}
            ]
        }));

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].content, "Build this feature");
        assert_eq!(messages[1].content, "Implemented.");
        assert!(messages[1].reasoning.is_empty());
        assert!(messages[1].tool_events.is_empty());
        assert_eq!(messages[2].content, "commit");
        assert_eq!(messages[3].content, "Committed.");
    }

    #[test]
    fn approval_body_includes_optional_approval_id() {
        assert_eq!(
            approval_body(true, Some("looks good"), Some("approval-1")),
            json!({
                "approved": true,
                "message": "looks good",
                "approval_id": "approval-1",
            })
        );
        assert_eq!(
            approval_body(false, None, Some("")),
            json!({"approved": false, "message": ""})
        );
    }

    #[test]
    fn chat_completion_messages_skip_empty_placeholder_rows() {
        let user = HermesMessage::new("u1", HermesRole::User, "hello");
        let mut placeholder = HermesMessage::new("a1", HermesRole::Assistant, "");
        placeholder.status = MessageStatus::Streaming;
        let tool = HermesMessage::new("t1", HermesRole::Tool, "tool output");
        let error = HermesMessage::new("e1", HermesRole::Error, "failed");
        let assistant = HermesMessage::new("a2", HermesRole::Assistant, "hi");

        assert_eq!(
            chat_completion_messages(&[user, placeholder, tool, error, assistant], &[]),
            vec![
                json!({"role": "user", "content": "hello"}),
                json!({"role": "assistant", "content": "hi"}),
            ]
        );
    }
}
