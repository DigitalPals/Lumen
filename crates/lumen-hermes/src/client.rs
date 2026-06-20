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
        let http = Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds.max(1)))
            .build()?;
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
        let response = self.http.get(self.url("/health")).send().await?;
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
        let value = self.get_json("/api/sessions").await?;
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

    /// Opens an OpenAI-compatible chat completion SSE stream.
    ///
    /// # Errors
    /// Returns an error if the request cannot be started.
    pub async fn stream_chat_completions(&self, messages: &[HermesMessage]) -> Result<EventStream> {
        let api_messages: Vec<Value> = messages
            .iter()
            .filter(|message| {
                matches!(
                    message.role,
                    HermesRole::User | HermesRole::Assistant | HermesRole::System
                )
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
            .collect();
        let body = json!({"model": self.config.model, "messages": api_messages, "stream": true});
        self.post_sse("/v1/chat/completions", &body).await
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
        approved: bool,
        text: Option<&str>,
    ) -> Result<()> {
        let response = self
            .request(Method::POST, &format!("/v1/runs/{run_id}/approval"))
            .json(&json!({"approved": approved, "message": text.unwrap_or_default()}))
            .send()
            .await?;
        check_status(response).await.map(|_| ())
    }

    async fn get_json(&self, path: &str) -> Result<Value> {
        let response = self.request(Method::GET, path).send().await?;
        let response = check_status(response).await?;
        Ok(response.json().await?)
    }

    async fn post_json(&self, path: &str, body: &Value) -> Result<Value> {
        let response = self.request(Method::POST, path).json(body).send().await?;
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

async fn check_status(response: reqwest::Response) -> Result<reqwest::Response> {
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

fn parse_sessions(value: &Value) -> Vec<HermesSessionSummary> {
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

fn parse_session(value: &Value) -> Option<HermesSessionSummary> {
    let id = value
        .get("id")
        .or_else(|| value.get("session_id"))
        .and_then(Value::as_str)?;
    let title = value
        .get("title")
        .or_else(|| value.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("Hermes Chat");
    let updated_at = value
        .get("updated_at")
        .or_else(|| value.get("updatedAt"))
        .and_then(Value::as_str)
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        .map(|time| time.with_timezone(&chrono::Utc));
    Some(HermesSessionSummary {
        id: id.to_owned(),
        title: title.to_owned(),
        updated_at,
    })
}

fn parse_messages(value: &Value) -> Vec<HermesMessage> {
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
    let content = value
        .get("content")
        .or_else(|| value.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default();
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
    message.status = MessageStatus::Complete;
    Some(message)
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
}
