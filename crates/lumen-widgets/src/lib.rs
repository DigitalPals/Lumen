//! Reusable GTK4 widget components for Lumen.
//!
//! Provides primitive UI building blocks styled according to Lumen's design system.
//!
//! # Quick Start
//!
//! Import everything via the prelude:
//!
//! ```rust,no_run
//! use lumen_widgets::prelude::*;
//! ```
//!
//! Or import specific modules:
//!
//! ```rust,no_run
//! use lumen_widgets::primitives::card::{Card, CardClass};
//! use lumen_widgets::components::bar_buttons::{BarButton, BarButtonOutput};
//! ```

pub mod components;
pub mod icons;
pub mod primitives;
pub mod styling;
pub mod utils;
pub mod watchers;

pub use watchers::WatcherToken;

/// Convenient re-exports of all widget templates and class constants.
pub mod prelude {
    pub use crate::{
        components::{bar_buttons::*, bar_container::*},
        primitives::{
            alert::*, badge::*, buttons::*, card::*, checkbox::*, confirm_modal::*, dropdown::*,
            empty_state::*, password_input::*, popover::*, progress_bar::*, progress_ring::*,
            radio_group::*, separator::*, slider::*, spinner::*, status_dot::*, switch::*,
            text_input::*,
        },
        styling::{InlineStyling, resolve_color},
        utils::force_window_resize,
    };
}
