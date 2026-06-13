use std::error::Error as StdError;

use thiserror::Error;

pub(crate) fn error_chain(e: &dyn StdError) -> String {
    let mut msg = e.to_string();
    let mut source = e.source();
    while let Some(cause) = source {
        msg.push_str(": ");
        msg.push_str(&cause.to_string());
        source = cause.source();
    }
    msg
}

/// Errors that can occur while fetching model usage data.
#[derive(Error, Debug)]
pub enum Error {
    /// Credential file for the provider was not found.
    ///
    /// The user has not signed in with the provider's CLI on this machine.
    #[error("{provider} credentials not found at {path}")]
    CredentialsNotFound {
        /// Display name of the provider.
        provider: &'static str,
        /// Path that was checked for credentials.
        path: String,
    },

    /// Credential file exists but could not be parsed.
    #[error("cannot parse {provider} credentials: {reason}")]
    CredentialsInvalid {
        /// Display name of the provider.
        provider: &'static str,
        /// Description of the parse failure.
        reason: String,
    },

    /// Stored access token has expired.
    ///
    /// The token is refreshed by the provider's own CLI; this service never
    /// refreshes tokens itself (refresh tokens rotate, and racing the CLI
    /// would invalidate its session). Running the CLI again fixes this.
    #[error("{provider} access token has expired")]
    TokenExpired {
        /// Display name of the provider.
        provider: &'static str,
    },

    /// HTTP request to the provider failed.
    #[error("HTTP request to {provider} failed: {source}")]
    Http {
        /// Display name of the provider.
        provider: &'static str,
        /// The underlying HTTP error.
        #[source]
        source: reqwest::Error,
    },

    /// Provider returned an error status.
    #[error("{provider} returned HTTP {status}")]
    ProviderStatus {
        /// Display name of the provider.
        provider: &'static str,
        /// HTTP status code returned.
        status: reqwest::StatusCode,
    },

    /// Cannot parse the provider's usage response.
    #[error("cannot parse {provider} response: {reason}")]
    Parse {
        /// Display name of the provider.
        provider: &'static str,
        /// Description of the parse failure.
        reason: String,
    },

    /// Provider rate limit exceeded.
    #[error("{provider} rate limit exceeded")]
    RateLimited {
        /// Display name of the rate-limited provider.
        provider: &'static str,
    },
}

impl Error {
    pub(crate) fn http(provider: &'static str, source: reqwest::Error) -> Self {
        Self::Http { provider, source }
    }

    pub(crate) fn status(provider: &'static str, status: reqwest::StatusCode) -> Self {
        Self::ProviderStatus { provider, status }
    }

    pub(crate) fn parse(provider: &'static str, reason: impl Into<String>) -> Self {
        Self::Parse {
            provider,
            reason: reason.into(),
        }
    }

    pub(crate) fn credentials_invalid(provider: &'static str, reason: impl Into<String>) -> Self {
        Self::CredentialsInvalid {
            provider,
            reason: reason.into(),
        }
    }

    /// Whether retrying the request soon could plausibly succeed.
    ///
    /// Rate limits are deliberately non-retryable: hammering an already
    /// rate-limited usage endpoint only deepens the penalty, so the polling
    /// loop waits for the next regular interval instead.
    pub(crate) fn is_retryable(&self) -> bool {
        match self {
            Self::Http { source, .. } => {
                source.is_timeout() || source.is_connect() || source.is_request()
            }
            Self::ProviderStatus { status, .. } => status.is_server_error(),
            Self::CredentialsNotFound { .. }
            | Self::CredentialsInvalid { .. }
            | Self::TokenExpired { .. }
            | Self::Parse { .. }
            | Self::RateLimited { .. } => false,
        }
    }
}

/// Result type alias for model usage operations.
pub type Result<T> = std::result::Result<T, Error>;
