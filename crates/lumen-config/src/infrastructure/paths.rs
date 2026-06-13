//! Re-exports path resolution from lumen-core, plus config-local path helpers.

use std::path::PathBuf;

pub use lumen_core::paths::ConfigPaths;

/// Path to `themes/schema.json` for theme file validation.
pub fn theme_schema_json() -> PathBuf {
    ConfigPaths::themes_dir().join("schema.json")
}
