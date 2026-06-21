#![allow(deprecated, clippy::items_after_test_module)]

mod factory;

use std::{collections::HashMap, sync::Arc, time::Duration};

use gtk::prelude::*;
use lumen_config::{
    ConfigService,
    infrastructure::themes::Palette,
    schemas::styling::{HexColor, PaletteConfig, StylingConfig, ThemeProvider},
};
use lumen_hermes::{
    ApprovalKind, ApprovalRequest, BackgroundProcessItem, BackgroundProcessStatus,
    HermesChatService, HermesMessage, HermesRole, HermesSessionSummary, HermesStatus,
    MarkdownBlock, MessageStatus, SlashCommandSuggestion, SubagentItem, SubagentStatus, TodoItem,
    TodoStatus, ToolEvent, escape_pango_text, markdown_to_blocks, markdown_to_pango,
};
use lumen_widgets::watch;
use relm4::{gtk, prelude::*};

pub(super) use self::factory::Factory;
use crate::shell::{bar::dropdowns::scaled_dimension, helpers::COMPONENT_CSS_PRIORITY};

const BASE_WIDTH: f32 = 620.0;
const BASE_HEIGHT: f32 = 560.0;
const BACKGROUND_POLL_INTERVAL: Duration = Duration::from_secs(5);
const LOCAL_DRAFT_SCOPE: &str = "__lumen_hermes_chat_local__";

pub(crate) struct HermesChatDropdownInit {
    pub(crate) config: Arc<ConfigService>,
    pub(crate) hermes_chat: Arc<HermesChatService>,
}

pub(crate) struct HermesChatDropdown {
    config: Arc<ConfigService>,
    hermes_chat: Arc<HermesChatService>,
    input_sender: relm4::Sender<HermesChatDropdownMsg>,
    css: gtk::CssProvider,
    status: HermesStatus,
    messages: Vec<HermesMessage>,
    sessions: Vec<HermesSessionSummary>,
    active_session_id: Option<String>,
    approval: Option<ApprovalRequest>,
    todos: Vec<TodoItem>,
    subagents: Vec<SubagentItem>,
    background_processes: Vec<BackgroundProcessItem>,
    slash_suggestions: Vec<SlashCommandSuggestion>,
    slash_selection: usize,
    composer_drafts: HashMap<String, String>,
    queued_prompts: HashMap<String, Vec<QueuedPrompt>>,
    queue_sequence: u64,
    queue_drain_in_flight: bool,
    skip_queue_migration_once: bool,
    history_cursor: Option<usize>,
    history_draft: String,
    last_error: Option<String>,
    recording: bool,
    ui: HermesChatUi,
}

#[derive(Clone)]
pub(crate) struct HermesChatUi {
    title: gtk::Label,
    status: gtk::Label,
    status_group: gtk::Box,
    session_popover: gtk::Popover,
    session_current_name: gtk::Label,
    session_current_meta: gtk::Label,
    session_badge: gtk::Label,
    session_list: gtk::Box,
    session_activity_box: gtk::Box,
    transcript: gtk::Box,
    scroller: gtk::ScrolledWindow,
    composer: gtk::TextView,
    slash_box: gtk::Box,
    mic_button: gtk::Button,
    send: gtk::Button,
    stop: gtk::Button,
    new_chat: gtk::Button,
    approval_box: gtk::Box,
    approval_label: gtk::Label,
    approval_entry: gtk::PasswordEntry,
    approval_approve: gtk::Button,
    todo_box: gtk::Box,
    subagent_box: gtk::Box,
    background_box: gtk::Box,
    queue_box: gtk::Box,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct QueuedPrompt {
    id: String,
    text: String,
}

#[derive(Debug)]
pub(crate) enum HermesChatDropdownMsg {
    Send,
    SubmitOrAcceptSuggestion,
    Stop,
    NewChat,
    SelectSession(String),
    ToggleMic,
    AttachClicked,
    Approve,
    Reject,
    Opened(bool),
    ComposerChanged,
    ClearSlashSuggestions,
    HistoryOlder,
    HistoryNewer,
    QueuePromptNow(String),
    RemoveQueuedPrompt(String),
    StopBackgroundProcess(String),
    DismissBackgroundProcess(String),
}

#[derive(Debug)]
pub(crate) enum HermesChatDropdownCmd {
    RuntimeStateChanged,
    SessionsChanged,
    ActiveSessionChanged,
    ComposerPrefillChanged,
    SlashSuggestionsChanged,
    ConfigChanged,
    BackgroundRefreshTick,
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
        outer.set_css_classes(&["dropdown", "hc-root"]);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.set_css_classes(&["dropdown-header", "hc-header"]);
        let title = gtk::Label::new(Some("Hermes Chat"));
        title.set_css_classes(&["dropdown-title", "hc-title"]);
        title.set_xalign(0.0);
        title.set_hexpand(true);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_max_width_chars(34);
        header.append(&title);

        let status_group = gtk::Box::new(gtk::Orientation::Horizontal, 7);
        status_group.add_css_class("hc-status-group");
        status_group.set_valign(gtk::Align::Center);
        let status_dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        status_dot.add_css_class("hc-status-dot");
        status_dot.set_valign(gtk::Align::Center);
        status_group.append(&status_dot);
        let status = gtk::Label::new(Some("Connecting"));
        status.add_css_class("hc-status");
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        status.set_max_width_chars(34);
        status_group.append(&status);
        header.append(&status_group);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        actions.add_css_class("dropdown-actions");
        let new_chat = labeled_icon_button("ld-plus-symbolic", "New Session");
        new_chat.set_css_classes(&["secondary", "hc-action", "hc-new-chat"]);
        new_chat.set_tooltip_text(Some("Start a new Hermes session"));
        let new_sender = sender.input_sender().clone();
        new_chat.connect_clicked(move |_| new_sender.emit(HermesChatDropdownMsg::NewChat));
        actions.append(&new_chat);
        header.append(&actions);
        outer.append(&header);

        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.set_css_classes(&["dropdown-content", "hc-content"]);

        let session_section = gtk::Box::new(gtk::Orientation::Vertical, 8);
        session_section.add_css_class("hc-session-section");
        let session_label = gtk::Label::new(Some("Session"));
        session_label.add_css_class("hc-section-label");
        session_label.set_xalign(0.0);
        session_section.append(&session_label);

        let session_button = gtk::MenuButton::new();
        session_button.add_css_class("hc-session-button");
        session_button.set_hexpand(true);
        session_button.set_tooltip_text(Some("Switch Hermes session"));

        let session_button_content = gtk::Box::new(gtk::Orientation::Horizontal, 13);
        let session_icon = gtk::Image::from_icon_name("ld-message-circle-symbolic");
        session_icon.add_css_class("hc-session-icon");
        session_button_content.append(&session_icon);

        let session_text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        session_text.set_hexpand(true);
        let session_name_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let session_current_name = gtk::Label::new(Some("Hermes Chat"));
        session_current_name.add_css_class("hc-session-name");
        session_current_name.set_xalign(0.0);
        session_current_name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        session_name_row.append(&session_current_name);
        let session_badge = gtk::Label::new(Some("CURRENT"));
        session_badge.add_css_class("hc-session-badge");
        session_badge.set_valign(gtk::Align::Center);
        session_badge.set_visible(false);
        session_name_row.append(&session_badge);
        session_text.append(&session_name_row);
        let session_current_meta = gtk::Label::new(None);
        session_current_meta.add_css_class("hc-session-meta");
        session_current_meta.set_xalign(0.0);
        session_current_meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
        session_current_meta.set_visible(false);
        session_text.append(&session_current_meta);
        session_button_content.append(&session_text);

        let session_chevron = gtk::Image::from_icon_name("ld-chevron-down-symbolic");
        session_chevron.add_css_class("hc-session-chevron");
        session_button_content.append(&session_chevron);
        session_button.set_child(Some(&session_button_content));

        let session_popover = gtk::Popover::new();
        session_popover.add_css_class("hc-session-popover");
        session_popover.set_has_arrow(false);
        session_popover.set_position(gtk::PositionType::Bottom);
        let session_pop_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        session_pop_box.add_css_class("hc-session-pop");
        let session_pop_title = gtk::Label::new(Some("Recent sessions"));
        session_pop_title.add_css_class("hc-session-pop-title");
        session_pop_title.set_xalign(0.0);
        session_pop_box.append(&session_pop_title);
        let session_list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        session_list.add_css_class("hc-session-list");
        let session_list_scroller = gtk::ScrolledWindow::new();
        session_list_scroller.add_css_class("hc-session-list-scroller");
        session_list_scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        session_list_scroller.set_propagate_natural_height(true);
        session_list_scroller.set_max_content_height(320);
        session_list_scroller.set_child(Some(&session_list));
        session_pop_box.append(&session_list_scroller);
        session_popover.set_child(Some(&session_pop_box));
        session_button.set_popover(Some(&session_popover));
        let popover_width_anchor = session_button.clone();
        session_popover.connect_map(move |popover| {
            let width = popover_width_anchor.width();
            if width > 0 {
                popover.set_size_request(width, -1);
            }
        });

        session_section.append(&session_button);
        body.append(&session_section);

        let scroll_content = gtk::Box::new(gtk::Orientation::Vertical, 10);
        scroll_content.add_css_class("hc-scroll-content");
        let session_activity_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        session_activity_box.add_css_class("hc-session-activity");
        session_activity_box.set_visible(false);
        scroll_content.append(&session_activity_box);
        let transcript = gtk::Box::new(gtk::Orientation::Vertical, 10);
        transcript.add_css_class("hc-transcript");
        scroll_content.append(&transcript);
        let scroller = gtk::ScrolledWindow::new();
        scroller.add_css_class("hc-scroller");
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroller.set_child(Some(&scroll_content));
        body.append(&scroller);

        let approval_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        approval_box.add_css_class("hc-approval");
        let approval_label = gtk::Label::new(None);
        approval_label.set_wrap(true);
        approval_label.set_xalign(0.0);
        approval_box.append(&approval_label);
        let approval_entry = gtk::PasswordEntry::new();
        approval_entry.add_css_class("hc-approval-entry");
        approval_entry.set_hexpand(true);
        approval_entry.set_visible(false);
        approval_entry.set_placeholder_text(Some("Secret value"));
        approval_box.append(&approval_entry);
        let approval_actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let approve = gtk::Button::with_label("Approve");
        approve.add_css_class("primary");
        approve.set_cursor_from_name(Some("pointer"));
        let approve_sender = sender.input_sender().clone();
        approve.connect_clicked(move |_| approve_sender.emit(HermesChatDropdownMsg::Approve));
        let reject = gtk::Button::with_label("Reject");
        reject.add_css_class("secondary");
        reject.set_cursor_from_name(Some("pointer"));
        let reject_sender = sender.input_sender().clone();
        reject.connect_clicked(move |_| reject_sender.emit(HermesChatDropdownMsg::Reject));
        approval_actions.append(&approve);
        approval_actions.append(&reject);
        approval_box.append(&approval_actions);
        scroll_content.append(&approval_box);
        let todo_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        todo_box.add_css_class("hc-todos");
        scroll_content.append(&todo_box);
        let subagent_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        subagent_box.add_css_class("hc-subagents");
        scroll_content.append(&subagent_box);
        let background_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        background_box.add_css_class("hc-background");
        scroll_content.append(&background_box);
        let queue_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        queue_box.add_css_class("hc-queue");
        scroll_content.append(&queue_box);
        outer.append(&body);

        let composer_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        composer_box.set_css_classes(&["hc-composer"]);
        let slash_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        slash_box.add_css_class("hc-slash-suggestions");
        slash_box.set_visible(false);

        let attach_button = icon_button("ld-plus-symbolic");
        attach_button.set_css_classes(&["hc-icon-btn", "hc-attach"]);
        attach_button.set_valign(gtk::Align::End);
        attach_button.set_tooltip_text(Some("Attach image or document"));
        let attach_sender = sender.input_sender().clone();
        attach_button
            .connect_clicked(move |_| attach_sender.emit(HermesChatDropdownMsg::AttachClicked));
        composer_box.append(&attach_button);

        let composer = gtk::TextView::new();
        composer.add_css_class("hc-composer-input");
        composer.set_wrap_mode(gtk::WrapMode::WordChar);
        composer.set_vexpand(false);
        composer.set_hexpand(true);
        composer.set_size_request(-1, 24);
        let composer_sender = sender.input_sender().clone();
        composer
            .buffer()
            .connect_changed(move |_| composer_sender.emit(HermesChatDropdownMsg::ComposerChanged));
        let composer_key = gtk::EventControllerKey::new();
        let key_sender = sender.input_sender().clone();
        let key_composer = composer.clone();
        composer_key.connect_key_pressed(move |_, key, _, state| {
            handle_composer_key(&key_composer, key_sender.clone(), key, state)
        });
        composer.add_controller(composer_key);
        let composer_pill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        composer_pill.add_css_class("hc-input-pill");
        composer_pill.set_hexpand(true);
        composer_pill.set_valign(gtk::Align::End);
        composer_pill.append(&composer);
        composer_box.append(&composer_pill);

        let mic_button = icon_button("ld-mic-symbolic");
        mic_button.set_css_classes(&["hc-icon-btn", "hc-mic"]);
        mic_button.set_valign(gtk::Align::End);
        mic_button.set_tooltip_text(Some("Start voice input"));
        let mic_sender = sender.input_sender().clone();
        mic_button.connect_clicked(move |_| mic_sender.emit(HermesChatDropdownMsg::ToggleMic));
        composer_box.append(&mic_button);

        let send = icon_button("ld-arrow-up-symbolic");
        send.set_css_classes(&["hc-icon-btn", "hc-send"]);
        send.set_valign(gtk::Align::End);
        send.set_tooltip_text(Some("Send message"));
        let send_sender = sender.input_sender().clone();
        send.connect_clicked(move |_| send_sender.emit(HermesChatDropdownMsg::Send));
        composer_box.append(&send);

        let stop = icon_button("ld-square-symbolic");
        stop.set_css_classes(&["hc-icon-btn", "hc-stop"]);
        stop.set_valign(gtk::Align::End);
        stop.set_tooltip_text(Some("Stop Hermes"));
        let stop_sender = sender.input_sender().clone();
        stop.connect_clicked(move |_| stop_sender.emit(HermesChatDropdownMsg::Stop));
        composer_box.append(&stop);

        let composer_outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        composer_outer.set_css_classes(&["dropdown-footer", "hc-composer-wrap"]);
        composer_outer.append(&slash_box);
        composer_outer.append(&composer_box);
        outer.append(&composer_outer);

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
        let composer_prefill_prop = init.hermes_chat.composer_prefill.clone();
        let todos_prop = init.hermes_chat.todos.clone();
        let subagents_prop = init.hermes_chat.subagents.clone();
        let background_processes_prop = init.hermes_chat.background_processes.clone();
        let slash_suggestions_prop = init.hermes_chat.slash_suggestions.clone();
        let error_prop = init.hermes_chat.last_error.clone();
        watch!(
            sender,
            [
                status_prop.watch(),
                messages_prop.watch(),
                approval_prop.watch(),
                todos_prop.watch(),
                subagents_prop.watch(),
                background_processes_prop.watch(),
                error_prop.watch()
            ],
            |out| {
                let _ = out.send(HermesChatDropdownCmd::RuntimeStateChanged);
            }
        );
        watch!(sender, [sessions_prop.watch()], |out| {
            let _ = out.send(HermesChatDropdownCmd::SessionsChanged);
        });
        watch!(sender, [active_prop.watch()], |out| {
            let _ = out.send(HermesChatDropdownCmd::ActiveSessionChanged);
        });
        watch!(sender, [composer_prefill_prop.watch()], |out| {
            let _ = out.send(HermesChatDropdownCmd::ComposerPrefillChanged);
        });
        watch!(sender, [slash_suggestions_prop.watch()], |out| {
            let _ = out.send(HermesChatDropdownCmd::SlashSuggestionsChanged);
        });

        let config = init.config.config();
        let scale = config.styling.scale.clone();
        let dropdown_scale = config.modules.hermes_chat.dropdown_scale.clone();
        let show_tool_progress = init
            .config
            .config()
            .modules
            .hermes_chat
            .show_tool_progress
            .clone();
        watch!(
            sender,
            [
                scale.watch(),
                dropdown_scale.watch(),
                show_tool_progress.watch()
            ],
            |out| {
                let _ = out.send(HermesChatDropdownCmd::ConfigChanged);
            }
        );

        sender.command(|out, shutdown| {
            shutdown
                .register(async move {
                    let mut interval = tokio::time::interval(BACKGROUND_POLL_INTERVAL);
                    loop {
                        interval.tick().await;
                        if out
                            .send(HermesChatDropdownCmd::BackgroundRefreshTick)
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .drop_on_shutdown()
        });

        let mut model = Self {
            config: init.config,
            hermes_chat: init.hermes_chat,
            input_sender: sender.input_sender().clone(),
            css,
            status: HermesStatus::Connecting,
            messages: Vec::new(),
            sessions: Vec::new(),
            active_session_id: None,
            approval: None,
            todos: Vec::new(),
            subagents: Vec::new(),
            background_processes: Vec::new(),
            slash_suggestions: Vec::new(),
            slash_selection: 0,
            composer_drafts: HashMap::new(),
            queued_prompts: HashMap::new(),
            queue_sequence: 0,
            queue_drain_in_flight: false,
            skip_queue_migration_once: false,
            history_cursor: None,
            history_draft: String::new(),
            last_error: None,
            recording: false,
            ui: HermesChatUi {
                title,
                status,
                status_group,
                session_popover,
                session_current_name,
                session_current_meta,
                session_badge,
                session_list,
                session_activity_box,
                transcript,
                scroller,
                composer,
                slash_box,
                mic_button,
                send,
                stop,
                new_chat,
                approval_box,
                approval_label,
                approval_entry,
                approval_approve: approve,
                todo_box,
                subagent_box,
                background_box,
                queue_box,
            },
        };
        model.sync_state();
        model.render();
        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            HermesChatDropdownMsg::Send => self.submit_composer(),
            HermesChatDropdownMsg::SubmitOrAcceptSuggestion => {
                if self.accept_slash_suggestion() {
                    return;
                }
                self.submit_composer();
            }
            HermesChatDropdownMsg::Stop => self.hermes_chat.stop_current(),
            HermesChatDropdownMsg::NewChat => {
                let had_active_session = self.active_session_id.is_some();
                self.stash_current_composer_draft();
                self.composer_drafts.remove(LOCAL_DRAFT_SCOPE);
                self.skip_queue_migration_once = true;
                self.reset_history_browse();
                self.hermes_chat.new_session(None);
                if !had_active_session {
                    set_composer_text(&self.ui.composer, "");
                    self.hermes_chat.clear_slash_suggestions();
                }
            }
            HermesChatDropdownMsg::SelectSession(session_id) => {
                self.ui.session_popover.popdown();
                if self.active_session_id.as_deref() != Some(session_id.as_str()) {
                    self.stash_current_composer_draft();
                    self.skip_queue_migration_once = true;
                    self.reset_history_browse();
                    self.hermes_chat.select_session(session_id);
                }
            }
            HermesChatDropdownMsg::ToggleMic => {
                self.recording = !self.recording;
                self.render_composer_controls(&self.ui);
            }
            HermesChatDropdownMsg::AttachClicked => {
                self.hermes_chat.append_system_notice(String::from(
                    "Attachments aren't supported yet in the desktop Hermes chat.",
                ));
            }
            HermesChatDropdownMsg::Approve => {
                if self
                    .approval
                    .as_ref()
                    .is_some_and(|approval| approval_requires_sensitive_input(approval.kind))
                {
                    let value = self.ui.approval_entry.text().to_string();
                    self.hermes_chat.submit_approval(true, Some(value));
                    self.ui.approval_entry.set_text("");
                } else {
                    self.hermes_chat.submit_approval(true, None);
                }
            }
            HermesChatDropdownMsg::Reject => {
                self.ui.approval_entry.set_text("");
                self.hermes_chat.submit_approval(false, None);
            }
            HermesChatDropdownMsg::Opened(true) => {
                self.hermes_chat.connect();
                self.hermes_chat.refresh_background_processes();
            }
            HermesChatDropdownMsg::Opened(false) => {}
            HermesChatDropdownMsg::ComposerChanged => {
                let text = composer_text(&self.ui.composer);
                self.stash_composer_draft(&text);
                self.refresh_composer_slash_suggestions(text);
            }
            HermesChatDropdownMsg::ClearSlashSuggestions => {
                self.slash_selection = 0;
                self.hermes_chat.clear_slash_suggestions()
            }
            HermesChatDropdownMsg::HistoryOlder => {
                if self.slash_suggestions.is_empty() {
                    self.browse_history_older();
                } else {
                    self.select_previous_slash_suggestion();
                }
            }
            HermesChatDropdownMsg::HistoryNewer => {
                if self.slash_suggestions.is_empty() {
                    self.browse_history_newer();
                } else {
                    self.select_next_slash_suggestion();
                }
            }
            HermesChatDropdownMsg::QueuePromptNow(id) => self.send_queued_prompt_now(&id),
            HermesChatDropdownMsg::RemoveQueuedPrompt(id) => self.remove_queued_prompt(&id),
            HermesChatDropdownMsg::StopBackgroundProcess(id) => {
                self.hermes_chat.stop_background_process(&id);
            }
            HermesChatDropdownMsg::DismissBackgroundProcess(id) => {
                self.hermes_chat.dismiss_background_process(&id);
            }
        }
    }

    fn update_cmd(
        &mut self,
        msg: Self::CommandOutput,
        _sender: ComponentSender<Self>,
        root: &Self::Root,
    ) {
        match msg {
            HermesChatDropdownCmd::RuntimeStateChanged => {
                self.sync_runtime_state();
                self.drain_next_queued_if_idle();
                self.render_runtime_state();
            }
            HermesChatDropdownCmd::SessionsChanged => {
                self.sync_session_state();
                self.render_session_state(root);
            }
            HermesChatDropdownCmd::ActiveSessionChanged => {
                self.sync_session_state();
                self.render_runtime_state();
                self.render_session_state(root);
            }
            HermesChatDropdownCmd::ComposerPrefillChanged => self.consume_composer_prefill(),
            HermesChatDropdownCmd::SlashSuggestionsChanged => {
                self.sync_slash_suggestions();
                self.render_slash_suggestions(&self.ui);
            }
            HermesChatDropdownCmd::ConfigChanged => self.render(),
            HermesChatDropdownCmd::BackgroundRefreshTick => {
                if !root.is_visible() || !self.has_running_background_process() {
                    return;
                }
                self.hermes_chat.refresh_background_processes();
            }
        }
    }
}

impl HermesChatDropdown {
    fn submit_composer(&mut self) {
        let buffer = self.ui.composer.buffer();
        let text = composer_text(&self.ui.composer);
        if text.trim().is_empty() {
            return;
        }
        let is_slash = text.trim().starts_with('/');
        let handled = if self
            .approval
            .as_ref()
            .is_some_and(|approval| approval.kind == ApprovalKind::Clarification)
        {
            self.hermes_chat.submit_approval(true, Some(text.clone()));
            true
        } else {
            self.handle_slash_command(&text)
        };
        if !handled {
            if is_slash {
                self.hermes_chat.send_slash_command(text);
            } else if matches!(self.status, HermesStatus::Busy) {
                self.enqueue_prompt(text);
            } else {
                self.hermes_chat.send_message(text);
            }
        }
        buffer.set_text("");
        self.clear_current_composer_draft();
        self.reset_history_browse();
    }

    fn reset_history_browse(&mut self) {
        self.history_cursor = None;
        self.history_draft.clear();
    }

    fn accept_slash_suggestion(&mut self) -> bool {
        let Some(suggestion) = self.slash_suggestions.get(self.slash_selection) else {
            return false;
        };
        let insert_text = suggestion.insert_text.clone();
        if slash_suggestion_expands_to_args(&insert_text) {
            let expanded = format!("{} ", insert_text.trim());
            set_composer_text(&self.ui.composer, &expanded);
            refresh_slash_suggestions_for_text(&self.hermes_chat, &self.config, expanded);
        } else {
            set_composer_text(&self.ui.composer, &insert_text);
            self.hermes_chat.clear_slash_suggestions();
        }
        self.slash_selection = 0;
        self.reset_history_browse();
        true
    }

    fn current_composer_scope(&self) -> String {
        composer_scope(self.active_session_id.as_deref())
    }

    fn stash_current_composer_draft(&mut self) {
        let text = composer_text(&self.ui.composer);
        self.stash_composer_draft(&text);
    }

    fn stash_composer_draft(&mut self, text: &str) {
        if self.history_cursor.is_some() {
            return;
        }
        let scope = self.current_composer_scope();
        if text.is_empty() {
            self.composer_drafts.remove(&scope);
        } else {
            self.composer_drafts.insert(scope, text.to_owned());
        }
    }

    fn clear_current_composer_draft(&mut self) {
        let scope = self.current_composer_scope();
        self.composer_drafts.remove(&scope);
    }

    fn current_queue(&self) -> &[QueuedPrompt] {
        self.queued_prompts
            .get(&self.current_composer_scope())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn enqueue_prompt(&mut self, text: String) {
        self.queue_sequence += 1;
        let scope = self.current_composer_scope();
        let prompt = QueuedPrompt {
            id: format!("queued-{}", self.queue_sequence),
            text,
        };
        self.queued_prompts.entry(scope).or_default().push(prompt);
    }

    fn remove_queued_prompt(&mut self, id: &str) {
        let scope = self.current_composer_scope();
        if let Some(queue) = self.queued_prompts.get_mut(&scope) {
            queue.retain(|prompt| prompt.id != id);
            if queue.is_empty() {
                self.queued_prompts.remove(&scope);
            }
        }
    }

    fn send_queued_prompt_now(&mut self, id: &str) {
        let scope = self.current_composer_scope();
        let Some(queue) = self.queued_prompts.get_mut(&scope) else {
            return;
        };
        let Some(index) = queue.iter().position(|prompt| prompt.id == id) else {
            return;
        };
        if matches!(self.status, HermesStatus::Busy) {
            if index > 0 {
                let prompt = queue.remove(index);
                queue.insert(0, prompt);
            }
            self.hermes_chat.stop_current();
            return;
        }
        let prompt = queue.remove(index);
        if queue.is_empty() {
            self.queued_prompts.remove(&scope);
        }
        self.queue_drain_in_flight = true;
        self.hermes_chat.send_message(prompt.text);
    }

    fn drain_next_queued_if_idle(&mut self) {
        if self.queue_drain_in_flight {
            return;
        }
        if !matches!(self.status, HermesStatus::Connected) {
            return;
        }
        let scope = self.current_composer_scope();
        let Some(queue) = self.queued_prompts.get_mut(&scope) else {
            return;
        };
        if queue.is_empty() {
            return;
        };
        let prompt = queue.remove(0);
        if queue.is_empty() {
            self.queued_prompts.remove(&scope);
        }
        self.queue_drain_in_flight = true;
        self.hermes_chat.send_message(prompt.text);
    }

    fn has_running_background_process(&self) -> bool {
        self.background_processes
            .iter()
            .any(|process| process.status == BackgroundProcessStatus::Running)
    }

    fn migrate_queued_prompts(&mut self, from_scope: String, to_scope: String) {
        if from_scope == to_scope {
            return;
        }
        let Some(mut pending) = self.queued_prompts.remove(&from_scope) else {
            return;
        };
        if pending.is_empty() {
            return;
        }
        self.queued_prompts
            .entry(to_scope)
            .or_default()
            .append(&mut pending);
    }

    fn restore_composer_draft(&mut self) {
        let scope = self.current_composer_scope();
        let draft = self.composer_drafts.remove(&scope).unwrap_or_default();
        if composer_text(&self.ui.composer) != draft {
            set_composer_text(&self.ui.composer, &draft);
        }
        self.refresh_composer_slash_suggestions(draft);
    }

    fn refresh_composer_slash_suggestions(&self, text: String) {
        refresh_slash_suggestions_for_text(&self.hermes_chat, &self.config, text);
    }

    fn select_previous_slash_suggestion(&mut self) {
        if self.slash_suggestions.is_empty() || self.slash_selection == 0 {
            return;
        }
        self.slash_selection -= 1;
    }

    fn select_next_slash_suggestion(&mut self) {
        if self.slash_selection + 1 < self.slash_suggestions.len().min(6) {
            self.slash_selection += 1;
        }
    }

    fn browse_history_older(&mut self) {
        let history = user_message_history(&self.messages);
        if history.is_empty() {
            return;
        }
        let cursor = match self.history_cursor {
            Some(cursor) if cursor + 1 < history.len() => cursor + 1,
            Some(_) => return,
            None => {
                self.history_draft = composer_text(&self.ui.composer);
                0
            }
        };
        self.history_cursor = Some(cursor);
        set_composer_text(&self.ui.composer, &history[cursor]);
    }

    fn browse_history_newer(&mut self) {
        let Some(cursor) = self.history_cursor else {
            return;
        };
        let history = user_message_history(&self.messages);
        if cursor > 0 {
            let next_cursor = cursor - 1;
            self.history_cursor = Some(next_cursor);
            if let Some(text) = history.get(next_cursor) {
                set_composer_text(&self.ui.composer, text);
            }
            return;
        }
        let draft = std::mem::take(&mut self.history_draft);
        self.history_cursor = None;
        set_composer_text(&self.ui.composer, &draft);
    }

    fn sync_state(&mut self) {
        self.sync_runtime_state();
        self.sync_session_state();
        self.sync_slash_suggestions();
        self.consume_composer_prefill();
        self.drain_next_queued_if_idle();
    }

    fn sync_runtime_state(&mut self) {
        self.status = self.hermes_chat.status.get();
        if matches!(self.status, HermesStatus::Busy) {
            self.queue_drain_in_flight = false;
        }
        self.messages = self.hermes_chat.messages.get();
        self.approval = self.hermes_chat.approval.get();
        self.todos = self.hermes_chat.todos.get();
        self.subagents = self.hermes_chat.subagents.get();
        self.background_processes = self.hermes_chat.background_processes.get();
        self.last_error = self.hermes_chat.last_error.get();
    }

    fn sync_session_state(&mut self) {
        self.sessions = self.hermes_chat.sessions.get();
        let active_session_id = self.hermes_chat.active_session_id.get();
        if self.active_session_id != active_session_id {
            let previous_scope = self.current_composer_scope();
            let should_migrate_queue = !self.skip_queue_migration_once;
            self.skip_queue_migration_once = false;
            self.stash_current_composer_draft();
            self.reset_history_browse();
            self.active_session_id = active_session_id;
            if should_migrate_queue {
                self.migrate_queued_prompts(previous_scope, self.current_composer_scope());
            }
            self.restore_composer_draft();
            self.hermes_chat.refresh_background_processes();
        } else {
            self.skip_queue_migration_once = false;
            self.active_session_id = active_session_id;
        }
    }

    fn sync_slash_suggestions(&mut self) {
        let slash_suggestions = self.hermes_chat.slash_suggestions.get();
        if self.slash_suggestions != slash_suggestions
            || self.slash_selection >= slash_suggestions.len()
        {
            self.slash_selection = 0;
        }
        self.slash_suggestions = slash_suggestions;
    }

    fn consume_composer_prefill(&mut self) {
        let Some(prefill) = self.hermes_chat.composer_prefill.get() else {
            return;
        };
        self.hermes_chat.composer_prefill.set(None);
        self.reset_history_browse();
        self.stash_composer_draft(&prefill);
        let buffer = self.ui.composer.buffer();
        buffer.set_text(&prefill);
        let end = buffer.end_iter();
        buffer.place_cursor(&end);
        self.ui.composer.grab_focus();
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
        self.render_header_and_controls(ui);
        self.render_sessions(ui);
        self.render_session_activity(ui);
        self.render_messages(ui, modules.show_tool_progress.get());
        self.render_todos(ui);
        self.render_subagents(ui);
        self.render_background_processes(ui);
        self.render_queue(ui);
        self.render_approval(ui);
        self.render_slash_suggestions(ui);
    }

    fn render_runtime_state(&self) {
        let ui = &self.ui;
        let modules = &self.config.config().modules.hermes_chat;
        self.render_header_and_controls(ui);
        self.render_messages(ui, modules.show_tool_progress.get());
        self.render_todos(ui);
        self.render_subagents(ui);
        self.render_background_processes(ui);
        self.render_queue(ui);
        self.render_approval(ui);
    }

    fn render_session_state(&self, root: &gtk::Popover) {
        let ui = &self.ui;
        self.render_header_and_controls(ui);
        self.render_session_activity(ui);
        if !root.is_visible() || !matches!(self.status, HermesStatus::Busy) {
            self.render_sessions(ui);
        }
    }

    fn render_header_and_controls(&self, ui: &HermesChatUi) {
        let active_tool = active_tool_event(&self.messages);
        let status_text = status_text(
            &self.status,
            self.last_error.as_deref(),
            active_tool,
            self.approval.as_ref(),
        );
        let title_text = self.active_session_title();
        ui.title.set_label(&title_text);
        ui.title.set_tooltip_text(Some(&title_text));
        ui.status.set_label(&status_text);
        ui.status.set_tooltip_text(Some(&status_text));
        let status_class = self.status.css_class();
        ui.status.set_css_classes(&["hc-status", status_class]);
        ui.status_group
            .set_css_classes(&["hc-status-group", status_class]);
        ui.new_chat.set_sensitive(!matches!(
            self.status,
            HermesStatus::Disabled | HermesStatus::MissingApiKey | HermesStatus::Connecting
        ));
        self.render_composer_controls(ui);
    }

    fn render_composer_controls(&self, ui: &HermesChatUi) {
        let busy = matches!(self.status, HermesStatus::Busy);
        ui.send.set_sensitive(matches!(
            self.status,
            HermesStatus::Connected
                | HermesStatus::Busy
                | HermesStatus::Offline(_)
                | HermesStatus::Error(_)
        ));
        ui.send.set_tooltip_text(Some(if busy {
            "Queue message"
        } else {
            "Send message"
        }));
        ui.stop.set_visible(busy);
        ui.stop.set_sensitive(busy);
        if self.recording {
            ui.mic_button
                .set_css_classes(&["hc-icon-btn", "hc-mic", "recording"]);
            ui.mic_button.set_tooltip_text(Some("Stop recording"));
        } else {
            ui.mic_button.set_css_classes(&["hc-icon-btn", "hc-mic"]);
            ui.mic_button.set_tooltip_text(Some("Start voice input"));
        }
    }

    fn render_sessions(&self, ui: &HermesChatUi) {
        let active = self
            .active_session_id
            .as_deref()
            .and_then(|id| self.sessions.iter().find(|session| session.id == id));
        match active {
            Some(session) => {
                ui.session_current_name
                    .set_label(session.title.trim().if_empty("Hermes Chat"));
                let meta = session_picker_meta(session);
                ui.session_current_meta.set_label(&meta);
                ui.session_current_meta.set_visible(!meta.is_empty());
                ui.session_badge.set_visible(true);
            }
            None => {
                ui.session_current_name.set_label(
                    self.sessions.first().map_or("New session", |session| {
                        session.title.trim().if_empty("Hermes Chat")
                    }),
                );
                ui.session_current_meta.set_visible(false);
                ui.session_badge.set_visible(false);
            }
        }

        while let Some(child) = ui.session_list.first_child() {
            ui.session_list.remove(&child);
        }
        if self.sessions.is_empty() {
            let empty = gtk::Label::new(Some("No recent sessions"));
            empty.add_css_class("hc-session-pop-empty");
            empty.set_xalign(0.0);
            ui.session_list.append(&empty);
            return;
        }
        for session in &self.sessions {
            ui.session_list.append(&session_picker_row(
                session,
                self.active_session_id.as_deref(),
                &self.input_sender,
            ));
        }
    }

    fn render_session_activity(&self, ui: &HermesChatUi) {
        while let Some(child) = ui.session_activity_box.first_child() {
            ui.session_activity_box.remove(&child);
        }

        let activity = active_session_activity(&self.sessions);
        if activity.is_empty() {
            ui.session_activity_box.set_visible(false);
            return;
        }

        ui.session_activity_box.set_visible(true);
        ui.session_activity_box
            .append(&session_activity_header(&activity));
        for session in activity.iter().take(5) {
            ui.session_activity_box.append(&session_activity_row(
                session,
                self.active_session_id.as_deref(),
                &self.input_sender,
            ));
        }
        if activity.len() > 5 {
            let more = gtk::Label::new(Some(&format!(
                "...and {} more active sessions",
                activity.len() - 5
            )));
            more.add_css_class("hc-session-activity-more");
            more.set_xalign(0.0);
            ui.session_activity_box.append(&more);
        }
    }

    fn active_session_title(&self) -> String {
        self.active_session_id
            .as_deref()
            .and_then(|active| {
                self.sessions
                    .iter()
                    .find(|session| session.id == active)
                    .map(|session| session.title.trim())
            })
            .filter(|title| !title.is_empty())
            .unwrap_or("Hermes Chat")
            .to_owned()
    }

    fn render_messages(&self, ui: &HermesChatUi, show_tool_progress: bool) {
        while let Some(child) = ui.transcript.first_child() {
            ui.transcript.remove(&child);
        }
        if !self
            .messages
            .iter()
            .any(|message| message_has_visible_payload(message, show_tool_progress))
        {
            let empty = gtk::Label::new(Some("Start a new chat with Hermes Agent."));
            empty.add_css_class("hc-empty");
            empty.set_xalign(0.0);
            ui.transcript.append(&empty);
            return;
        }
        for message in self
            .messages
            .iter()
            .filter(|message| message_has_visible_payload(message, show_tool_progress))
        {
            ui.transcript
                .append(&message_row(message, show_tool_progress));
        }
    }

    fn render_approval(&self, ui: &HermesChatUi) {
        if let Some(approval) = &self.approval {
            ui.approval_box.set_visible(true);
            ui.approval_label
                .set_markup(&markdown_to_pango(&approval.prompt));
            let sensitive = approval_requires_sensitive_input(approval.kind);
            ui.approval_entry.set_visible(sensitive);
            ui.approval_entry
                .set_placeholder_text(Some(match approval.kind {
                    ApprovalKind::Sudo => "Sudo password",
                    ApprovalKind::Secret => "Secret value",
                    ApprovalKind::Approval | ApprovalKind::Clarification => "",
                }));
            ui.approval_approve
                .set_label(if sensitive { "Submit" } else { "Approve" });
            if sensitive {
                ui.approval_entry.grab_focus();
            }
        } else {
            ui.approval_box.set_visible(false);
            ui.approval_entry.set_text("");
            ui.approval_entry.set_visible(false);
            ui.approval_approve.set_label("Approve");
        }
    }

    fn render_todos(&self, ui: &HermesChatUi) {
        while let Some(child) = ui.todo_box.first_child() {
            ui.todo_box.remove(&child);
        }
        if self.todos.is_empty() {
            ui.todo_box.set_visible(false);
            return;
        }
        ui.todo_box.set_visible(true);
        ui.todo_box.append(&todo_header(&self.todos));
        for todo in &self.todos {
            ui.todo_box.append(&todo_row(todo));
        }
    }

    fn render_subagents(&self, ui: &HermesChatUi) {
        while let Some(child) = ui.subagent_box.first_child() {
            ui.subagent_box.remove(&child);
        }
        if self.subagents.is_empty() {
            ui.subagent_box.set_visible(false);
            return;
        }
        ui.subagent_box.set_visible(true);
        ui.subagent_box.append(&subagent_header(&self.subagents));
        for subagent in &self.subagents {
            ui.subagent_box.append(&subagent_row(subagent));
        }
    }

    fn render_background_processes(&self, ui: &HermesChatUi) {
        while let Some(child) = ui.background_box.first_child() {
            ui.background_box.remove(&child);
        }
        if self.background_processes.is_empty() {
            ui.background_box.set_visible(false);
            return;
        }
        ui.background_box.set_visible(true);
        ui.background_box
            .append(&background_header(&self.background_processes));
        for process in &self.background_processes {
            ui.background_box
                .append(&background_row(process, &self.input_sender));
        }
    }

    fn render_queue(&self, ui: &HermesChatUi) {
        while let Some(child) = ui.queue_box.first_child() {
            ui.queue_box.remove(&child);
        }
        let queue = self.current_queue();
        if queue.is_empty() {
            ui.queue_box.set_visible(false);
            return;
        }
        ui.queue_box.set_visible(true);
        ui.queue_box.append(&queue_header(queue.len()));
        for prompt in queue.iter().take(5) {
            ui.queue_box.append(&queue_row(
                prompt,
                matches!(self.status, HermesStatus::Busy),
                &self.input_sender,
            ));
        }
        if queue.len() > 5 {
            let more = gtk::Label::new(Some(&format!(
                "...and {} more queued prompts",
                queue.len() - 5
            )));
            more.add_css_class("hc-queue-more");
            more.set_xalign(0.0);
            ui.queue_box.append(&more);
        }
    }

    fn render_slash_suggestions(&self, ui: &HermesChatUi) {
        while let Some(child) = ui.slash_box.first_child() {
            ui.slash_box.remove(&child);
        }
        if self.slash_suggestions.is_empty() {
            ui.slash_box.set_visible(false);
            return;
        }
        ui.slash_box.set_visible(true);
        for (index, suggestion) in self.slash_suggestions.iter().take(6).enumerate() {
            ui.slash_box.append(&slash_suggestion_button(
                suggestion,
                &ui.composer,
                &self.hermes_chat,
                &self.config,
                index == self.slash_selection,
            ));
        }
    }

    fn handle_slash_command(&mut self, input: &str) -> bool {
        match parse_local_slash_command(input) {
            Some(LocalSlashCommand::New) => {
                self.skip_queue_migration_once = true;
                self.hermes_chat.new_session(None);
                true
            }
            Some(LocalSlashCommand::Help) => {
                self.hermes_chat
                    .show_slash_commands(slash_help_text().to_owned());
                true
            }
            Some(LocalSlashCommand::Sessions(query)) => {
                if query.is_empty() {
                    self.hermes_chat.append_system_notice(sessions_summary_text(
                        &self.sessions,
                        self.active_session_id.as_deref(),
                    ));
                    return true;
                }
                match resolve_session_query(&self.sessions, &query) {
                    Some(session) => {
                        self.skip_queue_migration_once = true;
                        self.hermes_chat.select_session(session.id.clone());
                        self.hermes_chat.append_system_notice(format!(
                            "Switching to session: {}",
                            session.title
                        ));
                    }
                    None if is_session_id_candidate(&query) => {
                        self.skip_queue_migration_once = true;
                        self.hermes_chat.select_session(query.clone());
                        self.hermes_chat
                            .append_system_notice(format!("Opening session: {query}"));
                    }
                    None => self.hermes_chat.append_system_notice(format!(
                        "No Hermes session matched `{}`. Use `/sessions` to list recent sessions.",
                        query
                    )),
                }
                true
            }
            Some(LocalSlashCommand::Title(title)) => {
                self.hermes_chat.set_session_title(title);
                true
            }
            Some(LocalSlashCommand::Yolo) => {
                self.hermes_chat.toggle_session_yolo();
                true
            }
            Some(LocalSlashCommand::Branch) => {
                self.skip_queue_migration_once = true;
                self.hermes_chat.branch_current_session();
                true
            }
            Some(LocalSlashCommand::Browser(args)) => {
                self.hermes_chat.manage_browser(args);
                true
            }
            Some(LocalSlashCommand::Handoff(platform)) => {
                self.hermes_chat.handoff_session(platform);
                true
            }
            Some(LocalSlashCommand::Profile(args)) => {
                self.hermes_chat.show_profiles(args);
                true
            }
            Some(LocalSlashCommand::Skin(args)) => {
                self.hermes_chat
                    .append_system_notice(handle_skin_command(&self.config, &args));
                true
            }
            Some(LocalSlashCommand::Unavailable(command, reason)) => {
                self.hermes_chat
                    .append_system_notice(unavailable_slash_message(&command, reason));
                true
            }
            None => false,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum LocalSlashCommand {
    New,
    Help,
    Sessions(String),
    Title(String),
    Yolo,
    Branch,
    Browser(String),
    Handoff(String),
    Profile(String),
    Skin(String),
    Unavailable(String, UnavailableSlashReason),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum UnavailableSlashReason {
    Advanced,
    Messaging,
    ModelPicker,
    Settings,
    Terminal,
}

fn parse_local_slash_command(input: &str) -> Option<LocalSlashCommand> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    let command_end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let command = trimmed[..command_end].to_ascii_lowercase();
    let args = trimmed[command_end..].trim().to_owned();

    match command.as_str() {
        "/new" | "/reset" => Some(LocalSlashCommand::New),
        "/help" | "/commands" => Some(LocalSlashCommand::Help),
        "/resume" | "/sessions" | "/switch" => Some(LocalSlashCommand::Sessions(args)),
        "/model" if args.is_empty() => Some(LocalSlashCommand::Unavailable(
            command,
            UnavailableSlashReason::ModelPicker,
        )),
        "/title" if !args.is_empty() => Some(LocalSlashCommand::Title(args)),
        "/yolo" => Some(LocalSlashCommand::Yolo),
        "/branch" | "/fork" => Some(LocalSlashCommand::Branch),
        "/browser" => Some(LocalSlashCommand::Browser(args)),
        "/handoff" => Some(LocalSlashCommand::Handoff(args)),
        "/profile" => Some(LocalSlashCommand::Profile(args)),
        "/skin" => Some(LocalSlashCommand::Skin(args)),
        "/approve" | "/deny" => Some(LocalSlashCommand::Unavailable(
            command,
            UnavailableSlashReason::Messaging,
        )),
        "/skills" => Some(LocalSlashCommand::Unavailable(
            command,
            UnavailableSlashReason::Settings,
        )),
        "/curator" | "/fast" | "/insights" | "/kanban" | "/reasoning" | "/reload-mcp"
        | "/reload-skills" | "/reload_mcp" | "/reload_skills" | "/voice" => Some(
            LocalSlashCommand::Unavailable(command, UnavailableSlashReason::Advanced),
        ),
        "/busy" | "/clear" | "/compact" | "/config" | "/copy" | "/cron" | "/details" | "/exit"
        | "/footer" | "/gateway" | "/gquota" | "/history" | "/image" | "/indicator" | "/logs"
        | "/mouse" | "/paste" | "/platforms" | "/plugins" | "/quit" | "/redraw" | "/reload"
        | "/restart" | "/sb" | "/set-home" | "/sethome" | "/snap" | "/snapshot" | "/statusbar"
        | "/toolsets" | "/update" | "/verbose" => Some(LocalSlashCommand::Unavailable(
            command,
            UnavailableSlashReason::Terminal,
        )),
        _ => None,
    }
}

fn unavailable_slash_message(command: &str, reason: UnavailableSlashReason) -> String {
    match reason {
        UnavailableSlashReason::Advanced => format!(
            "{command} is not shown in the desktop slash palette. Use the relevant desktop control or terminal interface instead."
        ),
        UnavailableSlashReason::Messaging => {
            format!("{command} is only used from messaging platforms.")
        }
        UnavailableSlashReason::ModelPicker => {
            format!("{command} uses the desktop model picker instead of a slash command.")
        }
        UnavailableSlashReason::Settings => {
            format!("{command} is managed from the desktop sidebar.")
        }
        UnavailableSlashReason::Terminal => {
            format!("{command} is only available in the terminal interface.")
        }
    }
}

fn handle_skin_command(config: &ConfigService, raw_arg: &str) -> String {
    handle_skin_command_for_styling(&config.config().styling, raw_arg)
}

fn refresh_slash_suggestions_for_text(
    hermes_chat: &Arc<HermesChatService>,
    config: &Arc<ConfigService>,
    text: String,
) {
    if let Some(suggestions) = skin_slash_suggestions_for_styling(&config.config().styling, &text) {
        hermes_chat.clear_slash_suggestions();
        hermes_chat.slash_suggestions.set(suggestions);
    } else {
        hermes_chat.refresh_slash_suggestions(text);
    }
}

fn slash_suggestion_expands_to_args(insert_text: &str) -> bool {
    let trimmed = insert_text.trim();
    if trimmed.is_empty() || trimmed.contains(char::is_whitespace) {
        return false;
    }
    matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "/browser"
            | "/handoff"
            | "/personality"
            | "/resume"
            | "/sessions"
            | "/skin"
            | "/switch"
            | "/tools"
    )
}

fn skin_slash_suggestions_for_styling(
    styling: &StylingConfig,
    input: &str,
) -> Option<Vec<SlashCommandSuggestion>> {
    let trimmed = input.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    let rest = lower.strip_prefix("/skin")?;
    if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
        return None;
    }
    if rest.is_empty() {
        return None;
    }

    let raw_arg = trimmed["/skin".len()..].trim_start();
    let prefix = raw_arg.to_ascii_lowercase();
    let active_theme = styling.palette_base_theme.get();
    let themes = styling.available.get();
    let commands = [
        ("/skin list", "Show available Lumen themes"),
        ("/skin next", "Cycle to the next Lumen theme"),
    ];

    let mut suggestions = commands
        .into_iter()
        .filter(|(command, _)| command["/skin ".len()..].starts_with(&prefix))
        .map(|(command, description)| SlashCommandSuggestion {
            insert_text: command.to_owned(),
            display: command.to_owned(),
            description: description.to_owned(),
            group: String::from("Themes"),
        })
        .collect::<Vec<_>>();

    suggestions.extend(
        themes
            .into_iter()
            .filter(|theme| theme.name.to_ascii_lowercase().starts_with(&prefix))
            .map(|theme| {
                let current = theme.name == active_theme;
                SlashCommandSuggestion {
                    insert_text: format!("/skin {}", theme.name),
                    display: format!("/skin {}", theme.name),
                    description: if current {
                        String::from("Current Lumen theme")
                    } else if theme.builtin {
                        String::from("Built-in Lumen theme")
                    } else {
                        String::from("Custom Lumen theme")
                    },
                    group: String::from("Themes"),
                }
            }),
    );

    suggestions.truncate(12);
    Some(suggestions)
}

fn handle_skin_command_for_styling(styling: &StylingConfig, raw_arg: &str) -> String {
    let themes = styling.available.get();
    if themes.is_empty() {
        return String::from("No desktop themes are available.");
    }

    let arg = raw_arg.trim();
    let active_theme = styling.palette_base_theme.get();
    let active_index = themes
        .iter()
        .position(|theme| theme.name == active_theme)
        .unwrap_or(0);

    if arg.is_empty() || arg.eq_ignore_ascii_case("next") {
        let target = &themes[(active_index + 1) % themes.len()];
        apply_skin_theme(styling, &target.name, &target.palette);
        return format!("Desktop theme switched to {}.", target.name);
    }

    if matches!(arg.to_ascii_lowercase().as_str(), "list" | "ls" | "status") {
        let mut lines = vec![String::from("Desktop themes:")];
        lines.extend(themes.iter().map(|theme| {
            format!(
                "{} {}",
                if theme.name == active_theme { "*" } else { " " },
                theme.name
            )
        }));
        lines.push(String::new());
        lines.push(String::from("Use /skin <name>, or /skin to cycle."));
        return lines.join("\n");
    }

    let normalized = skin_alias(arg);
    let target = themes.iter().find(|theme| {
        theme.name.eq_ignore_ascii_case(&normalized) || theme.name.eq_ignore_ascii_case(arg)
    });
    let Some(target) = target else {
        return format!(
            "Unknown desktop theme: {arg}\nAvailable: {}",
            themes
                .iter()
                .map(|theme| theme.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    };

    apply_skin_theme(styling, &target.name, &target.palette);
    format!("Desktop theme switched to {}.", target.name)
}

fn skin_alias(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "default" => String::from("lumen"),
        value => value.to_owned(),
    }
}

fn apply_skin_theme(styling: &StylingConfig, name: &str, palette: &Palette) {
    styling.theme_provider.set(ThemeProvider::Lumen);
    styling.palette_base_theme.set(name.to_owned());
    apply_skin_palette(&styling.palette, palette);
}

fn apply_skin_palette(target: &PaletteConfig, source: &Palette) {
    set_hex_if_valid(&target.bg, &source.bg);
    set_hex_if_valid(&target.surface, &source.surface);
    set_hex_if_valid(&target.elevated, &source.elevated);
    set_hex_if_valid(&target.fg, &source.fg);
    set_hex_if_valid(&target.fg_muted, &source.fg_muted);
    set_hex_if_valid(&target.primary, &source.primary);
    set_hex_if_valid(&target.red, &source.red);
    set_hex_if_valid(&target.yellow, &source.yellow);
    set_hex_if_valid(&target.green, &source.green);
    set_hex_if_valid(&target.blue, &source.blue);
}

fn set_hex_if_valid(property: &lumen_config::ConfigProperty<HexColor>, hex: &str) {
    if let Ok(value) = HexColor::new(hex) {
        property.set(value);
    }
}

#[cfg(test)]
mod tests {
    use lumen_config::{
        Config, infrastructure::themes::palettes::builtins, schemas::styling::ThemeProvider,
    };
    use lumen_hermes::{HermesMessage, HermesRole, HermesSessionSummary, MessageStatus, ToolEvent};

    use super::{
        LocalSlashCommand, UnavailableSlashReason, handle_skin_command_for_styling,
        is_session_id_candidate, message_has_visible_payload, parse_local_slash_command,
        resolve_session_query, session_activity_meta, session_preview_text, sessions_summary_text,
        skin_slash_suggestions_for_styling, slash_suggestion_expands_to_args,
        unavailable_slash_message,
    };

    #[test]
    fn bare_model_command_is_picker_owned() {
        assert_eq!(
            parse_local_slash_command("/model"),
            Some(LocalSlashCommand::Unavailable(
                String::from("/model"),
                UnavailableSlashReason::ModelPicker
            ))
        );
        assert_eq!(
            unavailable_slash_message("/model", UnavailableSlashReason::ModelPicker),
            "/model uses the desktop model picker instead of a slash command."
        );
    }

    #[test]
    fn model_command_with_argument_can_fall_through_to_backend() {
        assert_eq!(parse_local_slash_command("/model nous-hermes"), None);
    }

    #[test]
    fn help_and_commands_share_desktop_help_action() {
        assert_eq!(
            parse_local_slash_command("/help"),
            Some(LocalSlashCommand::Help)
        );
        assert_eq!(
            parse_local_slash_command("/commands"),
            Some(LocalSlashCommand::Help)
        );
    }

    #[test]
    fn browser_command_is_handled_locally() {
        assert_eq!(
            parse_local_slash_command("/browser connect http://127.0.0.1:9222"),
            Some(LocalSlashCommand::Browser(String::from(
                "connect http://127.0.0.1:9222"
            )))
        );
    }

    #[test]
    fn handoff_command_is_handled_locally() {
        assert_eq!(
            parse_local_slash_command("/handoff telegram"),
            Some(LocalSlashCommand::Handoff(String::from("telegram")))
        );
    }

    #[test]
    fn profile_command_is_handled_locally() {
        assert_eq!(
            parse_local_slash_command("/profile coder"),
            Some(LocalSlashCommand::Profile(String::from("coder")))
        );
    }

    #[test]
    fn skin_command_is_handled_locally() {
        assert_eq!(
            parse_local_slash_command("/skin list"),
            Some(LocalSlashCommand::Skin(String::from("list")))
        );
    }

    #[test]
    fn reload_commands_are_advanced_no_surface_commands() {
        for command in [
            "/reload-mcp",
            "/reload_mcp",
            "/reload-skills",
            "/reload_skills",
        ] {
            assert_eq!(
                parse_local_slash_command(command),
                Some(LocalSlashCommand::Unavailable(
                    String::from(command),
                    UnavailableSlashReason::Advanced
                ))
            );
        }
    }

    #[test]
    fn stop_command_falls_through_to_backend_like_desktop() {
        assert_eq!(parse_local_slash_command("/stop"), None);
    }

    #[test]
    fn status_and_usage_commands_fall_through_to_backend_like_desktop() {
        assert_eq!(parse_local_slash_command("/status"), None);
        assert_eq!(parse_local_slash_command("/usage"), None);
    }

    #[test]
    fn resume_session_id_candidates_match_desktop_patterns() {
        assert!(is_session_id_candidate("20250101_123456_a1B2c3"));
        assert!(is_session_id_candidate("0123456789abcdef0123456789ABCDEF"));
        assert!(is_session_id_candidate(
            " 0123456789abcdef0123456789ABCDEF "
        ));

        assert!(!is_session_id_candidate("release prep"));
        assert!(!is_session_id_candidate("20250101_123456_a1B2c"));
        assert!(!is_session_id_candidate("0123456789abcdef0123456789ABCDEG"));
    }

    #[test]
    fn resume_query_matches_session_preview_like_desktop() {
        let sessions = vec![HermesSessionSummary {
            id: String::from("session-1"),
            title: String::from("Release prep"),
            updated_at: None,
            is_active: false,
            needs_input: false,
            message_count: Some(2),
            preview: Some(String::from("Fix packaging release notes")),
            source: Some(String::from("desktop")),
        }];

        let session = resolve_session_query(&sessions, "packaging release").expect("preview match");

        assert_eq!(session.id, "session-1");
    }

    #[test]
    fn session_activity_and_summary_include_distinct_preview() {
        let session = HermesSessionSummary {
            id: String::from("session-1"),
            title: String::from("Release prep"),
            updated_at: None,
            is_active: true,
            needs_input: false,
            message_count: Some(2),
            preview: Some(String::from("Fix packaging release notes")),
            source: Some(String::from("desktop")),
        };

        assert_eq!(
            session_preview_text(&session).as_deref(),
            Some("Fix packaging release notes")
        );
        assert_eq!(
            session_activity_meta(&session, Some("session-1")),
            "current - active - 2 msgs - desktop"
        );

        let summary = sessions_summary_text(&[session], Some("session-1"));
        assert!(summary.contains("[current] [active] Release prep (session-1)"));
        assert!(summary.contains("    Fix packaging release notes"));
    }

    #[test]
    fn session_preview_text_skips_title_duplicate() {
        let session = HermesSessionSummary {
            id: String::from("session-1"),
            title: String::from("Release prep"),
            updated_at: None,
            is_active: false,
            needs_input: false,
            message_count: None,
            preview: Some(String::from("release prep")),
            source: None,
        };

        assert_eq!(session_preview_text(&session), None);
    }

    #[test]
    fn transcript_visibility_skips_blank_tool_placeholders() {
        let blank_tool = HermesMessage::new("tool-blank", HermesRole::Tool, "");
        assert!(!message_has_visible_payload(&blank_tool, true));

        let whitespace_tool = HermesMessage::new("tool-whitespace", HermesRole::Tool, "   \n");
        assert!(!message_has_visible_payload(&whitespace_tool, true));

        let tool_output = HermesMessage::new("tool-output", HermesRole::Tool, "tool output");
        assert!(message_has_visible_payload(&tool_output, false));

        let mut assistant_tool = HermesMessage::new("assistant-tool", HermesRole::Assistant, "");
        assistant_tool.tool_events.push(ToolEvent {
            id: String::from("search-1"),
            tool: String::from("web_search"),
            label: String::from("Searching docs"),
            status: String::from("running"),
            command: None,
            input: None,
            output: None,
            error: None,
            path: None,
            url: None,
            has_inline_diff: false,
            raw: None,
        });
        assert!(message_has_visible_payload(&assistant_tool, true));
        assert!(!message_has_visible_payload(&assistant_tool, false));

        let mut thought_only = HermesMessage::new("thought", HermesRole::Assistant, "");
        thought_only.tool_events.push(ToolEvent {
            id: String::from("thought-1"),
            tool: String::from("_thinking"),
            label: String::new(),
            status: String::from("running"),
            command: None,
            input: None,
            output: None,
            error: None,
            path: None,
            url: None,
            has_inline_diff: false,
            raw: None,
        });
        assert!(!message_has_visible_payload(&thought_only, true));

        let mut streaming = HermesMessage::new("streaming", HermesRole::Assistant, "");
        streaming.status = MessageStatus::Streaming;
        assert!(message_has_visible_payload(&streaming, false));
    }

    #[test]
    fn skin_command_lists_and_applies_lumen_themes() {
        let config = Config::default();
        let themes = builtins();
        config.styling.available.set(themes);
        config.styling.palette_base_theme.set(String::from("lumen"));
        config.styling.theme_provider.set(ThemeProvider::Wallust);

        let list = handle_skin_command_for_styling(&config.styling, "list");
        assert!(list.contains("Desktop themes:"));
        assert!(list.contains("* lumen"));
        assert!(list.contains("Use /skin <name>, or /skin to cycle."));

        let switched = handle_skin_command_for_styling(&config.styling, "nord");
        assert_eq!(switched, "Desktop theme switched to nord.");
        assert_eq!(config.styling.palette_base_theme.get(), "nord");
        assert_eq!(config.styling.theme_provider.get(), ThemeProvider::Lumen);

        let cycled = handle_skin_command_for_styling(&config.styling, "");
        assert!(cycled.starts_with("Desktop theme switched to "));
        assert_ne!(config.styling.palette_base_theme.get(), "nord");
    }

    #[test]
    fn skin_slash_suggestions_use_lumen_themes() {
        let config = Config::default();
        config.styling.available.set(builtins());
        config.styling.palette_base_theme.set(String::from("nord"));

        let suggestions =
            skin_slash_suggestions_for_styling(&config.styling, "/skin ").expect("skin args");
        assert_eq!(suggestions[0].insert_text, "/skin list");
        assert_eq!(suggestions[1].insert_text, "/skin next");
        assert!(
            suggestions
                .iter()
                .all(|suggestion| suggestion.group == "Themes")
        );

        let filtered =
            skin_slash_suggestions_for_styling(&config.styling, "/skin no").expect("filtered");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].insert_text, "/skin nord");
        assert_eq!(filtered[0].description, "Current Lumen theme");
        assert_eq!(filtered[0].group, "Themes");
    }

    #[test]
    fn bare_skin_slash_uses_normal_command_suggestions() {
        let config = Config::default();
        config.styling.available.set(builtins());

        assert!(skin_slash_suggestions_for_styling(&config.styling, "/skin").is_none());
        assert!(skin_slash_suggestions_for_styling(&config.styling, "/skinny").is_none());
    }

    #[test]
    fn bare_arg_taking_suggestions_expand_like_desktop() {
        for command in [
            "/browser",
            "/handoff",
            "/personality",
            "/resume",
            "/sessions",
            "/skin",
            "/switch",
            "/tools",
        ] {
            assert!(
                slash_suggestion_expands_to_args(command),
                "{command} should expand"
            );
        }

        for command in [
            "/skin nord",
            "/handoff telegram",
            "/title",
            "/profile",
            "/stop",
            "/toolsets",
        ] {
            assert!(
                !slash_suggestion_expands_to_args(command),
                "{command} should not expand"
            );
        }
    }
}

fn slash_help_text() -> &'static str {
    "Hermes chat commands:\n\
     /new or /reset - start a new chat\n\
     /sessions - list recent sessions, active work, and input waits\n\
     /resume <id, title, or preview> - switch to a recent session or stored session id\n\
     /switch <id, title, or preview> - alias for /resume\n\
     /branch or /fork - branch the latest message into a new chat\n\
     /browser [connect|disconnect|status] [url] - manage local browser CDP\n\
     /handoff <platform> - hand off this session to a messaging platform\n\
     /profile [list|name] - list profiles or set the profile for new dashboard chats\n\
     /skin [list|next|name] - list, cycle, or apply a Lumen theme\n\
     /title <name> - rename the current dashboard session\n\
     /yolo - toggle per-session YOLO approval bypass\n\
     /status - show current Hermes session status\n\
     /usage - show Hermes token usage for this session\n\
     /stop - stop running background processes through Hermes\n\
     /help or /commands - show Hermes backend command catalog when available\n\
     Unknown slash commands are sent to Hermes so backend and skill commands can run."
}

fn session_picker_meta(session: &HermesSessionSummary) -> String {
    let mut meta = Vec::new();
    if let Some(count) = session.message_count {
        meta.push(if count == 1 {
            String::from("1 msg")
        } else {
            format!("{count} msgs")
        });
    }
    if session.needs_input {
        meta.push(String::from("needs input"));
    } else if session.is_active {
        meta.push(String::from("active"));
    }
    if let Some(source) = session
        .source
        .as_deref()
        .filter(|source| !source.trim().is_empty())
    {
        meta.push(source.to_owned());
    }
    if let Some(updated_at) = session.updated_at.as_ref() {
        meta.push(updated_at.format("%m-%d %H:%M").to_string());
    }
    meta.join(" · ")
}

fn session_picker_row(
    session: &HermesSessionSummary,
    active_session_id: Option<&str>,
    sender: &relm4::Sender<HermesChatDropdownMsg>,
) -> gtk::Button {
    let is_current = active_session_id == Some(session.id.as_str());
    let button = gtk::Button::new();
    button.set_cursor_from_name(Some("pointer"));
    if is_current {
        button.set_css_classes(&["hc-session-pop-row", "current"]);
    } else {
        button.set_css_classes(&["hc-session-pop-row"]);
    }

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let icon = gtk::Image::from_icon_name("ld-message-circle-symbolic");
    icon.add_css_class("hc-session-pop-icon");
    row.append(&icon);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_hexpand(true);
    let name = gtk::Label::new(Some(session.title.trim().if_empty("Hermes Chat")));
    name.add_css_class("hc-session-pop-name");
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&name);
    let meta_text = session_picker_meta(session);
    if !meta_text.is_empty() {
        let meta = gtk::Label::new(Some(&meta_text));
        meta.add_css_class("hc-session-pop-meta");
        meta.set_xalign(0.0);
        meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&meta);
    }
    row.append(&text);

    if is_current {
        let check = gtk::Label::new(Some("✓"));
        check.add_css_class("hc-session-pop-check");
        check.set_valign(gtk::Align::Center);
        row.append(&check);
    }

    button.set_child(Some(&row));
    let session_id = session.id.clone();
    let sender = sender.clone();
    button.connect_clicked(move |_| {
        sender.emit(HermesChatDropdownMsg::SelectSession(session_id.clone()));
    });
    button
}

fn active_session_activity(sessions: &[HermesSessionSummary]) -> Vec<&HermesSessionSummary> {
    let mut activity = sessions
        .iter()
        .filter(|session| session.needs_input || session.is_active)
        .collect::<Vec<_>>();
    activity.sort_by_key(|session| {
        (
            !session.needs_input,
            !session.is_active,
            session.title.to_ascii_lowercase(),
        )
    });
    activity
}

fn session_activity_header(sessions: &[&HermesSessionSummary]) -> gtk::Label {
    let waiting = sessions
        .iter()
        .filter(|session| session.needs_input)
        .count();
    let active = sessions.iter().filter(|session| session.is_active).count();
    let header = gtk::Label::new(Some(&format!(
        "Sessions {active} active, {waiting} waiting"
    )));
    header.add_css_class("hc-session-activity-title");
    header.set_xalign(0.0);
    header
}

fn session_activity_row(
    session: &HermesSessionSummary,
    active_session_id: Option<&str>,
    sender: &relm4::Sender<HermesChatDropdownMsg>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.add_css_class("hc-session-activity-row");
    row.add_css_class(session_activity_status_class(session));

    let status = gtk::Label::new(Some(session_activity_status_glyph(session)));
    status.add_css_class("hc-session-activity-status");
    status.set_xalign(0.0);
    row.append(&status);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let title = gtk::Label::new(Some(session.title.trim().if_empty("Hermes Chat")));
    title.add_css_class("hc-session-activity-name");
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&title);

    if let Some(preview) = session_preview_text(session) {
        let preview_label = gtk::Label::new(Some(&preview));
        preview_label.add_css_class("hc-session-activity-preview");
        preview_label.set_xalign(0.0);
        preview_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&preview_label);
    }

    let meta = session_activity_meta(session, active_session_id);
    if !meta.is_empty() {
        let meta_label = gtk::Label::new(Some(&meta));
        meta_label.add_css_class("hc-session-activity-meta");
        meta_label.set_xalign(0.0);
        meta_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&meta_label);
    }
    row.append(&text);

    let open = gtk::Button::with_label(if active_session_id == Some(session.id.as_str()) {
        "Current"
    } else {
        "Open"
    });
    open.add_css_class("hc-session-activity-action");
    open.set_sensitive(active_session_id != Some(session.id.as_str()));
    open.set_tooltip_text(Some("Switch to this Hermes session"));
    open.set_cursor_from_name(Some("pointer"));
    let session_id = session.id.clone();
    let sender = sender.clone();
    open.connect_clicked(move |_| {
        sender.emit(HermesChatDropdownMsg::SelectSession(session_id.clone()));
    });
    row.append(&open);

    row
}

fn session_activity_status_class(session: &HermesSessionSummary) -> &'static str {
    if session.needs_input {
        "waiting"
    } else {
        "running"
    }
}

fn session_activity_status_glyph(session: &HermesSessionSummary) -> &'static str {
    if session.needs_input { "[!]" } else { "[>]" }
}

fn session_activity_meta(
    session: &HermesSessionSummary,
    active_session_id: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if active_session_id == Some(session.id.as_str()) {
        parts.push(String::from("current"));
    }
    if session.needs_input {
        parts.push(String::from("needs input"));
    } else if session.is_active {
        parts.push(String::from("active"));
    }
    if let Some(count) = session.message_count {
        parts.push(if count == 1 {
            String::from("1 msg")
        } else {
            format!("{count} msgs")
        });
    }
    if let Some(source) = session
        .source
        .as_deref()
        .filter(|source| !source.trim().is_empty())
    {
        parts.push(source.to_owned());
    }
    parts.join(" - ")
}

fn session_preview_text(session: &HermesSessionSummary) -> Option<String> {
    let title = session.title.trim();
    session
        .preview
        .as_deref()
        .map(str::trim)
        .filter(|preview| !preview.is_empty())
        .filter(|preview| !preview.eq_ignore_ascii_case(title))
        .map(str::to_owned)
}

fn sessions_summary_text(
    sessions: &[HermesSessionSummary],
    active_session_id: Option<&str>,
) -> String {
    if sessions.is_empty() {
        return String::from("No Hermes sessions are loaded yet.");
    }

    let active_count = sessions.iter().filter(|session| session.is_active).count();
    let waiting_count = sessions
        .iter()
        .filter(|session| session.needs_input)
        .count();
    let mut lines = vec![format!(
        "Recent Hermes sessions: {} loaded, {} active, {} waiting for input",
        sessions.len(),
        active_count,
        waiting_count
    )];
    for session in sessions.iter().take(12) {
        let mut markers = Vec::new();
        if active_session_id == Some(session.id.as_str()) {
            markers.push("[current]");
        }
        if session.needs_input {
            markers.push("[needs input]");
        } else if session.is_active {
            markers.push("[active]");
        }
        let marker = if markers.is_empty() {
            String::from("-")
        } else {
            markers.join(" ")
        };
        lines.push(format!(
            "{} {} ({})",
            marker,
            session.title.trim().if_empty("Hermes Chat"),
            session.id
        ));
        if let Some(preview) = session_preview_text(session) {
            lines.push(format!("    {preview}"));
        }
    }
    if sessions.len() > 12 {
        lines.push(format!("...and {} more.", sessions.len() - 12));
    }
    lines.join("\n")
}

fn resolve_session_query<'a>(
    sessions: &'a [HermesSessionSummary],
    query: &str,
) -> Option<&'a HermesSessionSummary> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let query_lower = query.to_ascii_lowercase();

    sessions
        .iter()
        .find(|session| session.id == query || session.title.eq_ignore_ascii_case(query))
        .or_else(|| {
            sessions.iter().find(|session| {
                session.id.starts_with(query)
                    || session.title.to_ascii_lowercase().contains(&query_lower)
                    || session
                        .preview
                        .as_deref()
                        .is_some_and(|preview| preview.to_ascii_lowercase().contains(&query_lower))
            })
        })
}

fn is_session_id_candidate(value: &str) -> bool {
    let trimmed = value.trim();
    (trimmed.len() == 32 && trimmed.bytes().all(|byte| byte.is_ascii_hexdigit()))
        || is_timestamp_session_id_candidate(trimmed)
}

fn is_timestamp_session_id_candidate(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 22
        && bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'_'
        && bytes[9..15].iter().all(u8::is_ascii_digit)
        && bytes[15] == b'_'
        && bytes[16..].iter().all(u8::is_ascii_hexdigit)
}

trait EmptyStrExt {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str;
}

impl EmptyStrExt for str {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.is_empty() { fallback } else { self }
    }
}

fn markdown_blocks_widget(markdown: &str, class_name: &str) -> gtk::Box {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
    body.add_css_class("hc-md");
    body.add_css_class(class_name);

    let blocks = markdown_to_blocks(markdown);
    if blocks.is_empty() {
        body.append(&markdown_label(&escape_pango_text(markdown), &["hc-md-p"]));
        return body;
    }

    for block in &blocks {
        body.append(&markdown_block_widget(block));
    }
    body
}

fn markdown_block_widget(block: &MarkdownBlock) -> gtk::Widget {
    match block {
        MarkdownBlock::Paragraph(markup) => markdown_label(markup, &["hc-md-p"]).upcast(),
        MarkdownBlock::Heading { level, markup } => {
            let markup = format!("<b>{markup}</b>");
            let level_class = match level {
                1 => "hc-md-h1",
                2 => "hc-md-h2",
                _ => "hc-md-h3",
            };
            markdown_label(&markup, &["hc-md-heading", level_class]).upcast()
        }
        MarkdownBlock::Code { language, text } => {
            markdown_code_block(language.as_deref(), text).upcast()
        }
        MarkdownBlock::BlockQuote(blocks) => {
            let quote = gtk::Box::new(gtk::Orientation::Vertical, 5);
            quote.add_css_class("hc-md-quote");
            for block in blocks {
                quote.append(&markdown_block_widget(block));
            }
            quote.upcast()
        }
        MarkdownBlock::List {
            ordered,
            start,
            items,
        } => markdown_list(*ordered, *start, items).upcast(),
        MarkdownBlock::Table { headers, rows } => markdown_table(headers, rows).upcast(),
        MarkdownBlock::Rule => {
            let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
            separator.add_css_class("hc-md-rule");
            separator.upcast()
        }
    }
}

fn markdown_label(markup: &str, classes: &[&str]) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_markup(markup);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.set_xalign(0.0);
    for class in classes {
        label.add_css_class(class);
    }
    label
}

fn markdown_text_label(text: &str, classes: &[&str]) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.set_xalign(0.0);
    for class in classes {
        label.add_css_class(class);
    }
    label
}

fn markdown_code_block(language: Option<&str>, text: &str) -> gtk::Box {
    let block = gtk::Box::new(gtk::Orientation::Vertical, 0);
    block.add_css_class("hc-md-code");

    if let Some(language) = language
        .map(str::trim)
        .filter(|language| !language.is_empty())
    {
        let language = gtk::Label::new(Some(language));
        language.set_xalign(0.0);
        language.add_css_class("hc-md-code-lang");
        block.append(&language);
    }

    let code = markdown_text_label(text.trim_end(), &["hc-md-code-body"]);
    block.append(&code);
    block
}

fn markdown_list(ordered: bool, start: u64, items: &[Vec<MarkdownBlock>]) -> gtk::Box {
    let list = gtk::Box::new(gtk::Orientation::Vertical, 3);
    list.add_css_class("hc-md-list");
    if ordered {
        list.add_css_class("ordered");
    }

    for (index, item) in items.iter().enumerate() {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.add_css_class("hc-md-list-item");

        let marker_text = if ordered {
            format!("{}.", start + index as u64)
        } else {
            String::from("•")
        };
        let marker = gtk::Label::new(Some(&marker_text));
        marker.set_xalign(1.0);
        marker.add_css_class("hc-md-list-marker");
        row.append(&marker);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
        content.add_css_class("hc-md-list-content");
        for block in item {
            content.append(&markdown_block_widget(block));
        }
        row.append(&content);
        list.append(&row);
    }

    list
}

fn markdown_table(headers: &[String], rows: &[Vec<String>]) -> gtk::Box {
    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("hc-md-table-frame");

    let table = gtk::Grid::new();
    table.add_css_class("hc-md-table");
    table.set_column_homogeneous(false);
    table.set_column_spacing(0);
    table.set_row_spacing(0);

    let mut row_index = 0;
    if !headers.is_empty() {
        for (column, cell) in headers.iter().enumerate() {
            table.attach(
                &markdown_table_cell(cell, true),
                column as i32,
                row_index,
                1,
                1,
            );
        }
        row_index += 1;
    }

    for row in rows {
        for (column, cell) in row.iter().enumerate() {
            table.attach(
                &markdown_table_cell(cell, false),
                column as i32,
                row_index,
                1,
                1,
            );
        }
        row_index += 1;
    }

    frame.append(&table);
    frame
}

fn markdown_table_cell(markup: &str, header: bool) -> gtk::Label {
    let classes = if header {
        ["hc-md-table-cell", "header"]
    } else {
        ["hc-md-table-cell", "body"]
    };
    markdown_label(markup.trim().if_empty(" "), &classes)
}

fn message_row(message: &HermesMessage, show_tool_progress: bool) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
    row.add_css_class("hc-message");
    row.add_css_class(match message.role {
        HermesRole::User => "user",
        HermesRole::Assistant => "assistant",
        HermesRole::System => "system",
        HermesRole::Tool => "tool",
        HermesRole::Error => "error",
    });
    row.append(&message_header(message));

    if !message.content.trim().is_empty() {
        row.append(&markdown_blocks_widget(
            message.content.trim(),
            "hc-message-body",
        ));
    }

    if !message.reasoning.trim().is_empty() {
        row.append(&reasoning_widget(message));
    }

    if show_tool_progress
        && message
            .tool_events
            .iter()
            .any(|event| !is_thought_tool_event(event))
    {
        row.append(&tool_events_widget(message));
    }
    row
}

fn message_has_visible_payload(message: &HermesMessage, show_tool_progress: bool) -> bool {
    !message.content.trim().is_empty()
        || !message.reasoning.trim().is_empty()
        || (message.role == HermesRole::Assistant && message.status == MessageStatus::Streaming)
        || (show_tool_progress
            && message
                .tool_events
                .iter()
                .any(|event| !is_thought_tool_event(event)))
}

fn todo_header(todos: &[TodoItem]) -> gtk::Label {
    let done = todos
        .iter()
        .filter(|todo| matches!(todo.status, TodoStatus::Completed | TodoStatus::Cancelled))
        .count();
    let header = gtk::Label::new(Some(&format!("Tasks {done}/{}", todos.len())));
    header.add_css_class("hc-todos-title");
    header.set_xalign(0.0);
    header
}

fn todo_row(todo: &TodoItem) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.add_css_class("hc-todo-row");
    row.add_css_class(todo_status_class(todo.status));

    let status = gtk::Label::new(Some(todo_status_glyph(todo.status)));
    status.add_css_class("hc-todo-status");
    status.set_xalign(0.0);
    row.append(&status);

    let content = gtk::Label::new(Some(todo.content.trim().if_empty("Untitled task")));
    content.add_css_class("hc-todo-content");
    content.set_wrap(true);
    content.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    content.set_xalign(0.0);
    content.set_hexpand(true);
    row.append(&content);

    row
}

fn todo_status_class(status: TodoStatus) -> &'static str {
    match status {
        TodoStatus::Pending => "pending",
        TodoStatus::InProgress => "running",
        TodoStatus::Completed => "done",
        TodoStatus::Cancelled => "cancelled",
    }
}

fn todo_status_glyph(status: TodoStatus) -> &'static str {
    match status {
        TodoStatus::Pending => "[ ]",
        TodoStatus::InProgress => "[>]",
        TodoStatus::Completed => "[x]",
        TodoStatus::Cancelled => "[-]",
    }
}

fn subagent_header(subagents: &[SubagentItem]) -> gtk::Label {
    let active = subagents
        .iter()
        .filter(|subagent| {
            matches!(
                subagent.status,
                SubagentStatus::Queued | SubagentStatus::Running
            )
        })
        .count();
    let header = gtk::Label::new(Some(&format!(
        "Subagents {active}/{} active",
        subagents.len()
    )));
    header.add_css_class("hc-subagents-title");
    header.set_xalign(0.0);
    header
}

fn subagent_row(subagent: &SubagentItem) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.add_css_class("hc-subagent-row");
    row.add_css_class(subagent_status_class(subagent.status));

    let status = gtk::Label::new(Some(subagent_status_glyph(subagent.status)));
    status.add_css_class("hc-subagent-status");
    status.set_xalign(0.0);
    row.append(&status);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let goal = gtk::Label::new(Some(subagent.goal.trim().if_empty("Subagent")));
    goal.add_css_class("hc-subagent-goal");
    goal.set_wrap(true);
    goal.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    goal.set_xalign(0.0);
    text.append(&goal);

    let meta = subagent_meta(subagent);
    if !meta.is_empty() {
        let meta_label = gtk::Label::new(Some(&meta));
        meta_label.add_css_class("hc-subagent-meta");
        meta_label.set_wrap(true);
        meta_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        meta_label.set_xalign(0.0);
        text.append(&meta_label);
    }
    row.append(&text);
    row
}

fn background_header(processes: &[BackgroundProcessItem]) -> gtk::Label {
    let running = processes
        .iter()
        .filter(|process| process.status == BackgroundProcessStatus::Running)
        .count();
    let header = gtk::Label::new(Some(&format!(
        "Background {running}/{} running",
        processes.len()
    )));
    header.add_css_class("hc-background-title");
    header.set_xalign(0.0);
    header
}

fn background_row(
    process: &BackgroundProcessItem,
    sender: &relm4::Sender<HermesChatDropdownMsg>,
) -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 4);
    container.add_css_class("hc-background-item");
    container.add_css_class(background_status_class(process.status));

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.add_css_class("hc-background-row");

    let status = gtk::Label::new(Some(background_status_glyph(process.status)));
    status.add_css_class("hc-background-status");
    status.set_xalign(0.0);
    row.append(&status);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let title = gtk::Label::new(Some(process.title.trim().if_empty("background process")));
    title.add_css_class("hc-background-title-text");
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&title);

    let meta = background_meta(process);
    if !meta.is_empty() {
        let meta_label = gtk::Label::new(Some(&meta));
        meta_label.add_css_class("hc-background-meta");
        meta_label.set_xalign(0.0);
        meta_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&meta_label);
    }
    row.append(&text);

    let action = gtk::Button::with_label(if process.status == BackgroundProcessStatus::Running {
        "Stop"
    } else {
        "Dismiss"
    });
    action.add_css_class("hc-background-action");
    action.set_cursor_from_name(Some("pointer"));
    action.set_tooltip_text(Some(
        if process.status == BackgroundProcessStatus::Running {
            "Stop this background process"
        } else {
            "Dismiss this background process"
        },
    ));
    let process_id = process.id.clone();
    let sender = sender.clone();
    let running = process.status == BackgroundProcessStatus::Running;
    action.connect_clicked(move |_| {
        if running {
            sender.emit(HermesChatDropdownMsg::StopBackgroundProcess(
                process_id.clone(),
            ));
        } else {
            sender.emit(HermesChatDropdownMsg::DismissBackgroundProcess(
                process_id.clone(),
            ));
        }
    });
    row.append(&action);

    container.append(&row);
    if let Some(output) = process
        .output
        .as_deref()
        .map(str::trim)
        .filter(|output| !output.is_empty())
    {
        container.append(&background_output_widget(output));
    }
    container
}

fn background_output_widget(output: &str) -> gtk::Expander {
    let expander = gtk::Expander::new(Some("Output"));
    expander.add_css_class("hc-background-output");

    let body = gtk::Label::new(Some(output));
    body.add_css_class("hc-background-output-body");
    body.set_wrap(true);
    body.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    body.set_selectable(true);
    body.set_xalign(0.0);
    expander.set_child(Some(&body));
    expander
}

fn background_meta(process: &BackgroundProcessItem) -> String {
    let mut parts = vec![background_status_label(process.status).to_owned()];
    if let Some(exit_code) = process.exit_code {
        parts.push(format!("exit {exit_code}"));
    }
    parts.join(" - ")
}

fn background_status_class(status: BackgroundProcessStatus) -> &'static str {
    match status {
        BackgroundProcessStatus::Running => "running",
        BackgroundProcessStatus::Completed => "done",
        BackgroundProcessStatus::Failed => "error",
    }
}

fn background_status_glyph(status: BackgroundProcessStatus) -> &'static str {
    match status {
        BackgroundProcessStatus::Running => "[>]",
        BackgroundProcessStatus::Completed => "[x]",
        BackgroundProcessStatus::Failed => "[!]",
    }
}

fn background_status_label(status: BackgroundProcessStatus) -> &'static str {
    match status {
        BackgroundProcessStatus::Running => "Running",
        BackgroundProcessStatus::Completed => "Done",
        BackgroundProcessStatus::Failed => "Failed",
    }
}

fn queue_header(count: usize) -> gtk::Label {
    let label = if count == 1 {
        String::from("Queue 1 prompt")
    } else {
        format!("Queue {count} prompts")
    };
    let header = gtk::Label::new(Some(&label));
    header.add_css_class("hc-queue-title");
    header.set_xalign(0.0);
    header
}

fn queue_row(
    prompt: &QueuedPrompt,
    busy: bool,
    sender: &relm4::Sender<HermesChatDropdownMsg>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.add_css_class("hc-queue-row");

    let text = gtk::Label::new(Some(prompt.text.trim().if_empty("Queued prompt")));
    text.add_css_class("hc-queue-text");
    text.set_xalign(0.0);
    text.set_hexpand(true);
    text.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.set_tooltip_text(Some(prompt.text.trim()));
    row.append(&text);

    let send_now = gtk::Button::with_label(if busy { "Next" } else { "Send" });
    send_now.add_css_class("hc-queue-action");
    send_now.set_cursor_from_name(Some("pointer"));
    send_now.set_tooltip_text(Some(if busy {
        "Send this prompt after stopping the current response"
    } else {
        "Send this queued prompt now"
    }));
    let prompt_id = prompt.id.clone();
    let send_sender = sender.clone();
    send_now.connect_clicked(move |_| {
        send_sender.emit(HermesChatDropdownMsg::QueuePromptNow(prompt_id.clone()));
    });
    row.append(&send_now);

    let remove = gtk::Button::with_label("Remove");
    remove.add_css_class("hc-queue-action");
    remove.set_cursor_from_name(Some("pointer"));
    remove.set_tooltip_text(Some("Remove this queued prompt"));
    let prompt_id = prompt.id.clone();
    let remove_sender = sender.clone();
    remove.connect_clicked(move |_| {
        remove_sender.emit(HermesChatDropdownMsg::RemoveQueuedPrompt(prompt_id.clone()));
    });
    row.append(&remove);

    row
}

fn subagent_meta(subagent: &SubagentItem) -> String {
    let mut parts = vec![subagent_status_label(subagent.status).to_owned()];
    if let Some(tool) = subagent
        .current_tool
        .as_deref()
        .filter(|tool| !tool.trim().is_empty())
    {
        parts.push(format!("using {}", tool_display_name(tool)));
    }
    if let (Some(index), Some(count)) = (subagent.task_index, subagent.task_count)
        && count > 1
    {
        parts.push(format!("task {}/{}", index + 1, count));
    }
    if let Some(summary) = subagent
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
    {
        parts.push(summary.to_owned());
    }
    parts.join(" - ")
}

fn subagent_status_class(status: SubagentStatus) -> &'static str {
    match status {
        SubagentStatus::Queued => "pending",
        SubagentStatus::Running => "running",
        SubagentStatus::Completed => "done",
        SubagentStatus::Failed => "error",
        SubagentStatus::Interrupted => "cancelled",
    }
}

fn subagent_status_glyph(status: SubagentStatus) -> &'static str {
    match status {
        SubagentStatus::Queued => "[ ]",
        SubagentStatus::Running => "[>]",
        SubagentStatus::Completed => "[x]",
        SubagentStatus::Failed => "[!]",
        SubagentStatus::Interrupted => "[-]",
    }
}

fn subagent_status_label(status: SubagentStatus) -> &'static str {
    match status {
        SubagentStatus::Queued => "Queued",
        SubagentStatus::Running => "Running",
        SubagentStatus::Completed => "Done",
        SubagentStatus::Failed => "Failed",
        SubagentStatus::Interrupted => "Interrupted",
    }
}

fn slash_suggestion_button(
    suggestion: &SlashCommandSuggestion,
    composer: &gtk::TextView,
    hermes_chat: &Arc<HermesChatService>,
    config: &Arc<ConfigService>,
    selected: bool,
) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("hc-slash-suggestion");
    if selected {
        button.add_css_class("selected");
    }
    button.set_focusable(false);
    button.set_cursor_from_name(Some("pointer"));

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_hexpand(true);

    let group = gtk::Label::new(Some(suggestion.group.trim().if_empty("Command")));
    group.add_css_class("hc-slash-group");
    group.set_xalign(0.0);
    row.append(&group);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_hexpand(true);
    let display = gtk::Label::new(Some(
        suggestion.display.trim().if_empty(&suggestion.insert_text),
    ));
    display.add_css_class("hc-slash-display");
    display.set_xalign(0.0);
    display.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&display);
    if !suggestion.description.trim().is_empty() {
        let description = gtk::Label::new(Some(suggestion.description.trim()));
        description.add_css_class("hc-slash-description");
        description.set_xalign(0.0);
        description.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&description);
    }
    row.append(&text);

    button.set_child(Some(&row));

    let composer = composer.clone();
    let hermes_chat = Arc::clone(hermes_chat);
    let config = Arc::clone(config);
    let insert_text = suggestion.insert_text.clone();
    button.connect_clicked(move |_| {
        let buffer = composer.buffer();
        let text = if slash_suggestion_expands_to_args(&insert_text) {
            format!("{} ", insert_text.trim())
        } else {
            insert_text.clone()
        };
        buffer.set_text(&text);
        let end = buffer.end_iter();
        buffer.place_cursor(&end);
        composer.grab_focus();
        if slash_suggestion_expands_to_args(&insert_text) {
            refresh_slash_suggestions_for_text(&hermes_chat, &config, text);
        } else {
            hermes_chat.clear_slash_suggestions();
        }
    });
    button
}

fn composer_text(composer: &gtk::TextView) -> String {
    let buffer = composer.buffer();
    let start = buffer.start_iter();
    let end = buffer.end_iter();
    buffer.text(&start, &end, true).to_string()
}

fn set_composer_text(composer: &gtk::TextView, text: &str) {
    let buffer = composer.buffer();
    buffer.set_text(text);
    let end = buffer.end_iter();
    buffer.place_cursor(&end);
    composer.grab_focus();
}

fn composer_scope(session_id: Option<&str>) -> String {
    session_id
        .filter(|session_id| !session_id.is_empty())
        .unwrap_or(LOCAL_DRAFT_SCOPE)
        .to_owned()
}

fn user_message_history(messages: &[HermesMessage]) -> Vec<String> {
    messages
        .iter()
        .rev()
        .filter(|message| message.role == HermesRole::User)
        .map(|message| message.content.trim())
        .filter(|content| !content.is_empty())
        .map(str::to_owned)
        .collect()
}

fn handle_composer_key(
    composer: &gtk::TextView,
    sender: relm4::Sender<HermesChatDropdownMsg>,
    key: gtk::gdk::Key,
    state: gtk::gdk::ModifierType,
) -> gtk::glib::Propagation {
    match key {
        gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter
            if !state.contains(gtk::gdk::ModifierType::SHIFT_MASK) =>
        {
            sender.emit(HermesChatDropdownMsg::SubmitOrAcceptSuggestion);
            gtk::glib::Propagation::Stop
        }
        gtk::gdk::Key::Escape => {
            sender.emit(HermesChatDropdownMsg::ClearSlashSuggestions);
            gtk::glib::Propagation::Stop
        }
        gtk::gdk::Key::Up if state.is_empty() => {
            if composer_text(composer).contains('\n') {
                return gtk::glib::Propagation::Proceed;
            }
            sender.emit(HermesChatDropdownMsg::HistoryOlder);
            gtk::glib::Propagation::Stop
        }
        gtk::gdk::Key::Down if state.is_empty() => {
            if composer_text(composer).contains('\n') {
                return gtk::glib::Propagation::Proceed;
            }
            sender.emit(HermesChatDropdownMsg::HistoryNewer);
            gtk::glib::Propagation::Stop
        }
        _ => gtk::glib::Propagation::Proceed,
    }
}

fn reasoning_widget(message: &HermesMessage) -> gtk::Expander {
    let expander = gtk::Expander::new(Some("Reasoning"));
    expander.add_css_class("hc-reasoning");
    expander.set_expanded(message.status == MessageStatus::Streaming);

    let body = markdown_blocks_widget(message.reasoning.trim(), "hc-reasoning-body");
    expander.set_child(Some(&body));
    expander
}

fn message_header(message: &HermesMessage) -> gtk::Box {
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    header.add_css_class("hc-message-header");

    let label = gtk::Label::new(Some(&message_role_label(message)));
    label.add_css_class("hc-message-role");
    label.set_xalign(0.0);
    header.append(&label);

    if message.role == HermesRole::Assistant && message.status == MessageStatus::Streaming {
        let spinner = gtk::Spinner::new();
        spinner.add_css_class("hc-message-spinner");
        spinner.set_valign(gtk::Align::Center);
        spinner.start();
        header.append(&spinner);
    }

    header
}

fn message_role_label(message: &HermesMessage) -> String {
    match message.role {
        HermesRole::User => "You",
        HermesRole::Assistant => match message.status {
            MessageStatus::Streaming => "Hermes",
            MessageStatus::Stopped => "Hermes stopped",
            MessageStatus::Error => "Hermes error",
            MessageStatus::Complete => "Hermes",
        },
        HermesRole::System => "System",
        HermesRole::Tool => "Tool",
        HermesRole::Error => "Error",
    }
    .to_owned()
}

fn approval_requires_sensitive_input(kind: ApprovalKind) -> bool {
    matches!(kind, ApprovalKind::Sudo | ApprovalKind::Secret)
}

fn tool_events_widget(message: &HermesMessage) -> gtk::Expander {
    let events = message
        .tool_events
        .iter()
        .filter(|event| !is_thought_tool_event(event))
        .collect::<Vec<_>>();
    let count = events.len();
    let label = if count == 1 {
        String::from("Tool activity (1)")
    } else {
        format!("Tool activity ({count})")
    };
    let expander = gtk::Expander::new(Some(&label));
    expander.add_css_class("hc-tool-activity");
    expander.set_expanded(message.status == MessageStatus::Streaming);

    let tools = gtk::Box::new(gtk::Orientation::Vertical, 4);
    tools.add_css_class("hc-tool-events");
    for event in events {
        let event = tool_event_for_display(message, event);
        tools.append(&tool_event_row(&event));
    }
    expander.set_child(Some(&tools));
    expander
}

fn tool_event_for_display(message: &HermesMessage, event: &ToolEvent) -> ToolEvent {
    if tool_status_is_finished(&event.status) {
        return event.clone();
    }
    let status = match message.status {
        MessageStatus::Complete => "completed",
        MessageStatus::Stopped => "cancelled",
        MessageStatus::Error => "failed",
        MessageStatus::Streaming => return event.clone(),
    };
    let mut event = event.clone();
    event.status = status.to_owned();
    event
}

fn tool_event_row(event: &ToolEvent) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.add_css_class("hc-tool-event");
    row.add_css_class(tool_status_class(&event.status));

    let icon = gtk::Image::from_icon_name(tool_icon_name(&event.tool));
    icon.add_css_class("hc-tool-icon");
    row.append(&icon);

    let label = gtk::Label::new(None);
    label.set_markup(&tool_event_markup(event));
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);

    row
}

fn tool_event_markup(event: &ToolEvent) -> String {
    let status = escape_pango_text(tool_status_label(&event.status));
    let tool = escape_pango_text(&tool_display_name(&event.tool));
    let label = event.label.trim();
    let mut lines = if label.is_empty() || label.eq_ignore_ascii_case(event.tool.trim()) {
        format!("<b>{tool}</b> <span foreground=\"#8f9aa8\">{status}</span>")
    } else {
        format!(
            "<b>{tool}</b> <span foreground=\"#8f9aa8\">{status}</span>\n{}",
            escape_pango_text(label)
        )
    };
    for detail in tool_event_detail_lines(event) {
        lines.push('\n');
        lines.push_str("<span foreground=\"#8f9aa8\">");
        lines.push_str(&escape_pango_text(&detail));
        lines.push_str("</span>");
    }
    lines
}

fn tool_event_detail_lines(event: &ToolEvent) -> Vec<String> {
    let mut lines = Vec::new();
    push_tool_detail(&mut lines, "cmd", event.command.as_deref(), &event.label);
    push_tool_detail(&mut lines, "input", event.input.as_deref(), &event.label);
    push_tool_detail(&mut lines, "path", event.path.as_deref(), &event.label);
    push_tool_detail(&mut lines, "url", event.url.as_deref(), &event.label);
    push_tool_detail(&mut lines, "output", event.output.as_deref(), &event.label);
    push_tool_detail(&mut lines, "error", event.error.as_deref(), &event.label);
    if event.has_inline_diff {
        lines.push(String::from("diff: available"));
    }
    lines.truncate(5);
    lines
}

fn push_tool_detail(lines: &mut Vec<String>, name: &str, value: Option<&str>, label: &str) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    if value.eq_ignore_ascii_case(label.trim()) {
        return;
    }
    let value = if value.chars().count() > 220 {
        format!("{}...", value.chars().take(220).collect::<String>())
    } else {
        value.to_owned()
    };
    lines.push(format!("{name}: {value}"));
}

fn active_tool_event(messages: &[HermesMessage]) -> Option<&ToolEvent> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == HermesRole::Assistant)
        .and_then(|message| {
            message
                .tool_events
                .iter()
                .rev()
                .filter(|event| !is_thought_tool_event(event))
                .find(|event| !tool_status_is_finished(&event.status))
                .or_else(|| {
                    message
                        .tool_events
                        .iter()
                        .rev()
                        .find(|event| !is_thought_tool_event(event))
                })
        })
}

fn tool_status_is_finished(status: &str) -> bool {
    matches!(
        normalized_tool_status(status).as_str(),
        "completed"
            | "done"
            | "success"
            | "succeeded"
            | "failed"
            | "error"
            | "cancelled"
            | "canceled"
    )
}

fn tool_status_class(status: &str) -> &'static str {
    match normalized_tool_status(status).as_str() {
        "completed" | "done" | "success" | "succeeded" => "done",
        "failed" | "error" | "cancelled" | "canceled" => "error",
        "queued" | "pending" => "pending",
        _ => "running",
    }
}

fn tool_status_label(status: &str) -> &'static str {
    match normalized_tool_status(status).as_str() {
        "completed" | "done" | "success" | "succeeded" => "Done",
        "failed" | "error" => "Failed",
        "cancelled" | "canceled" => "Cancelled",
        "queued" | "pending" => "Queued",
        _ => "Running",
    }
}

fn normalized_tool_status(status: &str) -> String {
    status.trim().to_ascii_lowercase().replace([' ', '_'], "-")
}

fn tool_display_name(tool: &str) -> String {
    let tool = tool.trim();
    if tool.is_empty() {
        return String::from("Tool");
    }
    tool.split(['_', '-', '.'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first
                    .to_uppercase()
                    .chain(chars.flat_map(char::to_lowercase))
                    .collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_thought_tool_event(event: &ToolEvent) -> bool {
    let tool = event
        .tool
        .trim()
        .trim_start_matches('_')
        .to_ascii_lowercase()
        .replace([' ', '-', '.'], "_");
    let label = event
        .label
        .trim()
        .trim_start_matches('_')
        .to_ascii_lowercase()
        .replace([' ', '-', '.'], "_");
    matches!(
        tool.as_str(),
        "thinking" | "reasoning" | "thought" | "thoughts" | "chain_of_thought"
    ) || tool.contains("reasoning")
        || matches!(
            label.as_str(),
            "thinking" | "reasoning" | "thought" | "thoughts" | "chain_of_thought"
        )
}

fn tool_icon_name(tool: &str) -> &'static str {
    let tool = tool.to_ascii_lowercase();
    if tool.contains("terminal")
        || tool.contains("shell")
        || tool.contains("command")
        || tool.contains("exec")
        || tool.contains("process")
    {
        "ld-terminal-symbolic"
    } else if tool.contains("web") || tool.contains("search") || tool.contains("browser") {
        "ld-globe-symbolic"
    } else if tool.contains("file") || tool.contains("patch") || tool.contains("write") {
        "ld-file-text-symbolic"
    } else if tool.contains("code") {
        "ld-code-symbolic"
    } else {
        "ld-settings-symbolic"
    }
}

fn status_text(
    status: &HermesStatus,
    last_error: Option<&str>,
    active_tool: Option<&ToolEvent>,
    approval: Option<&ApprovalRequest>,
) -> String {
    let detail = match status {
        HermesStatus::Offline(message) | HermesStatus::Error(message) => Some(message.as_str()),
        HermesStatus::MissingApiKey | HermesStatus::AuthFailed => last_error,
        HermesStatus::Disabled
        | HermesStatus::Connecting
        | HermesStatus::Connected
        | HermesStatus::Busy => None,
    }
    .filter(|message| !message.trim().is_empty());

    match status {
        HermesStatus::Disabled => String::from("Hermes disabled"),
        HermesStatus::MissingApiKey => detail.map_or_else(
            || String::from("Hermes API key missing"),
            |detail| format!("Hermes API key missing: {detail}"),
        ),
        HermesStatus::Connecting => String::from("Connecting to Hermes..."),
        HermesStatus::Connected => String::from("Hermes ready"),
        HermesStatus::Busy
            if approval
                .is_some_and(|approval| approval_requires_sensitive_input(approval.kind)) =>
        {
            String::from("Waiting for secure input")
        }
        HermesStatus::Busy if approval.is_some() => String::from("Waiting for approval"),
        HermesStatus::Busy => active_tool.map_or_else(
            || String::from("Hermes is working..."),
            |event| {
                let tool = tool_display_name(&event.tool);
                let label = event.label.trim();
                if label.is_empty() || label.eq_ignore_ascii_case(event.tool.trim()) {
                    format!("Using {tool}")
                } else {
                    format!("Using {tool}: {label}")
                }
            },
        ),
        HermesStatus::AuthFailed => detail.map_or_else(
            || String::from("Hermes authentication failed"),
            |detail| format!("Hermes authentication failed: {detail}"),
        ),
        HermesStatus::Offline(_) => detail.map_or_else(
            || String::from("Hermes offline"),
            |detail| format!("Hermes offline: {detail}"),
        ),
        HermesStatus::Error(_) => detail.map_or_else(
            || String::from("Hermes error"),
            |detail| format!("Hermes error: {detail}"),
        ),
    }
}

fn icon_button(icon_name: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_cursor_from_name(Some("pointer"));
    let icon = gtk::Image::from_icon_name(icon_name);
    button.set_child(Some(&icon));
    button
}

fn labeled_icon_button(icon_name: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_cursor_from_name(Some("pointer"));

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.set_valign(gtk::Align::Center);

    let icon = gtk::Image::from_icon_name(icon_name);
    content.append(&icon);

    let label = gtk::Label::new(Some(label));
    content.append(&label);

    button.set_child(Some(&content));
    button
}

fn build_css(full_scale: f32, dropdown_scale: f32) -> String {
    [
        build_layout_css(full_scale, dropdown_scale),
        build_message_css(full_scale, dropdown_scale),
        build_composer_css(full_scale, dropdown_scale),
        build_state_css(dropdown_scale),
    ]
    .concat()
}

#[allow(clippy::too_many_lines)]
fn build_layout_css(full_scale: f32, dropdown_scale: f32) -> String {
    format!(
        r#"
        .hermes-chat-dropdown .hc-root {{
            min-width: {width}px;
        }}
        .hermes-chat-dropdown .dropdown-header {{
            padding: calc(var(--space-sm) * {dropdown_scale}) calc(var(--space-md) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .dropdown-content {{
            padding: calc((var(--space-sm) + var(--space-xs)) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .dropdown-footer {{
            padding: calc(var(--space-sm) * {dropdown_scale}) calc(var(--space-md) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-header {{ margin-bottom: 0; }}
        .hermes-chat-dropdown .hc-title {{ font-weight: var(--weight-bold); font-size: calc(var(--text-lg) * {dropdown_scale}); }}
        .hermes-chat-dropdown .hc-status {{ color: var(--fg-muted); font-size: calc(var(--text-sm) * {dropdown_scale}); margin-left: calc(var(--space-sm) * {dropdown_scale}); }}
        .hermes-chat-dropdown .hc-status.ok {{ color: #4fb86a; }}
        .hermes-chat-dropdown .hc-status.busy {{ color: #61afef; }}
        .hermes-chat-dropdown .hc-status.error {{ color: #e2604f; }}
        .hermes-chat-dropdown .hc-status.disabled {{ color: var(--fg-muted); }}
        .hermes-chat-dropdown .hc-status-dot {{
            min-width: calc(7px * {dropdown_scale});
            min-height: calc(7px * {dropdown_scale});
            border-radius: 9999px;
            background: var(--fg-muted);
        }}
        .hermes-chat-dropdown .hc-status-group.ok .hc-status-dot {{ background: #4fb86a; }}
        .hermes-chat-dropdown .hc-status-group.busy .hc-status-dot {{ background: #61afef; }}
        .hermes-chat-dropdown .hc-status-group.error .hc-status-dot {{ background: #e2604f; }}
        .hermes-chat-dropdown .hc-status-group.disabled .hc-status-dot {{ background: var(--fg-muted); }}
        .hermes-chat-dropdown .hc-action image {{
            -gtk-icon-size: calc(var(--icon-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-content {{
            background-color: var(--dropdown-surface);
        }}
        .hermes-chat-dropdown .hc-session-section {{
            margin-bottom: calc(var(--space-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-button {{
            background: var(--bg-base);
            border: 1px solid var(--border-default);
            border-radius: var(--rounding-element);
            padding: calc(var(--space-sm) * {dropdown_scale}) calc(var(--space-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-button:hover {{
            border-color: var(--accent);
            background: var(--bg-elevated);
        }}
        .hermes-chat-dropdown .hc-session-icon {{
            color: var(--accent);
            -gtk-icon-size: calc(var(--icon-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-name {{
            color: var(--fg-default);
            font-size: calc(var(--text-md) * {dropdown_scale});
            font-weight: var(--weight-semibold);
        }}
        .hermes-chat-dropdown .hc-session-badge {{
            color: var(--accent);
            background: var(--accent-subtle);
            border: 1px solid var(--accent-subtle);
            border-radius: 9999px;
            font-size: calc(var(--text-xs) * {dropdown_scale});
            font-weight: var(--weight-bold);
            padding: calc(2px * {dropdown_scale}) calc(var(--space-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-meta {{
            color: var(--fg-muted);
            font-size: calc(var(--text-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-chevron {{
            color: var(--fg-muted);
            -gtk-icon-size: calc(var(--icon-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-popover > contents {{
            background: var(--bg-elevated);
            border: 1px solid var(--border-default);
            border-radius: var(--rounding-container);
            padding: calc(var(--space-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-pop {{
            min-width: calc(22rem * {full_scale});
        }}
        .hermes-chat-dropdown .hc-session-pop-title {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            font-weight: var(--weight-bold);
            padding: calc(var(--space-sm) * {dropdown_scale}) calc(var(--space-sm) * {dropdown_scale}) calc(var(--space-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-pop-row {{
            background: transparent;
            border: 0;
            border-radius: var(--rounding-element);
            padding: calc(var(--space-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-pop-row:hover {{
            background: var(--bg-overlay);
        }}
        .hermes-chat-dropdown .hc-session-pop-row.current {{
            background: var(--bg-selected);
        }}
        .hermes-chat-dropdown .hc-session-pop-icon {{
            color: var(--fg-muted);
            -gtk-icon-size: calc(var(--icon-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-pop-name {{
            color: var(--fg-default);
            font-size: calc(var(--text-sm) * {dropdown_scale});
            font-weight: var(--weight-medium);
        }}
        .hermes-chat-dropdown .hc-session-pop-meta {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-pop-check {{
            color: var(--accent);
            font-weight: var(--weight-bold);
        }}
        .hermes-chat-dropdown .hc-session-pop-empty {{
            color: var(--fg-muted);
            font-size: calc(var(--text-sm) * {dropdown_scale});
            padding: calc(var(--space-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-activity {{
            border: 1px solid alpha(#61afef, 0.28);
            border-radius: var(--rounding-element);
            padding: calc(var(--space-sm) * {dropdown_scale});
            margin-bottom: calc(var(--space-sm) * {dropdown_scale});
            background: alpha(#61afef, 0.04);
        }}
        .hermes-chat-dropdown .hc-session-activity-title {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            font-weight: var(--weight-semibold);
        }}
        .hermes-chat-dropdown .hc-session-activity-row {{
            color: var(--fg-default);
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-activity-status {{
            color: var(--fg-muted);
            font-family: monospace;
            min-width: calc(1.6rem * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-activity-row.running .hc-session-activity-status {{
            color: #61afef;
        }}
        .hermes-chat-dropdown .hc-session-activity-row.waiting .hc-session-activity-status {{
            color: #d19a66;
        }}
        .hermes-chat-dropdown .hc-session-activity-name {{
            color: var(--fg-default);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            font-weight: var(--weight-semibold);
        }}
        .hermes-chat-dropdown .hc-session-activity-preview {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-activity-meta,
        .hermes-chat-dropdown .hc-session-activity-more {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-session-activity-action {{
            min-height: calc(1.5rem * {dropdown_scale});
            padding: 0 calc(var(--space-xs) * {dropdown_scale});
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-section-label {{
            color: var(--fg-muted);
            font-size: calc(var(--text-sm) * {dropdown_scale});
            font-weight: var(--weight-semibold);
            min-width: calc(4.5rem * {full_scale});
        }}
        .hermes-chat-dropdown .hc-scroller {{
            background-color: var(--bg-base);
            border: 1px solid var(--border-default);
            border-radius: var(--rounding-element);
        }}
        .hermes-chat-dropdown .hc-transcript {{
            padding: calc(var(--space-sm) * {dropdown_scale});
        }}
"#,
        width = scaled_dimension(BASE_WIDTH, full_scale * dropdown_scale),
        full_scale = full_scale,
        dropdown_scale = dropdown_scale
    )
}

#[allow(clippy::too_many_lines)]
fn build_message_css(full_scale: f32, dropdown_scale: f32) -> String {
    format!(
        r#"
        .hermes-chat-dropdown .hc-message {{
            background: var(--bg-elevated);
            border: 1px solid var(--border-default);
            border-radius: var(--rounding-element);
            padding: calc(var(--space-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-message.user {{
            background: alpha(#4f8cff, 0.12);
            border-color: alpha(#4f8cff, 0.24);
        }}
        .hermes-chat-dropdown .hc-message.tool {{
            background: alpha(#61afef, 0.08);
            border-color: alpha(#61afef, 0.22);
        }}
        .hermes-chat-dropdown .hc-message.error {{
            background: alpha(#e2604f, 0.08);
            border-color: alpha(#e2604f, 0.55);
        }}
        .hermes-chat-dropdown .hc-message-role {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            font-weight: var(--weight-semibold);
        }}
        .hermes-chat-dropdown .hc-message-header {{
            min-height: calc(1.1rem * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-message-spinner {{
            -gtk-icon-size: calc(var(--icon-xs) * {dropdown_scale});
            color: #61afef;
        }}
        .hermes-chat-dropdown .hc-message-body {{
            color: var(--fg-default);
            font-size: calc(var(--text-sm) * {dropdown_scale});
            line-height: 1.35;
        }}
        .hermes-chat-dropdown .hc-md {{
            color: var(--fg-default);
            font-size: calc(var(--text-sm) * {dropdown_scale});
            line-height: 1.35;
        }}
        .hermes-chat-dropdown .hc-md-p {{
            color: var(--fg-default);
            font-size: calc(var(--text-sm) * {dropdown_scale});
            line-height: 1.35;
        }}
        .hermes-chat-dropdown .hc-md-heading {{
            color: var(--fg-default);
            margin-top: calc(var(--space-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-md-h1 {{
            font-size: calc(var(--text-lg) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-md-h2 {{
            font-size: calc(var(--text-md) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-md-h3 {{
            font-size: calc(var(--text-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-md-code {{
            background: alpha(#111827, 0.38);
            border: 1px solid var(--border-default);
            border-radius: var(--rounding-element);
            margin: calc(var(--space-xs) * {dropdown_scale}) 0;
        }}
        .hermes-chat-dropdown .hc-md-code-lang {{
            color: var(--fg-muted);
            font-family: monospace;
            font-size: calc(var(--text-xs) * {dropdown_scale});
            padding: calc(var(--space-xs) * {dropdown_scale}) calc(var(--space-sm) * {dropdown_scale}) 0;
        }}
        .hermes-chat-dropdown .hc-md-code-body {{
            color: var(--fg-default);
            font-family: monospace;
            font-size: calc(var(--text-xs) * {dropdown_scale});
            line-height: 1.35;
            padding: calc(var(--space-xs) * {dropdown_scale}) calc(var(--space-sm) * {dropdown_scale}) calc(var(--space-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-md-quote {{
            border-left: calc(3px * {full_scale}) solid alpha(#61afef, 0.45);
            margin: calc(var(--space-xs) * {dropdown_scale}) 0;
            padding-left: calc(var(--space-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-md-quote .hc-md-p {{
            color: var(--fg-muted);
        }}
        .hermes-chat-dropdown .hc-md-list {{
            margin: calc(var(--space-xs) * {dropdown_scale}) 0;
        }}
        .hermes-chat-dropdown .hc-md-list-marker {{
            color: var(--fg-muted);
            font-size: calc(var(--text-sm) * {dropdown_scale});
            min-width: calc(1.35rem * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-md-list-content {{
            color: var(--fg-default);
        }}
        .hermes-chat-dropdown .hc-md-table-frame {{
            border: 1px solid var(--border-default);
            border-radius: var(--rounding-element);
            margin: calc(var(--space-xs) * {dropdown_scale}) 0;
        }}
        .hermes-chat-dropdown .hc-md-table-cell {{
            border-bottom: 1px solid var(--border-default);
            border-right: 1px solid var(--border-default);
            color: var(--fg-default);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            padding: calc(var(--space-xs) * {dropdown_scale}) calc(var(--space-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-md-table-cell.header {{
            background: alpha(#61afef, 0.08);
            color: var(--fg-muted);
            font-weight: var(--weight-semibold);
        }}
        .hermes-chat-dropdown .hc-md-rule {{
            margin: calc(var(--space-xs) * {dropdown_scale}) 0;
        }}
        .hermes-chat-dropdown .hc-reasoning {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            margin-top: calc(var(--space-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-reasoning-body {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            line-height: 1.3;
            padding-top: calc(var(--space-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-tool-events {{
            border-left: calc(2px * {full_scale}) solid alpha(#61afef, 0.45);
            padding-left: calc(var(--space-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-tool-activity {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            margin-top: calc(var(--space-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-tool-activity > title {{
            color: var(--fg-muted);
            font-weight: var(--weight-semibold);
        }}
        .hermes-chat-dropdown .hc-tool-event {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-tool-event.running {{
            color: #61afef;
        }}
        .hermes-chat-dropdown .hc-tool-event.done {{
            color: #4fb86a;
        }}
        .hermes-chat-dropdown .hc-tool-event.error {{
            color: #e2604f;
        }}
        .hermes-chat-dropdown .hc-tool-icon {{
            -gtk-icon-size: calc(var(--icon-xs) * {dropdown_scale});
            margin-top: calc(0.1rem * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-todos {{
            border: 1px solid alpha(#61afef, 0.35);
            border-radius: var(--rounding-element);
            padding: calc(var(--space-sm) * {dropdown_scale});
            margin-top: calc(var(--space-sm) * {dropdown_scale});
            background: alpha(#61afef, 0.05);
        }}
        .hermes-chat-dropdown .hc-todos-title {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            font-weight: var(--weight-semibold);
        }}
        .hermes-chat-dropdown .hc-todo-row {{
            color: var(--fg-default);
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-todo-status {{
            color: var(--fg-muted);
            font-family: monospace;
            min-width: calc(1.6rem * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-todo-row.running .hc-todo-status {{
            color: #61afef;
        }}
        .hermes-chat-dropdown .hc-todo-row.done .hc-todo-status {{
            color: #4fb86a;
        }}
        .hermes-chat-dropdown .hc-todo-row.cancelled .hc-todo-status {{
            color: #8f9aa8;
        }}
        .hermes-chat-dropdown .hc-subagents {{
            border: 1px solid alpha(#d19a66, 0.35);
            border-radius: var(--rounding-element);
            padding: calc(var(--space-sm) * {dropdown_scale});
            margin-top: calc(var(--space-sm) * {dropdown_scale});
            background: alpha(#d19a66, 0.05);
        }}
        .hermes-chat-dropdown .hc-subagents-title {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            font-weight: var(--weight-semibold);
        }}
        .hermes-chat-dropdown .hc-subagent-row {{
            color: var(--fg-default);
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-subagent-status {{
            color: var(--fg-muted);
            font-family: monospace;
            min-width: calc(1.6rem * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-subagent-goal {{
            color: var(--fg-default);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            font-weight: var(--weight-semibold);
        }}
        .hermes-chat-dropdown .hc-subagent-meta {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-subagent-row.running .hc-subagent-status {{
            color: #61afef;
        }}
        .hermes-chat-dropdown .hc-subagent-row.done .hc-subagent-status {{
            color: #4fb86a;
        }}
        .hermes-chat-dropdown .hc-subagent-row.error .hc-subagent-status {{
            color: #e2604f;
        }}
        .hermes-chat-dropdown .hc-subagent-row.cancelled .hc-subagent-status {{
            color: #8f9aa8;
        }}
        .hermes-chat-dropdown .hc-background {{
            border: 1px solid alpha(#c678dd, 0.35);
            border-radius: var(--rounding-element);
            padding: calc(var(--space-sm) * {dropdown_scale});
            margin-top: calc(var(--space-sm) * {dropdown_scale});
            background: alpha(#c678dd, 0.05);
        }}
        .hermes-chat-dropdown .hc-background-title {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            font-weight: var(--weight-semibold);
        }}
        .hermes-chat-dropdown .hc-background-row {{
            color: var(--fg-default);
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-background-status {{
            color: var(--fg-muted);
            font-family: monospace;
            min-width: calc(1.6rem * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-background-title-text {{
            color: var(--fg-default);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            font-weight: var(--weight-semibold);
        }}
        .hermes-chat-dropdown .hc-background-meta {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-background-action {{
            min-height: calc(1.5rem * {dropdown_scale});
            padding: 0 calc(var(--space-xs) * {dropdown_scale});
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-background-output {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            margin-left: calc(1.9rem * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-background-output-body {{
            color: var(--fg-muted);
            font-family: monospace;
            font-size: calc(var(--text-xs) * {dropdown_scale});
            line-height: 1.3;
            padding-top: calc(var(--space-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-background-item.running .hc-background-status {{
            color: #61afef;
        }}
        .hermes-chat-dropdown .hc-background-item.done .hc-background-status {{
            color: #4fb86a;
        }}
        .hermes-chat-dropdown .hc-background-item.error .hc-background-status {{
            color: #e2604f;
        }}
        .hermes-chat-dropdown .hc-queue {{
            border: 1px solid alpha(#4f8cff, 0.35);
            border-radius: var(--rounding-element);
            padding: calc(var(--space-sm) * {dropdown_scale});
            margin-top: calc(var(--space-sm) * {dropdown_scale});
            background: alpha(#4f8cff, 0.05);
        }}
        .hermes-chat-dropdown .hc-queue-title {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            font-weight: var(--weight-semibold);
        }}
        .hermes-chat-dropdown .hc-queue-row {{
            color: var(--fg-default);
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-queue-text {{
            color: var(--fg-default);
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-queue-action {{
            min-height: calc(1.5rem * {dropdown_scale});
            padding: 0 calc(var(--space-xs) * {dropdown_scale});
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-queue-more {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
    "#,
        full_scale = full_scale,
        dropdown_scale = dropdown_scale
    )
}

#[allow(clippy::too_many_lines)]
fn build_composer_css(full_scale: f32, dropdown_scale: f32) -> String {
    format!(
        r#"
        .hermes-chat-dropdown .hc-composer-wrap {{
            background-color: var(--bg-elevated);
            border-bottom-left-radius: var(--rounding-container);
            border-bottom-right-radius: var(--rounding-container);
            border-top: 1px solid var(--border-default);
        }}
        .hermes-chat-dropdown .hc-composer {{
            background-color: transparent;
        }}
        .hermes-chat-dropdown .hc-input-pill {{
            background-color: var(--bg-overlay);
            border: 1px solid var(--border-strong);
            border-radius: var(--rounding-element);
            padding: calc(var(--space-sm) * {dropdown_scale}) calc(var(--space-md) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-input-pill:focus-within {{
            border-color: var(--accent);
        }}
        .hermes-chat-dropdown .hc-composer-input,
        .hermes-chat-dropdown .hc-composer-input text {{
            background: transparent;
            color: var(--fg-default);
            font-size: calc(var(--text-md) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-slash-suggestions {{
            background: var(--bg-overlay);
            border: 1px solid var(--border-default);
            border-radius: var(--rounding-element);
            margin-bottom: calc(var(--space-sm) * {dropdown_scale});
            padding: calc(var(--space-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-slash-suggestion {{
            background: transparent;
            border: 0;
            border-radius: var(--rounding-element);
            padding: calc(var(--space-xs) * {dropdown_scale}) calc(var(--space-sm) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-slash-suggestion:hover {{
            background: alpha(#61afef, 0.12);
        }}
        .hermes-chat-dropdown .hc-slash-suggestion.selected {{
            background: alpha(#61afef, 0.18);
        }}
        .hermes-chat-dropdown .hc-slash-group {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
            min-width: calc(4.5rem * {full_scale});
        }}
        .hermes-chat-dropdown .hc-slash-display {{
            color: var(--fg-default);
            font-size: calc(var(--text-sm) * {dropdown_scale});
            font-weight: var(--weight-semibold);
        }}
        .hermes-chat-dropdown .hc-slash-description {{
            color: var(--fg-muted);
            font-size: calc(var(--text-xs) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-icon-btn {{
            min-width: calc(2.6rem * {full_scale});
            min-height: calc(2.6rem * {full_scale});
            padding: 0;
            border-radius: 9999px;
            border: 1px solid var(--border-strong);
            background: var(--bg-overlay);
            color: var(--fg-muted);
        }}
        .hermes-chat-dropdown .hc-icon-btn:hover {{
            background: var(--bg-elevated);
            color: var(--fg-default);
            border-color: var(--accent);
        }}
        .hermes-chat-dropdown .hc-icon-btn:disabled {{
            opacity: 0.45;
        }}
        .hermes-chat-dropdown .hc-icon-btn image {{
            -gtk-icon-size: calc(var(--icon-md) * {dropdown_scale});
        }}
        .hermes-chat-dropdown .hc-send {{
            background: var(--accent);
            border-color: var(--accent);
            color: var(--fg-on-accent);
        }}
        .hermes-chat-dropdown .hc-send:hover {{
            background: var(--accent-hover);
            border-color: var(--accent-hover);
            color: var(--fg-on-accent);
        }}
        .hermes-chat-dropdown .hc-mic.recording {{
            background: var(--accent);
            border-color: var(--accent);
            color: var(--fg-on-accent);
        }}
        .hermes-chat-dropdown .hc-stop {{
            background: alpha(#e2604f, 0.15);
            border-color: alpha(#e2604f, 0.55);
            color: #e2604f;
        }}
        .hermes-chat-dropdown .hc-stop:hover {{
            background: #e2604f;
            border-color: #e2604f;
            color: #ffffff;
        }}
"#,
        full_scale = full_scale,
        dropdown_scale = dropdown_scale
    )
}

fn build_state_css(dropdown_scale: f32) -> String {
    format!(
        r#"
        .hermes-chat-dropdown .hc-approval {{ border: 1px solid alpha(#e0a93e, 0.5); border-radius: var(--rounding-element); padding: calc(var(--space-sm) * {dropdown_scale}); margin-top: calc(var(--space-sm) * {dropdown_scale}); }}
        .hermes-chat-dropdown .hc-approval-entry {{ margin-top: calc(var(--space-xs) * {dropdown_scale}); }}
        .hermes-chat-dropdown .hc-empty {{ color: var(--fg-muted); padding: calc(var(--space-md) * {dropdown_scale}); }}
"#,
        dropdown_scale = dropdown_scale
    )
}
