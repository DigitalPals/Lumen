//! Subscription usage monitoring for AI coding agents.
//!
//! Polls the usage endpoints behind Claude Code and Codex CLI subscriptions
//! and exposes rate-limit windows (5-hour session, weekly, model-specific)
//! as reactive properties — the same numbers the CLIs show in their own
//! `/usage` and `/status` screens.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use lumen_model_usage::ModelUsageService;
//!
//! let usage = ModelUsageService::builder().build();
//!
//! if let Some(snapshot) = usage.usage.get().as_ref() {
//!     for entry in &snapshot.providers {
//!         match &entry.result {
//!             Ok(data) => {
//!                 let pct = data.min_remaining_percent().unwrap_or(100.0);
//!                 println!("{}: {pct:.0}% remaining", entry.kind.display_name());
//!             }
//!             Err(kind) => println!("{}: {kind:?}", entry.kind.display_name()),
//!         }
//!     }
//! }
//! ```
//!
//! # Watching for Changes
//!
//! ```rust,no_run
//! use lumen_model_usage::ModelUsageService;
//! use tokio_stream::StreamExt;
//!
//! # async fn watch(usage: ModelUsageService) {
//! let mut stream = usage.usage.watch();
//! while let Some(snapshot) = stream.next().await {
//!     if let Some(s) = snapshot.as_ref() {
//!         println!("Updated at {}", s.updated_at);
//!     }
//! }
//! # }
//! ```
//!
//! # Configuration
//!
//! | Method | Effect |
//! |--------|--------|
//! | `poll_interval(Duration)` | How often to fetch (clamped to ≥ 2 minutes) |
//! | `providers(Vec<ProviderKind>)` | Which providers to poll |
//! | `claude_credentials_path(path)` | Override Claude Code credential file |
//! | `codex_auth_path(path)` | Override Codex CLI auth file |
//!
//! # Providers and Credentials
//!
//! | Provider | Credentials read (never written) |
//! |----------|----------------------------------|
//! | [`Claude`](ProviderKind::Claude) | `~/.claude/.credentials.json` (or `$CLAUDE_CONFIG_DIR`) |
//! | [`Codex`](ProviderKind::Codex) | `~/.codex/auth.json` (or `$CODEX_HOME`) |
//!
//! Credential files are re-read on every poll, so tokens refreshed by the
//! CLIs are picked up automatically. This service **never refreshes tokens
//! itself**: refresh tokens rotate on use, and a background refresh would
//! race the CLI and invalidate its session. An expired token surfaces as
//! [`ModelUsageErrorKind::TokenExpired`] until the user runs the CLI again.
//!
//! # Stability
//!
//! The upstream usage endpoints are internal APIs observed from the
//! providers' own clients, not documented public surface. The service is
//! built to degrade gracefully — any fetch failure is categorized into
//! [`ModelUsageErrorKind`] and reported per provider — but field names and
//! semantics may change without notice.

mod builder;
pub(crate) mod credentials;

/// Model usage error types and result aliases.
pub mod error;

/// Usage data models: snapshots, windows, credits.
pub mod model;

pub(crate) mod polling;

/// Usage data provider implementations.
pub mod provider;

mod service;

pub use builder::ModelUsageServiceBuilder;
pub use error::{Error, Result};
pub use model::{
    Credits, ProviderEntry, ProviderKind, ProviderUsage, UsageSnapshot, UsageWindow, WindowKind,
};
pub use provider::{ClaudeProvider, CodexProvider, UsageProvider};
pub use service::{ModelUsageErrorKind, ModelUsageService, ModelUsageStatus};

#[doc = include_str!("../README.md")]
#[cfg(doctest)]
pub struct ReadmeDocTests;
