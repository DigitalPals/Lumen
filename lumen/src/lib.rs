//! Lumen CLI - Compositor-agnostic desktop environment CLI.
//!
//! CLI commands for managing Lumen services:
//!
//! - `lumen panel` - Start/stop/control the panel GUI
//! - `lumen media` - Control media players
//! - `lumen wallpaper` - Manage wallpapers
//! - `lumen config` - Query/set configuration
//! - `lumen icons` - Manage icon packs
//!
//! The GUI panel runs via `lumen shell` (or `lumen panel start` for daemon mode).

/// Configuration schema definitions and validation.
pub use lumen_config as config;

/// Documentation generation for configuration schemas.
pub mod docs;

/// Command-line interface.
pub mod cli;

/// Core runtime infrastructure.
pub mod core;
