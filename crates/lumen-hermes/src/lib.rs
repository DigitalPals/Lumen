//! Native Hermes Agent chat client and reactive service for Lumen.
//!
//! This crate talks to an externally hosted Hermes Agent API server. The
//! service keeps a local copy of chat history for the Lumen UI while requests
//! and tools execute on the remote Hermes server.

mod client;
mod dashboard;
mod error;
mod markdown;
mod model;
mod service;
mod sse;
mod store;

pub use client::{HermesClient, normalize_endpoint_url};
pub use error::{Error, Result};
pub use markdown::{
    MarkdownBlock, escape_pango_text, markdown_to_blocks, markdown_to_pango, markdownish_to_pango,
};
pub use model::{
    ApprovalKind, ApprovalRequest, BackgroundProcessItem, BackgroundProcessStatus, ChatAttachment,
    ConnectionConfig, HermesMessage, HermesRole, HermesSessionSummary, HermesStatus,
    LocalHistoryMode, MessageStatus, SlashCommandSuggestion, SubagentItem, SubagentStatus,
    TodoItem, TodoStatus, ToolEvent, TransportMode,
};
pub use service::{HermesChatService, HermesChatServiceBuilder};
pub use sse::{SseDecoder, SseEvent, parse_sse_events};
