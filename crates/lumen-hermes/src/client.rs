use std::{pin::Pin, time::Duration};

use async_stream::try_stream;
use futures::{Stream, StreamExt};
use reqwest::{Client, Method, header};
use serde_json::{Value, json};

use crate::{
    ConnectionConfig, Error, HermesMessage, HermesRole, HermesSessionSummary, MessageStatus,
    Result, SseDecoder, SseEvent,
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
        let body = json!({"message": content, "content": content, "stream": true});
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
        let api_messages = chat_completion_messages(messages);
        let body = json!({"model": self.config.model, "messages": api_messages, "stream": true});
        self.post_sse("/v1/chat/completions", &body).await
    }

    /// Sends an OpenAI-compatible chat completion request and waits for the completed answer.
    ///
    /// # Errors
    /// Returns an error if the request fails.
    pub async fn chat_completions(&self, messages: &[HermesMessage]) -> Result<Value> {
        let api_messages = chat_completion_messages(messages);
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

fn chat_completion_messages(messages: &[HermesMessage]) -> Vec<Value> {
    messages
        .iter()
        .filter(|message| {
            matches!(
                message.role,
                HermesRole::User | HermesRole::Assistant | HermesRole::System
            ) && !message.content.trim().is_empty()
        })
        .map(|message| {
            json!({
                "role": match message.role {
                    HermesRole::User => "user",
                    HermesRole::Assistant => "assistant",
                    HermesRole::System => "system",
                    HermesRole::Tool | HermesRole::Error => "system",
                },
                "content": message.content,
            })
        })
        .collect()
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
    let array = value
        .get("messages")
        .or_else(|| value.get("data"))
        .and_then(Value::as_array)
        .or_else(|| value.as_array());
    array
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, item)| parse_message(index, item))
        .collect()
}

fn parse_message(index: usize, value: &Value) -> Option<HermesMessage> {
    let content = message_text(value);
    let role = match value
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("assistant")
    {
        "user" => HermesRole::User,
        "system" => HermesRole::System,
        "tool" => HermesRole::Tool,
        "error" => HermesRole::Error,
        _ => HermesRole::Assistant,
    };
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("remote-{index}"));
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
            chat_completion_messages(&[user, placeholder, tool, error, assistant]),
            vec![
                json!({"role": "user", "content": "hello"}),
                json!({"role": "assistant", "content": "hi"}),
            ]
        );
    }
}
