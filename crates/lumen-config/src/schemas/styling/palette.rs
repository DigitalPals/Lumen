use lumen_derive::lumen_config;

use super::HexColor;
use crate::{ConfigProperty, infrastructure::themes::palettes::lumen_theme};

fn hex(s: &str) -> HexColor {
    HexColor::new(s).unwrap_or_else(|_| HexColor::new(lumen_theme::RED).unwrap_or_default())
}

/// Color palette configuration for the active theme.
#[lumen_config(i18n_prefix = "settings-palette")]
pub struct PaletteConfig {
    /// Base background color (darkest).
    #[default(hex(lumen_theme::BG))]
    pub bg: ConfigProperty<HexColor>,

    /// Card and sidebar background.
    #[default(hex(lumen_theme::SURFACE))]
    pub surface: ConfigProperty<HexColor>,

    /// Raised element background.
    #[default(hex(lumen_theme::ELEVATED))]
    pub elevated: ConfigProperty<HexColor>,

    /// Primary text color.
    #[default(hex(lumen_theme::FG))]
    pub fg: ConfigProperty<HexColor>,

    /// Secondary text color.
    #[serde(rename = "fg-muted")]
    #[default(hex(lumen_theme::FG_MUTED))]
    pub fg_muted: ConfigProperty<HexColor>,

    /// Accent color for interactive elements.
    #[default(hex(lumen_theme::PRIMARY))]
    pub primary: ConfigProperty<HexColor>,

    /// Red semantic color.
    #[default(hex(lumen_theme::RED))]
    pub red: ConfigProperty<HexColor>,

    /// Yellow semantic color.
    #[default(hex(lumen_theme::YELLOW))]
    pub yellow: ConfigProperty<HexColor>,

    /// Green semantic color.
    #[default(hex(lumen_theme::GREEN))]
    pub green: ConfigProperty<HexColor>,

    /// Blue semantic color.
    #[default(hex(lumen_theme::BLUE))]
    pub blue: ConfigProperty<HexColor>,
}
