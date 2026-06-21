use thiserror::Error;

/// Result alias for Hermes client/service operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors surfaced by the Hermes integration.
#[derive(Debug, Error)]
pub enum Error {
    /// Endpoint URL is empty or otherwise invalid.
    #[error("invalid Hermes endpoint: {0}")]
    InvalidEndpoint(String),
    /// Authentication material is missing.
    #[error("Hermes API key is not configured")]
    MissingApiKey,
    /// Dashboard WebSocket token is missing.
    #[error("Hermes dashboard token is not configured")]
    MissingDashboardToken,
    /// HTTP transport failed.
    #[error("Hermes network error: {0}")]
    Http(#[from] reqwest::Error),
    /// Hermes returned an unsuccessful status.
    #[error("Hermes returned HTTP {status}: {message}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Redacted server error message.
        message: String,
    },
    /// JSON parsing failed.
    #[error("Hermes JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// I/O failed while reading or writing local-only history.
    #[error("Hermes local history error: {0}")]
    Io(#[from] std::io::Error),
    /// WebSocket transport failed.
    #[error("Hermes dashboard WebSocket error: {0}")]
    WebSocket(String),
    /// The server returned an event shape this client cannot apply.
    #[error("unsupported Hermes event: {0}")]
    UnsupportedEvent(String),
}

impl Error {
    /// User-facing category for the bar/dropdown.
    pub fn short_message(&self) -> String {
        match self {
            Self::InvalidEndpoint(_) => String::from("Invalid endpoint"),
            Self::MissingApiKey => String::from("Missing API key"),
            Self::MissingDashboardToken => String::from("Missing dashboard token"),
            Self::Http(_) => String::from("Server unreachable"),
            Self::Api { status: 401, .. } => String::from("Authentication failed"),
            Self::Api { status: 429, .. } => String::from("Hermes is busy"),
            Self::Api { message, .. } => message.clone(),
            Self::Json(_) => String::from("Invalid server response"),
            Self::Io(_) => String::from("Local history error"),
            Self::WebSocket(_) => String::from("Dashboard WebSocket failed"),
            Self::UnsupportedEvent(_) => String::from("Unsupported server event"),
        }
    }
}
