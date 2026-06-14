//! Spec types that describe page structure: pages, sections, and individual settings.

use std::any::Any;

use relm4::gtk;

use crate::{property_handle::PropertyHandle, row::RowBehavior};

pub(crate) type Keepalive = Box<dyn Any>;

pub(crate) struct SettingRowInit {
    pub(crate) i18n_key: Option<&'static str>,
    pub(crate) handle: PropertyHandle,
    pub(crate) control: gtk::Widget,
    pub(crate) keepalive: Keepalive,
    pub(crate) full_width: bool,
    pub(crate) dirty_badge: Option<gtk::Label>,
    pub(crate) behavior: RowBehavior,
    pub(crate) unit: Option<String>,
}

pub(crate) struct SectionSpec {
    pub(crate) title_key: &'static str,
    pub(crate) items: Vec<SettingRowInit>,
}

pub(crate) struct PageSpec {
    pub(crate) header_key: &'static str,
    pub(crate) sections: Vec<SectionSpec>,
    /// Optional custom body, appended after the header. Used by pages like
    /// About that show static content instead of config-bound setting rows.
    pub(crate) content: Option<Box<dyn FnOnce() -> gtk::Widget>>,
}

pub(crate) fn page_spec(header_key: &'static str, sections: Vec<SectionSpec>) -> PageSpec {
    PageSpec {
        header_key,
        sections,
        content: None,
    }
}

/// A page whose body is a single custom widget rather than setting-row sections.
pub(crate) fn page_custom(
    header_key: &'static str,
    content: impl FnOnce() -> gtk::Widget + 'static,
) -> PageSpec {
    PageSpec {
        header_key,
        sections: Vec::new(),
        content: Some(Box::new(content)),
    }
}
