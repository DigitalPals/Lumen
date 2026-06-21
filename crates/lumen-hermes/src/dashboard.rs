use std::{fs, net::IpAddr, path::PathBuf, time::Duration};

use futures::{SinkExt, StreamExt};
use reqwest::{Client, Method, Url};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::HeaderValue},
};
use tokio_util::sync::CancellationToken;

use crate::{
    ConnectionConfig, Error, HermesMessage, HermesSessionSummary, Result,
    client::{check_status, normalize_endpoint_url, parse_messages, parse_sessions},
};

const SESSION_TOKEN_HEADER: &str = "X-Hermes-Session-Token";
const DESKTOP_USER_AGENT: &str = "Hermes-Desktop";
const DESKTOP_CLIENT_HEADER: &str = "X-Hermes-Client";

/// Hermes Desktop/TUI dashboard client.
#[derive(Debug, Clone)]
pub(crate) struct DashboardClient {
    http: Client,
    base_url: String,
    token: String,
    timeout: Duration,
}

impl DashboardClient {
    /// Builds a dashboard client from resolved config.
    pub(crate) async fn new(config: ConnectionConfig) -> Result<Self> {
        if !config.enabled {
            return Err(Error::InvalidEndpoint(String::from("module disabled")));
        }
        let base_url = normalize_endpoint_url(&config.endpoint_url)?;
        let timeout = Duration::from_secs(config.timeout_seconds.max(1));
        let http = Client::builder()
            .connect_timeout(timeout)
            .user_agent(DESKTOP_USER_AGENT)
            .build()?;
        let token =
            resolve_dashboard_token(&http, &base_url, config.dashboard_token.as_deref()).await?;
        Ok(Self {
            http,
            base_url,
            token,
            timeout,
        })
    }

    /// Returns dashboard capabilities known to this client.
    pub(crate) fn capabilities(&self) -> Value {
        json!({
            "transport_mode": "dashboard-ws",
            "events": [
                "gateway.ready",
                "session.info",
                "message.start",
                "message.delta",
                "thinking.delta",
                "reasoning.delta",
                "reasoning.available",
                "tool.progress",
                "tool.generating",
                "tool.start",
                "tool.complete",
                "approval.request",
                "clarify.request",
                "sudo.request",
                "secret.request",
                "message.complete",
                "review.summary",
                "error"
            ],
        })
    }

    /// Lists dashboard sessions.
    pub(crate) async fn list_sessions(&self) -> Result<Vec<HermesSessionSummary>> {
        let value = self
            .get_json("/api/sessions?limit=50&offset=0&order=recent")
            .await?;
        Ok(parse_sessions(&value))
    }

    /// Gets messages for a stored dashboard session.
    pub(crate) async fn session_messages(&self, session_id: &str) -> Result<Vec<HermesMessage>> {
        let value = self
            .get_json(&format!("/api/sessions/{session_id}/messages"))
            .await?;
        Ok(parse_messages(&value))
    }

    /// Returns the dashboard profile's active model selection.
    pub(crate) async fn model_info(&self) -> Result<Value> {
        self.get_json("/api/model/info").await
    }

    /// Returns Hermes profiles known to the dashboard.
    pub(crate) async fn profiles(&self) -> Result<Value> {
        self.get_json("/api/profiles").await
    }

    /// Returns the dashboard's active profile descriptor.
    pub(crate) async fn active_profile(&self) -> Result<Value> {
        self.get_json("/api/profiles/active").await
    }

    /// Opens the dashboard WebSocket.
    pub(crate) async fn connect_ws(&self) -> Result<DashboardConnection> {
        let url = dashboard_ws_url(&self.base_url, &self.token)?;
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|err| Error::WebSocket(err.to_string()))?;
        request
            .headers_mut()
            .insert("User-Agent", HeaderValue::from_static(DESKTOP_USER_AGENT));
        request.headers_mut().insert(
            DESKTOP_CLIENT_HEADER,
            HeaderValue::from_static(DESKTOP_USER_AGENT),
        );
        let (socket, _) = connect_async(request)
            .await
            .map_err(|err| Error::WebSocket(err.to_string()))?;
        Ok(DashboardConnection { socket, next_id: 0 })
    }

    /// Opens a short dashboard RPC connection, sends one request, and waits for its response.
    pub(crate) async fn request_once(
        &self,
        method: &str,
        params: Value,
        token: &CancellationToken,
    ) -> Result<Value> {
        let mut connection = self.connect_ws().await?;
        connection.wait_ready(token).await?;
        let request_id = connection.send_request(method, params).await?;
        connection.wait_response(&request_id, token).await
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, self.url(path))
            .header(SESSION_TOKEN_HEADER, &self.token)
            .header(DESKTOP_CLIENT_HEADER, DESKTOP_USER_AGENT)
    }

    async fn get_json(&self, path: &str) -> Result<Value> {
        let response = self
            .request(Method::GET, path)
            .timeout(self.timeout)
            .send()
            .await?;
        let response = check_status(response).await?;
        Ok(response.json().await?)
    }
}

/// Active dashboard JSON-RPC WebSocket.
pub(crate) struct DashboardConnection {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl DashboardConnection {
    /// Waits for the initial gateway ready event.
    pub(crate) async fn wait_ready(&mut self, token: &CancellationToken) -> Result<()> {
        while let Some(frame) = self.next_frame(token).await? {
            if matches!(
                frame,
                DashboardFrame::Event(DashboardRpcEvent { event_type, .. })
                    if event_type == "gateway.ready"
            ) {
                return Ok(());
            }
        }
        Err(Error::WebSocket(String::from(
            "dashboard socket closed before gateway.ready",
        )))
    }

    /// Sends a JSON-RPC request and returns its id.
    pub(crate) async fn send_request(&mut self, method: &str, params: Value) -> Result<String> {
        self.next_id += 1;
        let request_id = format!("lumen-{}", self.next_id);
        let body = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        });
        self.socket
            .send(Message::Text(body.to_string().into()))
            .await
            .map_err(|err| Error::WebSocket(err.to_string()))?;
        Ok(request_id)
    }

    /// Reads the next event or response frame.
    pub(crate) async fn next_frame(
        &mut self,
        token: &CancellationToken,
    ) -> Result<Option<DashboardFrame>> {
        loop {
            let message = tokio::select! {
                () = token.cancelled() => return Ok(None),
                message = self.socket.next() => message,
            };
            let Some(message) = message else {
                return Ok(None);
            };
            match message.map_err(|err| Error::WebSocket(err.to_string()))? {
                Message::Text(text) => {
                    if let Some(frame) = parse_dashboard_frame(text.as_str())? {
                        return Ok(Some(frame));
                    }
                }
                Message::Binary(bytes) => {
                    let text = String::from_utf8(bytes.to_vec())
                        .map_err(|err| Error::WebSocket(err.to_string()))?;
                    if let Some(frame) = parse_dashboard_frame(&text)? {
                        return Ok(Some(frame));
                    }
                }
                Message::Close(_) => return Ok(None),
                Message::Ping(bytes) => {
                    self.socket
                        .send(Message::Pong(bytes))
                        .await
                        .map_err(|err| Error::WebSocket(err.to_string()))?;
                }
                Message::Pong(_) | Message::Frame(_) => {}
            }
        }
    }

    pub(crate) async fn wait_response(
        &mut self,
        request_id: &str,
        token: &CancellationToken,
    ) -> Result<Value> {
        while let Some(frame) = self.next_frame(token).await? {
            if let DashboardFrame::Response { id, result, error } = frame
                && id == request_id
            {
                if let Some(error) = error {
                    return Err(dashboard_rpc_error(error));
                }
                return Ok(result.unwrap_or(Value::Null));
            }
        }
        Err(Error::WebSocket(String::from(
            "dashboard socket closed before response",
        )))
    }
}

/// Dashboard event notification.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DashboardRpcEvent {
    pub(crate) event_type: String,
    pub(crate) session_id: Option<String>,
    pub(crate) payload: Value,
}

/// Dashboard JSON-RPC frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DashboardFrame {
    Event(DashboardRpcEvent),
    Response {
        id: String,
        result: Option<Value>,
        error: Option<Value>,
    },
}

async fn resolve_dashboard_token(
    http: &Client,
    base_url: &str,
    configured: Option<&str>,
) -> Result<String> {
    if let Some(token) = configured
        .map(str::trim)
        .filter(|token| !token.is_empty() && !token.starts_with('$'))
    {
        return Ok(token.to_owned());
    }

    if is_loopback_endpoint(base_url) {
        let response = http.get(format!("{base_url}/")).send().await?;
        let html = check_status(response).await?.text().await?;
        if let Some(token) = extract_dashboard_token(&html) {
            return Ok(token);
        }
    }

    if let Some(token) = read_desktop_remote_token() {
        return Ok(token);
    }

    Err(Error::MissingDashboardToken)
}

fn read_desktop_remote_token() -> Option<String> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    fs::read_to_string(config_home.join("hermes-desktop").join("remote-token"))
        .ok()
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
}

fn dashboard_ws_url(base_url: &str, token: &str) -> Result<String> {
    let mut url = Url::parse(base_url).map_err(|_| Error::InvalidEndpoint(base_url.to_owned()))?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        _ => return Err(Error::InvalidEndpoint(base_url.to_owned())),
    };
    url.set_scheme(scheme)
        .map_err(|_| Error::InvalidEndpoint(base_url.to_owned()))?;
    let base_path = url.path().trim_end_matches('/');
    let path = if base_path.is_empty() {
        String::from("/api/ws")
    } else {
        format!("{base_path}/api/ws")
    };
    url.set_path(&path);
    url.query_pairs_mut().clear().append_pair("token", token);
    Ok(url.to_string())
}

fn parse_dashboard_frame(text: &str) -> Result<Option<DashboardFrame>> {
    let value: Value = serde_json::from_str(text)?;
    if let Some(id) = rpc_id(&value)
        && (value.get("result").is_some() || value.get("error").is_some())
    {
        return Ok(Some(DashboardFrame::Response {
            id,
            result: value.get("result").cloned(),
            error: value.get("error").cloned(),
        }));
    }

    if value.get("method").and_then(Value::as_str) == Some("event") {
        let params = value.get("params").unwrap_or(&Value::Null);
        let event_type = params
            .get("type")
            .or_else(|| params.get("event"))
            .and_then(Value::as_str)
            .ok_or_else(|| Error::UnsupportedEvent(String::from("dashboard event type")))?;
        let payload = params
            .get("payload")
            .cloned()
            .unwrap_or_else(|| params.clone());
        let session_id = params
            .get("session_id")
            .or_else(|| params.get("sessionId"))
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .map(str::to_owned);
        return Ok(Some(DashboardFrame::Event(DashboardRpcEvent {
            event_type: event_type.to_owned(),
            session_id,
            payload,
        })));
    }

    if let Some(event_type) = value.get("type").and_then(Value::as_str) {
        let payload = value
            .get("payload")
            .cloned()
            .unwrap_or_else(|| value.clone());
        let session_id = value
            .get("session_id")
            .or_else(|| value.get("sessionId"))
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .map(str::to_owned);
        return Ok(Some(DashboardFrame::Event(DashboardRpcEvent {
            event_type: event_type.to_owned(),
            session_id,
            payload,
        })));
    }

    Ok(None)
}

fn rpc_id(value: &Value) -> Option<String> {
    value
        .get("id")
        .and_then(|id| {
            id.as_str()
                .map(str::to_owned)
                .or_else(|| id.as_i64().map(|id| id.to_string()))
                .or_else(|| id.as_u64().map(|id| id.to_string()))
        })
        .filter(|id| !id.trim().is_empty())
}

fn dashboard_rpc_error(error: Value) -> Error {
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .unwrap_or("dashboard request failed")
        .to_owned();
    Error::Api {
        status: 500,
        message,
    }
}

fn is_loopback_endpoint(base_url: &str) -> bool {
    Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        })
}

fn extract_dashboard_token(html: &str) -> Option<String> {
    let marker = "window.__HERMES_SESSION_TOKEN__";
    let start = html.find(marker)?;
    let after_marker = &html[start + marker.len()..];
    let assignment = after_marker.find('=')?;
    let value = after_marker[assignment + 1..].trim_start();
    extract_js_string(value).filter(|token| !token.trim().is_empty())
}

fn extract_js_string(value: &str) -> Option<String> {
    if value.starts_with('"') {
        return extract_json_string(value);
    }
    if value.starts_with('\'') {
        return extract_single_quoted_string(value);
    }
    None
}

fn extract_json_string(value: &str) -> Option<String> {
    let mut escaped = false;
    for (index, char) in value.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        if char == '\\' {
            escaped = true;
            continue;
        }
        if char == '"' {
            return serde_json::from_str::<String>(&value[..=index]).ok();
        }
    }
    None
}

fn extract_single_quoted_string(value: &str) -> Option<String> {
    let mut output = String::new();
    let mut escaped = false;
    for char in value.chars().skip(1) {
        if escaped {
            output.push(char);
            escaped = false;
            continue;
        }
        if char == '\\' {
            escaped = true;
            continue;
        }
        if char == '\'' {
            return Some(output);
        }
        output.push(char);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_ws_url_preserves_base_path_and_encodes_token() {
        assert_eq!(
            dashboard_ws_url("http://127.0.0.1:8642", "abc 123").expect("url"),
            "ws://127.0.0.1:8642/api/ws?token=abc+123"
        );
        assert_eq!(
            dashboard_ws_url("https://example.test/root", "secret").expect("url"),
            "wss://example.test/root/api/ws?token=secret"
        );
    }

    #[test]
    fn extracts_dashboard_token_from_script() {
        assert_eq!(
            extract_dashboard_token(
                r#"<script>window.__HERMES_SESSION_TOKEN__ = "tok\"en";</script>"#
            ),
            Some(String::from("tok\"en"))
        );
        assert_eq!(
            extract_dashboard_token(
                "<script>window.__HERMES_SESSION_TOKEN__ = 'token-2';</script>"
            ),
            Some(String::from("token-2"))
        );
    }

    #[test]
    fn parses_dashboard_rpc_event_and_response() {
        assert_eq!(
            parse_dashboard_frame(
                r#"{"jsonrpc":"2.0","method":"event","params":{"type":"message.delta","payload":{"text":"hi"}}}"#
            )
            .expect("parse event"),
            Some(DashboardFrame::Event(DashboardRpcEvent {
                event_type: String::from("message.delta"),
                session_id: None,
                payload: json!({"text": "hi"}),
            }))
        );
        assert_eq!(
            parse_dashboard_frame(r#"{"jsonrpc":"2.0","id":"lumen-1","result":{"ok":true}}"#)
                .expect("parse response"),
            Some(DashboardFrame::Response {
                id: String::from("lumen-1"),
                result: Some(json!({"ok": true})),
                error: None,
            })
        );
    }
}
