//! About page: static information about Lumen — logo, overview, the full
//! module catalogue, and credits. Unlike other pages this has no config-bound
//! rows, so it uses `page_custom` to render a hand-built body.

use lumen_config::Config;
use lumen_i18n::t;
use relm4::gtk::{self, gdk, glib, prelude::*};

use crate::pages::{nav::LeafEntry, spec::page_custom};

const WAYLE_URL: &str = "https://github.com/wayle-rs/wayle";
const LUMEN_URL: &str = "https://github.com/lumen-rs/lumen";

// Brand lockup (mark + monospace wordmark), pre-rendered from the SVG so it
// renders without a gdk-pixbuf SVG loader. Dark variant has a light wordmark
// (for dark themes), light variant a dark wordmark; picked by theme luminance.
const LOCKUP_DARK: &[u8] = include_bytes!("../../../assets/lumen-lockup-dark.png");
const LOCKUP_LIGHT: &[u8] = include_bytes!("../../../assets/lumen-lockup-light.png");
const LOCKUP_W: i32 = 300;
const LOCKUP_H: i32 = 76;

/// Bar modules shown in the catalogue: (name i18n key, description i18n key).
/// Names reuse the sidebar nav keys so they stay translated in one place.
const MODULES: &[(&str, &str)] = &[
    ("settings-nav-battery", "settings-about-module-battery"),
    ("settings-nav-bluetooth", "settings-about-module-bluetooth"),
    (
        "settings-nav-brightness",
        "settings-about-module-brightness",
    ),
    ("settings-nav-cava", "settings-about-module-cava"),
    ("settings-nav-clock", "settings-about-module-clock"),
    ("settings-nav-cpu", "settings-about-module-cpu"),
    ("settings-nav-custom", "settings-about-module-custom"),
    ("settings-nav-dashboard", "settings-about-module-dashboard"),
    (
        "settings-nav-hyprland-workspaces",
        "settings-about-module-hyprland-workspaces",
    ),
    (
        "settings-nav-hyprsunset",
        "settings-about-module-hyprsunset",
    ),
    (
        "settings-nav-idle-inhibit",
        "settings-about-module-idle-inhibit",
    ),
    (
        "settings-nav-keybind-mode",
        "settings-about-module-keybind-mode",
    ),
    (
        "settings-nav-keyboard-input",
        "settings-about-module-keyboard-input",
    ),
    (
        "settings-nav-mango-workspaces",
        "settings-about-module-mango-workspaces",
    ),
    ("settings-nav-media", "settings-about-module-media"),
    (
        "settings-nav-microphone",
        "settings-about-module-microphone",
    ),
    (
        "settings-nav-model-usage",
        "settings-about-module-model-usage",
    ),
    ("settings-nav-netstat", "settings-about-module-netstat"),
    ("settings-nav-network", "settings-about-module-network"),
    (
        "settings-nav-niri-workspaces",
        "settings-about-module-niri-workspaces",
    ),
    (
        "settings-nav-notification",
        "settings-about-module-notification",
    ),
    ("settings-nav-power", "settings-about-module-power"),
    ("settings-nav-ram", "settings-about-module-ram"),
    ("settings-nav-separator", "settings-about-module-separator"),
    ("settings-nav-storage", "settings-about-module-storage"),
    ("settings-nav-systray", "settings-about-module-systray"),
    ("settings-nav-volume", "settings-about-module-volume"),
    ("settings-nav-weather", "settings-about-module-weather"),
    (
        "settings-nav-window-title",
        "settings-about-module-window-title",
    ),
    (
        "settings-nav-world-clock",
        "settings-about-module-world-clock",
    ),
];

pub(crate) fn entry(_config: &Config) -> LeafEntry {
    LeafEntry {
        id: "about",
        i18n_key: "settings-nav-about",
        icon: "ld-info-symbolic",
        spec: page_custom("settings-page-about", build_about_content),
    }
}

fn build_about_content() -> gtk::Widget {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    root.add_css_class("settings-about");

    root.append(&build_hero());
    root.append(&build_overview_card());
    root.append(&build_modules_card());
    root.append(&build_credits_card());

    root.upcast()
}

/// Logo lockup, version, and tagline, centered at the top of the page.
fn build_hero() -> gtk::Box {
    let hero = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .build();
    hero.add_css_class("settings-about-hero");

    hero.append(&build_lockup());

    let version = gtk::Label::new(Some(&format!(
        "{} {}",
        t("settings-about-version-label"),
        env!("CARGO_PKG_VERSION")
    )));
    version.add_css_class("settings-about-version");
    hero.append(&version);

    let tagline = gtk::Label::builder()
        .label(t("settings-about-tagline"))
        .wrap(true)
        .justify(gtk::Justification::Center)
        .build();
    tagline.add_css_class("settings-about-tagline");
    hero.append(&tagline);

    hero
}

/// The brand lockup as a `Picture`, swapping to the theme-correct variant once
/// the widget is mapped and the theme's foreground color is known.
fn build_lockup() -> gtk::Picture {
    let picture = gtk::Picture::new();
    picture.add_css_class("settings-about-logo");
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_can_shrink(true);
    picture.set_halign(gtk::Align::Center);
    picture.set_size_request(LOCKUP_W, LOCKUP_H);

    if let Some(texture) = lockup_texture(LOCKUP_DARK) {
        picture.set_paintable(Some(&texture));
    }

    picture.connect_map(|picture| {
        // A light foreground color means a dark theme, which wants the
        // light-wordmark (dark) lockup; otherwise use the light variant.
        let fg = picture.color();
        let luminance = 0.2126 * fg.red() + 0.7152 * fg.green() + 0.0722 * fg.blue();
        let bytes = if luminance >= 0.5 {
            LOCKUP_DARK
        } else {
            LOCKUP_LIGHT
        };
        if let Some(texture) = lockup_texture(bytes) {
            picture.set_paintable(Some(&texture));
        }
    });

    picture
}

fn lockup_texture(bytes: &'static [u8]) -> Option<gdk::Texture> {
    gdk::Texture::from_bytes(&glib::Bytes::from_static(bytes)).ok()
}

/// "What is Lumen?" overview.
fn build_overview_card() -> gtk::Box {
    let card = card("settings-about-overview-title");
    card.append(&body_label("settings-about-overview"));
    card
}

/// The full catalogue of bar modules, name + short description per row.
fn build_modules_card() -> gtk::Box {
    let card = card("settings-about-modules-title");

    let grid = gtk::Grid::new();
    grid.add_css_class("settings-about-modules");
    grid.set_row_spacing(10);
    grid.set_column_spacing(20);

    for (row, (name_key, desc_key)) in MODULES.iter().enumerate() {
        let name = gtk::Label::builder()
            .label(t(name_key))
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .xalign(0.0)
            .build();
        name.add_css_class("settings-about-module-name");

        let desc = gtk::Label::builder()
            .label(t(desc_key))
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .hexpand(true)
            .wrap(true)
            .build();
        desc.add_css_class("settings-about-module-desc");

        grid.attach(&name, 0, row as i32, 1, 1);
        grid.attach(&desc, 1, row as i32, 1, 1);
    }

    card.append(&grid);
    card
}

/// Credits: the Wayle fork acknowledgement, repo links, and license.
fn build_credits_card() -> gtk::Box {
    let card = card("settings-about-credits-title");

    card.append(&body_label("settings-about-fork"));

    let links = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Start)
        .build();
    links.add_css_class("settings-about-links");
    links.append(&link(WAYLE_URL, "settings-about-wayle-link"));
    links.append(&link(LUMEN_URL, "settings-about-repo-link"));
    card.append(&links);

    let license = gtk::Label::builder()
        .label(t("settings-about-license"))
        .halign(gtk::Align::Start)
        .build();
    license.add_css_class("settings-about-license");
    card.append(&license);

    card
}

/// A titled card matching the `.settings-group` look used elsewhere.
fn card(title_key: &str) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    card.add_css_class("settings-about-card");

    let title = gtk::Label::builder()
        .label(t(title_key))
        .halign(gtk::Align::Start)
        .build();
    title.add_css_class("settings-about-card-title");
    card.append(&title);

    card
}

/// A left-aligned, wrapping paragraph from an i18n key.
fn body_label(key: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(t(key))
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .wrap(true)
        .build();
    label.add_css_class("settings-about-text");
    label
}

/// A browser-opening link button labelled from an i18n key.
fn link(uri: &str, label_key: &str) -> gtk::LinkButton {
    let button = gtk::LinkButton::with_label(uri, &t(label_key));
    button.set_halign(gtk::Align::Start);
    button.add_css_class("settings-about-link");
    button
}
