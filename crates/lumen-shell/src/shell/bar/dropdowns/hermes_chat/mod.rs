#![allow(deprecated)]

mod factory;

use std::sync::Arc;

use gtk::prelude::*;
use lumen_config::ConfigService;
use lumen_hermes::{
    ApprovalRequest, HermesChatService, HermesMessage, HermesRole, HermesSessionSummary,
    HermesStatus, MessageStatus, markdownish_to_pango,
};
use lumen_widgets::watch;
use relm4::{gtk, prelude::*};

pub(super) use self::factory::Factory;
use crate::shell::{bar::dropdowns::scaled_dimension, helpers::COMPONENT_CSS_PRIORITY};

const BASE_WIDTH: f32 = 620.0;
const BASE_HEIGHT: f32 = 560.0;

pub(crate) struct HermesChatDropdownInit {
    pub(crate) config: Arc<ConfigService>,
    pub(crate) hermes_chat: Arc<HermesChatService>,
}

pub(crate) struct HermesChatDropdown {
    config: Arc<ConfigService>,
    hermes_chat: Arc<HermesChatService>,
    css: gtk::CssProvider,
    status: HermesStatus,
    messages: Vec<HermesMessage>,
    sessions: Vec<HermesSessionSummary>,
    active_session_id: Option<String>,
    approval: Option<ApprovalRequest>,
    ui: HermesChatUi,
}

#[derive(Clone)]
pub(crate) struct HermesChatUi {
    status: gtk::Label,
    session_selector: gtk::ComboBoxText,
    transcript: gtk::Box,
    scroller: gtk::ScrolledWindow,
    composer: gtk::TextView,
    send: gtk::Button,
    stop: gtk::Button,
    new_chat: gtk::Button,
    runtime_warning: gtk::Label,
    approval_box: gtk::Box,
    approval_label: gtk::Label,
}

#[derive(Debug)]
pub(crate) enum HermesChatDropdownMsg {
    Send,
    Stop,
    NewChat,
    SessionChanged,
    Approve,
    Reject,
    Opened(bool),
}

#[derive(Debug)]
pub(crate) enum HermesChatDropdownCmd {
    StateChanged,
    ConfigChanged,
}

impl Component for HermesChatDropdown {
    type Init = HermesChatDropdownInit;
    type Input = HermesChatDropdownMsg;
    type Output = ();
    type CommandOutput = HermesChatDropdownCmd;
    type Root = gtk::Popover;
    type Widgets = ();

    fn init_root() -> Self::Root {
        let popover = gtk::Popover::new();
        popover.set_css_classes(&["dropdown", "hermes-chat-dropdown"]);
        popover.set_has_arrow(false);
        popover
    }

    #[allow(clippy::too_many_lines)]
    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let css = gtk::CssProvider::new();
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&display, &css, COMPONENT_CSS_PRIORITY);
        }

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        outer.add_css_class("hc-root");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.add_css_class("hc-header");
        let title = gtk::Label::new(Some("Hermes Chat"));
        title.add_css_class("hc-title");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        header.append(&title);

        let status = gtk::Label::new(Some("Connecting"));
        status.add_css_class("hc-status");
        header.append(&status);
        outer.append(&header);

        let session_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        session_row.add_css_class("hc-session-row");
        let session_selector = gtk::ComboBoxText::new();
        session_selector.set_hexpand(true);
        let session_sender = sender.input_sender().clone();
        session_selector.connect_changed(move |_| {
            session_sender.emit(HermesChatDropdownMsg::SessionChanged);
        });
        session_row.append(&session_selector);
        let new_chat = gtk::Button::with_label("New");
        let new_sender = sender.input_sender().clone();
        new_chat.connect_clicked(move |_| new_sender.emit(HermesChatDropdownMsg::NewChat));
        session_row.append(&new_chat);
        outer.append(&session_row);

        let runtime_warning = gtk::Label::new(Some(
            "Remote runtime: Hermes tools run on the API server host, not this desktop.",
        ));
        runtime_warning.add_css_class("hc-runtime-warning");
        runtime_warning.set_wrap(true);
        runtime_warning.set_xalign(0.0);
        outer.append(&runtime_warning);

        let transcript = gtk::Box::new(gtk::Orientation::Vertical, 10);
        transcript.add_css_class("hc-transcript");
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroller.set_child(Some(&transcript));
        outer.append(&scroller);

        let approval_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        approval_box.add_css_class("hc-approval");
        let approval_label = gtk::Label::new(None);
        approval_label.set_wrap(true);
        approval_label.set_xalign(0.0);
        approval_box.append(&approval_label);
        let approval_actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let approve = gtk::Button::with_label("Approve");
        let approve_sender = sender.input_sender().clone();
        approve.connect_clicked(move |_| approve_sender.emit(HermesChatDropdownMsg::Approve));
        let reject = gtk::Button::with_label("Reject");
        let reject_sender = sender.input_sender().clone();
        reject.connect_clicked(move |_| reject_sender.emit(HermesChatDropdownMsg::Reject));
        approval_actions.append(&approve);
        approval_actions.append(&reject);
        approval_box.append(&approval_actions);
        outer.append(&approval_box);

        let composer_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        composer_box.add_css_class("hc-composer");
        let composer = gtk::TextView::new();
        composer.set_wrap_mode(gtk::WrapMode::WordChar);
        composer.set_vexpand(false);
        composer.set_hexpand(true);
        composer.set_size_request(-1, 74);
        composer_box.append(&composer);
        let button_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let send = gtk::Button::with_label("Send");
        let send_sender = sender.input_sender().clone();
        send.connect_clicked(move |_| send_sender.emit(HermesChatDropdownMsg::Send));
        let stop = gtk::Button::with_label("Stop");
        let stop_sender = sender.input_sender().clone();
        stop.connect_clicked(move |_| stop_sender.emit(HermesChatDropdownMsg::Stop));
        button_box.append(&send);
        button_box.append(&stop);
        composer_box.append(&button_box);
        outer.append(&composer_box);

        root.set_child(Some(&outer));
        let visibility_sender = sender.input_sender().clone();
        root.connect_visible_notify(move |popover| {
            visibility_sender.emit(HermesChatDropdownMsg::Opened(popover.is_visible()));
        });

        let status_prop = init.hermes_chat.status.clone();
        let messages_prop = init.hermes_chat.messages.clone();
        let sessions_prop = init.hermes_chat.sessions.clone();
        let active_prop = init.hermes_chat.active_session_id.clone();
        let approval_prop = init.hermes_chat.approval.clone();
        watch!(
            sender,
            [
                status_prop.watch(),
                messages_prop.watch(),
                sessions_prop.watch(),
                active_prop.watch(),
                approval_prop.watch()
            ],
            |out| {
                let _ = out.send(HermesChatDropdownCmd::StateChanged);
            }
        );

        let dropdown_scale = init
            .config
            .config()
            .modules
            .hermes_chat
            .dropdown_scale
            .clone();
        let show_warning = init
            .config
            .config()
            .modules
            .hermes_chat
            .show_runtime_warning
            .clone();
        watch!(
            sender,
            [dropdown_scale.watch(), show_warning.watch()],
            |out| {
                let _ = out.send(HermesChatDropdownCmd::ConfigChanged);
            }
        );

        let mut model = Self {
            config: init.config,
            hermes_chat: init.hermes_chat,
            css,
            status: HermesStatus::Connecting,
            messages: Vec::new(),
            sessions: Vec::new(),
            active_session_id: None,
            approval: None,
            ui: HermesChatUi {
                status,
                session_selector,
                transcript,
                scroller,
                composer,
                send,
                stop,
                new_chat,
                runtime_warning,
                approval_box,
                approval_label,
            },
        };
        model.sync_state();
        model.render();
        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            HermesChatDropdownMsg::Send => {
                let buffer = self.ui.composer.buffer();
                let start = buffer.start_iter();
                let end = buffer.end_iter();
                let text = buffer.text(&start, &end, true).to_string();
                if !text.trim().is_empty() {
                    self.hermes_chat.send_message(text);
                    buffer.set_text("");
                }
            }
            HermesChatDropdownMsg::Stop => self.hermes_chat.stop_current(),
            HermesChatDropdownMsg::NewChat => self
                .hermes_chat
                .new_session(Some(String::from("Lumen Chat"))),
            HermesChatDropdownMsg::SessionChanged => {
                if let Some(session_id) = self.ui.session_selector.active_id() {
                    let session_id = session_id.to_string();
                    if self.active_session_id.as_deref() != Some(session_id.as_str()) {
                        self.hermes_chat.select_session(session_id);
                    }
                }
            }
            HermesChatDropdownMsg::Approve => self.hermes_chat.submit_approval(true, None),
            HermesChatDropdownMsg::Reject => self.hermes_chat.submit_approval(false, None),
            HermesChatDropdownMsg::Opened(true) => self.hermes_chat.connect(),
            HermesChatDropdownMsg::Opened(false) => {}
        }
    }

    fn update_cmd(
        &mut self,
        msg: Self::CommandOutput,
        _sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            HermesChatDropdownCmd::StateChanged => self.sync_state(),
            HermesChatDropdownCmd::ConfigChanged => {}
        }
        self.render();
    }
}

impl HermesChatDropdown {
    fn sync_state(&mut self) {
        self.status = self.hermes_chat.status.get();
        self.messages = self.hermes_chat.messages.get();
        self.sessions = self.hermes_chat.sessions.get();
        self.active_session_id = self.hermes_chat.active_session_id.get();
        self.approval = self.hermes_chat.approval.get();
    }

    fn render(&self) {
        let ui = &self.ui;
        let modules = &self.config.config().modules.hermes_chat;
        let scale = self.config.config().styling.scale.get().value();
        let dropdown_scale = modules.dropdown_scale.get().value();
        self.css.load_from_string(&build_css(scale, dropdown_scale));
        ui.scroller
            .set_min_content_height(scaled_dimension(BASE_HEIGHT, scale * dropdown_scale));
        ui.scroller
            .set_max_content_height(scaled_dimension(BASE_HEIGHT, scale * dropdown_scale));
        ui.scroller
            .set_min_content_width(scaled_dimension(BASE_WIDTH, scale * dropdown_scale));
        ui.status.set_label(self.status.label());
        ui.status
            .set_css_classes(&["hc-status", self.status.css_class()]);
        ui.send.set_sensitive(matches!(
            self.status,
            HermesStatus::Connected | HermesStatus::Offline(_) | HermesStatus::Error(_)
        ));
        ui.stop
            .set_sensitive(matches!(self.status, HermesStatus::Busy));
        ui.new_chat
            .set_sensitive(!matches!(self.status, HermesStatus::Busy));
        ui.runtime_warning
            .set_visible(modules.show_runtime_warning.get());
        self.render_sessions(ui);
        self.render_messages(ui);
        self.render_approval(ui);
    }

    fn render_sessions(&self, ui: &HermesChatUi) {
        ui.session_selector.remove_all();
        for session in &self.sessions {
            ui.session_selector
                .append(Some(&session.id), &session.title);
        }
        if let Some(active) = &self.active_session_id {
            ui.session_selector.set_active_id(Some(active));
        } else if !self.sessions.is_empty() {
            ui.session_selector.set_active(Some(0));
        }
    }

    fn render_messages(&self, ui: &HermesChatUi) {
        while let Some(child) = ui.transcript.first_child() {
            ui.transcript.remove(&child);
        }
        if self.messages.is_empty() {
            let empty = gtk::Label::new(Some("Start a new chat with Hermes Agent."));
            empty.add_css_class("hc-empty");
            empty.set_xalign(0.0);
            ui.transcript.append(&empty);
            return;
        }
        for message in &self.messages {
            ui.transcript.append(&message_row(message));
        }
    }

    fn render_approval(&self, ui: &HermesChatUi) {
        if let Some(approval) = &self.approval {
            ui.approval_box.set_visible(true);
            ui.approval_label.set_label(&approval.prompt);
        } else {
            ui.approval_box.set_visible(false);
        }
    }
}

fn message_row(message: &HermesMessage) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
    row.add_css_class("hc-message");
    row.add_css_class(match message.role {
        HermesRole::User => "user",
        HermesRole::Assistant => "assistant",
        HermesRole::System => "system",
        HermesRole::Tool => "tool",
        HermesRole::Error => "error",
    });
    let header = gtk::Label::new(Some(match message.role {
        HermesRole::User => "You",
        HermesRole::Assistant => match message.status {
            MessageStatus::Streaming => "Hermes is typing…",
            MessageStatus::Stopped => "Hermes stopped",
            MessageStatus::Error => "Hermes error",
            MessageStatus::Complete => "Hermes",
        },
        HermesRole::System => "System",
        HermesRole::Tool => "Tool",
        HermesRole::Error => "Error",
    }));
    header.add_css_class("hc-message-role");
    header.set_xalign(0.0);
    row.append(&header);

    if !message.content.is_empty() {
        let body = gtk::Label::new(None);
        body.set_markup(&markdownish_to_pango(&message.content));
        body.set_wrap(true);
        body.set_selectable(true);
        body.set_xalign(0.0);
        body.add_css_class("hc-message-body");
        row.append(&body);
    }

    for event in &message.tool_events {
        let tool = gtk::Label::new(Some(&format!(
            "{} · {} · {}",
            event.status, event.tool, event.label
        )));
        tool.add_css_class("hc-tool-event");
        tool.set_xalign(0.0);
        tool.set_wrap(true);
        row.append(&tool);
    }
    row
}

fn build_css(full_scale: f32, dropdown_scale: f32) -> String {
    format!(
        r#"
        .hermes-chat-dropdown .hc-root {{
            min-width: {width}px;
            background: var(--bg-surface);
            border: 1px solid var(--border-default);
            border-radius: var(--rounding-window);
            padding: calc(var(--space-md) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-header {{ margin-bottom: calc(var(--space-sm) * {dropdown_scale}); }}
        .hermes-chat-dropdown .hc-title {{ font-weight: var(--weight-bold); font-size: calc(var(--text-lg) * {dropdown_scale}); }}
        .hermes-chat-dropdown .hc-status {{ color: var(--fg-muted); font-size: calc(var(--text-sm) * {dropdown_scale}); }}
        .hermes-chat-dropdown .hc-status.ok {{ color: #4fb86a; }}
        .hermes-chat-dropdown .hc-status.busy {{ color: #61afef; }}
        .hermes-chat-dropdown .hc-status.error {{ color: #e2604f; }}
        .hermes-chat-dropdown .hc-session-row {{ margin-bottom: calc(var(--space-sm) * {dropdown_scale}); }}
        .hermes-chat-dropdown .hc-runtime-warning {{ color: #e0a93e; font-size: calc(var(--text-xs) * {dropdown_scale}); margin-bottom: calc(var(--space-sm) * {dropdown_scale}); }}
        .hermes-chat-dropdown .hc-transcript {{ padding: calc(var(--space-sm) * {dropdown_scale}); }}
        .hermes-chat-dropdown .hc-message {{ padding: calc(var(--space-sm) * {dropdown_scale}); border-radius: var(--rounding-element); border: 1px solid var(--border-default); background: var(--bg-elevated); }}
        .hermes-chat-dropdown .hc-message.user {{ background: alpha(#4f8cff, 0.12); }}
        .hermes-chat-dropdown .hc-message.error {{ border-color: alpha(#e2604f, 0.55); }}
        .hermes-chat-dropdown .hc-message-role {{ color: var(--fg-muted); font-size: calc(var(--text-xs) * {dropdown_scale}); font-weight: var(--weight-semibold); }}
        .hermes-chat-dropdown .hc-message-body {{ color: var(--fg-default); font-size: calc(var(--text-sm) * {dropdown_scale}); }}
        .hermes-chat-dropdown .hc-tool-event {{ color: var(--fg-muted); font-family: var(--font-mono); font-size: calc(var(--text-xs) * {dropdown_scale}); }}
        .hermes-chat-dropdown .hc-composer {{ margin-top: calc(var(--space-sm) * {dropdown_scale}); }}
        .hermes-chat-dropdown .hc-approval {{ border: 1px solid alpha(#e0a93e, 0.5); border-radius: var(--rounding-element); padding: calc(var(--space-sm) * {dropdown_scale}); margin-top: calc(var(--space-sm) * {dropdown_scale}); }}
        .hermes-chat-dropdown .hc-empty {{ color: var(--fg-muted); padding: calc(var(--space-md) * {dropdown_scale}); }}
        "#,
        width = scaled_dimension(BASE_WIDTH, full_scale * dropdown_scale),
        dropdown_scale = dropdown_scale
    )
}
