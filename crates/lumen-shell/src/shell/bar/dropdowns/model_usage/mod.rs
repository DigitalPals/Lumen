mod factory;
mod helpers;

use std::{cell::Cell, rc::Rc, sync::Arc};

use chrono::Utc;
use gtk::prelude::*;
use lumen_config::{ConfigService, schemas::modules::ProviderOrder};
use lumen_model_usage::{
    ModelUsageService, ModelUsageStatus, ProviderEntry, ProviderKind, UsageSnapshot, UsageWindow,
};
use lumen_widgets::watch;
use relm4::{gtk, prelude::*};

pub(super) use self::factory::Factory;
use self::helpers::{
    credit_card, fmt_countdown, fmt_reset_abs, format_local_time, subtitle, unavailable_notice,
};
use crate::shell::{bar::dropdowns::scaled_dimension, helpers::COMPONENT_CSS_PRIORITY};

const BASE_WIDTH: f32 = 560.0;
/// Estimated horizontal space around the cards viewport (`.dropdown-content`
/// padding plus the `.dropdown` border), slightly overestimated so the
/// first-open width fallback errs toward a too-tall (gap) rather than a
/// too-short (scrollbar) measure of wrapped labels.
const CARDS_H_INSET: f32 = 28.0;
/// Cards area is capped at this height; taller content scrolls.
///
/// The cards viewport is measured and pinned right before each popup because
/// layer-shell popovers cannot reliably change size while mapped (the
/// compositor leaves them at the mapped size or dismisses them).
const MAX_CARDS_HEIGHT: f32 = 560.0;
/// Cards-area height reserved while the first payload is still loading, so
/// the popover maps at roughly the size of the loaded content.
const LOADING_HEIGHT: f32 = 300.0;
/// Reopening the dropdown within this window keeps the cached data.
const REOPEN_FETCH_SECS: i64 = 30;
/// Matches `lumen-model-usage`'s service-side minimum poll interval.
const MIN_REFRESH_INTERVAL_SECS: u32 = 120;
const BAR_SEGMENTS: i32 = 30;

const INSET_RGB: (f64, f64, f64) = (0.051, 0.067, 0.082);
const BORDER_RGB: (f64, f64, f64) = (0.137, 0.169, 0.208);

struct ProviderSpec {
    kind: ProviderKind,
    name: &'static str,
    tool: &'static str,
    accent_rgb: (f64, f64, f64),
    dot_class: &'static str,
}

const PROVIDERS: [ProviderSpec; 2] = [
    ProviderSpec {
        kind: ProviderKind::Claude,
        name: "Claude",
        tool: "Claude Code",
        accent_rgb: (0.851, 0.467, 0.341),
        dot_class: "claude",
    },
    ProviderSpec {
        kind: ProviderKind::Codex,
        name: "Codex",
        tool: "Codex CLI",
        accent_rgb: (0.310, 0.722, 0.659),
        dot_class: "codex",
    },
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Ok,
    Warn,
    Danger,
}

fn pct_level(pct: f64) -> Level {
    if pct <= 10.0 {
        Level::Danger
    } else if pct <= 25.0 {
        Level::Warn
    } else {
        Level::Ok
    }
}

fn level_class(level: Level) -> Option<&'static str> {
    match level {
        Level::Ok => None,
        Level::Warn => Some("warn"),
        Level::Danger => Some("danger"),
    }
}

fn level_rgb(level: Level, accent: (f64, f64, f64)) -> (f64, f64, f64) {
    match level {
        Level::Ok => accent,
        Level::Warn => (0.878, 0.663, 0.243),
        Level::Danger => (0.886, 0.376, 0.310),
    }
}

pub(crate) struct ModelUsageDropdownInit {
    pub(crate) config: Arc<ConfigService>,
    pub(crate) model_usage: Arc<ModelUsageService>,
}

pub(crate) struct ModelUsageDropdown {
    config: Arc<ConfigService>,
    model_usage: Arc<ModelUsageService>,
    css: gtk::CssProvider,
    snapshot: Option<Arc<UsageSnapshot>>,
    status: ModelUsageStatus,
    active: usize,
    /// Scaled `MAX_CARDS_HEIGHT`, shared with the pre-popup pinning closure.
    cards_cap: Rc<Cell<i32>>,
    /// Scaled cards width used for height-for-width measuring before the
    /// first allocation, shared with the pre-popup pinning closure.
    cards_width: Rc<Cell<i32>>,
}

struct TabUi {
    button: gtk::Button,
    pct: gtk::Label,
}

pub(crate) struct ModelUsageUi {
    tabs_box: gtk::Box,
    tabs: Vec<TabUi>,
    refresh: gtk::Button,
    tool: gtk::Label,
    meta: gtk::Label,
    stack: gtk::Stack,
    /// One cards page per `PROVIDERS` entry; the homogeneous stack sizes to
    /// the tallest page so switching tabs never overflows the pinned height.
    provider_cards: [gtk::Box; PROVIDERS.len()],
    /// Stack page shown while no payload exists (loading or fetch error).
    status_card: gtk::Box,
    updated: gtk::Label,
    next_poll: gtk::Label,
    countdowns: Vec<(gtk::Label, i64)>,
}

#[derive(Debug)]
pub(crate) enum ModelUsageDropdownMsg {
    Refresh,
    SelectTab(usize),
    Opened(bool),
}

#[derive(Debug)]
pub(crate) enum ModelUsageDropdownCmd {
    /// The service published a new snapshot or status change.
    Loaded,
    ConfigChanged,
    Tick,
}

impl Component for ModelUsageDropdown {
    type Init = ModelUsageDropdownInit;
    type Input = ModelUsageDropdownMsg;
    type Output = ();
    type CommandOutput = ModelUsageDropdownCmd;
    type Root = gtk::Popover;
    type Widgets = ModelUsageUi;

    fn init_root() -> Self::Root {
        let popover = gtk::Popover::new();
        popover.set_css_classes(&["dropdown", "model-usage-dropdown"]);
        popover.set_has_arrow(false);
        popover
    }

    #[allow(clippy::too_many_lines)]
    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let config = init.config;
        let model_usage = init.model_usage;
        let css = gtk::CssProvider::new();
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&display, &css, COMPONENT_CSS_PRIORITY);
        }

        // Standard dropdown container: themed surface, rounded border.
        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        outer.set_css_classes(&["dropdown"]);

        // Header: provider tab switcher + refresh action.
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.add_css_class("dropdown-header");

        let tabs_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        tabs_box.add_css_class("mb-tabs");
        tabs_box.set_halign(gtk::Align::Start);
        tabs_box.set_valign(gtk::Align::Center);
        tabs_box.set_hexpand(true);
        header.append(&tabs_box);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        actions.add_css_class("dropdown-actions");
        let refresh = gtk::Button::from_icon_name("tb-refresh-symbolic");
        refresh.add_css_class("mb-refresh");
        refresh.set_valign(gtk::Align::Center);
        refresh.set_tooltip_text(Some("Refresh usage"));
        let refresh_sender = sender.input_sender().clone();
        refresh.connect_clicked(move |_| refresh_sender.emit(ModelUsageDropdownMsg::Refresh));
        actions.append(&refresh);
        header.append(&actions);
        outer.append(&header);

        // Content: provider meta row + scrollable cards.
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.add_css_class("dropdown-content");

        let provider_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        provider_row.add_css_class("mb-provider-row");

        let tool = gtk::Label::new(None);
        tool.add_css_class("mb-tool");
        tool.set_xalign(0.0);
        provider_row.append(&tool);

        let meta = gtk::Label::new(None);
        meta.add_css_class("mb-meta");
        meta.set_xalign(0.0);
        meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
        meta.set_hexpand(true);
        provider_row.append(&meta);

        body.append(&provider_row);

        // Both providers' cards stay built inside a homogeneous stack: its
        // measured height is the max of all pages, so the height pinned at
        // popup fits whichever tab the user switches to while it is open.
        let stack = gtk::Stack::new();
        stack.set_hhomogeneous(true);
        stack.set_vhomogeneous(true);
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_transition_duration(120);

        let provider_cards = [(); PROVIDERS.len()].map(|()| {
            let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
            page.add_css_class("mb-cards");
            stack.add_child(&page);
            page
        });
        let status_card = gtk::Box::new(gtk::Orientation::Vertical, 12);
        status_card.add_css_class("mb-cards");
        stack.add_child(&status_card);

        // The cards viewport is pinned to the content height (capped) right
        // before each popup; taller content scrolls.
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_child(Some(&stack));
        body.append(&scroll);
        outer.append(&body);

        // Footer: updated time + next poll countdown.
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        footer.add_css_class("dropdown-footer");

        let updated_key = gtk::Label::new(Some("updated"));
        updated_key.set_css_classes(&["mb-foot", "key"]);
        footer.append(&updated_key);

        let updated = gtk::Label::new(None);
        updated.add_css_class("mb-foot");
        updated.set_xalign(0.0);
        updated.set_hexpand(true);
        footer.append(&updated);

        let next_poll_key = gtk::Label::new(Some("next poll"));
        next_poll_key.set_css_classes(&["mb-foot", "key"]);
        footer.append(&next_poll_key);

        let next_poll = gtk::Label::new(None);
        next_poll.add_css_class("mb-foot");
        footer.append(&next_poll);
        outer.append(&footer);

        root.set_child(Some(&outer));

        let cards_cap = Rc::new(Cell::new(scaled_dimension(MAX_CARDS_HEIGHT, 1.0)));
        let cards_width = Rc::new(Cell::new(scaled_dimension(BASE_WIDTH - CARDS_H_INSET, 1.0)));
        let visibility_sender = sender.input_sender().clone();
        {
            let stack = stack.clone();
            let scroll = scroll.clone();
            let cards_cap = cards_cap.clone();
            let cards_width = cards_width.clone();
            root.connect_visible_notify(move |popover| {
                if popover.is_visible() {
                    // Pin the cards viewport to the current content height
                    // before the popover maps: this runs synchronously inside
                    // popup(), and a mapped layer-shell popover must not
                    // change size afterwards (tab switches and data loads
                    // re-render inside the pinned viewport instead). Before
                    // the first allocation the stack has no width yet, so
                    // fall back to the expected cards width: measuring at -1
                    // undersizes wrapped labels and causes a scrollbar.
                    let for_size = if stack.width() > 0 {
                        stack.width()
                    } else {
                        cards_width.get()
                    };
                    let (_, natural, _, _) = stack.measure(gtk::Orientation::Vertical, for_size);
                    let height = natural.min(cards_cap.get()).max(1);
                    scroll.set_min_content_height(height);
                    scroll.set_max_content_height(height);
                }
                visibility_sender.emit(ModelUsageDropdownMsg::Opened(popover.is_visible()));
            });
        }

        // 1s tick drives countdowns, the next-poll label, and auto-refresh.
        sender.command(|out, shutdown| {
            shutdown
                .register(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
                    loop {
                        interval.tick().await;
                        if out.send(ModelUsageDropdownCmd::Tick).is_err() {
                            break;
                        }
                    }
                })
                .drop_on_shutdown()
        });

        // Re-apply size and tab order when their settings change.
        {
            let model_usage_config = config.config().modules.model_usage.clone();
            let order = model_usage_config.provider_order.clone();
            let dropdown_scale = model_usage_config.dropdown_scale.clone();
            watch!(sender, [order.watch(), dropdown_scale.watch()], |out| {
                let _ = out.send(ModelUsageDropdownCmd::ConfigChanged);
            });
        }

        // Mirror the service's reactive state into this component.
        {
            let usage_prop = model_usage.usage.clone();
            let status_prop = model_usage.status.clone();
            watch!(sender, [usage_prop.watch(), status_prop.watch()], |out| {
                let _ = out.send(ModelUsageDropdownCmd::Loaded);
            });
        }

        let mut model = Self {
            config,
            snapshot: model_usage.usage.get(),
            status: model_usage.status.get(),
            model_usage,
            css,
            active: 0,
            cards_cap,
            cards_width,
        };

        let widgets_init = ModelUsageUi {
            tabs_box,
            tabs: Vec::new(),
            refresh,
            tool,
            meta,
            stack,
            provider_cards,
            status_card,
            updated,
            next_poll,
            countdowns: Vec::new(),
        };
        let mut widgets = widgets_init;
        model.apply_size(&root, &widgets);
        model.rebuild_tabs(&mut widgets, &sender);
        model.normalize_active();
        model.render(&mut widgets);

        ComponentParts { model, widgets }
    }

    fn update_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        message: Self::Input,
        _sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            ModelUsageDropdownMsg::Refresh => {
                self.model_usage.refresh();
            }
            ModelUsageDropdownMsg::SelectTab(index) => {
                if self.active != index {
                    self.active = index;
                    // Both tabs' cards are already built; just switch pages.
                    self.update_active(widgets);
                }
            }
            ModelUsageDropdownMsg::Opened(visible) => {
                let stale = self
                    .snapshot
                    .as_ref()
                    .map(|snapshot| {
                        (Utc::now() - snapshot.updated_at).num_seconds() >= REOPEN_FETCH_SECS
                    })
                    .unwrap_or(true);
                if visible && stale && !self.busy() {
                    self.model_usage.refresh();
                }
            }
        }
    }

    fn update_cmd_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        message: Self::CommandOutput,
        sender: ComponentSender<Self>,
        root: &Self::Root,
    ) {
        match message {
            ModelUsageDropdownCmd::ConfigChanged => {
                self.apply_size(root, widgets);
                self.normalize_active();
                self.rebuild_tabs(widgets, &sender);
                self.render(widgets);
            }
            ModelUsageDropdownCmd::Loaded => {
                self.snapshot = self.model_usage.usage.get();
                self.status = self.model_usage.status.get();
                self.normalize_active();
                self.render(widgets);
            }
            ModelUsageDropdownCmd::Tick => {
                if root.is_visible() {
                    let now = Utc::now().timestamp();
                    for (label, resets_at) in &widgets.countdowns {
                        label.set_label(&fmt_countdown(*resets_at - now));
                    }
                    widgets.next_poll.set_label(&self.next_poll_value());
                }
            }
        }
    }
}

impl ModelUsageDropdown {
    fn busy(&self) -> bool {
        self.status == ModelUsageStatus::Loading
    }

    /// Provider tab order as indices into `PROVIDERS`.
    fn order(&self) -> [usize; 2] {
        match self
            .config
            .config()
            .modules
            .model_usage
            .provider_order
            .get()
        {
            ProviderOrder::ClaudeFirst => [0, 1],
            ProviderOrder::CodexFirst => [1, 0],
        }
    }

    fn refresh_interval_secs(&self) -> u64 {
        u64::from(
            self.config
                .config()
                .modules
                .model_usage
                .refresh_interval_seconds
                .get()
                .max(MIN_REFRESH_INTERVAL_SECS),
        )
    }

    /// Combined global UI scale and dropdown scale.
    fn scale(&self) -> f32 {
        let config = self.config.config();
        config.styling.scale.get().value() * config.modules.model_usage.dropdown_scale.get().value()
    }

    fn apply_size(&self, root: &gtk::Popover, ui: &ModelUsageUi) {
        let scale = self.scale();
        // Width is fixed; height follows the content, measured and pinned by
        // the pre-popup closure so the popover never resizes while mapped.
        root.set_width_request(scaled_dimension(BASE_WIDTH, scale));
        root.set_height_request(-1);
        self.cards_cap
            .set(scaled_dimension(MAX_CARDS_HEIGHT, scale));
        self.cards_width
            .set(scaled_dimension(BASE_WIDTH - CARDS_H_INSET, scale));
        let spacing = scaled_dimension(12.0, scale);
        for page in &ui.provider_cards {
            page.set_spacing(spacing);
        }
        ui.status_card.set_spacing(spacing);
        let dropdown_scale = self
            .config
            .config()
            .modules
            .model_usage
            .dropdown_scale
            .get()
            .value();
        self.css.load_from_string(&build_css(scale, dropdown_scale));
    }

    fn rebuild_tabs(&self, ui: &mut ModelUsageUi, sender: &ComponentSender<Self>) {
        while let Some(child) = ui.tabs_box.first_child() {
            ui.tabs_box.remove(&child);
        }
        ui.tabs.clear();
        let scale = self.scale();
        for (position, provider_index) in self.order().into_iter().enumerate() {
            let tab = build_tab(&PROVIDERS[provider_index], position, scale, sender);
            ui.tabs_box.append(&tab.button);
            ui.tabs.push(tab);
        }
    }

    fn provider(&self, provider_index: usize) -> Option<&ProviderEntry> {
        self.snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.provider(PROVIDERS[provider_index].kind))
    }

    fn normalize_active(&mut self) {
        self.active =
            normalized_active_position(self.active, &self.order(), self.snapshot.as_deref());
    }

    fn updated_text(&self) -> String {
        self.snapshot
            .as_ref()
            .map(|snapshot| format_local_time(snapshot.updated_at))
            .unwrap_or_else(|| String::from("never"))
    }

    fn next_poll_value(&self) -> String {
        if self.busy() {
            return String::from("polling...");
        }
        let interval = self.refresh_interval_secs();
        let elapsed = self
            .snapshot
            .as_ref()
            .map(|snapshot| (Utc::now() - snapshot.updated_at).num_seconds().max(0) as u64)
            .unwrap_or(interval);
        format!("{}s", interval.saturating_sub(elapsed))
    }

    /// Full rebuild of all data-driven widgets.
    fn render(&self, ui: &mut ModelUsageUi) {
        let order = self.order();

        // Tab percentages.
        for (position, tab) in ui.tabs.iter().enumerate() {
            let provider_index = order[position.min(order.len() - 1)];
            let provider = self.provider(provider_index);
            let (text, class) = match provider.map(|entry| &entry.result) {
                Some(Err(_)) => (String::from("--%"), "danger"),
                Some(Ok(usage)) => match usage.min_remaining_percent() {
                    Some(pct) => (
                        format!("{pct:.0}%"),
                        level_class(pct_level(pct)).unwrap_or("dim"),
                    ),
                    None => (String::from("--"), "dim"),
                },
                None => (String::from("--"), "dim"),
            };
            tab.pct.set_label(&text);
            tab.pct.set_css_classes(&["mb-tab-pct", class]);
        }

        // Cards. Every page is rebuilt: the homogeneous stack then requests
        // the height of the tallest page, so the height pinned at popup fits
        // both tabs. The status page is emptied once a payload exists so it
        // stops contributing to that height.
        ui.countdowns.clear();
        clear_children(&ui.status_card);
        for page in &ui.provider_cards {
            clear_children(page);
        }
        if self.snapshot.is_some() {
            for (provider_index, page) in ui.provider_cards.iter().enumerate() {
                self.render_provider_page(page, provider_index, &mut ui.countdowns);
            }
        } else if self.busy() {
            ui.status_card.append(&build_loading(self.scale()));
        } else {
            ui.status_card.append(&build_notice(
                "No usage data",
                "Usage has not been fetched yet.",
                "danger",
            ));
        }

        self.update_active(ui);

        // Footer.
        ui.updated.set_label(&self.updated_text());
        ui.next_poll.set_label(&self.next_poll_value());
        ui.refresh.set_sensitive(!self.busy());
    }

    /// Cards for one provider, rendered into its stack page.
    fn render_provider_page(
        &self,
        page: &gtk::Box,
        provider_index: usize,
        countdowns: &mut Vec<(gtk::Label, i64)>,
    ) {
        let spec = &PROVIDERS[provider_index];
        let Some(entry) = self.provider(provider_index) else {
            page.append(&build_notice(
                "Provider disabled",
                "Enable this provider in the module settings to poll its usage.",
                "dim",
            ));
            return;
        };

        let usage = match &entry.result {
            Ok(usage) => usage,
            Err(error) => {
                let (title, body) = unavailable_notice(spec.kind, error);
                page.append(&build_notice(&title, &body, "danger"));
                return;
            }
        };
        if usage.windows.is_empty() {
            page.append(&build_notice(
                "No usage windows",
                "The CLI credentials were found, but no live limit windows were returned.",
                "dim",
            ));
        }

        let scale = self.scale();
        let spacing = scaled_dimension(12.0, scale);
        let grid = gtk::Grid::new();
        grid.set_column_homogeneous(true);
        grid.set_column_spacing(spacing as u32);
        grid.set_row_spacing(spacing as u32);
        let mut cell = 0_i32;
        for window in &usage.windows {
            let card = build_window_card(window, spec, scale, countdowns);
            grid.attach(&card, cell % 2, cell / 2, 1, 1);
            cell += 1;
        }
        if let Some((title, value, detail)) = usage.credits.as_ref().and_then(credit_card) {
            grid.attach(
                &build_credit_card(&title, &value, &detail, scale),
                cell % 2,
                cell / 2,
                1,
                1,
            );
            cell += 1;
        }
        if cell > 0 {
            page.append(&grid);
        }
    }

    /// Reflects `self.active` in the tabs, the meta row, and the visible
    /// stack page. Cheap: no widgets are rebuilt.
    fn update_active(&self, ui: &ModelUsageUi) {
        let order = self.order();
        for (position, tab) in ui.tabs.iter().enumerate() {
            if position == self.active {
                tab.button.add_css_class("active");
            } else {
                tab.button.remove_css_class("active");
            }
        }

        let active_index = order[self.active.min(order.len() - 1)];
        let spec = &PROVIDERS[active_index];
        ui.tool.set_label(spec.tool);
        let meta_text = self
            .provider(active_index)
            .and_then(|entry| entry.result.as_ref().ok())
            .map(|usage| subtitle(spec.kind, usage))
            .unwrap_or_default();
        ui.meta.set_visible(!meta_text.is_empty());
        ui.meta.set_label(&meta_text);

        if self.snapshot.is_some() {
            ui.stack.set_visible_child(&ui.provider_cards[active_index]);
        } else {
            ui.stack.set_visible_child(&ui.status_card);
        }
    }
}

fn normalized_active_position(
    active: usize,
    order: &[usize],
    snapshot: Option<&UsageSnapshot>,
) -> usize {
    let active = active.min(order.len() - 1);
    if provider_available(snapshot, order[active]) {
        return active;
    }

    order
        .iter()
        .position(|provider_index| provider_available(snapshot, *provider_index))
        .unwrap_or(0)
}

fn provider_available(snapshot: Option<&UsageSnapshot>, provider_index: usize) -> bool {
    snapshot
        .and_then(|snapshot| snapshot.provider(PROVIDERS[provider_index].kind))
        .is_some_and(|entry| entry.result.is_ok())
}

fn clear_children(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn build_tab(
    spec: &'static ProviderSpec,
    index: usize,
    scale: f32,
    sender: &ComponentSender<ModelUsageDropdown>,
) -> TabUi {
    let button = gtk::Button::new();
    button.set_css_classes(&["mb-tab"]);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, scaled_dimension(8.0, scale));
    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.set_css_classes(&["mb-dot", spec.dot_class]);
    let dot_size = scaled_dimension(8.0, scale);
    dot.set_size_request(dot_size, dot_size);
    dot.set_valign(gtk::Align::Center);
    row.append(&dot);

    let name = gtk::Label::new(Some(spec.name));
    name.add_css_class("mb-tab-name");
    row.append(&name);

    let pct = gtk::Label::new(None);
    pct.add_css_class("mb-tab-pct");
    row.append(&pct);

    button.set_child(Some(&row));
    let tab_sender = sender.input_sender().clone();
    button.connect_clicked(move |_| tab_sender.emit(ModelUsageDropdownMsg::SelectTab(index)));

    TabUi { button, pct }
}

fn build_window_card(
    window: &UsageWindow,
    spec: &ProviderSpec,
    scale: f32,
    countdowns: &mut Vec<(gtk::Label, i64)>,
) -> gtk::Box {
    let pct = window.remaining_percent();
    let level = pct_level(pct);
    let class = level_class(level);

    let card = gtk::Box::new(gtk::Orientation::Vertical, scaled_dimension(12.0, scale));
    card.set_css_classes(&["mb-card"]);
    if let Some(class) = class {
        card.add_css_class(class);
    }

    // Title row with optional LOW/CRITICAL badge.
    let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::new(Some(&window.label.to_uppercase()));
    title.add_css_class("mb-card-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    // One-line card labels must ellipsize: an unbreakable natural width
    // raises the card (and homogeneous grid) minimum above the viewport,
    // which clamps the pre-popup height measure and causes a scrollbar.
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title_row.append(&title);
    if let Some(class) = class {
        let badge = gtk::Label::new(Some(if level == Level::Danger {
            "CRITICAL"
        } else {
            "LOW"
        }));
        badge.set_css_classes(&["mb-card-badge", class]);
        badge.set_valign(gtk::Align::Center);
        title_row.append(&badge);
    }
    card.append(&title_row);

    // Big "80% remaining" line.
    let value_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    value_row.set_baseline_position(gtk::BaselinePosition::Bottom);

    let number = gtk::Label::new(Some(&format!("{pct:.0}")));
    number.set_css_classes(&["mb-value", class.unwrap_or("ok")]);
    number.set_valign(gtk::Align::Baseline);
    value_row.append(&number);

    let unit = gtk::Label::new(Some("%"));
    unit.add_css_class("mb-value-unit");
    unit.set_valign(gtk::Align::Baseline);
    value_row.append(&unit);

    let caption = gtk::Label::new(Some("remaining"));
    caption.add_css_class("mb-value-caption");
    caption.set_valign(gtk::Align::Baseline);
    value_row.append(&caption);
    card.append(&value_row);

    card.append(&build_segment_bar(
        pct,
        level_rgb(level, spec.accent_rgb),
        scale,
    ));

    // Reset countdown row.
    let reset_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    match window.resets_at {
        Some(resets_at) => {
            let key = gtk::Label::new(Some("resets in "));
            key.set_css_classes(&["mb-reset", "key"]);
            reset_row.append(&key);

            let countdown = gtk::Label::new(None);
            countdown.add_css_class("mb-reset");
            countdown.set_xalign(0.0);
            countdown.set_hexpand(true);
            countdown.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let epoch = resets_at.timestamp();
            countdown.set_label(&fmt_countdown(epoch - Utc::now().timestamp()));
            reset_row.append(&countdown);
            countdowns.push((countdown, epoch));

            let absolute = gtk::Label::new(Some(&fmt_reset_abs(resets_at)));
            absolute.set_css_classes(&["mb-reset", "faint"]);
            absolute.set_ellipsize(gtk::pango::EllipsizeMode::End);
            reset_row.append(&absolute);
        }
        None => {
            let unavailable = gtk::Label::new(Some("reset time unavailable"));
            unavailable.set_css_classes(&["mb-reset", "faint"]);
            unavailable.set_xalign(0.0);
            unavailable.set_hexpand(true);
            reset_row.append(&unavailable);
        }
    }
    card.append(&reset_row);
    card
}

fn build_segment_bar(pct: f64, color: (f64, f64, f64), scale: f32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_height(scaled_dimension(10.0, scale));
    area.set_hexpand(true);
    area.set_draw_func(move |_, cr, width, height| {
        let gap = 2.0;
        let total = f64::from(width);
        let h = f64::from(height);
        let seg = ((total - gap * f64::from(BAR_SEGMENTS - 1)) / f64::from(BAR_SEGMENTS)).max(1.0);
        let filled = ((pct / 100.0) * f64::from(BAR_SEGMENTS)).round() as i32;
        for i in 0..BAR_SEGMENTS {
            let x = f64::from(i) * (seg + gap);
            if i < filled {
                cr.set_source_rgb(color.0, color.1, color.2);
                cr.rectangle(x, 0.0, seg, h);
                let _ = cr.fill();
            } else {
                cr.set_source_rgb(INSET_RGB.0, INSET_RGB.1, INSET_RGB.2);
                cr.rectangle(x, 0.0, seg, h);
                let _ = cr.fill();
                cr.set_source_rgb(BORDER_RGB.0, BORDER_RGB.1, BORDER_RGB.2);
                cr.set_line_width(1.0);
                cr.rectangle(x + 0.5, 0.5, seg - 1.0, h - 1.0);
                let _ = cr.stroke();
            }
        }
    });
    area
}

fn build_credit_card(title: &str, value: &str, detail: &str, scale: f32) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, scaled_dimension(12.0, scale));
    card.set_css_classes(&["mb-card"]);

    let title_label = gtk::Label::new(Some(&title.to_uppercase()));
    title_label.add_css_class("mb-card-title");
    title_label.set_xalign(0.0);
    title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    card.append(&title_label);

    // The 21pt mono value ("$3.50 / $20.00") is the widest card content;
    // without ellipsizing it pushes the grid minimum above the viewport.
    // Long values get a smaller font instead of being cut off: the card is
    // half the dropdown wide and both card width and font scale with @FS@,
    // so character-count tiers hold at any scale (~12 mono chars fit at
    // 21pt).
    let value_label = gtk::Label::new(Some(value));
    value_label.add_css_class("mb-credit-value");
    match value.chars().count() {
        0..=12 => {}
        13..=15 => value_label.add_css_class("compact"),
        16..=18 => value_label.add_css_class("tight"),
        _ => value_label.add_css_class("mini"),
    }
    value_label.set_xalign(0.0);
    value_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    card.append(&value_label);

    if !detail.is_empty() {
        let detail_label = gtk::Label::new(Some(detail));
        detail_label.set_css_classes(&["mb-reset", "faint"]);
        detail_label.set_xalign(0.0);
        detail_label.set_wrap(true);
        card.append(&detail_label);
    }
    card
}

fn build_notice(title: &str, body: &str, accent_class: &str) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 4);
    card.set_css_classes(&["mb-card", "mb-notice"]);

    let title_label = gtk::Label::new(Some(title));
    title_label.set_css_classes(&["mb-notice-title", accent_class]);
    title_label.set_xalign(0.0);
    card.append(&title_label);

    let body_label = gtk::Label::new(Some(body));
    body_label.add_css_class("mb-notice-body");
    body_label.set_xalign(0.0);
    body_label.set_wrap(true);
    card.append(&body_label);
    card
}

/// Centered spinner shown while the first payload is being fetched. The card
/// reserves `LOADING_HEIGHT` so the popover maps near its loaded size.
fn build_loading(scale: f32) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.set_css_classes(&["mb-card", "mb-notice"]);
    card.set_size_request(-1, scaled_dimension(LOADING_HEIGHT, scale));

    let inner = gtk::Box::new(gtk::Orientation::Vertical, 8);
    inner.set_vexpand(true);
    inner.set_valign(gtk::Align::Center);

    let spinner = gtk::Spinner::new();
    spinner.set_halign(gtk::Align::Center);
    spinner.start();
    inner.append(&spinner);

    let title = gtk::Label::new(Some("Fetching usage"));
    title.set_css_classes(&["mb-notice-title", "ok"]);
    title.set_halign(gtk::Align::Center);
    inner.append(&title);

    let body = gtk::Label::new(Some(
        "Reading local CLI credentials and requesting live limit windows.",
    ));
    body.add_css_class("mb-notice-body");
    body.set_wrap(true);
    body.set_justify(gtk::Justification::Center);
    body.set_halign(gtk::Align::Center);
    inner.append(&body);

    card.append(&inner);
    card
}

/// ModelUsage-specific styles. Token values (`--text-*`, `--space-*`) already
/// include the global UI scale, so they are multiplied by the dropdown scale
/// only (`@DS@`); raw px/pt values get the full combined scale (`@FS@`).
#[allow(clippy::too_many_lines)]
fn build_css(full_scale: f32, dropdown_scale: f32) -> String {
    const TEMPLATE: &str = r#"
        .model-usage-dropdown .dropdown-header {
            padding: calc(var(--space-sm) * @DS@) calc(var(--space-md) * @DS@);
        }
        .model-usage-dropdown .dropdown-content {
            padding: calc((var(--space-sm) + var(--space-xs)) * @DS@);
        }
        .model-usage-dropdown .dropdown-footer {
            padding: calc(var(--space-sm) * @DS@) calc(var(--space-md) * @DS@);
        }
        .model-usage-dropdown .mb-tabs {
            background-color: var(--bg-base);
            border: 1px solid var(--border-default);
            border-radius: var(--rounding-element);
            padding: calc(3px * @FS@);
        }
        .model-usage-dropdown .mb-tab {
            background: none;
            border: none;
            box-shadow: none;
            padding: calc(6px * @FS@) calc(12px * @FS@);
            border-radius: var(--rounding-element);
            min-height: 0;
        }
        .model-usage-dropdown .mb-tab:hover {
            background-color: var(--bg-hover);
        }
        .model-usage-dropdown .mb-tab.active {
            background-color: var(--bg-elevated);
            border: 1px solid var(--border-default);
        }
        .model-usage-dropdown .mb-tab-name {
            color: var(--fg-muted);
            font-weight: var(--weight-semibold);
            font-size: calc(var(--text-md) * @DS@);
        }
        .model-usage-dropdown .mb-tab.active .mb-tab-name {
            color: var(--fg-default);
        }
        .model-usage-dropdown .mb-tab-pct {
            font-family: var(--font-mono);
            font-size: calc(var(--text-xs) * @DS@);
            font-weight: var(--weight-semibold);
        }
        .model-usage-dropdown .mb-tab-pct.dim { color: var(--fg-muted); }
        .model-usage-dropdown .mb-tab-pct.warn { color: #e0a93e; }
        .model-usage-dropdown .mb-tab-pct.danger { color: #e2604f; }
        .model-usage-dropdown .mb-dot {
            border-radius: calc(4px * @FS@);
            min-width: calc(8px * @FS@);
            min-height: calc(8px * @FS@);
        }
        .model-usage-dropdown .mb-dot.claude { background-color: #d97757; }
        .model-usage-dropdown .mb-dot.codex { background-color: #4fb8a8; }
        .model-usage-dropdown .mb-tab:not(.active) .mb-dot { opacity: 0.45; }
        .model-usage-dropdown .mb-refresh {
            background: none;
            border: 1px solid var(--border-default);
            border-radius: var(--rounding-element);
            min-width: calc(28px * @FS@);
            min-height: calc(28px * @FS@);
            padding: 0;
            color: var(--fg-muted);
            box-shadow: none;
        }
        .model-usage-dropdown .mb-refresh:hover { color: var(--fg-default); }
        .model-usage-dropdown .mb-provider-row {
            padding-bottom: calc(var(--space-md) * @DS@);
        }
        .model-usage-dropdown .mb-tool {
            color: var(--fg-default);
            font-weight: var(--weight-semibold);
            font-size: calc(var(--text-lg) * @DS@);
        }
        .model-usage-dropdown .mb-meta {
            color: var(--fg-muted);
            font-family: var(--font-mono);
            font-size: calc(var(--text-sm) * @DS@);
        }
        .model-usage-dropdown .mb-cards {
            /* The grid fills the ScrolledWindow viewport edge-to-edge, so each
               card's 1px border lands on the clip boundary and gets cut at
               fractional scale. Inset the cards so the borders clear the clip.
               Vertical needs only a hair: the viewport height is measured and
               pinned to include this padding, so the rows sit exactly inside.
               Horizontal needs more: the width is fixed (not measured), and
               the homogeneous grid's column rounding pushes the outer cards
               back into a small inset, so the outer left/right borders keep
               getting half-clipped. The larger horizontal inset absorbs that
               rounding; the cards lose a few px of width with width to spare. */
            padding: calc(2px * @FS@) calc(6px * @FS@);
        }
        .model-usage-dropdown .mb-card {
            background-color: var(--bg-elevated);
            border: 1px solid var(--border-default);
            border-radius: var(--rounding-element);
            padding: calc(var(--space-md) * @DS@);
        }
        .model-usage-dropdown .mb-card.warn { border-color: alpha(#e0a93e, 0.35); }
        .model-usage-dropdown .mb-card.danger { border-color: alpha(#e2604f, 0.35); }
        .model-usage-dropdown .mb-card-title {
            color: var(--fg-muted);
            font-weight: var(--weight-semibold);
            font-size: calc(var(--text-sm) * @DS@);
            letter-spacing: 1px;
        }
        .model-usage-dropdown .mb-card-badge {
            font-family: var(--font-mono);
            font-size: calc(var(--text-xs) * @DS@);
            font-weight: var(--weight-bold);
            border: 1px solid;
            border-radius: calc(4px * @FS@);
            padding: calc(1px * @FS@) calc(6px * @FS@);
        }
        .model-usage-dropdown .mb-card-badge.warn {
            color: #e0a93e;
            border-color: alpha(#e0a93e, 0.4);
        }
        .model-usage-dropdown .mb-card-badge.danger {
            color: #e2604f;
            border-color: alpha(#e2604f, 0.4);
        }
        .model-usage-dropdown .mb-value {
            font-family: var(--font-mono);
            font-size: calc(27pt * @FS@);
            font-weight: var(--weight-semibold);
        }
        .model-usage-dropdown .mb-value.ok { color: var(--fg-default); }
        .model-usage-dropdown .mb-value.warn { color: #e0a93e; }
        .model-usage-dropdown .mb-value.danger { color: #e2604f; }
        .model-usage-dropdown .mb-value-unit {
            font-family: var(--font-mono);
            font-size: calc(14pt * @FS@);
            color: var(--fg-muted);
        }
        .model-usage-dropdown .mb-value-caption {
            font-size: calc(var(--text-sm) * @DS@);
            color: var(--fg-subtle);
            margin-left: calc(var(--space-sm) * @DS@);
        }
        .model-usage-dropdown .mb-credit-value {
            font-family: var(--font-mono);
            font-size: calc(21pt * @FS@);
            font-weight: var(--weight-semibold);
            color: var(--fg-default);
        }
        .model-usage-dropdown .mb-credit-value.compact { font-size: calc(17pt * @FS@); }
        .model-usage-dropdown .mb-credit-value.tight { font-size: calc(14pt * @FS@); }
        .model-usage-dropdown .mb-credit-value.mini { font-size: calc(12pt * @FS@); }
        .model-usage-dropdown .mb-reset {
            font-family: var(--font-mono);
            font-size: calc(var(--text-sm) * @DS@);
            color: var(--fg-default);
        }
        .model-usage-dropdown .mb-reset.key { color: var(--fg-muted); }
        .model-usage-dropdown .mb-reset.faint { color: var(--fg-subtle); }
        .model-usage-dropdown .mb-notice-title {
            font-weight: var(--weight-semibold);
            font-size: calc(var(--text-md) * @DS@);
        }
        .model-usage-dropdown .mb-notice-title.ok { color: #46b576; }
        .model-usage-dropdown .mb-notice-title.dim { color: var(--fg-muted); }
        .model-usage-dropdown .mb-notice-title.danger { color: #e2604f; }
        .model-usage-dropdown .mb-notice-body {
            color: var(--fg-muted);
            font-size: calc(var(--text-sm) * @DS@);
        }
        .model-usage-dropdown .mb-foot {
            font-family: var(--font-mono);
            font-size: calc(var(--text-xs) * @DS@);
            color: var(--fg-muted);
        }
        .model-usage-dropdown .mb-foot.key { color: var(--fg-subtle); }
    "#;
    TEMPLATE
        .replace("@DS@", &format!("{dropdown_scale:.4}"))
        .replace("@FS@", &format!("{full_scale:.4}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_model_usage::{ModelUsageErrorKind, ProviderUsage};

    fn snapshot(providers: Vec<ProviderEntry>) -> UsageSnapshot {
        UsageSnapshot {
            updated_at: Utc::now(),
            providers,
        }
    }

    fn ok(kind: ProviderKind) -> ProviderEntry {
        ProviderEntry {
            kind,
            result: Ok(ProviderUsage {
                plan: None,
                account: None,
                windows: Vec::new(),
                credits: None,
            }),
        }
    }

    fn err(kind: ProviderKind) -> ProviderEntry {
        ProviderEntry {
            kind,
            result: Err(ModelUsageErrorKind::CredentialsNotFound),
        }
    }

    #[test]
    fn selects_codex_when_only_codex_available_and_claude_is_first() {
        let snapshot = snapshot(vec![ok(ProviderKind::Codex)]);

        assert_eq!(normalized_active_position(0, &[0, 1], Some(&snapshot)), 1);
    }

    #[test]
    fn selects_codex_first_tab_when_codex_is_first() {
        let snapshot = snapshot(vec![ok(ProviderKind::Codex)]);

        assert_eq!(normalized_active_position(0, &[1, 0], Some(&snapshot)), 0);
    }

    #[test]
    fn preserves_current_active_tab_when_it_is_available() {
        let snapshot = snapshot(vec![ok(ProviderKind::Claude), ok(ProviderKind::Codex)]);

        assert_eq!(normalized_active_position(1, &[0, 1], Some(&snapshot)), 1);
    }

    #[test]
    fn falls_back_to_first_tab_when_no_provider_is_available() {
        let snapshot = snapshot(vec![err(ProviderKind::Claude), err(ProviderKind::Codex)]);

        assert_eq!(normalized_active_position(1, &[0, 1], Some(&snapshot)), 0);
    }
}
