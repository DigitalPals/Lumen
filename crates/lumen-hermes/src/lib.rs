//! Native Hermes Agent chat client and reactive service for Lumen.
//!
//! This crate talks to an externally hosted Hermes Agent API server. The
//! service keeps a local copy of chat history for the Lumen UI while requests
//! and tools execute on the remote Hermes server.

mod client;
mod error;
mod markdown;
mod model;
mod service;
mod sse;
mod store;

pub use client::{HermesClient, normalize_endpoint_url};
pub use error::{Error, Result};
pub use markdown::{escape_pango_text, markdownish_to_pango};
pub use model::{
    ApprovalRequest, ConnectionConfig, HermesMessage, HermesRole, HermesSessionSummary,
    HermesStatus, LocalHistoryMode, MessageStatus, ToolEvent, TransportMode,
};
pub use service::{HermesChatService, HermesChatServiceBuilder};
pub use sse::{SseDecoder, SseEvent, parse_sse_events};
