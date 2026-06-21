use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use futures::StreamExt;
use lumen_core::Property;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::{
    ApprovalKind, ApprovalRequest, BackgroundProcessItem, BackgroundProcessStatus,
    ConnectionConfig, Error, HermesClient, HermesMessage, HermesRole, HermesSessionSummary,
    HermesStatus, LocalHistoryMode, MessageStatus, Result, SlashCommandSuggestion, SseEvent,
    SubagentItem, SubagentStatus, TodoItem, TodoStatus, ToolEvent, TransportMode,
    client::{EventStream, normalize_endpoint_url, parse_messages, parse_session},
    dashboard::{DashboardClient, DashboardFrame, DashboardRpcEvent},
    store::LocalHistoryStore,
};

const DASHBOARD_POST_COMPLETE_SETTLE: Duration = Duration::from_millis(750);
const HANDOFF_POLL_INTERVAL: Duration = Duration::from_millis(800);
const HANDOFF_TIMEOUT: Duration = Duration::from_secs(60);
const SESSION_SUMMARY_CACHE_TTL: Duration = Duration::from_secs(15);
const TRANSCRIPT_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const TRANSCRIPT_CACHE_MAX_SESSIONS: usize = 12;

type SharedSessionSummaryCache = Arc<Mutex<SessionSummaryCache>>;
type SharedTranscriptCache = Arc<Mutex<TranscriptCache>>;

#[derive(Debug, Default)]
struct SessionSummaryCache {
    sessions: Vec<HermesSessionSummary>,
    fetched_at: Option<Instant>,
}

#[derive(Debug, Default)]
struct TranscriptCache {
    entries: HashMap<String, TranscriptCacheEntry>,
    order: VecDeque<String>,
}

#[derive(Debug, Clone)]
struct TranscriptCacheEntry {
    messages: Vec<HermesMessage>,
    fingerprint: Option<SessionFingerprint>,
    fetched_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionFingerprint {
    updated_at: Option<chrono::DateTime<chrono::Utc>>,
    message_count: Option<u64>,
}

/// Builder for [`HermesChatService`].
#[derive(Debug)]
pub struct HermesChatServiceBuilder {
    config: ConnectionConfig,
    history_path: Option<PathBuf>,
}

impl HermesChatServiceBuilder {
    /// Creates a builder with default disabled config.
    pub fn new() -> Self {
        Self {
            config: ConnectionConfig::default(),
            history_path: None,
        }
    }

    /// Sets runtime connection config.
    pub fn config(mut self, config: ConnectionConfig) -> Self {
        self.config = config;
        self
    }

    /// Sets local-only history path.
    pub fn history_path(mut self, path: PathBuf) -> Self {
        self.history_path = Some(path);
        self
    }

    /// Builds the service.
    pub fn build(self) -> HermesChatService {
        HermesChatService::new(self.config, self.history_path)
    }
}

impl Default for HermesChatServiceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Reactive Hermes chat service consumed by shell widgets.
#[derive(Debug)]
pub struct HermesChatService {
    config: RwLock<ConnectionConfig>,
    store: Option<Arc<LocalHistoryStore>>,
    config_sequence: Arc<AtomicU64>,
    connect_sequence: Arc<AtomicU64>,
    select_sequence: Arc<AtomicU64>,
    stream_token: Mutex<Option<CancellationToken>>,
    stream_sequence: AtomicU64,
    slash_suggestion_sequence: Arc<AtomicU64>,
    active_stream_id: Arc<RwLock<Option<u64>>>,
    active_run_id: Arc<RwLock<Option<String>>>,
    selected_dashboard_profile: Arc<RwLock<Option<String>>>,
    session_summary_cache: SharedSessionSummaryCache,
    transcript_cache: SharedTranscriptCache,

    /// Connection status.
    pub status: Property<HermesStatus>,
    /// Raw capabilities JSON advertised by Hermes.
    pub capabilities: Property<Option<Arc<Value>>>,
    /// Server-side sessions, when available.
    pub sessions: Property<Vec<HermesSessionSummary>>,
    /// Active session id.
    pub active_session_id: Property<Option<String>>,
    /// Local-only transcript shown by Lumen.
    pub messages: Property<Vec<HermesMessage>>,
    /// Pending approval/clarification prompt.
    pub approval: Property<Option<ApprovalRequest>>,
    /// Whether per-session YOLO approval bypass is active or armed.
    pub yolo_active: Property<bool>,
    /// Pending text that a dashboard slash command wants loaded into the composer.
    pub composer_prefill: Property<Option<String>>,
    /// Live todo list reported by Hermes for the active session.
    pub todos: Property<Vec<TodoItem>>,
    /// Live subagent/delegated-task status reported by Hermes for the active session.
    pub subagents: Property<Vec<SubagentItem>>,
    /// Live background process status reported by Hermes for the active session.
    pub background_processes: Property<Vec<BackgroundProcessItem>>,
    /// Live slash command suggestions for the composer.
    pub slash_suggestions: Property<Vec<SlashCommandSuggestion>>,
    /// Last user-facing error.
    pub last_error: Property<Option<String>>,
}

impl HermesChatService {
    /// Returns a builder.
    pub fn builder() -> HermesChatServiceBuilder {
        HermesChatServiceBuilder::new()
    }

    fn new(config: ConnectionConfig, history_path: Option<PathBuf>) -> Self {
        let store = history_path.map(LocalHistoryStore::new).map(Arc::new);
        let (active_session_id, messages) = if config.local_history == LocalHistoryMode::Full {
            store
                .as_ref()
                .and_then(|store| store.load().ok())
                .unwrap_or((None, Vec::new()))
        } else {
            (None, Vec::new())
        };
        let status = if config.enabled {
            HermesStatus::Connecting
        } else {
            HermesStatus::Disabled
        };
        let service = Self {
            config: RwLock::new(config),
            store,
            config_sequence: Arc::new(AtomicU64::new(0)),
            connect_sequence: Arc::new(AtomicU64::new(0)),
            select_sequence: Arc::new(AtomicU64::new(0)),
            stream_token: Mutex::new(None),
            stream_sequence: AtomicU64::new(0),
            slash_suggestion_sequence: Arc::new(AtomicU64::new(0)),
            active_stream_id: Arc::new(RwLock::new(None)),
            active_run_id: Arc::new(RwLock::new(None)),
            selected_dashboard_profile: Arc::new(RwLock::new(None)),
            session_summary_cache: Arc::new(Mutex::new(SessionSummaryCache::default())),
            transcript_cache: Arc::new(Mutex::new(TranscriptCache::default())),
            status: Property::new(status),
            capabilities: Property::new(None),
            sessions: Property::new(Vec::new()),
            active_session_id: Property::new(active_session_id),
            messages: Property::new(messages),
            approval: Property::new(None),
            yolo_active: Property::new(false),
            composer_prefill: Property::new(None),
            todos: Property::new(Vec::new()),
            subagents: Property::new(Vec::new()),
            background_processes: Property::new(Vec::new()),
            slash_suggestions: Property::new(Vec::new()),
            last_error: Property::new(None),
        };
        service.connect();
        service
    }

    /// Replaces runtime config, preserving transcript, and reconnects.
    pub fn update_config(&self, config: ConnectionConfig) {
        if let Ok(mut guard) = self.config.write() {
            if *guard == config {
                return;
            }
            if stream_config_requires_cancel(&guard, &config) {
                self.cancel_current_for_config_change(guard.clone());
            }
            if remote_state_requires_reset(&guard, &config) {
                self.clear_remote_state();
            }
            *guard = config;
            self.config_sequence.fetch_add(1, Ordering::Relaxed);
        }
        self.connect();
    }

    /// Connects/refreshes capabilities and sessions in the background.
    pub fn connect(&self) {
        let connect_id = self.connect_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let config = self.config();
        if let Some((status, last_error)) = unavailable_config_state(&config) {
            self.clear_remote_state();
            self.status.set(status);
            self.last_error.set(last_error);
            return;
        }

        let auto_select_session = auto_select_first_session_on_connect(config.transport_mode);
        if let (Some(capabilities), Some(sessions)) = (
            self.capabilities.get(),
            cached_session_summaries(&self.session_summary_cache),
        ) {
            let discovered_dashboard =
                capabilities.get("transport_mode").and_then(Value::as_str) == Some("dashboard-ws");
            if auto_select_session
                && self.active_session_id.get().is_none()
                && !discovered_dashboard
            {
                self.active_session_id
                    .set(sessions.first().map(|session| session.id.clone()));
            }
            self.sessions.set(sessions);
            self.last_error.set(None);
            set_status_unless_busy(&self.status, HermesStatus::Connected);
            return;
        }

        set_status_unless_busy(&self.status, HermesStatus::Connecting);
        let status = self.status.clone();
        let capabilities_prop = self.capabilities.clone();
        let sessions_prop = self.sessions.clone();
        let active_prop = self.active_session_id.clone();
        let error_prop = self.last_error.clone();
        let connect_sequence = self.connect_sequence.clone();
        let session_summary_cache = self.session_summary_cache.clone();
        tokio::spawn(async move {
            match connect_inner(config).await {
                Ok((capabilities, sessions)) => {
                    if !connect_is_current(&connect_sequence, connect_id) {
                        return;
                    }
                    let discovered_dashboard =
                        capabilities.get("transport_mode").and_then(Value::as_str)
                            == Some("dashboard-ws");
                    capabilities_prop.set(Some(Arc::new(capabilities)));
                    if auto_select_session && active_prop.get().is_none() && !discovered_dashboard {
                        active_prop.set(sessions.first().map(|session| session.id.clone()));
                    }
                    cache_session_summaries(&session_summary_cache, sessions.clone());
                    sessions_prop.set(sessions);
                    error_prop.set(None);
                    set_status_unless_busy(&status, HermesStatus::Connected);
                }
                Err(err) => {
                    if !connect_is_current(&connect_sequence, connect_id) {
                        return;
                    }
                    let message = err.short_message();
                    error_prop.set(Some(message.clone()));
                    set_status_unless_busy(&status, status_from_error(&err, message));
                }
            }
        });
    }

    /// Starts a new chat, creating a server session when the selected transport supports it.
    #[allow(clippy::too_many_lines)]
    pub fn new_session(&self, title: Option<String>) {
        let config = self.config();
        if self.reject_unavailable_config(&config) {
            return;
        }
        self.cancel_current_for_replacement();
        if self.should_clear_local_chat_for_new_session(&config) {
            self.clear_local_chat(&config);
            return;
        }
        self.select_sequence.fetch_add(1, Ordering::Relaxed);

        let config_id = self.config_sequence.load(Ordering::Relaxed);
        let local_history = config.local_history;
        let status = self.status.clone();
        let sessions_prop = self.sessions.clone();
        let active_prop = self.active_session_id.clone();
        let messages_prop = self.messages.clone();
        let todos_prop = self.todos.clone();
        let subagents_prop = self.subagents.clone();
        let error_prop = self.last_error.clone();
        let store = self.store.clone();
        let session_summary_cache = self.session_summary_cache.clone();
        let transcript_cache = self.transcript_cache.clone();
        let config_sequence = self.config_sequence.clone();
        let selected_dashboard_profile = self.selected_dashboard_profile();
        tokio::spawn(async move {
            status.set(HermesStatus::Connecting);
            if matches!(config.transport_mode, TransportMode::DashboardWs) {
                let result = create_dashboard_session(
                    config.clone(),
                    title.clone(),
                    selected_dashboard_profile,
                )
                .await;
                match result {
                    Ok(session) => {
                        if !config_is_current(&config_sequence, config_id) {
                            return;
                        }
                        let mut sessions = sessions_prop.get();
                        sessions.insert(0, session.clone());
                        cache_session_summaries(&session_summary_cache, sessions.clone());
                        sessions_prop.set(sessions);
                        active_prop.set(Some(session.id.clone()));
                        messages_prop.set(Vec::new());
                        todos_prop.set(Vec::new());
                        subagents_prop.set(Vec::new());
                        cache_transcript(
                            &transcript_cache,
                            session.id.clone(),
                            Vec::new(),
                            session_fingerprint(&session),
                            Vec::new(),
                        );
                        save_local_history(
                            store.as_ref(),
                            local_history,
                            Some(session.id.clone()),
                            &[],
                        );
                        error_prop.set(None);
                        status.set(HermesStatus::Connected);
                    }
                    Err(err) => {
                        if !config_is_current(&config_sequence, config_id) {
                            return;
                        }
                        let message = err.short_message();
                        error_prop.set(Some(message.clone()));
                        status.set(status_from_error(&err, message));
                    }
                }
                return;
            }
            match HermesClient::new(config) {
                Ok(client) => match client.create_session(title.as_deref()).await {
                    Ok(session) => {
                        if !config_is_current(&config_sequence, config_id) {
                            return;
                        }
                        let mut sessions = sessions_prop.get();
                        sessions.insert(0, session.clone());
                        cache_session_summaries(&session_summary_cache, sessions.clone());
                        sessions_prop.set(sessions);
                        active_prop.set(Some(session.id.clone()));
                        messages_prop.set(Vec::new());
                        todos_prop.set(Vec::new());
                        subagents_prop.set(Vec::new());
                        cache_transcript(
                            &transcript_cache,
                            session.id.clone(),
                            Vec::new(),
                            session_fingerprint(&session),
                            Vec::new(),
                        );
                        save_local_history(
                            store.as_ref(),
                            local_history,
                            Some(session.id.clone()),
                            &[],
                        );
                        error_prop.set(None);
                        status.set(HermesStatus::Connected);
                    }
                    Err(err) => {
                        if !config_is_current(&config_sequence, config_id) {
                            return;
                        }
                        let message = err.short_message();
                        error_prop.set(Some(message.clone()));
                        status.set(status_from_error(&err, message));
                    }
                },
                Err(err) => {
                    if !config_is_current(&config_sequence, config_id) {
                        return;
                    }
                    let message = err.short_message();
                    error_prop.set(Some(message.clone()));
                    status.set(status_from_error(&err, message));
                }
            }
        });
    }

    /// Selects a session and loads its server messages while preserving local history on disk.
    #[allow(clippy::too_many_lines)]
    pub fn select_session(&self, session_id: String) {
        let config = self.config();
        if self.reject_unavailable_config(&config) {
            return;
        }
        cache_active_transcript(
            &self.transcript_cache,
            &self.active_session_id,
            &self.sessions,
            &self.messages,
        );
        self.cancel_current_for_replacement();
        let select_id = self.select_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let config_id = self.config_sequence.load(Ordering::Relaxed);
        let sessions_snapshot = self.sessions.get();
        let fingerprint = session_fingerprint_by_id(&sessions_snapshot, &session_id);
        let cached = cached_transcript(&self.transcript_cache, &session_id, fingerprint.as_ref());
        self.active_session_id.set(Some(session_id.clone()));
        self.todos.set(Vec::new());
        self.subagents.set(Vec::new());
        self.background_processes.set(Vec::new());
        if let Some(store) = self.store.as_ref() {
            store.set_active_session_id(Some(session_id.clone()));
        }
        let should_refresh = cached.as_ref().is_none_or(|cached| cached.needs_refresh);
        if let Some(cached) = cached {
            self.messages.set(cached.messages.clone());
            save_local_history(
                self.store.as_ref(),
                config.local_history,
                Some(session_id.clone()),
                &cached.messages,
            );
        } else {
            self.messages.set(Vec::new());
        }
        if !should_refresh {
            self.last_error.set(None);
            return;
        }
        let local_history = config.local_history;
        let active_prop = self.active_session_id.clone();
        let messages = self.messages.clone();
        let status = self.status.clone();
        let error_prop = self.last_error.clone();
        let store = self.store.clone();
        let config_sequence = self.config_sequence.clone();
        let select_sequence = self.select_sequence.clone();
        let transcript_cache = self.transcript_cache.clone();
        tokio::spawn(async move {
            if matches!(config.transport_mode, TransportMode::DashboardWs) {
                select_dashboard_session(SelectSessionContext {
                    config,
                    config_id,
                    select_id,
                    requested_session_id: session_id,
                    fingerprint,
                    local_history,
                    active_session_id: active_prop,
                    messages,
                    status,
                    last_error: error_prop,
                    store,
                    config_sequence,
                    select_sequence,
                    transcript_cache,
                })
                .await;
                return;
            }
            match HermesClient::new(config) {
                Ok(client) => match client.session_messages(&session_id).await {
                    Ok(remote_messages) => {
                        if !selection_is_current(
                            &config_sequence,
                            config_id,
                            &select_sequence,
                            select_id,
                        ) {
                            return;
                        }
                        messages.set(remote_messages.clone());
                        cache_transcript(
                            &transcript_cache,
                            session_id.clone(),
                            Vec::new(),
                            fingerprint,
                            remote_messages.clone(),
                        );
                        save_local_history(
                            store.as_ref(),
                            local_history,
                            Some(session_id.clone()),
                            &remote_messages,
                        );
                        error_prop.set(None);
                    }
                    Err(err) => {
                        if !selection_is_current(
                            &config_sequence,
                            config_id,
                            &select_sequence,
                            select_id,
                        ) {
                            return;
                        }
                        let message = err.short_message();
                        error_prop.set(Some(message.clone()));
                        status.set(status_from_error(&err, message));
                    }
                },
                Err(err) => {
                    if !selection_is_current(
                        &config_sequence,
                        config_id,
                        &select_sequence,
                        select_id,
                    ) {
                        return;
                    }
                    let message = err.short_message();
                    error_prop.set(Some(message.clone()));
                    status.set(status_from_error(&err, message));
                }
            }
        });
    }

    /// Sends a message and waits for the completed answer.
    #[allow(clippy::too_many_lines)]
    pub fn send_message(&self, content: String) {
        let content = content.trim().to_owned();
        if content.is_empty() {
            return;
        }
        let config = self.config();
        if self.reject_unavailable_config(&config) {
            return;
        }
        self.select_sequence.fetch_add(1, Ordering::Relaxed);
        self.cancel_current_for_replacement();
        self.todos.set(Vec::new());
        self.subagents.set(Vec::new());
        self.background_processes.set(Vec::new());
        let token = CancellationToken::new();
        let stream_id = self.stream_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        if let Ok(mut guard) = self.stream_token.lock() {
            *guard = Some(token.clone());
        }
        if let Ok(mut guard) = self.active_stream_id.write() {
            *guard = Some(stream_id);
        }

        let user_id = format!("local-user-{}", chrono::Utc::now().timestamp_millis());
        let assistant_id = format!("local-assistant-{}", chrono::Utc::now().timestamp_millis());
        let mut current = self.messages.get();
        current.push(HermesMessage::new(
            user_id,
            HermesRole::User,
            content.clone(),
        ));
        trim_history(&mut current, config.history_limit);
        self.messages.set(current.clone());
        self.persist(&config, &current);

        let status = self.status.clone();
        let messages_prop = self.messages.clone();
        let active_prop = self.active_session_id.clone();
        let sessions_prop = self.sessions.clone();
        let approval_prop = self.approval.clone();
        let yolo_prop = self.yolo_active.clone();
        let selected_dashboard_profile = self.selected_dashboard_profile();
        let todos_prop = self.todos.clone();
        let subagents_prop = self.subagents.clone();
        let background_processes = self.background_processes.clone();
        let last_error = self.last_error.clone();
        let store = self.store.clone();
        let session_summary_cache = self.session_summary_cache.clone();
        let transcript_cache = self.transcript_cache.clone();
        let active_stream_id = self.active_stream_id.clone();
        let active_run_id = self.active_run_id.clone();
        tokio::spawn(async move {
            status.set(HermesStatus::Busy);
            let result = stream_message(StreamContext {
                config: config.clone(),
                content,
                stream_id,
                assistant_id: assistant_id.clone(),
                token: token.clone(),
                messages: messages_prop.clone(),
                active_session_id: active_prop.clone(),
                sessions: sessions_prop.clone(),
                session_summary_cache: session_summary_cache.clone(),
                approval: approval_prop,
                yolo_active: yolo_prop,
                selected_dashboard_profile,
                todos: todos_prop,
                subagents: subagents_prop,
                active_stream_id: active_stream_id.clone(),
                active_run_id: active_run_id.clone(),
            })
            .await;
            let is_current = stream_is_current(&active_stream_id, stream_id);
            if token.is_cancelled() {
                mark_message(
                    &messages_prop,
                    &assistant_id,
                    MessageStatus::Stopped,
                    None,
                    config.history_limit,
                );
                if is_current {
                    status.set(HermesStatus::Connected);
                }
            } else if let Err(err) = result {
                let message = err.short_message();
                append_error(&messages_prop, message.clone(), config.history_limit);
                if is_current {
                    last_error.set(Some(message.clone()));
                    status.set(status_from_error(&err, message));
                }
            } else {
                mark_message(
                    &messages_prop,
                    &assistant_id,
                    MessageStatus::Complete,
                    None,
                    config.history_limit,
                );
                if is_current {
                    last_error.set(None);
                    status.set(HermesStatus::Connected);
                }
            }
            if is_current {
                save_local_history(
                    store.as_ref(),
                    config.local_history,
                    active_prop.get(),
                    &messages_prop.get(),
                );
                cache_active_transcript(
                    &transcript_cache,
                    &active_prop,
                    &sessions_prop,
                    &messages_prop,
                );
                spawn_refresh_background_processes(
                    config.clone(),
                    active_prop.get(),
                    background_processes,
                );
            }
            clear_stream_if_current(&active_stream_id, &active_run_id, stream_id);
        });
    }

    /// Executes a slash command through the Hermes dashboard gateway when available.
    ///
    /// Non-dashboard transports fall back to sending the command text as a normal
    /// prompt so API-compatible servers still receive user intent.
    pub fn send_slash_command(&self, command_line: String) {
        let command_line = command_line.trim().to_owned();
        if command_line.is_empty() {
            return;
        }
        let config = self.config();
        if self.reject_unavailable_config(&config) {
            return;
        }
        if !matches!(config.transport_mode, TransportMode::DashboardWs) {
            self.send_message(command_line);
            return;
        }

        self.select_sequence.fetch_add(1, Ordering::Relaxed);
        self.cancel_current_for_replacement();
        self.todos.set(Vec::new());
        self.subagents.set(Vec::new());
        self.background_processes.set(Vec::new());
        let token = CancellationToken::new();
        let stream_id = self.stream_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        if let Ok(mut guard) = self.stream_token.lock() {
            *guard = Some(token.clone());
        }
        if let Ok(mut guard) = self.active_stream_id.write() {
            *guard = Some(stream_id);
        }

        let status = self.status.clone();
        let messages_prop = self.messages.clone();
        let active_prop = self.active_session_id.clone();
        let sessions_prop = self.sessions.clone();
        let approval_prop = self.approval.clone();
        let yolo_prop = self.yolo_active.clone();
        let selected_dashboard_profile = self.selected_dashboard_profile();
        let composer_prefill_prop = self.composer_prefill.clone();
        let todos_prop = self.todos.clone();
        let subagents_prop = self.subagents.clone();
        let background_processes = self.background_processes.clone();
        let last_error = self.last_error.clone();
        let store = self.store.clone();
        let session_summary_cache = self.session_summary_cache.clone();
        let transcript_cache = self.transcript_cache.clone();
        let active_stream_id = self.active_stream_id.clone();
        let active_run_id = self.active_run_id.clone();
        tokio::spawn(async move {
            status.set(HermesStatus::Busy);
            let result = run_dashboard_slash_command(SlashContext {
                config: config.clone(),
                command_line,
                stream_id,
                token: token.clone(),
                messages: messages_prop.clone(),
                active_session_id: active_prop.clone(),
                sessions: sessions_prop.clone(),
                session_summary_cache: session_summary_cache.clone(),
                approval: approval_prop,
                yolo_active: yolo_prop,
                selected_dashboard_profile,
                composer_prefill: composer_prefill_prop,
                todos: todos_prop,
                subagents: subagents_prop,
                active_stream_id: active_stream_id.clone(),
                active_run_id: active_run_id.clone(),
            })
            .await;
            let is_current = stream_is_current(&active_stream_id, stream_id);
            if token.is_cancelled() {
                if is_current {
                    status.set(HermesStatus::Connected);
                }
            } else if let Err(err) = result {
                let message = err.short_message();
                append_error(&messages_prop, message.clone(), config.history_limit);
                if is_current {
                    last_error.set(Some(message.clone()));
                    status.set(status_from_error(&err, message));
                }
            } else if is_current {
                last_error.set(None);
                status.set(HermesStatus::Connected);
            }
            if is_current {
                save_local_history(
                    store.as_ref(),
                    config.local_history,
                    active_prop.get(),
                    &messages_prop.get(),
                );
                cache_active_transcript(
                    &transcript_cache,
                    &active_prop,
                    &sessions_prop,
                    &messages_prop,
                );
                spawn_refresh_background_processes(
                    config.clone(),
                    active_prop.get(),
                    background_processes,
                );
            }
            clear_stream_if_current(&active_stream_id, &active_run_id, stream_id);
        });
    }

    /// Renames the active dashboard session through the same `session.title`
    /// RPC used by Hermes Desktop and the TUI.
    ///
    /// Bare `/title` remains a backend slash command; callers should only route
    /// non-empty title arguments here.
    pub fn set_session_title(&self, title: String) {
        let title = title.trim().to_owned();
        if title.is_empty() {
            self.send_slash_command(String::from("/title"));
            return;
        }

        let config = self.config();
        if self.reject_unavailable_config(&config) {
            return;
        }
        if !matches!(config.transport_mode, TransportMode::DashboardWs) {
            self.send_slash_command(format!("/title {title}"));
            return;
        }

        let messages_prop = self.messages.clone();
        let active_prop = self.active_session_id.clone();
        let sessions_prop = self.sessions.clone();
        let last_error = self.last_error.clone();
        let store = self.store.clone();
        let session_summary_cache = self.session_summary_cache.clone();
        let transcript_cache = self.transcript_cache.clone();
        let selected_dashboard_profile = self.selected_dashboard_profile();
        tokio::spawn(async move {
            match set_dashboard_session_title(
                config.clone(),
                title,
                active_prop.clone(),
                sessions_prop.clone(),
                session_summary_cache.clone(),
                selected_dashboard_profile,
            )
            .await
            {
                Ok(message) => {
                    append_system_message(&messages_prop, message, config.history_limit);
                    last_error.set(None);
                }
                Err(err) => {
                    let message = format!("error: {}", err.short_message());
                    append_system_message(&messages_prop, message.clone(), config.history_limit);
                    last_error.set(Some(message));
                }
            }
            save_local_history(
                store.as_ref(),
                config.local_history,
                active_prop.get(),
                &messages_prop.get(),
            );
            cache_active_transcript(
                &transcript_cache,
                &active_prop,
                &sessions_prop,
                &messages_prop,
            );
        });
    }

    /// Toggles per-session YOLO approval bypass through dashboard `config.set`.
    ///
    /// When no dashboard session exists yet, this arms the flag locally and the
    /// next dashboard-created session applies it before the first prompt.
    pub fn toggle_session_yolo(&self) {
        let next = !self.yolo_active.get();
        let config = self.config();
        if self.reject_unavailable_config(&config) {
            return;
        }
        if !matches!(config.transport_mode, TransportMode::DashboardWs) {
            self.append_system_notice("/yolo is available with dashboard-ws transport.");
            return;
        }

        if self
            .active_session_id
            .get()
            .as_deref()
            .is_none_or(str::is_empty)
        {
            self.yolo_active.set(next);
            self.append_system_notice(yolo_armed_message(next));
            return;
        }

        let messages_prop = self.messages.clone();
        let active_prop = self.active_session_id.clone();
        let sessions_prop = self.sessions.clone();
        let yolo_prop = self.yolo_active.clone();
        let last_error = self.last_error.clone();
        let store = self.store.clone();
        let selected_dashboard_profile = self.selected_dashboard_profile();
        tokio::spawn(async move {
            match set_dashboard_session_yolo(
                config.clone(),
                next,
                active_prop.clone(),
                sessions_prop,
                selected_dashboard_profile,
            )
            .await
            {
                Ok(active) => {
                    yolo_prop.set(active);
                    append_system_message(
                        &messages_prop,
                        yolo_session_message(active),
                        config.history_limit,
                    );
                    last_error.set(None);
                }
                Err(err) => {
                    let message = format!("Could not toggle YOLO: {}", err.short_message());
                    append_system_message(&messages_prop, message.clone(), config.history_limit);
                    last_error.set(Some(message));
                }
            }
            save_local_history(
                store.as_ref(),
                config.local_history,
                active_prop.get(),
                &messages_prop.get(),
            );
        });
    }

    /// Branches the latest user/assistant message into a new dashboard session.
    pub fn branch_current_session(&self) {
        let config = self.config();
        if self.reject_unavailable_config(&config) {
            return;
        }
        if !matches!(config.transport_mode, TransportMode::DashboardWs) {
            self.append_system_notice("/branch is available with dashboard-ws transport.");
            return;
        }
        if self.active_session_id.get().is_none() {
            self.append_system_notice("Start or resume a chat before branching.");
            return;
        }
        if matches!(self.status.get(), HermesStatus::Busy) {
            self.append_system_notice("Stop the current turn before branching this chat.");
            return;
        }
        self.select_sequence.fetch_add(1, Ordering::Relaxed);

        let Some(seed) = branch_seed_message(&self.messages.get()) else {
            self.append_system_notice("This message has no text to branch from.");
            return;
        };
        let branch_messages = branch_seed_to_messages(&seed);

        let status = self.status.clone();
        let sessions_prop = self.sessions.clone();
        let active_prop = self.active_session_id.clone();
        let messages_prop = self.messages.clone();
        let todos_prop = self.todos.clone();
        let subagents_prop = self.subagents.clone();
        let background_processes_prop = self.background_processes.clone();
        let slash_suggestions_prop = self.slash_suggestions.clone();
        let last_error = self.last_error.clone();
        let store = self.store.clone();
        let session_summary_cache = self.session_summary_cache.clone();
        let transcript_cache = self.transcript_cache.clone();
        let selected_dashboard_profile = self.selected_dashboard_profile();
        tokio::spawn(async move {
            status.set(HermesStatus::Connecting);
            match create_dashboard_branch_session(config.clone(), seed, selected_dashboard_profile)
                .await
            {
                Ok(session) => {
                    let mut sessions = sessions_prop.get();
                    sessions.insert(0, session.clone());
                    cache_session_summaries(&session_summary_cache, sessions.clone());
                    sessions_prop.set(sessions);
                    active_prop.set(Some(session.id.clone()));
                    messages_prop.set(branch_messages.clone());
                    todos_prop.set(Vec::new());
                    subagents_prop.set(Vec::new());
                    background_processes_prop.set(Vec::new());
                    slash_suggestions_prop.set(Vec::new());
                    last_error.set(None);
                    status.set(HermesStatus::Connected);
                    cache_transcript(
                        &transcript_cache,
                        session.id.clone(),
                        Vec::new(),
                        session_fingerprint(&session),
                        branch_messages.clone(),
                    );
                    save_local_history(
                        store.as_ref(),
                        config.local_history,
                        Some(session.id),
                        &branch_messages,
                    );
                }
                Err(err) => {
                    let message = err.short_message();
                    append_error(&messages_prop, message.clone(), config.history_limit);
                    last_error.set(Some(message.clone()));
                    status.set(status_from_error(&err, message));
                }
            }
        });
    }

    /// Manages the gateway-host browser CDP connection through dashboard `browser.manage`.
    pub fn manage_browser(&self, args: String) {
        let config = self.config();
        if self.reject_unavailable_config(&config) {
            return;
        }
        if !matches!(config.transport_mode, TransportMode::DashboardWs) {
            self.append_system_notice("/browser is available with dashboard-ws transport.");
            return;
        }
        if !dashboard_endpoint_is_loopback(&config.endpoint_url) {
            self.append_system_notice(
                "/browser manages a Chromium-family browser on the gateway host - only available when connected to a local gateway.",
            );
            return;
        }

        let request = match parse_browser_manage_args(&args) {
            Ok(request) => request,
            Err(message) => {
                self.append_system_notice(message);
                return;
            }
        };

        let messages_prop = self.messages.clone();
        let active_prop = self.active_session_id.clone();
        let sessions_prop = self.sessions.clone();
        let last_error = self.last_error.clone();
        let store = self.store.clone();
        let selected_dashboard_profile = self.selected_dashboard_profile();
        tokio::spawn(async move {
            let result = request_dashboard_browser_manage(
                config.clone(),
                active_prop.clone(),
                sessions_prop,
                &request,
                selected_dashboard_profile,
            )
            .await;
            let content = match result {
                Ok(value) => {
                    last_error.set(None);
                    browser_manage_output(&request, &value)
                }
                Err(err) => {
                    let message = err.short_message();
                    last_error.set(Some(message.clone()));
                    format!("error: {message}")
                }
            };
            if let Some(messages) =
                append_system_notice_to(&messages_prop, content, config.history_limit)
            {
                save_local_history(
                    store.as_ref(),
                    config.local_history,
                    active_prop.get(),
                    &messages,
                );
            }
        });
    }

    /// Hands the active dashboard session to a messaging platform.
    pub fn handoff_session(&self, platform: String) {
        let platform = platform.trim().to_ascii_lowercase();
        if platform.is_empty() {
            self.append_system_notice("Choose a destination");
            return;
        }

        let config = self.config();
        if self.reject_unavailable_config(&config) {
            return;
        }
        if !matches!(config.transport_mode, TransportMode::DashboardWs) {
            self.append_system_notice("/handoff is available with dashboard-ws transport.");
            return;
        }
        let Some(session_id) = self
            .active_session_id
            .get()
            .filter(|session_id| !session_id.trim().is_empty())
        else {
            self.append_system_notice("Could not create a new session");
            return;
        };

        let messages_prop = self.messages.clone();
        let active_prop = self.active_session_id.clone();
        let last_error = self.last_error.clone();
        let store = self.store.clone();
        tokio::spawn(async move {
            let result =
                request_dashboard_handoff(config.clone(), session_id.clone(), platform.clone())
                    .await;
            let content = match result {
                Ok(()) => {
                    last_error.set(None);
                    handoff_success_message(&platform)
                }
                Err(err) => {
                    let message = err.short_message();
                    let content = handoff_failed_message(&message);
                    last_error.set(Some(content.clone()));
                    content
                }
            };
            if let Some(messages) =
                append_system_notice_to(&messages_prop, content, config.history_limit)
            {
                save_local_history(
                    store.as_ref(),
                    config.local_history,
                    active_prop.get(),
                    &messages,
                );
            }
        });
    }

    /// Shows the available slash commands, using the dashboard catalog when available.
    pub fn show_slash_commands(&self, fallback: impl Into<String>) {
        let fallback = fallback.into();
        let config = self.config();
        if self.reject_unavailable_config(&config) {
            return;
        }
        if !matches!(config.transport_mode, TransportMode::DashboardWs) {
            self.append_system_notice(fallback);
            return;
        }

        let messages_prop = self.messages.clone();
        let store = self.store.clone();
        let active_prop = self.active_session_id.clone();
        let last_error = self.last_error.clone();
        tokio::spawn(async move {
            let token = CancellationToken::new();
            let result = async {
                let client = DashboardClient::new(config.clone()).await?;
                client
                    .request_once("commands.catalog", json!({}), &token)
                    .await
            }
            .await;
            let content = match result {
                Ok(catalog) => {
                    last_error.set(None);
                    slash_commands_catalog_text(&catalog).unwrap_or_else(|| fallback.clone())
                }
                Err(err) => {
                    let message = err.short_message();
                    last_error.set(Some(message.clone()));
                    format!(
                        "Could not load Hermes command catalog: {message}\n\n{}",
                        fallback
                    )
                }
            };
            if let Some(messages) =
                append_system_notice_to(&messages_prop, content, config.history_limit)
            {
                save_local_history(
                    store.as_ref(),
                    config.local_history,
                    active_prop.get(),
                    &messages,
                );
            }
        });
    }

    /// Lists profiles or selects the Hermes profile used for future dashboard chats.
    pub fn show_profiles(&self, arg: impl Into<String>) {
        let arg = arg.into();
        let config = self.config();
        if self.reject_unavailable_config(&config) {
            return;
        }
        if !matches!(config.transport_mode, TransportMode::DashboardWs) {
            self.append_system_notice("/profile is available with dashboard-ws transport.");
            return;
        }

        let messages_prop = self.messages.clone();
        let store = self.store.clone();
        let active_prop = self.active_session_id.clone();
        let last_error = self.last_error.clone();
        let selected_dashboard_profile = self.selected_dashboard_profile.clone();
        tokio::spawn(async move {
            let current_selection = selected_dashboard_profile
                .read()
                .ok()
                .and_then(|profile| profile.clone());
            let result = async {
                let client = DashboardClient::new(config.clone()).await?;
                let profiles = client.profiles().await?;
                let active = client.active_profile().await.ok();
                Ok::<_, Error>(profile_command_result(
                    &profiles,
                    active.as_ref(),
                    current_selection.as_deref(),
                    &arg,
                ))
            }
            .await;
            let content = match result {
                Ok(result) => {
                    if let Some(profile) = result.selected_profile
                        && let Ok(mut guard) = selected_dashboard_profile.write()
                    {
                        *guard = Some(profile);
                    }
                    last_error.set(None);
                    result.content
                }
                Err(err) => {
                    let message = err.short_message();
                    last_error.set(Some(message.clone()));
                    format!("Could not load Hermes profiles: {message}")
                }
            };
            if let Some(messages) =
                append_system_notice_to(&messages_prop, content, config.history_limit)
            {
                save_local_history(
                    store.as_ref(),
                    config.local_history,
                    active_prop.get(),
                    &messages,
                );
            }
        });
    }

    /// Refreshes live slash command suggestions for the current composer text.
    pub fn refresh_slash_suggestions(&self, input: String) {
        let suggestions_id = self
            .slash_suggestion_sequence
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        let input = input.trim_start().to_owned();
        if !should_show_slash_suggestions(&input) {
            self.slash_suggestions.set(Vec::new());
            return;
        }

        if let Some(suggestions) = session_slash_suggestions(&input, &self.sessions.get()) {
            self.slash_suggestions.set(suggestions);
            return;
        }

        let fallback = local_slash_suggestions(&input, &self.sessions.get());
        self.slash_suggestions.set(fallback.clone());

        let config = self.config();
        if !matches!(config.transport_mode, TransportMode::DashboardWs) {
            return;
        }

        let suggestions_prop = self.slash_suggestions.clone();
        let sequence = self.slash_suggestion_sequence.clone();
        tokio::spawn(async move {
            let token = CancellationToken::new();
            let result = async {
                let client = DashboardClient::new(config.clone()).await?;
                if input == "/" {
                    client
                        .request_once("commands.catalog", json!({}), &token)
                        .await
                        .map(|value| catalog_slash_suggestions(&value, ""))
                } else {
                    client
                        .request_once("complete.slash", json!({ "text": input }), &token)
                        .await
                        .map(|value| complete_slash_suggestions(&value, &input))
                }
            }
            .await;

            if sequence.load(Ordering::Relaxed) != suggestions_id {
                return;
            }

            if let Ok(suggestions) = result
                && !suggestions.is_empty()
            {
                suggestions_prop.set(suggestions);
            }
        });
    }

    /// Clears live slash command suggestions.
    pub fn clear_slash_suggestions(&self) {
        self.slash_suggestion_sequence
            .fetch_add(1, Ordering::Relaxed);
        self.slash_suggestions.set(Vec::new());
    }

    /// Refreshes dashboard background process rows for the active session.
    pub fn refresh_background_processes(&self) {
        let config = self.config();
        let session_id = self.active_session_id.get();
        let background_processes = self.background_processes.clone();
        spawn_refresh_background_processes(config, session_id, background_processes);
    }

    /// Removes a background process row from the local status stack.
    pub fn dismiss_background_process(&self, process_id: &str) {
        remove_background_process(&self.background_processes, process_id);
    }

    /// Stops a running dashboard background process and removes it from the local status stack.
    pub fn stop_background_process(&self, process_id: &str) {
        let process_id = process_id.trim().to_owned();
        if process_id.is_empty() {
            return;
        }
        let config = self.config();
        let session_id = self.active_session_id.get();
        remove_background_process(&self.background_processes, &process_id);
        if !matches!(config.transport_mode, TransportMode::DashboardWs) {
            return;
        }
        let Some(session_id) = session_id.filter(|session_id| !session_id.trim().is_empty()) else {
            return;
        };
        tokio::spawn(async move {
            let token = CancellationToken::new();
            let result = async {
                let client = DashboardClient::new(config).await?;
                client
                    .request_once(
                        "process.kill",
                        json!({
                            "process_id": process_id,
                            "session_id": session_id,
                        }),
                        &token,
                    )
                    .await
                    .map(|_| ())
            }
            .await;
            if let Err(err) = result {
                debug!(error = %err.short_message(), "could not stop Hermes background process");
            }
        });
    }

    /// Appends a local system notice to the transcript without contacting Hermes.
    pub fn append_system_notice(&self, content: impl Into<String>) {
        let config = self.config();
        if let Some(messages) =
            append_system_notice_to(&self.messages, content, config.history_limit)
        {
            self.persist(&config, &messages);
        }
    }

    /// Cancels the active stream and asks Hermes to stop any tracked run.
    pub fn stop_current(&self) {
        self.stop_current_with_config(self.config());
    }

    fn stop_current_with_config(&self, config: ConnectionConfig) {
        if let Ok(mut guard) = self.stream_token.lock()
            && let Some(token) = guard.take()
        {
            token.cancel();
        }
        let run_id = self
            .active_run_id
            .write()
            .ok()
            .and_then(|mut guard| guard.take());
        if let Some(run_id) = run_id {
            spawn_stop_run(config, run_id);
        }
    }

    fn cancel_current_for_replacement(&self) {
        if let Ok(mut guard) = self.active_stream_id.write() {
            *guard = None;
        }
        self.stop_current_with_config(self.config());
    }

    fn cancel_current_for_config_change(&self, previous_config: ConnectionConfig) {
        if let Ok(mut guard) = self.active_stream_id.write() {
            *guard = None;
        }
        self.approval.set(None);
        self.yolo_active.set(false);
        self.composer_prefill.set(None);
        self.todos.set(Vec::new());
        self.subagents.set(Vec::new());
        self.background_processes.set(Vec::new());
        self.slash_suggestions.set(Vec::new());
        self.stop_current_with_config(previous_config);
    }

    fn clear_remote_state(&self) {
        self.select_sequence.fetch_add(1, Ordering::Relaxed);
        clear_session_caches(&self.session_summary_cache, &self.transcript_cache);
        self.capabilities.set(None);
        self.sessions.set(Vec::new());
        self.active_session_id.set(None);
        if let Some(store) = self.store.as_ref() {
            store.set_active_session_id(None);
        }
        self.approval.set(None);
        self.yolo_active.set(false);
        self.composer_prefill.set(None);
        self.todos.set(Vec::new());
        self.subagents.set(Vec::new());
        self.background_processes.set(Vec::new());
        self.slash_suggestions.set(Vec::new());
    }

    fn clear_local_chat(&self, config: &ConnectionConfig) {
        self.select_sequence.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut cache) = self.transcript_cache.lock() {
            cache.clear();
        }
        self.active_session_id.set(None);
        if let Some(store) = self.store.as_ref() {
            store.set_active_session_id(None);
        }
        self.messages.set(Vec::new());
        self.approval.set(None);
        self.yolo_active.set(false);
        self.composer_prefill.set(None);
        self.todos.set(Vec::new());
        self.subagents.set(Vec::new());
        self.background_processes.set(Vec::new());
        self.slash_suggestions.set(Vec::new());
        set_status_after_local_chat_clear(&self.status, &self.last_error);
        save_local_history(self.store.as_ref(), config.local_history, None, &[]);
    }

    fn reject_unavailable_config(&self, config: &ConnectionConfig) -> bool {
        if let Some((status, last_error)) = unavailable_config_state(config) {
            self.clear_remote_state();
            self.status.set(status);
            self.last_error.set(last_error);
            true
        } else {
            false
        }
    }

    fn should_clear_local_chat_for_new_session(&self, config: &ConnectionConfig) -> bool {
        local_new_chat_transport(config.transport_mode)
            || (matches!(config.transport_mode, TransportMode::Auto)
                && connected_via_chat_completions(&self.capabilities))
    }

    /// Responds to a pending approval request.
    #[allow(clippy::too_many_lines)]
    pub fn submit_approval(&self, approved: bool, message: Option<String>) {
        let config = self.config();
        if self.reject_unavailable_config(&config) {
            return;
        }
        let Some(approval) = self.approval.get() else {
            return;
        };
        let approval_prop = self.approval.clone();
        let last_error = self.last_error.clone();
        if approval.run_id.trim().is_empty() {
            approval_prop.set(None);
            last_error.set(Some(String::from("Missing approval run id")));
            return;
        }
        let config_id = self.config_sequence.load(Ordering::Relaxed);
        let config_sequence = self.config_sequence.clone();
        tokio::spawn(async move {
            if matches!(config.transport_mode, TransportMode::DashboardWs) {
                let result = async {
                    let client = DashboardClient::new(config).await?;
                    let token = CancellationToken::new();
                    let (method, params) = match approval.kind {
                        ApprovalKind::Approval => (
                            "approval.respond",
                            json!({
                                "choice": if approved { "once" } else { "deny" },
                                "session_id": approval.run_id,
                            }),
                        ),
                        ApprovalKind::Clarification => {
                            let request_id = approval.approval_id.as_deref().ok_or_else(|| {
                                Error::UnsupportedEvent(String::from("clarify missing request id"))
                            })?;
                            (
                                "clarify.respond",
                                json!({
                                    "request_id": request_id,
                                    "answer": if approved {
                                        message.as_deref().unwrap_or_default()
                                    } else {
                                        ""
                                    },
                                }),
                            )
                        }
                        ApprovalKind::Sudo => {
                            let request_id = approval.approval_id.as_deref().ok_or_else(|| {
                                Error::UnsupportedEvent(String::from("sudo missing request id"))
                            })?;
                            (
                                "sudo.respond",
                                json!({
                                    "request_id": request_id,
                                    "password": if approved {
                                        message.as_deref().unwrap_or_default()
                                    } else {
                                        ""
                                    },
                                }),
                            )
                        }
                        ApprovalKind::Secret => {
                            let request_id = approval.approval_id.as_deref().ok_or_else(|| {
                                Error::UnsupportedEvent(String::from("secret missing request id"))
                            })?;
                            (
                                "secret.respond",
                                json!({
                                    "request_id": request_id,
                                    "value": if approved {
                                        message.as_deref().unwrap_or_default()
                                    } else {
                                        ""
                                    },
                                }),
                            )
                        }
                    };
                    client
                        .request_once(method, params, &token)
                        .await
                        .map(|_| ())
                }
                .await;
                match result {
                    Ok(()) => {
                        if config_is_current(&config_sequence, config_id) {
                            approval_prop.set(None);
                        }
                    }
                    Err(err) => {
                        if config_is_current(&config_sequence, config_id) {
                            last_error.set(Some(err.short_message()));
                        }
                    }
                }
                return;
            }
            match HermesClient::new(config) {
                Ok(client) => match client
                    .submit_approval(
                        &approval.run_id,
                        approval.approval_id.as_deref(),
                        approved,
                        message.as_deref(),
                    )
                    .await
                {
                    Ok(()) => {
                        if config_is_current(&config_sequence, config_id) {
                            approval_prop.set(None);
                        }
                    }
                    Err(err) => {
                        if config_is_current(&config_sequence, config_id) {
                            last_error.set(Some(err.short_message()));
                        }
                    }
                },
                Err(err) => {
                    if config_is_current(&config_sequence, config_id) {
                        last_error.set(Some(err.short_message()));
                    }
                }
            }
        });
    }

    fn config(&self) -> ConnectionConfig {
        let mut config = self
            .config
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        apply_discovered_transport(&mut config, self.capabilities.get().as_deref());
        config
    }

    fn selected_dashboard_profile(&self) -> Option<String> {
        self.selected_dashboard_profile
            .read()
            .ok()
            .and_then(|profile| profile.clone())
            .filter(|profile| !profile.trim().is_empty())
    }

    fn persist(&self, config: &ConnectionConfig, messages: &[HermesMessage]) {
        save_local_history(
            self.store.as_ref(),
            config.local_history,
            self.active_session_id.get(),
            messages,
        );
        if let Some(session_id) = self.active_session_id.get() {
            let fingerprint = session_fingerprint_by_id(&self.sessions.get(), &session_id);
            cache_transcript(
                &self.transcript_cache,
                session_id,
                Vec::new(),
                fingerprint,
                messages.to_vec(),
            );
        }
    }
}

fn save_local_history(
    store: Option<&Arc<LocalHistoryStore>>,
    local_history: LocalHistoryMode,
    active_session_id: Option<String>,
    messages: &[HermesMessage],
) {
    if local_history != LocalHistoryMode::Full {
        return;
    }
    if let Some(store) = store {
        store.set_active_session_id(active_session_id);
        if let Err(err) = store.save(messages) {
            warn!(error = %err, "could not save Hermes local history");
        }
    }
}

impl SessionSummaryCache {
    fn fresh_sessions(&self) -> Option<Vec<HermesSessionSummary>> {
        self.fetched_at
            .filter(|fetched_at| fetched_at.elapsed() <= SESSION_SUMMARY_CACHE_TTL)
            .map(|_| self.sessions.clone())
    }

    fn replace(&mut self, sessions: Vec<HermesSessionSummary>) {
        self.sessions = sessions;
        self.fetched_at = Some(Instant::now());
    }

    fn clear(&mut self) {
        self.sessions.clear();
        self.fetched_at = None;
    }
}

#[derive(Debug, Clone)]
struct CachedTranscript {
    messages: Vec<HermesMessage>,
    needs_refresh: bool,
}

impl TranscriptCache {
    fn lookup(
        &mut self,
        session_id: &str,
        fingerprint: Option<&SessionFingerprint>,
    ) -> Option<CachedTranscript> {
        let entry = self.entries.get(session_id).cloned()?;
        self.touch(session_id);
        let needs_refresh = transcript_entry_needs_refresh(&entry, fingerprint);
        Some(CachedTranscript {
            messages: entry.messages,
            needs_refresh,
        })
    }

    fn insert(
        &mut self,
        session_id: String,
        aliases: impl IntoIterator<Item = String>,
        fingerprint: Option<SessionFingerprint>,
        messages: Vec<HermesMessage>,
    ) {
        let entry = TranscriptCacheEntry {
            messages,
            fingerprint,
            fetched_at: Instant::now(),
        };
        self.upsert(session_id, entry.clone());
        for alias in aliases {
            self.upsert(alias, entry.clone());
        }
        self.evict_oldest();
    }

    fn upsert(&mut self, session_id: String, entry: TranscriptCacheEntry) {
        if session_id.trim().is_empty() {
            return;
        }
        self.entries.insert(session_id.clone(), entry);
        self.touch(&session_id);
    }

    fn touch(&mut self, session_id: &str) {
        self.order.retain(|current| current != session_id);
        self.order.push_back(session_id.to_owned());
    }

    fn evict_oldest(&mut self) {
        while self.entries.len() > TRANSCRIPT_CACHE_MAX_SESSIONS {
            let Some(session_id) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&session_id);
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
}

fn transcript_entry_needs_refresh(
    entry: &TranscriptCacheEntry,
    fingerprint: Option<&SessionFingerprint>,
) -> bool {
    if entry.fetched_at.elapsed() > TRANSCRIPT_CACHE_TTL {
        return true;
    }
    fingerprint.is_some_and(|fingerprint| entry.fingerprint.as_ref() != Some(fingerprint))
}

fn cached_session_summaries(
    cache: &SharedSessionSummaryCache,
) -> Option<Vec<HermesSessionSummary>> {
    cache.lock().ok().and_then(|cache| cache.fresh_sessions())
}

fn cache_session_summaries(cache: &SharedSessionSummaryCache, sessions: Vec<HermesSessionSummary>) {
    if let Ok(mut cache) = cache.lock() {
        cache.replace(sessions);
    }
}

fn clear_session_caches(
    session_summary_cache: &SharedSessionSummaryCache,
    transcript_cache: &SharedTranscriptCache,
) {
    if let Ok(mut cache) = session_summary_cache.lock() {
        cache.clear();
    }
    if let Ok(mut cache) = transcript_cache.lock() {
        cache.clear();
    }
}

fn session_fingerprint(session: &HermesSessionSummary) -> Option<SessionFingerprint> {
    if session.updated_at.is_none() && session.message_count.is_none() {
        return None;
    }
    Some(SessionFingerprint {
        updated_at: session.updated_at,
        message_count: session.message_count,
    })
}

fn session_fingerprint_by_id(
    sessions: &[HermesSessionSummary],
    session_id: &str,
) -> Option<SessionFingerprint> {
    sessions
        .iter()
        .find(|session| session.id == session_id)
        .and_then(session_fingerprint)
}

fn cache_transcript(
    cache: &SharedTranscriptCache,
    session_id: String,
    aliases: impl IntoIterator<Item = String>,
    fingerprint: Option<SessionFingerprint>,
    messages: Vec<HermesMessage>,
) {
    if let Ok(mut cache) = cache.lock() {
        cache.insert(session_id, aliases, fingerprint, messages);
    }
}

fn cached_transcript(
    cache: &SharedTranscriptCache,
    session_id: &str,
    fingerprint: Option<&SessionFingerprint>,
) -> Option<CachedTranscript> {
    cache
        .lock()
        .ok()
        .and_then(|mut cache| cache.lookup(session_id, fingerprint))
}

fn cache_active_transcript(
    cache: &SharedTranscriptCache,
    active_session_id: &Property<Option<String>>,
    sessions: &Property<Vec<HermesSessionSummary>>,
    messages: &Property<Vec<HermesMessage>>,
) {
    let Some(session_id) = active_session_id
        .get()
        .filter(|session_id| !session_id.trim().is_empty())
    else {
        return;
    };
    let summaries = sessions.get();
    let fingerprint = session_fingerprint_by_id(&summaries, &session_id);
    cache_transcript(cache, session_id, Vec::new(), fingerprint, messages.get());
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BranchSeedMessage {
    role: HermesRole,
    content: String,
}

impl BranchSeedMessage {
    fn as_dashboard_message(&self) -> HermesMessage {
        HermesMessage::new(
            format!(
                "local-branch-{}-{}",
                self.role.as_dashboard_str(),
                chrono::Utc::now().timestamp_millis()
            ),
            self.role,
            self.content.clone(),
        )
    }
}

trait DashboardRoleExt {
    fn as_dashboard_str(&self) -> &'static str;
}

impl DashboardRoleExt for HermesRole {
    fn as_dashboard_str(&self) -> &'static str {
        match self {
            HermesRole::User => "user",
            HermesRole::Assistant => "assistant",
            HermesRole::System => "system",
            HermesRole::Tool => "tool",
            HermesRole::Error => "error",
        }
    }
}

fn branch_seed_message(messages: &[HermesMessage]) -> Option<BranchSeedMessage> {
    messages
        .iter()
        .rev()
        .find(|message| matches!(message.role, HermesRole::User | HermesRole::Assistant))
        .and_then(|message| {
            let content = message.content.trim();
            if content.is_empty() {
                return None;
            }
            Some(BranchSeedMessage {
                role: message.role,
                content: content.to_owned(),
            })
        })
}

fn branch_seed_to_messages(seed: &BranchSeedMessage) -> Vec<HermesMessage> {
    vec![seed.as_dashboard_message()]
}

fn spawn_refresh_background_processes(
    config: ConnectionConfig,
    session_id: Option<String>,
    background_processes: Property<Vec<BackgroundProcessItem>>,
) {
    if !matches!(config.transport_mode, TransportMode::DashboardWs) {
        background_processes.set(Vec::new());
        return;
    }
    let Some(session_id) = session_id.filter(|session_id| !session_id.trim().is_empty()) else {
        background_processes.set(Vec::new());
        return;
    };
    tokio::spawn(async move {
        let token = CancellationToken::new();
        let result = async {
            let client = DashboardClient::new(config).await?;
            client
                .request_once("process.list", json!({ "session_id": session_id }), &token)
                .await
        }
        .await;
        if let Ok(value) = result {
            background_processes.set(parse_background_processes(&value));
        }
    });
}

fn parse_background_processes(value: &Value) -> Vec<BackgroundProcessItem> {
    value
        .get("processes")
        .or_else(|| value.get("items"))
        .or_else(|| value.as_array().map(|_| value))
        .and_then(Value::as_array)
        .map(|processes| {
            processes
                .iter()
                .filter_map(parse_background_process)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_background_process(value: &Value) -> Option<BackgroundProcessItem> {
    let id = first_string(value, &["session_id", "process_id", "id"])?;
    let command = first_string(value, &["command", "title", "name"])
        .unwrap_or_else(|| String::from("background process"));
    let title = command
        .lines()
        .next()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or("background process")
        .to_owned();
    let status_text = first_string(value, &["status", "state"]).unwrap_or_default();
    let exit_code = value
        .get("exit_code")
        .or_else(|| value.get("exitCode"))
        .and_then(Value::as_i64);
    let status = match status_text.to_ascii_lowercase().as_str() {
        "exited" | "done" | "complete" | "completed" | "success" | "succeeded" => {
            if exit_code.unwrap_or(0) == 0 {
                BackgroundProcessStatus::Completed
            } else {
                BackgroundProcessStatus::Failed
            }
        }
        "failed" | "failure" | "error" => BackgroundProcessStatus::Failed,
        _ => BackgroundProcessStatus::Running,
    };
    Some(BackgroundProcessItem {
        id,
        title,
        status,
        exit_code,
        output: first_string(value, &["output_tail", "output", "tail"]),
    })
}

fn remove_background_process(
    background_processes: &Property<Vec<BackgroundProcessItem>>,
    process_id: &str,
) {
    let process_id = process_id.trim();
    if process_id.is_empty() {
        return;
    }
    let current = background_processes.get();
    let next = current
        .iter()
        .filter(|process| process.id != process_id)
        .cloned()
        .collect::<Vec<_>>();
    if next.len() != current.len() {
        background_processes.set(next);
    }
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| value.get(*key)?.as_str())
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_owned)
}

fn config_has_missing_api_key(config: &ConnectionConfig) -> bool {
    let Some(key) = config.api_key.as_deref() else {
        return true;
    };
    key.is_empty() || key.starts_with('$')
}

fn unavailable_config_state(config: &ConnectionConfig) -> Option<(HermesStatus, Option<String>)> {
    if !config.enabled {
        return Some((HermesStatus::Disabled, None));
    }
    if !matches!(
        config.transport_mode,
        TransportMode::Auto | TransportMode::DashboardWs
    ) && config_has_missing_api_key(config)
    {
        return Some((
            HermesStatus::MissingApiKey,
            Some(String::from("Missing API key")),
        ));
    }
    None
}

fn stream_config_requires_cancel(previous: &ConnectionConfig, next: &ConnectionConfig) -> bool {
    previous.enabled != next.enabled
        || previous.endpoint_url != next.endpoint_url
        || previous.api_key != next.api_key
        || previous.dashboard_token != next.dashboard_token
        || previous.model != next.model
        || previous.session_key != next.session_key
        || previous.transport_mode != next.transport_mode
        || previous.local_history != next.local_history
}

fn remote_state_requires_reset(previous: &ConnectionConfig, next: &ConnectionConfig) -> bool {
    previous.enabled != next.enabled
        || previous.endpoint_url != next.endpoint_url
        || previous.api_key != next.api_key
        || previous.dashboard_token != next.dashboard_token
        || previous.session_key != next.session_key
        || previous.transport_mode != next.transport_mode
}

fn local_new_chat_transport(transport_mode: TransportMode) -> bool {
    matches!(
        transport_mode,
        TransportMode::Runs | TransportMode::ChatCompletions | TransportMode::DashboardWs
    )
}

fn auto_select_first_session_on_connect(transport_mode: TransportMode) -> bool {
    !matches!(transport_mode, TransportMode::DashboardWs)
}

fn connected_via_chat_completions(capabilities: &Property<Option<Arc<Value>>>) -> bool {
    capabilities
        .get()
        .as_ref()
        .and_then(|value| value.get("transport_mode"))
        .and_then(Value::as_str)
        == Some("chat-completions")
}

fn apply_discovered_transport(config: &mut ConnectionConfig, capabilities: Option<&Value>) {
    if config.transport_mode != TransportMode::Auto {
        return;
    }
    match capabilities
        .and_then(|value| value.get("transport_mode"))
        .and_then(Value::as_str)
    {
        Some("dashboard-ws") => config.transport_mode = TransportMode::DashboardWs,
        Some("chat-completions") => config.transport_mode = TransportMode::ChatCompletions,
        _ => {}
    }
}

fn spawn_stop_run(config: ConnectionConfig, run_id: String) {
    tokio::spawn(async move {
        if matches!(config.transport_mode, TransportMode::DashboardWs) {
            match DashboardClient::new(config).await {
                Ok(client) => {
                    let token = CancellationToken::new();
                    if let Err(err) = client
                        .request_once("session.interrupt", json!({"session_id": run_id}), &token)
                        .await
                    {
                        debug!(error = %err, "Hermes dashboard interrupt failed");
                    }
                }
                Err(err) => debug!(error = %err, "Hermes dashboard interrupt client build failed"),
            }
            return;
        }
        match HermesClient::new(config) {
            Ok(client) => {
                if let Err(err) = client.stop_run(&run_id).await {
                    debug!(error = %err, "Hermes run stop failed");
                }
            }
            Err(err) => debug!(error = %err, "Hermes run stop client build failed"),
        }
    });
}

fn connect_is_current(connect_sequence: &Arc<AtomicU64>, connect_id: u64) -> bool {
    connect_sequence.load(Ordering::Relaxed) == connect_id
}

fn config_is_current(config_sequence: &Arc<AtomicU64>, config_id: u64) -> bool {
    config_sequence.load(Ordering::Relaxed) == config_id
}

fn selection_is_current(
    config_sequence: &Arc<AtomicU64>,
    config_id: u64,
    select_sequence: &Arc<AtomicU64>,
    select_id: u64,
) -> bool {
    config_is_current(config_sequence, config_id)
        && select_sequence.load(Ordering::Relaxed) == select_id
}

fn set_status_unless_busy(status: &Property<HermesStatus>, next: HermesStatus) {
    if !matches!(status.get(), HermesStatus::Busy) {
        status.set(next);
    }
}

fn set_status_after_local_chat_clear(
    status: &Property<HermesStatus>,
    last_error: &Property<Option<String>>,
) {
    if matches!(status.get(), HermesStatus::Connected | HermesStatus::Busy) {
        last_error.set(None);
        status.set(HermesStatus::Connected);
    }
}

fn stream_is_current(active_stream_id: &Arc<RwLock<Option<u64>>>, stream_id: u64) -> bool {
    active_stream_id
        .read()
        .is_ok_and(|guard| guard.as_ref().is_some_and(|active| *active == stream_id))
}

fn set_session_if_current(
    active_stream_id: &Arc<RwLock<Option<u64>>>,
    active_session_id: &Property<Option<String>>,
    sessions: &Property<Vec<HermesSessionSummary>>,
    stream_id: u64,
    session: HermesSessionSummary,
) -> Option<String> {
    if !stream_is_current(active_stream_id, stream_id) {
        return None;
    }
    let session_id = session.id.clone();
    active_session_id.set(Some(session_id.clone()));
    let mut current_sessions = sessions.get();
    current_sessions.insert(0, session);
    sessions.set(current_sessions);
    Some(session_id)
}

fn set_run_if_current(
    active_stream_id: &Arc<RwLock<Option<u64>>>,
    active_run_id: &Arc<RwLock<Option<String>>>,
    stream_id: u64,
    run_id: String,
) {
    if stream_is_current(active_stream_id, stream_id)
        && let Ok(mut guard) = active_run_id.write()
    {
        *guard = Some(run_id);
    }
}

fn clear_stream_if_current(
    active_stream_id: &Arc<RwLock<Option<u64>>>,
    active_run_id: &Arc<RwLock<Option<String>>>,
    stream_id: u64,
) {
    if let Ok(mut guard) = active_stream_id.write()
        && *guard == Some(stream_id)
    {
        *guard = None;
        if let Ok(mut run_guard) = active_run_id.write() {
            *run_guard = None;
        }
    }
}

async fn connect_inner(config: ConnectionConfig) -> Result<(Value, Vec<HermesSessionSummary>)> {
    let transport_mode = config.transport_mode;
    if matches!(transport_mode, TransportMode::DashboardWs) {
        return connect_dashboard_inner(config).await;
    }
    let raw_config = config.clone();
    let client = HermesClient::new(config)?;
    match transport_mode {
        TransportMode::ChatCompletions => connect_chat_completions_inner(&client).await,
        TransportMode::Auto => match connect_dashboard_inner(raw_config).await {
            Ok(result) => Ok(result),
            Err(err) => {
                debug!(
                    error = %err.short_message(),
                    "Hermes dashboard auto discovery failed; falling back to API transports"
                );
                match connect_hermes_inner(&client).await {
                    Ok(result) => Ok(result),
                    Err(_) => connect_chat_completions_inner(&client).await,
                }
            }
        },
        TransportMode::Sessions | TransportMode::Runs => connect_hermes_inner(&client).await,
        TransportMode::DashboardWs => unreachable!("handled before HTTP client construction"),
    }
}

async fn connect_dashboard_inner(
    config: ConnectionConfig,
) -> Result<(Value, Vec<HermesSessionSummary>)> {
    let client = DashboardClient::new(config).await?;
    let sessions = client.list_sessions().await.unwrap_or_default();
    Ok((client.capabilities(), sessions))
}

async fn create_dashboard_session(
    config: ConnectionConfig,
    title: Option<String>,
    profile: Option<String>,
) -> Result<HermesSessionSummary> {
    let token = CancellationToken::new();
    let client = DashboardClient::new(config).await?;
    let mut connection = client.connect_ws().await?;
    connection.wait_ready(&token).await?;
    let mut params = dashboard_session_create_params(&client, profile.as_deref()).await;
    if let Some(title) = title.as_deref().filter(|title| !title.trim().is_empty()) {
        params["title"] = json!(title);
    }
    let request_id = connection.send_request("session.create", params).await?;
    let value = connection.wait_response(&request_id, &token).await?;
    Ok(
        parse_session(&value).unwrap_or_else(|| HermesSessionSummary {
            id: dashboard_session_id_from_value(&value)
                .unwrap_or_else(|| format!("dashboard-{}", chrono::Utc::now().timestamp_millis())),
            title: title
                .as_deref()
                .filter(|title| !title.trim().is_empty())
                .unwrap_or("Hermes Chat")
                .to_owned(),
            updated_at: None,
            is_active: false,
            needs_input: false,
            message_count: None,
            preview: None,
            source: None,
        }),
    )
}

async fn create_dashboard_branch_session(
    config: ConnectionConfig,
    seed: BranchSeedMessage,
    profile: Option<String>,
) -> Result<HermesSessionSummary> {
    let token = CancellationToken::new();
    let client = DashboardClient::new(config).await?;
    let mut connection = client.connect_ws().await?;
    connection.wait_ready(&token).await?;
    let mut params = dashboard_session_create_params(&client, profile.as_deref()).await;
    params["title"] = json!("Branch");
    params["messages"] = json!([{
        "role": seed.role.as_dashboard_str(),
        "content": seed.content,
    }]);
    let request_id = connection.send_request("session.create", params).await?;
    let value = connection.wait_response(&request_id, &token).await?;
    Ok(
        parse_session(&value).unwrap_or_else(|| HermesSessionSummary {
            id: dashboard_session_id_from_value(&value)
                .unwrap_or_else(|| format!("dashboard-{}", chrono::Utc::now().timestamp_millis())),
            title: String::from("Branch"),
            updated_at: None,
            is_active: false,
            needs_input: false,
            message_count: Some(1),
            preview: None,
            source: Some(String::from("desktop")),
        }),
    )
}

async fn set_dashboard_session_title(
    config: ConnectionConfig,
    title: String,
    active_session_id: Property<Option<String>>,
    sessions: Property<Vec<HermesSessionSummary>>,
    session_summary_cache: SharedSessionSummaryCache,
    profile: Option<String>,
) -> Result<String> {
    let token = CancellationToken::new();
    let client = DashboardClient::new(config).await?;
    let mut connection = client.connect_ws().await?;
    connection.wait_ready(&token).await?;
    let session_id = title_dashboard_session_id(
        &client,
        &mut connection,
        &token,
        &active_session_id,
        &sessions,
        profile.as_deref(),
    )
    .await?;
    let request_id = connection
        .send_request(
            "session.title",
            json!({
                "session_id": session_id,
                "title": title.clone(),
            }),
        )
        .await?;
    let value = connection.wait_response(&request_id, &token).await?;
    let final_title = dashboard_session_title(&value)
        .unwrap_or_else(|| title.trim().to_owned())
        .trim()
        .to_owned();
    let queued = bool_field(&value, &["pending", "queued"]).unwrap_or(false);

    let mut identities = Vec::new();
    push_optional_id(&mut identities, Some(&session_id));
    push_optional_id(&mut identities, active_session_id.get().as_deref());
    push_optional_id(
        &mut identities,
        dashboard_session_id_from_value(&value).as_deref(),
    );
    push_optional_id(
        &mut identities,
        dashboard_stored_session_id_from_value(&value).as_deref(),
    );
    update_session_title_locally(&sessions, &identities, &final_title);
    if let Ok(refreshed) = client.list_sessions().await {
        replace_refreshed_sessions(&sessions, &session_summary_cache, refreshed);
    } else {
        cache_session_summaries(&session_summary_cache, sessions.get());
    }

    Ok(session_title_message(&final_title, queued))
}

async fn set_dashboard_session_yolo(
    config: ConnectionConfig,
    enabled: bool,
    active_session_id: Property<Option<String>>,
    sessions: Property<Vec<HermesSessionSummary>>,
    profile: Option<String>,
) -> Result<bool> {
    let token = CancellationToken::new();
    let client = DashboardClient::new(config).await?;
    let mut connection = client.connect_ws().await?;
    connection.wait_ready(&token).await?;
    let session_id = title_dashboard_session_id(
        &client,
        &mut connection,
        &token,
        &active_session_id,
        &sessions,
        profile.as_deref(),
    )
    .await?;
    request_dashboard_session_yolo(&mut connection, &token, &session_id, enabled).await
}

async fn request_dashboard_session_yolo(
    connection: &mut crate::dashboard::DashboardConnection,
    token: &CancellationToken,
    session_id: &str,
    enabled: bool,
) -> Result<bool> {
    let request_id = connection
        .send_request(
            "config.set",
            json!({
                "key": "yolo",
                "session_id": session_id,
                "value": if enabled { "1" } else { "0" },
            }),
        )
        .await?;
    let value = connection.wait_response(&request_id, token).await?;
    Ok(dashboard_yolo_active(&value).unwrap_or(enabled))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BrowserManageAction {
    Connect,
    Disconnect,
    Status,
}

impl BrowserManageAction {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Disconnect => "disconnect",
            Self::Status => "status",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BrowserManageRequest {
    action: BrowserManageAction,
    url: Option<String>,
}

async fn request_dashboard_browser_manage(
    config: ConnectionConfig,
    active_session_id: Property<Option<String>>,
    sessions: Property<Vec<HermesSessionSummary>>,
    request: &BrowserManageRequest,
    profile: Option<String>,
) -> Result<Value> {
    let token = CancellationToken::new();
    let client = DashboardClient::new(config).await?;
    let mut connection = client.connect_ws().await?;
    connection.wait_ready(&token).await?;
    let session_id = title_dashboard_session_id(
        &client,
        &mut connection,
        &token,
        &active_session_id,
        &sessions,
        profile.as_deref(),
    )
    .await?;
    let mut params = json!({
        "action": request.action.as_str(),
        "session_id": session_id,
    });
    if let Some(url) = request.url.as_deref() {
        params["url"] = json!(url);
    }
    let request_id = connection.send_request("browser.manage", params).await?;
    connection.wait_response(&request_id, &token).await
}

fn parse_browser_manage_args(args: &str) -> std::result::Result<BrowserManageRequest, String> {
    let mut parts = args.split_whitespace();
    let raw_action = parts.next().unwrap_or("status");
    let action = match raw_action.to_ascii_lowercase().as_str() {
        "connect" => BrowserManageAction::Connect,
        "disconnect" => BrowserManageAction::Disconnect,
        "status" => BrowserManageAction::Status,
        _ => {
            return Err(String::from(
                "usage: /browser [connect|disconnect|status] [url] - persistent: set browser.cdp_url in config.yaml",
            ));
        }
    };
    let url = if action == BrowserManageAction::Connect {
        let url = parts.collect::<Vec<_>>().join(" ");
        Some(if url.trim().is_empty() {
            String::from("http://127.0.0.1:9222")
        } else {
            url.trim().to_owned()
        })
    } else {
        None
    };
    Ok(BrowserManageRequest { action, url })
}

fn browser_manage_output(request: &BrowserManageRequest, value: &Value) -> String {
    let mut lines = Vec::new();
    if let Some(url) = request.url.as_deref() {
        lines.push(format!(
            "checking Chromium-family browser remote debugging at {url}..."
        ));
    }
    if let Some(messages) = value.get("messages").and_then(Value::as_array) {
        lines.extend(
            messages
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(str::to_owned),
        );
    }

    match request.action {
        BrowserManageAction::Status => {
            if bool_field(value, &["connected"]).unwrap_or(false) {
                lines.push(format!(
                    "browser connected: {}",
                    string_field(value, &["url"])
                        .unwrap_or_else(|| String::from("(url unavailable)"))
                ));
            } else {
                lines.push(String::from(
                    "browser not connected (try /browser connect <url> or set browser.cdp_url in config.yaml)",
                ));
            }
        }
        BrowserManageAction::Disconnect => lines.push(String::from("browser disconnected")),
        BrowserManageAction::Connect => {
            if bool_field(value, &["connected"]).unwrap_or(false) {
                lines.push(String::from(
                    "Browser connected to live Chromium-family browser via CDP",
                ));
                lines.push(format!(
                    "Endpoint: {}",
                    string_field(value, &["url"])
                        .unwrap_or_else(|| String::from("(url unavailable)"))
                ));
                lines.push(String::from(
                    "next browser tool call will use this CDP endpoint",
                ));
            } else if lines.is_empty() {
                lines.push(String::from(
                    "browser not connected (try /browser connect <url> or set browser.cdp_url in config.yaml)",
                ));
            }
        }
    }
    lines.join("\n")
}

fn dashboard_endpoint_is_loopback(endpoint_url: &str) -> bool {
    let Ok(normalized) = normalize_endpoint_url(endpoint_url) else {
        return false;
    };
    let Ok(url) = reqwest::Url::parse(&normalized) else {
        return false;
    };
    match url.host_str() {
        Some("localhost") => true,
        Some(host) => host
            .parse::<IpAddr>()
            .map(|addr| addr.is_loopback())
            .unwrap_or(false),
        None => false,
    }
}

async fn request_dashboard_handoff(
    config: ConnectionConfig,
    session_id: String,
    platform: String,
) -> Result<()> {
    let token = CancellationToken::new();
    let client = DashboardClient::new(config).await?;
    let mut connection = client.connect_ws().await?;
    connection.wait_ready(&token).await?;
    let request_id = connection
        .send_request(
            "handoff.request",
            json!({
                "platform": platform.clone(),
                "session_id": session_id.clone(),
            }),
        )
        .await?;
    connection.wait_response(&request_id, &token).await?;

    let deadline = tokio::time::Instant::now() + HANDOFF_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(HANDOFF_POLL_INTERVAL).await;
        let request_id = connection
            .send_request("handoff.state", json!({"session_id": session_id.clone()}))
            .await?;
        let state = match connection.wait_response(&request_id, &token).await {
            Ok(value) => value,
            Err(err) => {
                debug!(error = %err.short_message(), "handoff.state failed while polling");
                continue;
            }
        };
        match handoff_state(&state).as_deref() {
            Some("completed") => return Ok(()),
            Some("failed") => {
                return Err(Error::Api {
                    status: 400,
                    message: handoff_error(&state)
                        .unwrap_or_else(|| handoff_failed_message(&platform)),
                });
            }
            _ => {}
        }
    }

    let timed_out = handoff_timed_out_message();
    let request_id = connection
        .send_request(
            "handoff.fail",
            json!({
                "error": timed_out,
                "session_id": session_id.clone(),
            }),
        )
        .await?;
    if let Ok(value) = connection.wait_response(&request_id, &token).await
        && handoff_state(&value).as_deref() == Some("completed")
    {
        return Ok(());
    }
    Err(Error::Api {
        status: 408,
        message: timed_out,
    })
}

fn handoff_state(value: &Value) -> Option<String> {
    string_field(value, &["state"]).map(|state| state.to_ascii_lowercase())
}

fn handoff_error(value: &Value) -> Option<String> {
    string_field(value, &["error"])
}

fn handoff_success_message(platform: &str) -> String {
    format!("Handed off to {platform}. Resume here anytime.")
}

fn handoff_failed_message(error: &str) -> String {
    format!("Handoff failed: {error}")
}

fn handoff_timed_out_message() -> String {
    String::from("Timed out waiting for the gateway. Is `hermes gateway` running?")
}

async fn title_dashboard_session_id(
    client: &DashboardClient,
    connection: &mut crate::dashboard::DashboardConnection,
    token: &CancellationToken,
    active_session_id: &Property<Option<String>>,
    sessions: &Property<Vec<HermesSessionSummary>>,
    profile: Option<&str>,
) -> Result<String> {
    if let Some(session_id) = active_session_id
        .get()
        .filter(|session_id| !session_id.trim().is_empty())
    {
        let request_id = connection
            .send_request(
                "session.resume",
                json!({"session_id": session_id.clone(), "cols": 80}),
            )
            .await?;
        let value = match connection.wait_response(&request_id, token).await {
            Ok(value) => value,
            Err(err) if dashboard_session_not_found(&err) => {
                if active_session_id.get().as_deref() == Some(session_id.as_str()) {
                    active_session_id.set(None);
                }
                debug!(
                    session_id = %session_id,
                    "Dashboard session was not found while setting title; creating a new session"
                );
                return create_dashboard_title_session(
                    client,
                    connection,
                    token,
                    active_session_id,
                    sessions,
                    profile,
                )
                .await;
            }
            Err(err) => return Err(err),
        };
        if let Some(stored_session_id) = dashboard_stored_session_id_from_value(&value) {
            active_session_id.set(Some(stored_session_id));
        }
        return Ok(dashboard_session_id_from_value(&value).unwrap_or(session_id));
    }

    create_dashboard_title_session(
        client,
        connection,
        token,
        active_session_id,
        sessions,
        profile,
    )
    .await
}

async fn create_dashboard_title_session(
    client: &DashboardClient,
    connection: &mut crate::dashboard::DashboardConnection,
    token: &CancellationToken,
    active_session_id: &Property<Option<String>>,
    sessions: &Property<Vec<HermesSessionSummary>>,
    profile: Option<&str>,
) -> Result<String> {
    let params = dashboard_session_create_params(client, profile).await;
    let request_id = connection.send_request("session.create", params).await?;
    let value = connection.wait_response(&request_id, token).await?;
    let session = parse_session(&value).unwrap_or_else(|| HermesSessionSummary {
        id: dashboard_session_id_from_value(&value)
            .unwrap_or_else(|| format!("dashboard-{}", chrono::Utc::now().timestamp_millis())),
        title: String::from("Hermes Chat"),
        updated_at: None,
        is_active: false,
        needs_input: false,
        message_count: None,
        preview: None,
        source: None,
    });
    let runtime_session_id = dashboard_session_id_from_value(&value).unwrap_or_else(|| {
        dashboard_stored_session_id_from_value(&value).unwrap_or_else(|| session.id.clone())
    });
    active_session_id.set(Some(session.id.clone()));
    let mut current = sessions.get();
    current.insert(0, session);
    sessions.set(current);
    Ok(runtime_session_id)
}

async fn connect_hermes_inner(client: &HermesClient) -> Result<(Value, Vec<HermesSessionSummary>)> {
    client.health().await?;
    let capabilities = client.capabilities().await?;
    let sessions = client.list_sessions().await.unwrap_or_default();
    Ok((capabilities, sessions))
}

async fn connect_chat_completions_inner(
    client: &HermesClient,
) -> Result<(Value, Vec<HermesSessionSummary>)> {
    let models = client.models().await?;
    Ok((
        json!({
            "transport_mode": "chat-completions",
            "models": models,
        }),
        Vec::new(),
    ))
}

struct StreamContext {
    config: ConnectionConfig,
    content: String,
    stream_id: u64,
    assistant_id: String,
    token: CancellationToken,
    messages: Property<Vec<HermesMessage>>,
    active_session_id: Property<Option<String>>,
    sessions: Property<Vec<HermesSessionSummary>>,
    session_summary_cache: SharedSessionSummaryCache,
    approval: Property<Option<ApprovalRequest>>,
    yolo_active: Property<bool>,
    selected_dashboard_profile: Option<String>,
    todos: Property<Vec<TodoItem>>,
    subagents: Property<Vec<SubagentItem>>,
    active_stream_id: Arc<RwLock<Option<u64>>>,
    active_run_id: Arc<RwLock<Option<String>>>,
}

struct SlashContext {
    config: ConnectionConfig,
    command_line: String,
    stream_id: u64,
    token: CancellationToken,
    messages: Property<Vec<HermesMessage>>,
    active_session_id: Property<Option<String>>,
    sessions: Property<Vec<HermesSessionSummary>>,
    session_summary_cache: SharedSessionSummaryCache,
    approval: Property<Option<ApprovalRequest>>,
    yolo_active: Property<bool>,
    selected_dashboard_profile: Option<String>,
    composer_prefill: Property<Option<String>>,
    todos: Property<Vec<TodoItem>>,
    subagents: Property<Vec<SubagentItem>>,
    active_stream_id: Arc<RwLock<Option<u64>>>,
    active_run_id: Arc<RwLock<Option<String>>>,
}

struct SelectSessionContext {
    config: ConnectionConfig,
    config_id: u64,
    select_id: u64,
    requested_session_id: String,
    fingerprint: Option<SessionFingerprint>,
    local_history: LocalHistoryMode,
    active_session_id: Property<Option<String>>,
    messages: Property<Vec<HermesMessage>>,
    status: Property<HermesStatus>,
    last_error: Property<Option<String>>,
    store: Option<Arc<LocalHistoryStore>>,
    config_sequence: Arc<AtomicU64>,
    select_sequence: Arc<AtomicU64>,
    transcript_cache: SharedTranscriptCache,
}

#[allow(clippy::cognitive_complexity)]
async fn select_dashboard_session(ctx: SelectSessionContext) {
    let client = match DashboardClient::new(ctx.config.clone()).await {
        Ok(client) => client,
        Err(err) => {
            set_select_error(&ctx, &err, false);
            return;
        }
    };

    let prefetch_client = client.clone();
    let prefetch_session_id = ctx.requested_session_id.clone();
    let mut prefetch =
        tokio::spawn(async move { prefetch_client.session_messages(&prefetch_session_id).await });

    let resume_client = client;
    let resume_session_id = ctx.requested_session_id.clone();
    let mut resume = tokio::spawn(async move {
        let token = CancellationToken::new();
        resume_client
            .request_once(
                "session.resume",
                json!({"session_id": resume_session_id, "cols": 80}),
                &token,
            )
            .await
    });

    let mut prefetched_messages = None;
    let mut prefetch_done = false;
    let mut resume_done = false;
    let mut resume_result = None;

    while !prefetch_done || !resume_done {
        tokio::select! {
            result = &mut prefetch, if !prefetch_done => {
                prefetch_done = true;
                match result {
                    Ok(Ok(remote_messages)) => {
                        if selection_is_current(
                            &ctx.config_sequence,
                            ctx.config_id,
                            &ctx.select_sequence,
                            ctx.select_id,
                        ) {
                            prefetched_messages = Some(remote_messages.clone());
                            publish_selected_transcript(
                                &ctx,
                                ctx.requested_session_id.clone(),
                                Vec::new(),
                                remote_messages,
                            );
                        }
                    }
                    Ok(Err(err)) => {
                        debug!(error = %err.short_message(), "Could not prefetch dashboard session messages");
                    }
                    Err(err) => {
                        debug!(error = %err, "Dashboard session message prefetch task failed");
                    }
                }
            }
            result = &mut resume, if !resume_done => {
                resume_done = true;
                resume_result = Some(match result {
                    Ok(result) => result,
                    Err(err) => Err(Error::WebSocket(err.to_string())),
                });
            }
        }
    }

    let Some(resume_result) = resume_result else {
        return;
    };

    match resume_result {
        Ok(value) => {
            if !selection_is_current(
                &ctx.config_sequence,
                ctx.config_id,
                &ctx.select_sequence,
                ctx.select_id,
            ) {
                return;
            }
            let stored_session_id = dashboard_stored_session_id_from_value(&value)
                .unwrap_or_else(|| ctx.requested_session_id.clone());
            let aliases = if stored_session_id == ctx.requested_session_id {
                Vec::new()
            } else {
                vec![ctx.requested_session_id.clone()]
            };
            let remote_messages = prefetched_messages.unwrap_or_else(|| parse_messages(&value));
            ctx.active_session_id.set(Some(stored_session_id.clone()));
            publish_selected_transcript(&ctx, stored_session_id, aliases, remote_messages);
        }
        Err(err) => {
            let has_prefetched_messages = prefetched_messages.is_some();
            set_select_error(&ctx, &err, has_prefetched_messages);
        }
    }
}

fn publish_selected_transcript(
    ctx: &SelectSessionContext,
    session_id: String,
    aliases: Vec<String>,
    messages: Vec<HermesMessage>,
) {
    if !selection_is_current(
        &ctx.config_sequence,
        ctx.config_id,
        &ctx.select_sequence,
        ctx.select_id,
    ) {
        return;
    }
    ctx.messages.set(messages.clone());
    cache_transcript(
        &ctx.transcript_cache,
        session_id,
        aliases,
        ctx.fingerprint.clone(),
        messages.clone(),
    );
    save_local_history(
        ctx.store.as_ref(),
        ctx.local_history,
        ctx.active_session_id.get(),
        &messages,
    );
    ctx.last_error.set(None);
}

fn set_select_error(ctx: &SelectSessionContext, err: &Error, transcript_visible: bool) {
    if !selection_is_current(
        &ctx.config_sequence,
        ctx.config_id,
        &ctx.select_sequence,
        ctx.select_id,
    ) {
        return;
    }
    let message = err.short_message();
    ctx.last_error.set(Some(message.clone()));
    if !transcript_visible {
        ctx.status.set(status_from_error(err, message));
    }
}

async fn stream_message(ctx: StreamContext) -> Result<()> {
    if matches!(ctx.config.transport_mode, TransportMode::DashboardWs) {
        return stream_dashboard_message(ctx).await;
    }

    let client = HermesClient::new(ctx.config.clone())?;
    if matches!(ctx.config.transport_mode, TransportMode::Runs) {
        return stream_run_message(ctx, &client).await;
    }

    let session_id = stream_session_id(&ctx, &client).await?;
    if stream_cancelled_or_stale(&ctx) {
        return Ok(());
    }

    let Some(mut stream) = open_message_stream(&ctx, &client, session_id.as_deref()).await? else {
        return Ok(());
    };
    let answer_buffer = Property::new(Vec::new());
    while let Some(event) = next_stream_event(&mut stream, &ctx.token).await {
        let run_id_hint = ctx
            .active_run_id
            .read()
            .ok()
            .and_then(|guard| guard.clone());
        apply_event_with_answer_buffer(
            &ctx.messages,
            &answer_buffer,
            &ctx.assistant_id,
            event?,
            ctx.config.history_limit,
            ctx.config.show_tool_progress,
            &ctx.approval,
            run_id_hint.as_deref(),
        )?;
    }
    publish_collected_assistant(
        &ctx.messages,
        &ctx.assistant_id,
        &answer_buffer,
        ctx.config.history_limit,
    );
    refresh_http_session_summaries(&ctx, &client).await;
    Ok(())
}

async fn stream_dashboard_message(ctx: StreamContext) -> Result<()> {
    let client = DashboardClient::new(ctx.config.clone()).await?;
    let mut connection = client.connect_ws().await?;
    connection.wait_ready(&ctx.token).await?;
    if stream_cancelled_or_stale(&ctx) {
        return Ok(());
    }

    let session_id = stream_dashboard_session_id(&client, &ctx, &mut connection).await?;
    if stream_cancelled_or_stale(&ctx) {
        return Ok(());
    }
    set_run_if_current(
        &ctx.active_stream_id,
        &ctx.active_run_id,
        ctx.stream_id,
        session_id.clone(),
    );

    let submit_id = connection
        .send_request(
            "prompt.submit",
            json!({"session_id": session_id, "text": ctx.content}),
        )
        .await?;
    let answer_buffer = Property::new(Vec::new());
    while let Some(frame) = connection.next_frame(&ctx.token).await? {
        if stream_cancelled_or_stale(&ctx) {
            return Ok(());
        }
        match frame {
            DashboardFrame::Event(event) => {
                if apply_dashboard_session_info(
                    &ctx.sessions,
                    &ctx.yolo_active,
                    ctx.active_session_id.get().as_deref(),
                    &event,
                ) {
                    continue;
                }
                let complete = apply_dashboard_event_with_answer_buffer(
                    &answer_buffer,
                    &ctx.messages,
                    &ctx.assistant_id,
                    event,
                    ctx.config.history_limit,
                    ctx.config.show_tool_progress,
                    &ctx.approval,
                    &ctx.todos,
                    &ctx.subagents,
                    ctx.active_session_id.get().as_deref(),
                )?;
                if complete {
                    publish_collected_assistant(
                        &ctx.messages,
                        &ctx.assistant_id,
                        &answer_buffer,
                        ctx.config.history_limit,
                    );
                    drain_dashboard_post_complete_events(&ctx, &mut connection, &client).await;
                    break;
                }
            }
            DashboardFrame::Response { id, error, .. } if id == submit_id => {
                if let Some(error) = error {
                    return Err(dashboard_rpc_frame_error(error));
                }
            }
            DashboardFrame::Response {
                error: Some(error), ..
            } => {
                debug!(error = %dashboard_rpc_frame_error(error), "Ignoring unrelated dashboard RPC error");
            }
            DashboardFrame::Response { .. } => {}
        }
    }
    Ok(())
}

async fn run_dashboard_slash_command(ctx: SlashContext) -> Result<()> {
    let client = DashboardClient::new(ctx.config.clone()).await?;
    let mut connection = client.connect_ws().await?;
    connection.wait_ready(&ctx.token).await?;
    if slash_cancelled_or_stale(&ctx) {
        return Ok(());
    }

    let session_id = slash_dashboard_session_id(&client, &ctx, &mut connection).await?;
    if slash_cancelled_or_stale(&ctx) {
        return Ok(());
    }
    set_run_if_current(
        &ctx.active_stream_id,
        &ctx.active_run_id,
        ctx.stream_id,
        session_id.clone(),
    );

    let mut command_line = ctx.command_line.clone();
    for _ in 0..4 {
        let parsed = parse_slash_line(&command_line);
        let slash_result = match request_dashboard_slash_exec(
            &mut connection,
            &ctx,
            &session_id,
            &parsed,
        )
        .await
        {
            Ok(value) => value,
            Err(err) => {
                debug!(error = %err.short_message(), "dashboard slash.exec failed; falling back to command.dispatch");
                request_dashboard_command_dispatch(&mut connection, &ctx, &session_id, &parsed)
                    .await?
            }
        };

        if let Some(dispatch) = parse_command_dispatch(&slash_result) {
            match handle_dashboard_command_dispatch(
                &ctx,
                &client,
                &mut connection,
                &session_id,
                &parsed,
                dispatch,
            )
            .await?
            {
                DispatchOutcome::Done => {
                    refresh_dashboard_session_summaries_for_slash(&ctx, &client).await;
                    return Ok(());
                }
                DispatchOutcome::Alias(next) => {
                    command_line = next;
                    continue;
                }
            }
        }

        append_system_message(
            &ctx.messages,
            slash_exec_output(&slash_result, &parsed.name),
            ctx.config.history_limit,
        );
        refresh_dashboard_session_summaries_for_slash(&ctx, &client).await;
        return Ok(());
    }

    append_system_message(
        &ctx.messages,
        "Slash alias expansion stopped after too many redirects.",
        ctx.config.history_limit,
    );
    refresh_dashboard_session_summaries_for_slash(&ctx, &client).await;
    Ok(())
}

async fn request_dashboard_slash_exec(
    connection: &mut crate::dashboard::DashboardConnection,
    ctx: &SlashContext,
    session_id: &str,
    parsed: &ParsedSlashLine,
) -> Result<Value> {
    let request_id = connection
        .send_request(
            "slash.exec",
            json!({
                "session_id": session_id,
                "command": parsed.command,
            }),
        )
        .await?;
    connection.wait_response(&request_id, &ctx.token).await
}

async fn request_dashboard_command_dispatch(
    connection: &mut crate::dashboard::DashboardConnection,
    ctx: &SlashContext,
    session_id: &str,
    parsed: &ParsedSlashLine,
) -> Result<Value> {
    let request_id = connection
        .send_request(
            "command.dispatch",
            json!({
                "session_id": session_id,
                "name": parsed.name,
                "arg": parsed.arg,
            }),
        )
        .await?;
    connection.wait_response(&request_id, &ctx.token).await
}

async fn handle_dashboard_command_dispatch(
    ctx: &SlashContext,
    client: &DashboardClient,
    connection: &mut crate::dashboard::DashboardConnection,
    session_id: &str,
    parsed: &ParsedSlashLine,
    dispatch: CommandDispatch,
) -> Result<DispatchOutcome> {
    match dispatch {
        CommandDispatch::Exec { output } | CommandDispatch::Plugin { output } => {
            append_system_message(
                &ctx.messages,
                output.unwrap_or_else(|| String::from("(no output)")),
                ctx.config.history_limit,
            );
            Ok(DispatchOutcome::Done)
        }
        CommandDispatch::Alias { target } => {
            let next = if parsed.arg.is_empty() {
                format!("/{target}")
            } else {
                format!("/{target} {}", parsed.arg)
            };
            Ok(DispatchOutcome::Alias(next))
        }
        CommandDispatch::Send { message, notice } => {
            if let Some(notice) = notice.filter(|notice| !notice.trim().is_empty()) {
                append_system_message(&ctx.messages, notice, ctx.config.history_limit);
            }
            if message.trim().is_empty() {
                append_system_message(
                    &ctx.messages,
                    format!("/{name}: empty message", name = parsed.name),
                    ctx.config.history_limit,
                );
                return Ok(DispatchOutcome::Done);
            }
            submit_dashboard_slash_prompt(ctx, client, connection, session_id, message).await?;
            Ok(DispatchOutcome::Done)
        }
        CommandDispatch::Skill { name, message } => {
            append_system_message(
                &ctx.messages,
                format!("Loading skill: {name}"),
                ctx.config.history_limit,
            );
            let Some(message) = message.filter(|message| !message.trim().is_empty()) else {
                append_system_message(
                    &ctx.messages,
                    format!("/{name}: skill payload missing message"),
                    ctx.config.history_limit,
                );
                return Ok(DispatchOutcome::Done);
            };
            submit_dashboard_slash_prompt(ctx, client, connection, session_id, message).await?;
            Ok(DispatchOutcome::Done)
        }
        CommandDispatch::Prefill { message, notice } => {
            apply_prefill_dispatch(
                &ctx.messages,
                &ctx.composer_prefill,
                ctx.config.history_limit,
                &parsed.name,
                message,
                notice,
            );
            Ok(DispatchOutcome::Done)
        }
    }
}

fn apply_prefill_dispatch(
    messages: &Property<Vec<HermesMessage>>,
    composer_prefill: &Property<Option<String>>,
    history_limit: usize,
    command_name: &str,
    message: String,
    notice: Option<String>,
) {
    if let Some(notice) = notice.filter(|notice| !notice.trim().is_empty()) {
        append_system_message(messages, notice, history_limit);
    }
    if message.trim().is_empty() {
        append_system_message(
            messages,
            format!("/{command_name}: empty prefill"),
            history_limit,
        );
    } else {
        composer_prefill.set(Some(message));
    }
}

async fn submit_dashboard_slash_prompt(
    ctx: &SlashContext,
    client: &DashboardClient,
    connection: &mut crate::dashboard::DashboardConnection,
    session_id: &str,
    message: String,
) -> Result<()> {
    append_user_message(&ctx.messages, message.clone(), ctx.config.history_limit);
    let assistant_id = format!("local-assistant-{}", chrono::Utc::now().timestamp_millis());
    let submit_id = connection
        .send_request(
            "prompt.submit",
            json!({"session_id": session_id, "text": message}),
        )
        .await?;
    let answer_buffer = Property::new(Vec::new());
    while let Some(frame) = connection.next_frame(&ctx.token).await? {
        if slash_cancelled_or_stale(ctx) {
            return Ok(());
        }
        match frame {
            DashboardFrame::Event(event) => {
                if apply_dashboard_session_info(
                    &ctx.sessions,
                    &ctx.yolo_active,
                    ctx.active_session_id.get().as_deref(),
                    &event,
                ) {
                    continue;
                }
                let complete = apply_dashboard_event_with_answer_buffer(
                    &answer_buffer,
                    &ctx.messages,
                    &assistant_id,
                    event,
                    ctx.config.history_limit,
                    ctx.config.show_tool_progress,
                    &ctx.approval,
                    &ctx.todos,
                    &ctx.subagents,
                    ctx.active_session_id.get().as_deref(),
                )?;
                if complete {
                    publish_collected_assistant(
                        &ctx.messages,
                        &assistant_id,
                        &answer_buffer,
                        ctx.config.history_limit,
                    );
                    drain_dashboard_post_complete_events_for_slash(ctx, connection, client).await;
                    break;
                }
            }
            DashboardFrame::Response { id, error, .. } if id == submit_id => {
                if let Some(error) = error {
                    return Err(dashboard_rpc_frame_error(error));
                }
            }
            DashboardFrame::Response {
                error: Some(error), ..
            } => {
                debug!(error = %dashboard_rpc_frame_error(error), "Ignoring unrelated dashboard RPC error");
            }
            DashboardFrame::Response { .. } => {}
        }
    }
    Ok(())
}

#[allow(clippy::cognitive_complexity)]
async fn drain_dashboard_post_complete_events(
    ctx: &StreamContext,
    connection: &mut crate::dashboard::DashboardConnection,
    client: &DashboardClient,
) {
    loop {
        let frame = tokio::time::timeout(
            DASHBOARD_POST_COMPLETE_SETTLE,
            connection.next_frame(&ctx.token),
        )
        .await;
        match frame {
            Ok(Ok(Some(DashboardFrame::Event(event)))) => {
                let is_session_info = apply_dashboard_session_info(
                    &ctx.sessions,
                    &ctx.yolo_active,
                    ctx.active_session_id.get().as_deref(),
                    &event,
                );
                if is_session_info {
                    break;
                } else if append_dashboard_review_summary(
                    &ctx.messages,
                    &event,
                    ctx.config.history_limit,
                ) {
                    continue;
                } else {
                    debug!(
                        event = %event.event_type,
                        "Ignoring post-complete dashboard event"
                    );
                }
            }
            Ok(Ok(Some(DashboardFrame::Response {
                error: Some(error), ..
            }))) => {
                debug!(
                    error = %dashboard_rpc_frame_error(error),
                    "Ignoring post-complete dashboard RPC error"
                );
            }
            Ok(Ok(Some(DashboardFrame::Response { .. }))) => {}
            Ok(Ok(None)) => break,
            Ok(Err(err)) => {
                debug!(error = %err.short_message(), "Could not drain post-complete dashboard events");
                break;
            }
            Err(_) => break,
        }
    }
    refresh_dashboard_session_summaries(ctx, client).await;
}

#[allow(clippy::cognitive_complexity)]
async fn drain_dashboard_post_complete_events_for_slash(
    ctx: &SlashContext,
    connection: &mut crate::dashboard::DashboardConnection,
    client: &DashboardClient,
) {
    loop {
        let frame = tokio::time::timeout(
            DASHBOARD_POST_COMPLETE_SETTLE,
            connection.next_frame(&ctx.token),
        )
        .await;
        match frame {
            Ok(Ok(Some(DashboardFrame::Event(event)))) => {
                let is_session_info = apply_dashboard_session_info(
                    &ctx.sessions,
                    &ctx.yolo_active,
                    ctx.active_session_id.get().as_deref(),
                    &event,
                );
                if is_session_info {
                    break;
                } else if append_dashboard_review_summary(
                    &ctx.messages,
                    &event,
                    ctx.config.history_limit,
                ) {
                    continue;
                } else {
                    debug!(
                        event = %event.event_type,
                        "Ignoring post-complete dashboard event"
                    );
                }
            }
            Ok(Ok(Some(DashboardFrame::Response {
                error: Some(error), ..
            }))) => {
                debug!(
                    error = %dashboard_rpc_frame_error(error),
                    "Ignoring post-complete dashboard RPC error"
                );
            }
            Ok(Ok(Some(DashboardFrame::Response { .. }))) => {}
            Ok(Ok(None)) => break,
            Ok(Err(err)) => {
                debug!(error = %err.short_message(), "Could not drain post-complete dashboard events");
                break;
            }
            Err(_) => break,
        }
    }
    refresh_dashboard_session_summaries_for_slash(ctx, client).await;
}

async fn refresh_http_session_summaries(ctx: &StreamContext, client: &HermesClient) {
    if !matches!(
        ctx.config.transport_mode,
        TransportMode::Auto | TransportMode::Sessions | TransportMode::Runs
    ) || stream_cancelled_or_stale(ctx)
        || ctx.active_session_id.get().is_none()
    {
        return;
    }
    match client.list_sessions().await {
        Ok(sessions) if !stream_cancelled_or_stale(ctx) => {
            replace_refreshed_sessions(&ctx.sessions, &ctx.session_summary_cache, sessions)
        }
        Ok(_) => {}
        Err(err) => debug!(error = %err.short_message(), "Could not refresh Hermes sessions"),
    }
}

async fn refresh_dashboard_session_summaries(ctx: &StreamContext, client: &DashboardClient) {
    if stream_cancelled_or_stale(ctx) {
        return;
    }
    match client.list_sessions().await {
        Ok(sessions) if !stream_cancelled_or_stale(ctx) => {
            replace_refreshed_sessions(&ctx.sessions, &ctx.session_summary_cache, sessions)
        }
        Ok(_) => {}
        Err(err) => debug!(error = %err.short_message(), "Could not refresh dashboard sessions"),
    }
}

async fn refresh_dashboard_session_summaries_for_slash(
    ctx: &SlashContext,
    client: &DashboardClient,
) {
    if slash_cancelled_or_stale(ctx) {
        return;
    }
    match client.list_sessions().await {
        Ok(sessions) if !slash_cancelled_or_stale(ctx) => {
            replace_refreshed_sessions(&ctx.sessions, &ctx.session_summary_cache, sessions)
        }
        Ok(_) => {}
        Err(err) => debug!(error = %err.short_message(), "Could not refresh dashboard sessions"),
    }
}

fn replace_refreshed_sessions(
    sessions: &Property<Vec<HermesSessionSummary>>,
    session_summary_cache: &SharedSessionSummaryCache,
    refreshed: Vec<HermesSessionSummary>,
) {
    if !refreshed.is_empty() {
        cache_session_summaries(session_summary_cache, refreshed.clone());
        sessions.set(refreshed);
    }
}

async fn stream_dashboard_session_id(
    client: &DashboardClient,
    ctx: &StreamContext,
    connection: &mut crate::dashboard::DashboardConnection,
) -> Result<String> {
    if let Some(session_id) = ctx
        .active_session_id
        .get()
        .filter(|session_id| !session_id.trim().is_empty())
    {
        let request_id = connection
            .send_request(
                "session.resume",
                json!({"session_id": session_id.clone(), "cols": 80}),
            )
            .await?;
        let value = match connection.wait_response(&request_id, &ctx.token).await {
            Ok(value) => value,
            Err(err) if dashboard_session_not_found(&err) => {
                clear_active_dashboard_session_if_current(ctx, &session_id);
                debug!(
                    session_id = %session_id,
                    "Dashboard session was not found; creating a new session"
                );
                return create_dashboard_stream_session(client, ctx, connection).await;
            }
            Err(err) => return Err(err),
        };
        let runtime_session_id = dashboard_session_id_from_value(&value).unwrap_or(session_id);
        if stream_is_current(&ctx.active_stream_id, ctx.stream_id)
            && let Some(stored_session_id) = dashboard_stored_session_id_from_value(&value)
        {
            ctx.active_session_id.set(Some(stored_session_id));
        }
        return Ok(runtime_session_id);
    }

    create_dashboard_stream_session(client, ctx, connection).await
}

async fn slash_dashboard_session_id(
    client: &DashboardClient,
    ctx: &SlashContext,
    connection: &mut crate::dashboard::DashboardConnection,
) -> Result<String> {
    if let Some(session_id) = ctx
        .active_session_id
        .get()
        .filter(|session_id| !session_id.trim().is_empty())
    {
        let request_id = connection
            .send_request(
                "session.resume",
                json!({"session_id": session_id.clone(), "cols": 80}),
            )
            .await?;
        let value = match connection.wait_response(&request_id, &ctx.token).await {
            Ok(value) => value,
            Err(err) if dashboard_session_not_found(&err) => {
                clear_active_dashboard_session_for_slash_if_current(ctx, &session_id);
                debug!(
                    session_id = %session_id,
                    "Dashboard session was not found; creating a new session"
                );
                return create_dashboard_slash_session(client, ctx, connection).await;
            }
            Err(err) => return Err(err),
        };
        let runtime_session_id = dashboard_session_id_from_value(&value).unwrap_or(session_id);
        if stream_is_current(&ctx.active_stream_id, ctx.stream_id)
            && let Some(stored_session_id) = dashboard_stored_session_id_from_value(&value)
        {
            ctx.active_session_id.set(Some(stored_session_id));
        }
        return Ok(runtime_session_id);
    }

    create_dashboard_slash_session(client, ctx, connection).await
}

async fn create_dashboard_stream_session(
    client: &DashboardClient,
    ctx: &StreamContext,
    connection: &mut crate::dashboard::DashboardConnection,
) -> Result<String> {
    let params =
        dashboard_session_create_params(client, ctx.selected_dashboard_profile.as_deref()).await;
    let request_id = connection.send_request("session.create", params).await?;
    let value = connection.wait_response(&request_id, &ctx.token).await?;
    let session = parse_session(&value).unwrap_or_else(|| HermesSessionSummary {
        id: dashboard_session_id_from_value(&value)
            .unwrap_or_else(|| format!("dashboard-{}", chrono::Utc::now().timestamp_millis())),
        title: String::from("Hermes Chat"),
        updated_at: None,
        is_active: false,
        needs_input: false,
        message_count: None,
        preview: None,
        source: None,
    });
    let runtime_session_id = dashboard_session_id_from_value(&value).unwrap_or_else(|| {
        dashboard_stored_session_id_from_value(&value).unwrap_or_else(|| session.id.clone())
    });
    if set_session_if_current(
        &ctx.active_stream_id,
        &ctx.active_session_id,
        &ctx.sessions,
        ctx.stream_id,
        session,
    )
    .is_some()
    {
        cache_session_summaries(&ctx.session_summary_cache, ctx.sessions.get());
    }
    if ctx.yolo_active.get() {
        match request_dashboard_session_yolo(connection, &ctx.token, &runtime_session_id, true)
            .await
        {
            Ok(active) => ctx.yolo_active.set(active),
            Err(err) => {
                debug!(error = %err.short_message(), "Could not apply armed YOLO to dashboard session")
            }
        }
    }
    Ok(runtime_session_id)
}

async fn create_dashboard_slash_session(
    client: &DashboardClient,
    ctx: &SlashContext,
    connection: &mut crate::dashboard::DashboardConnection,
) -> Result<String> {
    let params =
        dashboard_session_create_params(client, ctx.selected_dashboard_profile.as_deref()).await;
    let request_id = connection.send_request("session.create", params).await?;
    let value = connection.wait_response(&request_id, &ctx.token).await?;
    let session = parse_session(&value).unwrap_or_else(|| HermesSessionSummary {
        id: dashboard_session_id_from_value(&value)
            .unwrap_or_else(|| format!("dashboard-{}", chrono::Utc::now().timestamp_millis())),
        title: String::from("Hermes Chat"),
        updated_at: None,
        is_active: false,
        needs_input: false,
        message_count: None,
        preview: None,
        source: None,
    });
    let runtime_session_id = dashboard_session_id_from_value(&value).unwrap_or_else(|| {
        dashboard_stored_session_id_from_value(&value).unwrap_or_else(|| session.id.clone())
    });
    if set_session_if_current(
        &ctx.active_stream_id,
        &ctx.active_session_id,
        &ctx.sessions,
        ctx.stream_id,
        session,
    )
    .is_some()
    {
        cache_session_summaries(&ctx.session_summary_cache, ctx.sessions.get());
    }
    if ctx.yolo_active.get() {
        match request_dashboard_session_yolo(connection, &ctx.token, &runtime_session_id, true)
            .await
        {
            Ok(active) => ctx.yolo_active.set(active),
            Err(err) => {
                debug!(error = %err.short_message(), "Could not apply armed YOLO to dashboard session")
            }
        }
    }
    Ok(runtime_session_id)
}

async fn dashboard_session_create_params(client: &DashboardClient, profile: Option<&str>) -> Value {
    let mut params = json!({
        "cols": 80,
        "source": "desktop",
        "client": "Hermes Desktop",
        "client_name": "Hermes Desktop",
    });
    apply_dashboard_session_profile(&mut params, profile);
    match client.model_info().await {
        Ok(info) => apply_dashboard_model_info(&mut params, &info),
        Err(err) => debug!(
            error = %err.short_message(),
            "Could not fetch dashboard model info for session.create"
        ),
    }
    params
}

fn apply_dashboard_session_profile(params: &mut Value, profile: Option<&str>) {
    if let Some(profile) = profile.map(str::trim).filter(|profile| !profile.is_empty()) {
        params["profile"] = json!(profile);
    }
}

fn apply_dashboard_model_info(params: &mut Value, info: &Value) {
    if let Some(model) = dashboard_model_info_field(info, "model") {
        params["model"] = json!(model);
    }
    if let Some(provider) = dashboard_model_info_field(info, "provider") {
        params["provider"] = json!(provider);
    }
}

fn dashboard_model_info_field(info: &Value, key: &str) -> Option<String> {
    info.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn clear_active_dashboard_session_if_current(ctx: &StreamContext, stale_session_id: &str) {
    if stream_is_current(&ctx.active_stream_id, ctx.stream_id)
        && ctx.active_session_id.get().as_deref() == Some(stale_session_id)
    {
        ctx.active_session_id.set(None);
    }
}

fn clear_active_dashboard_session_for_slash_if_current(ctx: &SlashContext, stale_session_id: &str) {
    if stream_is_current(&ctx.active_stream_id, ctx.stream_id)
        && ctx.active_session_id.get().as_deref() == Some(stale_session_id)
    {
        ctx.active_session_id.set(None);
    }
}

fn dashboard_session_not_found(err: &Error) -> bool {
    let Error::Api { message, .. } = err else {
        return false;
    };
    let message = message.to_ascii_lowercase();
    (message.contains("session") && message.contains("not found"))
        || (message.contains("no ") && message.contains("session") && message.contains("found"))
}

async fn stream_session_id(ctx: &StreamContext, client: &HermesClient) -> Result<Option<String>> {
    let session_id = ctx.active_session_id.get();
    if session_id.is_some()
        || !matches!(
            ctx.config.transport_mode,
            TransportMode::Auto | TransportMode::Sessions
        )
    {
        return Ok(session_id);
    }

    match client.create_session(None).await {
        Ok(_) if ctx.token.is_cancelled() => Ok(None),
        Ok(session) => {
            let session_id = set_session_if_current(
                &ctx.active_stream_id,
                &ctx.active_session_id,
                &ctx.sessions,
                ctx.stream_id,
                session,
            );
            if session_id.is_some() {
                cache_session_summaries(&ctx.session_summary_cache, ctx.sessions.get());
            }
            Ok(session_id)
        }
        Err(err) if matches!(ctx.config.transport_mode, TransportMode::Sessions) => Err(err),
        Err(_) => Ok(None),
    }
}

async fn open_message_stream(
    ctx: &StreamContext,
    client: &HermesClient,
    session_id: Option<&str>,
) -> Result<Option<EventStream>> {
    if matches!(ctx.config.transport_mode, TransportMode::Runs) {
        return open_run_stream(ctx, client).await;
    }

    if matches!(ctx.config.transport_mode, TransportMode::ChatCompletions) {
        return client
            .stream_chat_completions(&ctx.messages.get())
            .await
            .map(Some);
    }

    if let Some(session_id) = session_id {
        return client
            .stream_session_chat(session_id, &ctx.content)
            .await
            .map(Some);
    }

    if matches!(ctx.config.transport_mode, TransportMode::Auto) {
        client
            .stream_chat_completions(&ctx.messages.get())
            .await
            .map(Some)
    } else {
        Err(Error::UnsupportedEvent(String::from("missing session")))
    }
}

async fn open_run_stream(
    ctx: &StreamContext,
    client: &HermesClient,
) -> Result<Option<EventStream>> {
    let run_id = client
        .start_run(&ctx.content)
        .await?
        .ok_or_else(|| Error::UnsupportedEvent(String::from("missing run id")))?;
    if stream_cancelled_or_stale(ctx) {
        if let Err(err) = client.stop_run(&run_id).await {
            debug!(error = %err, "Hermes stale run stop failed");
        }
        return Ok(None);
    }
    set_run_if_current(
        &ctx.active_stream_id,
        &ctx.active_run_id,
        ctx.stream_id,
        run_id.clone(),
    );
    client.stream_run_events(&run_id).await.map(Some)
}

async fn stream_run_message(ctx: StreamContext, client: &HermesClient) -> Result<()> {
    let Some(mut stream) = open_run_stream(&ctx, client).await? else {
        return Ok(());
    };
    let answer_buffer = Property::new(Vec::new());
    while let Some(event) = next_stream_event(&mut stream, &ctx.token).await {
        let run_id_hint = ctx
            .active_run_id
            .read()
            .ok()
            .and_then(|guard| guard.clone());
        apply_event_with_answer_buffer(
            &ctx.messages,
            &answer_buffer,
            &ctx.assistant_id,
            event?,
            ctx.config.history_limit,
            ctx.config.show_tool_progress,
            &ctx.approval,
            run_id_hint.as_deref(),
        )?;
    }
    publish_collected_assistant(
        &ctx.messages,
        &ctx.assistant_id,
        &answer_buffer,
        ctx.config.history_limit,
    );
    refresh_http_session_summaries(&ctx, client).await;
    Ok(())
}

fn stream_cancelled_or_stale(ctx: &StreamContext) -> bool {
    ctx.token.is_cancelled() || !stream_is_current(&ctx.active_stream_id, ctx.stream_id)
}

fn slash_cancelled_or_stale(ctx: &SlashContext) -> bool {
    ctx.token.is_cancelled() || !stream_is_current(&ctx.active_stream_id, ctx.stream_id)
}

async fn next_stream_event(
    stream: &mut EventStream,
    token: &CancellationToken,
) -> Option<Result<SseEvent>> {
    tokio::select! {
        () = token.cancelled() => None,
        event = stream.next() => event,
    }
}

#[cfg(test)]
fn apply_event(
    messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    event: SseEvent,
    limit: usize,
    show_tool_progress: bool,
    approval: &Property<Option<ApprovalRequest>>,
    run_id_hint: Option<&str>,
) -> Result<()> {
    if event.data.trim() == "[DONE]" || event.event.as_deref() == Some("done") {
        return Ok(());
    }
    let event_name = event.event.as_deref().unwrap_or("message");
    match event_name {
        "assistant.delta" | "message.delta" | "response.output_text.delta" => {
            let delta = event_text_delta(&event.data);
            append_delta(messages, assistant_id, &delta, limit);
        }
        "assistant.completed" | "run.completed" | "message.completed" => {
            mark_message(messages, assistant_id, MessageStatus::Complete, None, limit);
        }
        name if is_tool_progress_event(name) => {
            if show_tool_progress
                && let Some(tool_event) = parse_tool_event(&event.data, event_name)?
            {
                push_tool_event(messages, assistant_id, tool_event, limit);
            }
        }
        "approval.required" | "run.requires_approval" | "clarify.required" => {
            let value: Value = serde_json::from_str(&event.data)?;
            let run_id = value
                .get("run_id")
                .or_else(|| value.get("runId"))
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .or(run_id_hint)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| Error::UnsupportedEvent(String::from("approval missing run id")))?
                .to_owned();
            let prompt = value
                .get("prompt")
                .or_else(|| value.get("message"))
                .or_else(|| value.get("question"))
                .and_then(Value::as_str)
                .unwrap_or("Hermes is requesting approval")
                .to_owned();
            approval.set(Some(ApprovalRequest {
                run_id,
                approval_id: value
                    .get("approval_id")
                    .or_else(|| value.get("approvalId"))
                    .and_then(Value::as_str)
                    .filter(|id| !id.trim().is_empty())
                    .map(str::to_owned),
                prompt,
                kind: ApprovalKind::Approval,
            }));
        }
        "message" => {
            if let Ok(value) = serde_json::from_str::<Value>(&event.data) {
                if let Some(delta) = openai_delta(&value) {
                    append_delta(messages, assistant_id, &delta, limit);
                } else if value.get("object").and_then(Value::as_str)
                    == Some("chat.completion.chunk")
                {
                    return Ok(());
                } else if let Some(delta) = value_text_delta(&value) {
                    append_delta(messages, assistant_id, delta, limit);
                }
            } else {
                append_delta(messages, assistant_id, &event.data, limit);
            }
        }
        "run.started" | "message.started" => {}
        "error" => {
            let value: Value =
                serde_json::from_str(&event.data).unwrap_or(Value::String(event.data));
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| value.as_str())
                .unwrap_or("Hermes stream error")
                .to_owned();
            return Err(Error::Api {
                status: 500,
                message,
            });
        }
        other => {
            debug!(event = other, "Ignoring Hermes SSE event");
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_event_with_answer_buffer(
    visible_messages: &Property<Vec<HermesMessage>>,
    answer_buffer: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    event: SseEvent,
    limit: usize,
    show_tool_progress: bool,
    approval: &Property<Option<ApprovalRequest>>,
    run_id_hint: Option<&str>,
) -> Result<()> {
    if event.data.trim() == "[DONE]" || event.event.as_deref() == Some("done") {
        return Ok(());
    }
    let event_name = event.event.as_deref().unwrap_or("message");
    match event_name {
        "assistant.delta" | "message.delta" | "response.output_text.delta" => {
            let delta = event_text_delta(&event.data);
            append_delta(answer_buffer, assistant_id, &delta, limit);
        }
        "assistant.completed" | "run.completed" | "message.completed" => {
            let value: Value =
                serde_json::from_str(&event.data).unwrap_or(Value::String(event.data));
            mark_message(
                answer_buffer,
                assistant_id,
                MessageStatus::Complete,
                completed_event_text(&value),
                limit,
            );
        }
        name if is_tool_progress_event(name) => {
            if show_tool_progress
                && let Some(tool_event) = parse_tool_event(&event.data, event_name)?
            {
                push_tool_event(visible_messages, assistant_id, tool_event, limit);
            }
        }
        "approval.required" | "run.requires_approval" | "clarify.required" => {
            let value: Value = serde_json::from_str(&event.data)?;
            let run_id = value
                .get("run_id")
                .or_else(|| value.get("runId"))
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .or(run_id_hint)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| Error::UnsupportedEvent(String::from("approval missing run id")))?
                .to_owned();
            let prompt = value
                .get("prompt")
                .or_else(|| value.get("message"))
                .or_else(|| value.get("question"))
                .and_then(Value::as_str)
                .unwrap_or("Hermes is requesting approval")
                .to_owned();
            approval.set(Some(ApprovalRequest {
                run_id,
                approval_id: value
                    .get("approval_id")
                    .or_else(|| value.get("approvalId"))
                    .and_then(Value::as_str)
                    .filter(|id| !id.trim().is_empty())
                    .map(str::to_owned),
                prompt,
                kind: ApprovalKind::Approval,
            }));
        }
        "message" => {
            if let Ok(value) = serde_json::from_str::<Value>(&event.data) {
                if let Some(delta) = openai_delta(&value) {
                    append_delta(answer_buffer, assistant_id, &delta, limit);
                } else if value.get("object").and_then(Value::as_str)
                    == Some("chat.completion.chunk")
                {
                    return Ok(());
                } else if let Some(delta) = value_text_delta(&value) {
                    append_delta(answer_buffer, assistant_id, delta, limit);
                }
            } else {
                append_delta(answer_buffer, assistant_id, &event.data, limit);
            }
        }
        "run.started" | "message.started" => {}
        "error" => {
            let value: Value =
                serde_json::from_str(&event.data).unwrap_or(Value::String(event.data));
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| value.as_str())
                .unwrap_or("Hermes stream error")
                .to_owned();
            return Err(Error::Api {
                status: 500,
                message,
            });
        }
        other => {
            debug!(event = other, "Ignoring Hermes SSE event");
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn apply_dashboard_event(
    messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    event: DashboardRpcEvent,
    limit: usize,
    show_tool_progress: bool,
    approval: &Property<Option<ApprovalRequest>>,
    todos: &Property<Vec<TodoItem>>,
    active_session_id: Option<&str>,
) -> Result<bool> {
    let subagents = Property::new(Vec::new());
    match event.event_type.as_str() {
        "gateway.ready" => {}
        "message.start" => {}
        "message.delta" => {
            ensure_streaming_assistant(messages, assistant_id, limit);
            if let Some(delta) = dashboard_text(&event.payload) {
                append_delta(messages, assistant_id, delta, limit);
            }
        }
        "reasoning.delta" => {
            if let Some(delta) = dashboard_text(&event.payload) {
                append_reasoning_delta(messages, assistant_id, delta, limit, false);
            }
        }
        "reasoning.available" => {
            if let Some(delta) = dashboard_text(&event.payload) {
                append_reasoning_delta(messages, assistant_id, delta, limit, true);
            }
        }
        "tool.progress" | "tool.generating" | "tool.start" => {
            apply_dashboard_tool_event(
                messages,
                assistant_id,
                todos,
                &subagents,
                &event,
                limit,
                show_tool_progress,
            );
        }
        "tool.complete" => {
            apply_dashboard_tool_event(
                messages,
                assistant_id,
                todos,
                &subagents,
                &event,
                limit,
                show_tool_progress,
            );
        }
        "approval.request" => {
            set_dashboard_approval(approval, &event, active_session_id)?;
        }
        "clarify.request" => {
            set_dashboard_clarification(approval, &event, active_session_id)?;
        }
        "sudo.request" => {
            set_dashboard_sensitive_input(approval, &event, active_session_id, ApprovalKind::Sudo)?;
        }
        "secret.request" => {
            set_dashboard_sensitive_input(
                approval,
                &event,
                active_session_id,
                ApprovalKind::Secret,
            )?;
        }
        "message.complete" => {
            complete_dashboard_message(messages, assistant_id, &event.payload, limit);
            return Ok(true);
        }
        "review.summary" => {
            append_dashboard_review_summary(messages, &event, limit);
        }
        "error" => {
            let message = event
                .payload
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| event.payload.as_str())
                .unwrap_or("Hermes dashboard stream error")
                .to_owned();
            return Err(Error::Api {
                status: 500,
                message,
            });
        }
        other => {
            debug!(event = other, "Ignoring Hermes dashboard event");
        }
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
fn apply_dashboard_event_with_answer_buffer(
    answer_buffer: &Property<Vec<HermesMessage>>,
    visible_messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    event: DashboardRpcEvent,
    limit: usize,
    show_tool_progress: bool,
    approval: &Property<Option<ApprovalRequest>>,
    todos: &Property<Vec<TodoItem>>,
    subagents: &Property<Vec<SubagentItem>>,
    active_session_id: Option<&str>,
) -> Result<bool> {
    match event.event_type.as_str() {
        "gateway.ready" => {}
        "message.start" => {}
        "message.delta" => {
            if let Some(delta) = dashboard_text(&event.payload) {
                append_delta(answer_buffer, assistant_id, delta, limit);
            }
        }
        "reasoning.delta" => {
            if let Some(delta) = dashboard_text(&event.payload) {
                append_reasoning_delta(answer_buffer, assistant_id, delta, limit, false);
            }
        }
        "reasoning.available" => {
            if let Some(delta) = dashboard_text(&event.payload) {
                append_reasoning_delta(answer_buffer, assistant_id, delta, limit, true);
            }
        }
        "tool.progress" | "tool.generating" | "tool.start" => {
            apply_dashboard_tool_event(
                visible_messages,
                assistant_id,
                todos,
                subagents,
                &event,
                limit,
                show_tool_progress,
            );
        }
        "tool.complete" => {
            apply_dashboard_tool_event(
                visible_messages,
                assistant_id,
                todos,
                subagents,
                &event,
                limit,
                show_tool_progress,
            );
        }
        event_type if is_subagent_event(event_type) => {
            if event.session_id.is_some() {
                apply_dashboard_subagent_event(subagents, &event.payload, event_type);
            }
        }
        "approval.request" => {
            set_dashboard_approval(approval, &event, active_session_id)?;
        }
        "clarify.request" => {
            set_dashboard_clarification(approval, &event, active_session_id)?;
        }
        "sudo.request" => {
            set_dashboard_sensitive_input(approval, &event, active_session_id, ApprovalKind::Sudo)?;
        }
        "secret.request" => {
            set_dashboard_sensitive_input(
                approval,
                &event,
                active_session_id,
                ApprovalKind::Secret,
            )?;
        }
        "message.complete" => {
            complete_dashboard_message(answer_buffer, assistant_id, &event.payload, limit);
            return Ok(true);
        }
        "review.summary" => {
            append_dashboard_review_summary(visible_messages, &event, limit);
        }
        "error" => {
            let message = event
                .payload
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| event.payload.as_str())
                .unwrap_or("Hermes dashboard stream error")
                .to_owned();
            return Err(Error::Api {
                status: 500,
                message,
            });
        }
        other => {
            debug!(event = other, "Ignoring Hermes dashboard event");
        }
    }
    Ok(false)
}

fn apply_dashboard_tool_event(
    messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    todos: &Property<Vec<TodoItem>>,
    subagents: &Property<Vec<SubagentItem>>,
    event: &DashboardRpcEvent,
    limit: usize,
    show_tool_progress: bool,
) {
    if apply_dashboard_todo_event(todos, &event.payload) {
        return;
    }
    apply_dashboard_delegate_task_event(subagents, &event.payload, &event.event_type);
    if !show_tool_progress {
        return;
    }
    let tool_event = parse_dashboard_tool_event(&event.payload, &event.event_type);
    if !is_thought_tool_event(&tool_event) {
        push_tool_event(messages, assistant_id, tool_event, limit);
    }
}

fn set_dashboard_approval(
    approval: &Property<Option<ApprovalRequest>>,
    event: &DashboardRpcEvent,
    active_session_id: Option<&str>,
) -> Result<()> {
    let session_id = dashboard_event_session_id(event, active_session_id)
        .ok_or_else(|| Error::UnsupportedEvent(String::from("approval missing session id")))?;
    let prompt = tool_event_string(&event.payload, &["description", "command", "message"])
        .unwrap_or_else(|| String::from("Hermes is requesting approval"));
    approval.set(Some(ApprovalRequest {
        run_id: session_id,
        approval_id: None,
        prompt,
        kind: ApprovalKind::Approval,
    }));
    Ok(())
}

fn set_dashboard_clarification(
    approval: &Property<Option<ApprovalRequest>>,
    event: &DashboardRpcEvent,
    active_session_id: Option<&str>,
) -> Result<()> {
    let session_id = dashboard_event_session_id(event, active_session_id)
        .ok_or_else(|| Error::UnsupportedEvent(String::from("clarify missing session id")))?;
    let prompt = tool_event_string(&event.payload, &["question", "message", "prompt"])
        .unwrap_or_else(|| String::from("Hermes needs clarification"));
    approval.set(Some(ApprovalRequest {
        run_id: session_id,
        approval_id: event
            .payload
            .get("request_id")
            .or_else(|| event.payload.get("requestId"))
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .map(str::to_owned),
        prompt,
        kind: ApprovalKind::Clarification,
    }));
    Ok(())
}

fn set_dashboard_sensitive_input(
    approval: &Property<Option<ApprovalRequest>>,
    event: &DashboardRpcEvent,
    active_session_id: Option<&str>,
    kind: ApprovalKind,
) -> Result<()> {
    let session_id = dashboard_event_session_id(event, active_session_id)
        .ok_or_else(|| Error::UnsupportedEvent(String::from("input request missing session id")))?;
    let request_id = event
        .payload
        .get("request_id")
        .or_else(|| event.payload.get("requestId"))
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| Error::UnsupportedEvent(String::from("input request missing request id")))?
        .to_owned();
    let prompt = match kind {
        ApprovalKind::Sudo => String::from("Hermes needs a sudo password for this session."),
        ApprovalKind::Secret => {
            let name = tool_event_string(&event.payload, &["env_var", "envVar", "name"])
                .unwrap_or_else(|| String::from("secret"));
            let prompt = tool_event_string(&event.payload, &["prompt", "message"])
                .unwrap_or_else(|| format!("Hermes needs a value for {name}."));
            format!("{prompt}\n\nThe value will be sent to Hermes as a secret response.")
        }
        ApprovalKind::Approval | ApprovalKind::Clarification => {
            String::from("Hermes needs input for this session.")
        }
    };
    approval.set(Some(ApprovalRequest {
        run_id: session_id,
        approval_id: Some(request_id),
        prompt,
        kind,
    }));
    Ok(())
}

fn complete_dashboard_message(
    messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    payload: &Value,
    limit: usize,
) {
    ensure_streaming_assistant(messages, assistant_id, limit);
    let reasoning = dashboard_reasoning(payload).map(str::to_owned);
    mark_message(
        messages,
        assistant_id,
        MessageStatus::Complete,
        dashboard_text(payload)
            .filter(|text| !text.is_empty())
            .map(str::to_owned),
        limit,
    );
    if let Some(reasoning) = reasoning.filter(|reasoning| !reasoning.trim().is_empty()) {
        set_message_reasoning(messages, assistant_id, reasoning, limit);
    }
}

fn apply_dashboard_session_info(
    sessions: &Property<Vec<HermesSessionSummary>>,
    yolo_active: &Property<bool>,
    active_session_id: Option<&str>,
    event: &DashboardRpcEvent,
) -> bool {
    if event.event_type != "session.info" {
        return false;
    }
    let identities = dashboard_session_identities(event, active_session_id);
    update_session_from_info(sessions, &identities, &event.payload);
    update_yolo_from_info(yolo_active, &event.payload);
    true
}

fn append_dashboard_review_summary(
    messages: &Property<Vec<HermesMessage>>,
    event: &DashboardRpcEvent,
    limit: usize,
) -> bool {
    if event.event_type != "review.summary" {
        return false;
    }
    let Some(text) = dashboard_text(&event.payload)
        .or_else(|| event.payload.get("message").and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
    else {
        return true;
    };
    append_system_notice_to(messages, text.to_owned(), limit);
    true
}

fn dashboard_session_title(payload: &Value) -> Option<String> {
    string_field(payload, &["title", "name", "session_title", "sessionTitle"])
        .or_else(|| {
            payload.get("session").and_then(|session| {
                string_field(session, &["title", "name", "session_title", "sessionTitle"])
            })
        })
        .or_else(|| {
            payload.get("info").and_then(|info| {
                string_field(info, &["title", "name", "session_title", "sessionTitle"])
            })
        })
}

fn dashboard_session_identities(
    event: &DashboardRpcEvent,
    active_session_id: Option<&str>,
) -> Vec<String> {
    let mut ids = Vec::new();
    push_optional_id(&mut ids, event.session_id.as_deref());
    push_optional_id(&mut ids, active_session_id);
    push_optional_id(
        &mut ids,
        dashboard_session_id_from_value(&event.payload).as_deref(),
    );
    push_optional_id(
        &mut ids,
        dashboard_stored_session_id_from_value(&event.payload).as_deref(),
    );
    if let Some(session) = event.payload.get("session") {
        push_optional_id(
            &mut ids,
            dashboard_session_id_from_value(session).as_deref(),
        );
        push_optional_id(
            &mut ids,
            dashboard_stored_session_id_from_value(session).as_deref(),
        );
    }
    ids
}

fn update_session_from_info(
    sessions: &Property<Vec<HermesSessionSummary>>,
    identities: &[String],
    payload: &Value,
) {
    if identities.is_empty() {
        return;
    }
    let title = dashboard_session_title(payload);
    let is_active = dashboard_session_bool(payload, &["is_active", "isActive", "running"]);
    let needs_input = dashboard_session_bool(
        payload,
        &[
            "needs_input",
            "needsInput",
            "awaiting_input",
            "awaitingInput",
            "blocked",
        ],
    );

    let mut current = sessions.get();
    let mut updated = false;
    for session in &mut current {
        if identities.iter().any(|id| id == &session.id) {
            let mut session_updated = false;
            if let Some(title) = title
                .as_deref()
                .map(str::trim)
                .filter(|title| !title.is_empty())
            {
                session.title = title.to_owned();
                session_updated = true;
            }
            if let Some(is_active) = is_active {
                session.is_active = is_active;
                session_updated = true;
            }
            if let Some(needs_input) = needs_input {
                session.needs_input = needs_input;
                session_updated = true;
            }
            if session_updated {
                session.updated_at = Some(chrono::Utc::now());
                updated = true;
            }
        }
    }
    if updated {
        sessions.set(current);
    }
}

fn update_session_title_locally(
    sessions: &Property<Vec<HermesSessionSummary>>,
    identities: &[String],
    title: &str,
) {
    if identities.is_empty() {
        return;
    }

    let mut current = sessions.get();
    let mut updated = false;
    for session in &mut current {
        if identities.iter().any(|id| id == &session.id) {
            session.title = title.to_owned();
            session.updated_at = Some(chrono::Utc::now());
            updated = true;
        }
    }
    if updated {
        sessions.set(current);
    }
}

fn session_title_message(title: &str, queued: bool) -> String {
    let title = title.trim();
    if title.is_empty() {
        String::from("Session title cleared.")
    } else if queued {
        format!("Session title set: {title} (queued while session initializes)")
    } else {
        format!("Session title set: {title}")
    }
}

fn update_yolo_from_info(yolo_active: &Property<bool>, payload: &Value) {
    if let Some(active) = dashboard_yolo_active(payload) {
        yolo_active.set(active);
    }
}

fn dashboard_yolo_active(value: &Value) -> Option<bool> {
    bool_field(value, &["yolo"])
        .or_else(|| bool_field(value.get("session")?, &["yolo"]))
        .or_else(|| bool_field(value.get("info")?, &["yolo"]))
        .or_else(|| {
            value
                .get("value")
                .and_then(Value::as_str)
                .map(str::trim)
                .and_then(|value| match value {
                    "1" => Some(true),
                    "0" => Some(false),
                    _ => None,
                })
        })
}

fn yolo_armed_message(active: bool) -> &'static str {
    if active {
        "YOLO armed for this chat"
    } else {
        "YOLO off"
    }
}

fn yolo_session_message(active: bool) -> &'static str {
    if active {
        "YOLO on for this session"
    } else {
        "YOLO off for this session"
    }
}

fn dashboard_session_bool(payload: &Value, names: &[&str]) -> Option<bool> {
    bool_field(payload, names)
        .or_else(|| {
            payload
                .get("session")
                .and_then(|session| bool_field(session, names))
        })
        .or_else(|| payload.get("info").and_then(|info| bool_field(info, names)))
}

fn bool_field(value: &Value, names: &[&str]) -> Option<bool> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_bool))
}

fn apply_dashboard_todo_event(todos: &Property<Vec<TodoItem>>, payload: &Value) -> bool {
    let is_todo = tool_event_string(
        payload,
        &["name", "tool", "tool_name", "toolName", "function"],
    )
    .as_deref()
    .is_some_and(|tool| tool.eq_ignore_ascii_case("todo"))
        || (payload.get("todos").is_some()
            && tool_event_string(
                payload,
                &["name", "tool", "tool_name", "toolName", "function"],
            )
            .is_none());
    if !is_todo {
        return false;
    }

    if let Some(next_todos) = parse_todos_from_payload(payload) {
        todos.set(next_todos);
    }
    true
}

fn parse_todos_from_payload(payload: &Value) -> Option<Vec<TodoItem>> {
    parse_todos_value(payload, 0)
        .or_else(|| {
            payload
                .get("todos")
                .and_then(|value| parse_todos_value(value, 0))
        })
        .or_else(|| {
            payload
                .get("result")
                .and_then(|value| parse_todos_value(value, 0))
        })
        .or_else(|| {
            payload
                .get("args")
                .and_then(|value| parse_todos_value(value, 0))
        })
        .or_else(|| {
            payload
                .get("arguments")
                .and_then(|value| parse_todos_value(value, 0))
        })
}

fn parse_todos_value(value: &Value, depth: u8) -> Option<Vec<TodoItem>> {
    if depth > 2 {
        return None;
    }
    if let Some(items) = value.as_array() {
        return Some(items.iter().filter_map(parse_todo_item).collect());
    }
    if let Some(text) = value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|value| parse_todos_value(&value, depth + 1));
    }
    if value.is_object()
        && let Some(todos) = value.get("todos")
    {
        return parse_todos_value(todos, depth + 1);
    }
    None
}

fn parse_todo_item(value: &Value) -> Option<TodoItem> {
    let id = string_field(value, &["id"])?;
    let content = string_field(value, &["content", "text", "title"])?;
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .and_then(parse_todo_status)?;
    Some(TodoItem {
        id,
        content,
        status,
    })
}

fn parse_todo_status(status: &str) -> Option<TodoStatus> {
    match status
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_")
        .as_str()
    {
        "pending" => Some(TodoStatus::Pending),
        "in_progress" => Some(TodoStatus::InProgress),
        "completed" | "complete" | "done" => Some(TodoStatus::Completed),
        "cancelled" | "canceled" => Some(TodoStatus::Cancelled),
        _ => None,
    }
}

fn apply_dashboard_subagent_event(
    subagents: &Property<Vec<SubagentItem>>,
    payload: &Value,
    event_type: &str,
) {
    if let Some(item) = parse_subagent_item(payload, event_type) {
        upsert_subagent(subagents, item);
    }
}

fn apply_dashboard_delegate_task_event(
    subagents: &Property<Vec<SubagentItem>>,
    payload: &Value,
    event_type: &str,
) {
    if !tool_event_string(
        payload,
        &["name", "tool", "tool_name", "toolName", "function"],
    )
    .is_some_and(|tool| tool.eq_ignore_ascii_case("delegate_task"))
    {
        return;
    }

    let args = parse_record_value(payload.get("args").or_else(|| payload.get("input")));
    let result = parse_record_value(payload.get("result"));
    let tasks = args
        .as_ref()
        .and_then(|args| args.get("tasks"))
        .and_then(Value::as_array)
        .map(|tasks| {
            tasks
                .iter()
                .filter_map(|task| parse_record_value(Some(task)))
                .collect::<Vec<_>>()
        })
        .filter(|tasks| !tasks.is_empty())
        .unwrap_or_else(|| args.clone().into_iter().collect());

    let task_count = tasks.len().max(1) as u64;
    let tool_id = tool_event_string(payload, &["tool_id", "tool_call_id", "id"])
        .unwrap_or_else(|| String::from("delegate_task"));
    let status = if event_type == "tool.complete" {
        if payload
            .get("error")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            SubagentStatus::Failed
        } else {
            SubagentStatus::Completed
        }
    } else {
        SubagentStatus::Running
    };

    for (index, task) in tasks.iter().enumerate() {
        let goal = string_field(task, &["goal"])
            .or_else(|| args.as_ref().and_then(|args| string_field(args, &["goal"])))
            .or_else(|| string_field(payload, &["context", "message", "preview"]))
            .unwrap_or_else(|| String::from("Delegated task"));
        let summary = result
            .as_ref()
            .and_then(|result| string_field(result, &["summary", "message"]))
            .or_else(|| string_field(payload, &["summary", "message", "preview"]));
        upsert_subagent(
            subagents,
            SubagentItem {
                id: format!("delegate-tool:{tool_id}:{index}"),
                goal,
                status,
                current_tool: if matches!(status, SubagentStatus::Running | SubagentStatus::Queued)
                {
                    Some(String::from("delegate_task"))
                } else {
                    None
                },
                session_id: None,
                task_count: Some(task_count),
                task_index: Some(index as u64),
                summary,
            },
        );
    }
}

fn parse_subagent_item(payload: &Value, event_type: &str) -> Option<SubagentItem> {
    let status = parse_subagent_status(
        payload
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or(event_type),
        event_type,
    );
    let id = string_field(payload, &["subagent_id", "subagentId", "id"]).unwrap_or_else(|| {
        let parent = string_field(payload, &["parent_id", "parentId"])
            .unwrap_or_else(|| String::from("root"));
        let index = number_field(payload, &["task_index", "taskIndex"]).unwrap_or(0);
        let goal = string_field(payload, &["goal"]).unwrap_or_else(|| String::from("Subagent"));
        format!("{parent}:{index}:{goal}")
    });
    let goal =
        string_field(payload, &["goal", "title"]).unwrap_or_else(|| String::from("Subagent"));
    let current_tool = if matches!(
        status,
        SubagentStatus::Completed | SubagentStatus::Failed | SubagentStatus::Interrupted
    ) {
        None
    } else {
        string_field(
            payload,
            &["tool_name", "toolName", "current_tool", "currentTool"],
        )
    };

    Some(SubagentItem {
        id,
        goal,
        status,
        current_tool,
        session_id: string_field(
            payload,
            &["child_session_id", "childSessionId", "session_id"],
        ),
        task_count: number_field(payload, &["task_count", "taskCount"]),
        task_index: number_field(payload, &["task_index", "taskIndex"]),
        summary: string_field(payload, &["summary", "text", "message"]),
    })
}

fn parse_subagent_status(status: &str, event_type: &str) -> SubagentStatus {
    match status
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_")
        .as_str()
    {
        "queued" | "pending" | "subagent_spawn_requested" => SubagentStatus::Queued,
        "completed" | "complete" | "done" | "subagent_complete" if !event_type.contains("fail") => {
            SubagentStatus::Completed
        }
        "failed" | "failure" | "error" => SubagentStatus::Failed,
        "interrupted" | "cancelled" | "canceled" => SubagentStatus::Interrupted,
        _ if event_type == "subagent.complete" => SubagentStatus::Completed,
        _ => SubagentStatus::Running,
    }
}

fn upsert_subagent(subagents: &Property<Vec<SubagentItem>>, item: SubagentItem) {
    let mut current = subagents.get();
    if let Some(existing) = current.iter_mut().find(|existing| existing.id == item.id) {
        if matches!(
            existing.status,
            SubagentStatus::Completed | SubagentStatus::Failed | SubagentStatus::Interrupted
        ) && matches!(
            item.status,
            SubagentStatus::Running | SubagentStatus::Queued
        ) {
            return;
        }
        *existing = merge_subagent(existing, item);
    } else {
        current.push(item);
    }
    current.sort_by_key(|item| item.task_index.unwrap_or(0));
    subagents.set(current);
}

fn merge_subagent(existing: &SubagentItem, mut next: SubagentItem) -> SubagentItem {
    if next.goal.trim().is_empty() {
        next.goal.clone_from(&existing.goal);
    }
    if next.current_tool.is_none() {
        next.current_tool.clone_from(&existing.current_tool);
    }
    if next.session_id.is_none() {
        next.session_id.clone_from(&existing.session_id);
    }
    if next.task_count.is_none() {
        next.task_count = existing.task_count;
    }
    if next.task_index.is_none() {
        next.task_index = existing.task_index;
    }
    if next.summary.is_none() {
        next.summary.clone_from(&existing.summary);
    }
    next
}

fn is_subagent_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "subagent.spawn_requested"
            | "subagent.start"
            | "subagent.thinking"
            | "subagent.tool"
            | "subagent.progress"
            | "subagent.complete"
    )
}

fn parse_record_value(value: Option<&Value>) -> Option<Value> {
    let value = value?;
    if value.is_object() {
        return Some(value.clone());
    }
    value
        .as_str()
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .filter(Value::is_object)
}

fn number_field(value: &Value, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_u64))
}

fn push_optional_id(ids: &mut Vec<String>, value: Option<&str>) {
    let Some(id) = value.map(str::trim).filter(|id| !id.is_empty()) else {
        return;
    };
    if !ids.iter().any(|existing| existing == id) {
        ids.push(id.to_owned());
    }
}

fn string_field(value: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn dashboard_event_session_id(
    event: &DashboardRpcEvent,
    active_session_id: Option<&str>,
) -> Option<String> {
    event
        .session_id
        .as_deref()
        .or_else(|| {
            event
                .payload
                .get("session_id")
                .or_else(|| event.payload.get("sessionId"))
                .and_then(Value::as_str)
        })
        .or(active_session_id)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

fn dashboard_text(payload: &Value) -> Option<&str> {
    payload
        .get("text")
        .or_else(|| payload.get("delta"))
        .or_else(|| payload.get("content"))
        .or_else(|| payload.get("rendered"))
        .and_then(Value::as_str)
        .or_else(|| payload.as_str())
}

fn dashboard_reasoning(payload: &Value) -> Option<&str> {
    payload
        .get("reasoning")
        .or_else(|| payload.get("reasoning_content"))
        .or_else(|| payload.get("reasoningContent"))
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .pointer("/message/reasoning")
                .or_else(|| payload.pointer("/message/reasoning_content"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            payload
                .pointer("/choices/0/message/reasoning")
                .or_else(|| payload.pointer("/choices/0/message/reasoning_content"))
                .and_then(Value::as_str)
        })
}

fn parse_dashboard_tool_event(payload: &Value, event_name: &str) -> ToolEvent {
    let tool = tool_event_string(
        payload,
        &["name", "tool", "tool_name", "toolName", "function"],
    )
    .unwrap_or_else(|| String::from("tool"));
    let label = tool_event_string(
        payload,
        &[
            "summary",
            "message",
            "description",
            "progress",
            "preview",
            "command",
            "status_text",
        ],
    )
    .or_else(|| {
        payload
            .get("args")
            .or_else(|| payload.get("arguments"))
            .and_then(|args| tool_event_string(args, &["command", "query", "path", "url"]))
    })
    .unwrap_or_else(|| tool.clone());
    let id = tool_event_string(
        payload,
        &[
            "tool_id",
            "toolId",
            "tool_call_id",
            "toolCallId",
            "call_id",
            "id",
        ],
    )
    .unwrap_or_else(|| format!("{tool}:{label}"));
    let status = tool_event_string(payload, &["status", "state", "phase"])
        .unwrap_or_else(|| dashboard_tool_status(event_name, payload).to_owned());
    let command = nested_tool_event_string(payload, &["command"]);
    let path = nested_tool_event_string(payload, &["path", "file", "file_path", "target_path"]);
    let url = nested_tool_event_string(payload, &["url", "href"]);
    let input = nested_tool_event_string(payload, &["input", "query", "pattern", "preview"])
        .or_else(|| path.clone())
        .or_else(|| url.clone());
    let output = nested_tool_event_string(
        payload,
        &[
            "output",
            "output_tail",
            "result",
            "stdout",
            "stderr",
            "summary",
        ],
    );
    let error = tool_event_error(payload);
    ToolEvent {
        id,
        tool,
        label,
        status,
        command,
        input,
        output,
        error,
        path,
        url,
        has_inline_diff: payload
            .get("inline_diff")
            .and_then(Value::as_str)
            .is_some_and(|diff| !diff.trim().is_empty()),
        raw: compact_tool_payload(payload),
    }
}

fn dashboard_tool_status(event_name: &str, payload: &Value) -> &'static str {
    if payload.get("error").is_some()
        || payload
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| status.eq_ignore_ascii_case("error"))
    {
        "failed"
    } else if event_name == "tool.complete" {
        "completed"
    } else if event_name == "tool.generating" {
        "generating"
    } else {
        "running"
    }
}

fn dashboard_session_id_from_value(value: &Value) -> Option<String> {
    value
        .get("session_id")
        .or_else(|| value.get("sessionId"))
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("session")
                .and_then(|session| {
                    session
                        .get("session_id")
                        .or_else(|| session.get("sessionId"))
                        .or_else(|| session.get("id"))
                })
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

fn dashboard_stored_session_id_from_value(value: &Value) -> Option<String> {
    value
        .get("stored_session_id")
        .or_else(|| value.get("storedSessionId"))
        .or_else(|| value.get("session_key"))
        .or_else(|| value.get("sessionKey"))
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("session")
                .and_then(|session| {
                    session
                        .get("stored_session_id")
                        .or_else(|| session.get("storedSessionId"))
                        .or_else(|| session.get("session_key"))
                        .or_else(|| session.get("sessionKey"))
                })
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

fn dashboard_rpc_frame_error(error: Value) -> Error {
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .unwrap_or("dashboard request failed")
        .to_owned();
    Error::Api {
        status: 500,
        message,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedSlashLine {
    command: String,
    name: String,
    arg: String,
}

fn parse_slash_line(line: &str) -> ParsedSlashLine {
    let command = line.trim().trim_start_matches('/').trim().to_owned();
    let mut parts = command.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or_default().trim().to_owned();
    let arg = parts.next().unwrap_or_default().trim().to_owned();
    ParsedSlashLine { command, name, arg }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CommandDispatch {
    Exec {
        output: Option<String>,
    },
    Plugin {
        output: Option<String>,
    },
    Alias {
        target: String,
    },
    Skill {
        name: String,
        message: Option<String>,
    },
    Send {
        message: String,
        notice: Option<String>,
    },
    Prefill {
        message: String,
        notice: Option<String>,
    },
}

enum DispatchOutcome {
    Done,
    Alias(String),
}

fn parse_command_dispatch(value: &Value) -> Option<CommandDispatch> {
    let event_type = value.get("type").and_then(Value::as_str)?;
    match event_type {
        "exec" => Some(CommandDispatch::Exec {
            output: optional_string(value, "output"),
        }),
        "plugin" => Some(CommandDispatch::Plugin {
            output: optional_string(value, "output"),
        }),
        "alias" => value
            .get("target")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|target| !target.is_empty())
            .map(|target| CommandDispatch::Alias {
                target: target.to_owned(),
            }),
        "skill" => value
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| CommandDispatch::Skill {
                name: name.to_owned(),
                message: optional_string(value, "message"),
            }),
        "send" => {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(|message| CommandDispatch::Send {
                    message: message.to_owned(),
                    notice: optional_string(value, "notice"),
                })
        }
        "prefill" => {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(|message| CommandDispatch::Prefill {
                    message: message.to_owned(),
                    notice: optional_string(value, "notice"),
                })
        }
        _ => None,
    }
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn slash_exec_output(value: &Value, name: &str) -> String {
    let body = optional_string(value, "output").unwrap_or_else(|| format!("/{name}: no output"));
    if let Some(warning) = optional_string(value, "warning") {
        format!("warning: {warning}\n{body}")
    } else {
        body
    }
}

fn slash_commands_catalog_text(value: &Value) -> Option<String> {
    let sections = slash_command_sections(value);
    if sections.is_empty() {
        return None;
    }

    let mut lines = vec![String::from("Hermes slash commands:")];
    for (section, pairs) in sections {
        if !section.is_empty() {
            lines.push(String::new());
            lines.push(format!("{section}:"));
        }
        for (command, description) in pairs {
            if description.is_empty() {
                lines.push(format!("  {command}"));
            } else {
                lines.push(format!("  {command} - {description}"));
            }
        }
    }

    let skill_count = value
        .get("skill_count")
        .or_else(|| value.get("skillCount"))
        .and_then(Value::as_u64);
    if let Some(skill_count) = skill_count.filter(|count| *count > 0) {
        lines.push(String::new());
        lines.push(format!("{skill_count} skill/quick commands available."));
    }

    Some(lines.join("\n"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProfileCommandResult {
    content: String,
    selected_profile: Option<String>,
}

fn profile_command_result(
    profiles: &Value,
    active: Option<&Value>,
    selected: Option<&str>,
    arg: &str,
) -> ProfileCommandResult {
    let names = profile_names(profiles);
    if names.is_empty() {
        return ProfileCommandResult {
            content: String::from("No Hermes profiles are available from this dashboard."),
            selected_profile: None,
        };
    }

    let active_name = active_profile_name(active).unwrap_or_else(|| String::from("default"));
    let selected_name = selected
        .map(str::trim)
        .filter(|profile| !profile.is_empty())
        .unwrap_or(active_name.as_str());
    let target = arg.trim();

    if target.is_empty()
        || matches!(
            target.to_ascii_lowercase().as_str(),
            "list" | "ls" | "status"
        )
    {
        let mut lines = vec![
            format!("Active Hermes profile: {active_name}"),
            format!("New chats profile: {selected_name}"),
            String::new(),
        ];
        lines.push(String::from("Hermes profiles:"));
        lines.extend(
            names
                .iter()
                .map(|name| format!("{} {}", if name == selected_name { "*" } else { " " }, name)),
        );
        lines.push(String::new());
        lines.push(String::from(
            "Use /profile <name> to open new dashboard chats with that Hermes profile.",
        ));
        return ProfileCommandResult {
            content: lines.join("\n"),
            selected_profile: None,
        };
    }

    if names.iter().any(|name| name == target) {
        return ProfileCommandResult {
            content: format!("New Hermes chats will use profile: {target}"),
            selected_profile: Some(target.to_owned()),
        };
    }

    ProfileCommandResult {
        content: format!(
            "Unknown Hermes profile: {target}\nAvailable: {}",
            names.join(", ")
        ),
        selected_profile: None,
    }
}

fn profile_names(value: &Value) -> Vec<String> {
    value
        .get("profiles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|profile| {
            profile
                .as_str()
                .or_else(|| profile.get("name").and_then(Value::as_str))
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
        })
        .collect()
}

fn active_profile_name(value: Option<&Value>) -> Option<String> {
    let value = value?;
    value
        .get("current")
        .or_else(|| value.get("active"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

fn slash_command_sections(value: &Value) -> Vec<(String, Vec<(String, String)>)> {
    let mut sections = Vec::new();
    if let Some(categories) = value.get("categories").and_then(Value::as_array) {
        for category in categories {
            let pairs = slash_command_pairs(category.get("pairs").unwrap_or(&Value::Null));
            if pairs.is_empty() {
                continue;
            }
            let name = category
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_owned();
            sections.push((name, pairs));
        }
    }

    if sections.is_empty() {
        let pairs = slash_command_pairs(value.get("pairs").unwrap_or(value));
        if !pairs.is_empty() {
            sections.push((String::new(), pairs));
        }
    }

    sections
}

fn slash_command_pairs(value: &Value) -> Vec<(String, String)> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(slash_command_pair)
        .filter(|(command, _)| slash_command_visible(command))
        .collect()
}

fn slash_command_pair(value: &Value) -> Option<(String, String)> {
    if let Some(pair) = value.as_array() {
        let command = pair.first().and_then(Value::as_str)?;
        let description = pair.get(1).and_then(Value::as_str).unwrap_or_default();
        return slash_command_text(command).map(|command| {
            (
                command,
                description.trim().replace(['\r', '\n'], " ").to_owned(),
            )
        });
    }

    let command = value
        .get("command")
        .or_else(|| value.get("text"))
        .or_else(|| value.get("name"))
        .and_then(Value::as_str)?;
    let description = value
        .get("description")
        .or_else(|| value.get("meta"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    slash_command_text(command).map(|command| {
        (
            command,
            description.trim().replace(['\r', '\n'], " ").to_owned(),
        )
    })
}

fn slash_command_text(command: &str) -> Option<String> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    Some(if command.starts_with('/') {
        command.to_owned()
    } else {
        format!("/{command}")
    })
}

fn slash_command_visible(command: &str) -> bool {
    !matches!(
        command.to_ascii_lowercase().as_str(),
        "/approve"
            | "/busy"
            | "/clear"
            | "/compact"
            | "/config"
            | "/copy"
            | "/cron"
            | "/curator"
            | "/deny"
            | "/details"
            | "/exit"
            | "/fast"
            | "/footer"
            | "/gateway"
            | "/gquota"
            | "/history"
            | "/image"
            | "/indicator"
            | "/insights"
            | "/kanban"
            | "/logs"
            | "/model"
            | "/mouse"
            | "/paste"
            | "/platforms"
            | "/plugins"
            | "/quit"
            | "/reasoning"
            | "/redraw"
            | "/reload"
            | "/reload-mcp"
            | "/reload-skills"
            | "/reload_mcp"
            | "/reload_skills"
            | "/restart"
            | "/sb"
            | "/set-home"
            | "/sethome"
            | "/skills"
            | "/snap"
            | "/snapshot"
            | "/statusbar"
            | "/toolsets"
            | "/update"
            | "/verbose"
            | "/voice"
    )
}

const LOCAL_SLASH_COMMANDS: &[(&str, &str, &str)] = &[
    ("/new", "Start a new chat", "Local"),
    ("/reset", "Start a new chat", "Local"),
    (
        "/branch",
        "Branch the latest message into a new chat",
        "Local",
    ),
    ("/fork", "Alias for /branch", "Local"),
    ("/browser", "Manage local browser CDP connection", "Local"),
    (
        "/handoff",
        "Hand off this session to a messaging platform",
        "Local",
    ),
    (
        "/profile",
        "Set the Hermes profile used for new dashboard chats",
        "Local",
    ),
    ("/skin", "Switch or list Lumen themes", "Local"),
    (
        "/sessions",
        "List recent sessions and active work",
        "Sessions",
    ),
    (
        "/resume",
        "Switch to a recent session by id, title, or preview",
        "Sessions",
    ),
    ("/switch", "Alias for /resume", "Sessions"),
    ("/title", "Rename the current dashboard session", "Local"),
    ("/yolo", "Toggle per-session YOLO approval bypass", "Local"),
    (
        "/commands",
        "Alias for /help; show desktop slash commands",
        "Local",
    ),
    ("/help", "Show desktop slash commands", "Local"),
    (
        "/agents",
        "Show active desktop sessions and running tasks",
        "Hermes",
    ),
    ("/tasks", "Alias for /agents", "Hermes"),
    ("/background", "Run a prompt in the background", "Hermes"),
    ("/bg", "Alias for /background", "Hermes"),
    ("/btw", "Alias for /background", "Hermes"),
    ("/compress", "Compress this conversation context", "Hermes"),
    ("/debug", "Create a debug report", "Hermes"),
    (
        "/goal",
        "Manage the standing goal for this session",
        "Hermes",
    ),
    (
        "/personality",
        "Switch personality for this session",
        "Hermes",
    ),
    ("/queue", "Queue a prompt for the next turn", "Hermes"),
    ("/q", "Alias for /queue", "Hermes"),
    ("/retry", "Retry the last user message", "Hermes"),
    (
        "/rollback",
        "List or restore filesystem checkpoints",
        "Hermes",
    ),
    ("/save", "Save the current transcript to JSON", "Hermes"),
    (
        "/steer",
        "Steer the current run after the next tool call",
        "Hermes",
    ),
    ("/status", "Show current session status", "Hermes"),
    ("/stop", "Stop running background processes", "Hermes"),
    (
        "/tools",
        "List or toggle tools available to the agent",
        "Hermes",
    ),
    ("/undo", "Remove the last user/assistant exchange", "Hermes"),
    ("/usage", "Show token usage for this session", "Hermes"),
    ("/version", "Show Hermes Agent version", "Hermes"),
];

fn should_show_slash_suggestions(input: &str) -> bool {
    input.starts_with('/') && !input.contains('\n')
}

fn local_slash_suggestions(
    input: &str,
    sessions: &[HermesSessionSummary],
) -> Vec<SlashCommandSuggestion> {
    session_slash_suggestions(input, sessions).unwrap_or_else(|| {
        let needle = input.to_ascii_lowercase();
        LOCAL_SLASH_COMMANDS
            .iter()
            .filter(|(command, description, _)| {
                command.starts_with(&needle) || description.to_ascii_lowercase().contains(&needle)
            })
            .map(|(command, description, group)| SlashCommandSuggestion {
                insert_text: (*command).to_owned(),
                display: (*command).to_owned(),
                description: (*description).to_owned(),
                group: (*group).to_owned(),
            })
            .take(8)
            .collect()
    })
}

fn session_slash_suggestions(
    input: &str,
    sessions: &[HermesSessionSummary],
) -> Option<Vec<SlashCommandSuggestion>> {
    let trimmed = input.trim();
    let lower = trimmed.to_ascii_lowercase();
    for command in ["/resume", "/sessions", "/switch"] {
        let Some(rest) = lower.strip_prefix(command) else {
            continue;
        };
        if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
            continue;
        }

        let query = trimmed[command.len()..].trim().to_ascii_lowercase();
        let suggestions = sessions
            .iter()
            .filter(|session| {
                query.is_empty()
                    || session.id.to_ascii_lowercase().contains(&query)
                    || session.title.to_ascii_lowercase().contains(&query)
                    || session
                        .preview
                        .as_deref()
                        .is_some_and(|preview| preview.to_ascii_lowercase().contains(&query))
                    || session
                        .source
                        .as_deref()
                        .is_some_and(|source| source.to_ascii_lowercase().contains(&query))
            })
            .take(7)
            .map(|session| SlashCommandSuggestion {
                insert_text: format!("{command} {}", session.id),
                display: non_empty_trimmed(&session.title, "Hermes Chat"),
                description: session_suggestion_description(session),
                group: String::from("Sessions"),
            })
            .collect::<Vec<_>>();
        return Some(suggestions);
    }
    None
}

fn session_suggestion_description(session: &HermesSessionSummary) -> String {
    let mut parts = Vec::new();
    if let Some(preview) = session
        .preview
        .as_deref()
        .map(str::trim)
        .filter(|preview| !preview.is_empty())
    {
        parts.push(preview.to_owned());
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

fn non_empty_trimmed(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

fn catalog_slash_suggestions(value: &Value, filter: &str) -> Vec<SlashCommandSuggestion> {
    let needle = filter.trim().to_ascii_lowercase();
    slash_command_sections(value)
        .into_iter()
        .flat_map(|(group, pairs)| {
            let needle = needle.clone();
            pairs
                .into_iter()
                .filter(move |(command, description)| {
                    needle.is_empty()
                        || command.to_ascii_lowercase().contains(&needle)
                        || description.to_ascii_lowercase().contains(&needle)
                })
                .map(move |(command, description)| SlashCommandSuggestion {
                    insert_text: command.clone(),
                    display: command,
                    description,
                    group: group.clone(),
                })
        })
        .take(12)
        .collect()
}

fn complete_slash_suggestions(value: &Value, input: &str) -> Vec<SlashCommandSuggestion> {
    let replace_from = value
        .get("replace_from")
        .or_else(|| value.get("replaceFrom"))
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(1);
    let is_argument_completion = replace_from > 1;
    let prefix = if is_argument_completion {
        input
            .get(..replace_from.min(input.len()))
            .unwrap_or(input)
            .to_owned()
    } else {
        String::new()
    };

    let mut suggestions = value
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let raw_text = item
                .get("text")
                .or_else(|| item.get("command"))
                .or_else(|| item.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            if raw_text.is_empty() {
                return None;
            }

            let insert_text = if is_argument_completion {
                format!("{prefix}{raw_text}")
            } else {
                slash_command_text(raw_text)?
            };
            if !is_argument_completion && !slash_command_visible(&insert_text) {
                return None;
            }

            let display = item
                .get("display")
                .map(slash_suggestion_value_text)
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| insert_text.clone());
            let description = item
                .get("meta")
                .or_else(|| item.get("description"))
                .map(slash_suggestion_value_text)
                .unwrap_or_default();
            let group = item
                .get("group")
                .map(slash_suggestion_value_text)
                .filter(|group| !group.is_empty())
                .unwrap_or_else(|| {
                    if is_argument_completion {
                        String::from("Options")
                    } else if insert_text.contains(':') {
                        String::from("Skills")
                    } else {
                        String::from("Commands")
                    }
                });

            Some(SlashCommandSuggestion {
                insert_text,
                display,
                description,
                group,
            })
        })
        .collect::<Vec<_>>();

    dedupe_slash_suggestions(&mut suggestions);
    suggestions.truncate(12);
    suggestions
}

fn slash_suggestion_value_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.trim().to_owned();
    }
    if let Some(parts) = value.as_array() {
        return parts
            .iter()
            .filter_map(|part| {
                part.as_str()
                    .map(str::to_owned)
                    .or_else(|| part.as_array()?.get(1)?.as_str().map(str::to_owned))
            })
            .collect::<Vec<_>>()
            .join("")
            .trim()
            .to_owned();
    }
    String::new()
}

fn dedupe_slash_suggestions(suggestions: &mut Vec<SlashCommandSuggestion>) {
    let mut seen = Vec::<String>::new();
    suggestions.retain(|suggestion| {
        if seen.contains(&suggestion.insert_text) {
            return false;
        }
        seen.push(suggestion.insert_text.clone());
        true
    });
}

fn event_text_delta(data: &str) -> String {
    serde_json::from_str::<Value>(data)
        .ok()
        .and_then(|value| value_text_delta(&value).map(str::to_owned))
        .unwrap_or_else(|| data.to_owned())
}

fn value_text_delta(value: &Value) -> Option<&str> {
    value
        .get("delta")
        .or_else(|| value.get("content"))
        .or_else(|| value.get("text"))
        .and_then(Value::as_str)
        .or_else(|| value.as_str())
}

fn openai_delta(value: &Value) -> Option<String> {
    value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn is_tool_progress_event(event_name: &str) -> bool {
    matches!(
        event_name,
        "tool.progress"
            | "tool.started"
            | "tool.completed"
            | "tool.failed"
            | "tool.error"
            | "hermes.tool.progress"
            | "hermes.tool.started"
            | "hermes.tool.completed"
            | "hermes.tool.failed"
            | "mcp.tool.started"
            | "mcp.tool.progress"
            | "mcp.tool.completed"
            | "mcp.tool.failed"
    ) || (event_name.contains(".tool.")
        && [
            "progress",
            "started",
            "completed",
            "failed",
            "error",
            "queued",
            "pending",
        ]
        .iter()
        .any(|status| event_name.contains(status)))
}

fn parse_tool_event(data: &str, event_name: &str) -> Result<Option<ToolEvent>> {
    let value = serde_json::from_str::<Value>(data).unwrap_or_else(
        |_| json!({"label": data, "status": status_from_tool_event_name(event_name)}),
    );
    let tool = tool_event_string(
        &value,
        &["tool", "tool_name", "toolName", "name", "function"],
    )
    .or_else(|| {
        value
            .get("tool")
            .and_then(|tool| tool_event_string(tool, &["name", "tool_name", "toolName"]))
    })
    .unwrap_or_else(|| String::from("tool"));
    let label = tool_event_string(
        &value,
        &[
            "label",
            "message",
            "content",
            "description",
            "preview",
            "command",
            "input",
        ],
    )
    .or_else(|| {
        value
            .get("args")
            .or_else(|| value.get("arguments"))
            .and_then(|args| tool_event_string(args, &["command", "query", "path", "url"]))
    })
    .unwrap_or_else(|| tool.clone());
    let id = tool_event_string(
        &value,
        &[
            "toolCallId",
            "tool_call_id",
            "toolUseId",
            "tool_use_id",
            "call_id",
            "id",
            "invocation_id",
        ],
    )
    .unwrap_or_else(|| format!("{tool}:{label}"));
    let status = tool_event_string(&value, &["status", "state", "phase"])
        .unwrap_or_else(|| status_from_tool_event_name(event_name).to_owned());
    let command = nested_tool_event_string(&value, &["command"]);
    let path = nested_tool_event_string(&value, &["path", "file", "file_path", "target_path"]);
    let url = nested_tool_event_string(&value, &["url", "href"]);
    let input = nested_tool_event_string(&value, &["input", "query", "pattern", "preview"])
        .or_else(|| path.clone())
        .or_else(|| url.clone());
    let output = nested_tool_event_string(
        &value,
        &[
            "output",
            "output_tail",
            "result",
            "stdout",
            "stderr",
            "summary",
        ],
    );
    let error = tool_event_error(&value);
    let event = ToolEvent {
        id,
        tool,
        label,
        status,
        command,
        input,
        output,
        error,
        path,
        url,
        has_inline_diff: value
            .get("inline_diff")
            .and_then(Value::as_str)
            .is_some_and(|diff| !diff.trim().is_empty()),
        raw: compact_tool_payload(&value),
    };
    if is_thought_tool_event(&event) {
        return Ok(None);
    }
    Ok(Some(event))
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

fn tool_event_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        let value = value.get(*key)?;
        if let Some(text) = value.as_str().filter(|text| !text.trim().is_empty()) {
            Some(text.trim().to_owned())
        } else if value.is_number() || value.is_boolean() {
            Some(value.to_string())
        } else {
            None
        }
    })
}

fn nested_tool_event_string(value: &Value, keys: &[&str]) -> Option<String> {
    tool_event_string(value, keys).or_else(|| {
        value
            .get("args")
            .or_else(|| value.get("arguments"))
            .and_then(|args| tool_event_string(args, keys))
    })
}

fn tool_event_error(value: &Value) -> Option<String> {
    value
        .get("error")
        .and_then(|error| {
            error
                .as_str()
                .map(str::to_owned)
                .or_else(|| tool_event_string(error, &["message", "error", "description"]))
                .or_else(|| {
                    (!error.is_null())
                        .then(|| error.to_string().chars().take(500).collect::<String>())
                })
        })
        .filter(|error| !error.trim().is_empty())
}

fn compact_tool_payload(value: &Value) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let raw = value.to_string();
    (!raw.is_empty()).then(|| raw.chars().take(2_000).collect())
}

fn status_from_tool_event_name(event_name: &str) -> &'static str {
    if event_name.contains("completed") || event_name.contains("done") {
        "completed"
    } else if event_name.contains("failed") || event_name.contains("error") {
        "failed"
    } else if event_name.contains("queued") || event_name.contains("pending") {
        "queued"
    } else {
        "running"
    }
}

fn ensure_streaming_assistant(
    messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    limit: usize,
) {
    let mut current = messages.get();
    if current.iter().any(|message| message.id == assistant_id) {
        return;
    }
    let mut message = HermesMessage::new(assistant_id, HermesRole::Assistant, "");
    message.status = MessageStatus::Streaming;
    current.push(message);
    trim_history(&mut current, limit);
    messages.replace(current);
}

fn append_delta(
    messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    delta: &str,
    limit: usize,
) {
    if delta.is_empty() {
        return;
    }
    let mut current = messages.get();
    if let Some(message) = current
        .iter_mut()
        .find(|message| message.id == assistant_id)
    {
        message.content.push_str(delta);
        message.status = MessageStatus::Streaming;
    } else {
        let mut message = HermesMessage::new(assistant_id, HermesRole::Assistant, delta);
        message.status = MessageStatus::Streaming;
        current.push(message);
    }
    trim_history(&mut current, limit);
    messages.replace(current);
}

fn append_reasoning_delta(
    messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    delta: &str,
    limit: usize,
    replace: bool,
) {
    if delta.is_empty() {
        return;
    }
    let mut current = messages.get();
    if let Some(message) = current
        .iter_mut()
        .find(|message| message.id == assistant_id)
    {
        if replace && !message.content.trim().is_empty() {
            return;
        }
        if replace {
            message.reasoning = delta.to_owned();
        } else {
            message.reasoning.push_str(delta);
        }
        message.status = MessageStatus::Streaming;
    } else {
        let mut message = HermesMessage::new(assistant_id, HermesRole::Assistant, "");
        message.reasoning = delta.to_owned();
        message.status = MessageStatus::Streaming;
        current.push(message);
    }
    trim_history(&mut current, limit);
    messages.replace(current);
}

fn set_message_reasoning(
    messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    reasoning: String,
    limit: usize,
) {
    let mut current = messages.get();
    if let Some(message) = current
        .iter_mut()
        .find(|message| message.id == assistant_id)
    {
        message.reasoning = reasoning;
    } else {
        let mut message = HermesMessage::new(assistant_id, HermesRole::Assistant, "");
        message.reasoning = reasoning;
        message.status = MessageStatus::Complete;
        current.push(message);
    }
    trim_history(&mut current, limit);
    messages.replace(current);
}

fn append_user_message(messages: &Property<Vec<HermesMessage>>, content: String, limit: usize) {
    let content = content.trim().to_owned();
    if content.is_empty() {
        return;
    }
    let mut current = messages.get();
    current.push(HermesMessage::new(
        format!("local-user-{}", chrono::Utc::now().timestamp_millis()),
        HermesRole::User,
        content,
    ));
    trim_history(&mut current, limit);
    messages.replace(current);
}

fn append_system_message(
    messages: &Property<Vec<HermesMessage>>,
    content: impl Into<String>,
    limit: usize,
) {
    let content = content.into();
    if content.trim().is_empty() {
        return;
    }
    let mut current = messages.get();
    current.push(HermesMessage::new(
        format!("local-system-{}", chrono::Utc::now().timestamp_millis()),
        HermesRole::System,
        content,
    ));
    trim_history(&mut current, limit);
    messages.replace(current);
}

fn push_tool_event(
    messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    event: ToolEvent,
    limit: usize,
) {
    let mut current = messages.get();
    if let Some(message) = current
        .iter_mut()
        .find(|message| message.id == assistant_id)
    {
        if let Some(existing) = message
            .tool_events
            .iter_mut()
            .find(|existing| existing.id == event.id)
        {
            *existing = event;
        } else {
            message.tool_events.push(event);
        }
    } else {
        let mut message = HermesMessage::new(assistant_id, HermesRole::Assistant, "");
        message.status = MessageStatus::Streaming;
        message.tool_events.push(event);
        current.push(message);
    }
    trim_history(&mut current, limit);
    messages.replace(current);
}

fn mark_message(
    messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    status: MessageStatus,
    content: Option<String>,
    limit: usize,
) {
    let mut current = messages.get();
    if let Some(message) = current
        .iter_mut()
        .find(|message| message.id == assistant_id)
    {
        message.status = status;
        if let Some(content) = content {
            message.content = content;
        }
        finalize_tool_events(message, status);
    } else if let Some(content) = content.filter(|content| !content.is_empty()) {
        let mut message = HermesMessage::new(assistant_id, HermesRole::Assistant, content);
        message.status = status;
        current.push(message);
    }
    trim_history(&mut current, limit);
    messages.replace(current);
}

fn assistant_response_text(response: &Value) -> Option<String> {
    response
        .pointer("/message/content")
        .and_then(Value::as_str)
        .or_else(|| {
            response
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
        })
        .or_else(|| response.get("final_response").and_then(Value::as_str))
        .or_else(|| response.get("content").and_then(Value::as_str))
        .or_else(|| response.get("text").and_then(Value::as_str))
        .map(str::to_owned)
}

fn completed_event_text(value: &Value) -> Option<String> {
    assistant_response_text(value).or_else(|| {
        value
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| {
                messages.iter().rev().find_map(|message| {
                    (message.get("role").and_then(Value::as_str) == Some("assistant"))
                        .then(|| assistant_response_text(message))
                        .flatten()
                })
            })
    })
}

fn publish_collected_assistant(
    messages: &Property<Vec<HermesMessage>>,
    assistant_id: &str,
    buffered: &Property<Vec<HermesMessage>>,
    limit: usize,
) {
    let Some(mut message) = buffered
        .get()
        .into_iter()
        .find(|message| message.id == assistant_id)
    else {
        mark_message(messages, assistant_id, MessageStatus::Complete, None, limit);
        return;
    };
    message.status = MessageStatus::Complete;
    finalize_tool_events(&mut message, MessageStatus::Complete);
    merge_message(messages, message, limit);
}

fn merge_message(messages: &Property<Vec<HermesMessage>>, message: HermesMessage, limit: usize) {
    let mut current = messages.get();
    if let Some(existing) = current
        .iter_mut()
        .find(|existing| existing.id == message.id)
    {
        existing.status = message.status;
        if !message.content.is_empty() {
            existing.content = message.content;
        }
        if !message.reasoning.trim().is_empty() {
            existing.reasoning = message.reasoning;
        }
        for event in message.tool_events {
            if let Some(existing_event) = existing
                .tool_events
                .iter_mut()
                .find(|existing_event| existing_event.id == event.id)
            {
                *existing_event = event;
            } else {
                existing.tool_events.push(event);
            }
        }
        finalize_tool_events(existing, message.status);
    } else {
        current.push(message);
    }
    trim_history(&mut current, limit);
    messages.replace(current);
}

fn finalize_tool_events(message: &mut HermesMessage, message_status: MessageStatus) {
    let next_status = match message_status {
        MessageStatus::Complete => "completed",
        MessageStatus::Stopped => "cancelled",
        MessageStatus::Error => "failed",
        MessageStatus::Streaming => return,
    };
    for event in &mut message.tool_events {
        if !tool_event_status_is_finished(&event.status) {
            event.status = next_status.to_owned();
        }
    }
}

fn tool_event_status_is_finished(status: &str) -> bool {
    matches!(
        status
            .trim()
            .to_ascii_lowercase()
            .replace([' ', '_'], "-")
            .as_str(),
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

fn append_error(messages: &Property<Vec<HermesMessage>>, message: String, limit: usize) {
    let mut current = messages.get();
    let mut row = HermesMessage::new(
        format!("local-error-{}", chrono::Utc::now().timestamp_millis()),
        HermesRole::Error,
        message,
    );
    row.status = MessageStatus::Error;
    current.push(row);
    trim_history(&mut current, limit);
    messages.replace(current);
}

fn append_system_notice_to(
    messages: &Property<Vec<HermesMessage>>,
    content: impl Into<String>,
    limit: usize,
) -> Option<Vec<HermesMessage>> {
    let content = content.into();
    if content.trim().is_empty() {
        return None;
    }

    let mut current = messages.get();
    current.push(HermesMessage::new(
        format!("local-system-{}", chrono::Utc::now().timestamp_millis()),
        HermesRole::System,
        content,
    ));
    trim_history(&mut current, limit);
    messages.set(current.clone());
    Some(current)
}

fn trim_history(messages: &mut Vec<HermesMessage>, limit: usize) {
    if limit == 0 || messages.len() <= limit {
        return;
    }
    let remove = messages.len() - limit;
    messages.drain(0..remove);
}

fn status_from_error(err: &Error, message: String) -> HermesStatus {
    match err {
        Error::MissingApiKey => HermesStatus::MissingApiKey,
        Error::MissingDashboardToken => HermesStatus::AuthFailed,
        Error::Api { status: 401, .. } => HermesStatus::AuthFailed,
        Error::Http(_) | Error::WebSocket(_) => HermesStatus::Offline(message),
        _ => HermesStatus::Error(message),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs, process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use serde_json::json;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        task::JoinHandle,
    };

    use super::*;
    use crate::SseEvent;

    fn temp_history_path(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        std::env::temp_dir()
            .join(format!("lumen-hermes-{name}-{}-{millis}", process::id()))
            .join("history.json")
    }

    fn remove_temp_history(path: &std::path::Path) {
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }

    fn disabled_test_service() -> HermesChatService {
        HermesChatService::new(
            ConnectionConfig {
                enabled: false,
                api_key: Some(String::from("secret")),
                ..ConnectionConfig::default()
            },
            None,
        )
    }

    fn make_service_ready(service: &HermesChatService) {
        make_service_ready_with_transport(service, TransportMode::Auto);
    }

    fn make_service_ready_with_transport(
        service: &HermesChatService,
        transport_mode: TransportMode,
    ) {
        if let Ok(mut guard) = service.config.write() {
            *guard = ConnectionConfig {
                enabled: true,
                endpoint_url: String::from("http://127.0.0.1:9"),
                api_key: Some(String::from("secret")),
                transport_mode,
                ..ConnectionConfig::default()
            };
        }
    }

    fn arm_stream(service: &HermesChatService, stream_id: u64) -> CancellationToken {
        let token = CancellationToken::new();
        if let Ok(mut guard) = service.stream_token.lock() {
            *guard = Some(token.clone());
        }
        if let Ok(mut guard) = service.active_stream_id.write() {
            *guard = Some(stream_id);
        }
        token
    }

    fn test_session_summary(id: &str, message_count: Option<u64>) -> HermesSessionSummary {
        HermesSessionSummary {
            id: id.to_owned(),
            title: format!("Session {id}"),
            updated_at: Some(chrono::Utc::now()),
            is_active: false,
            needs_input: false,
            message_count,
            preview: None,
            source: None,
        }
    }

    #[test]
    fn session_summary_cache_reuses_fresh_entries_only() {
        let mut cache = SessionSummaryCache::default();
        assert!(cache.fresh_sessions().is_none());

        cache.replace(vec![test_session_summary("s1", Some(1))]);
        assert_eq!(cache.fresh_sessions().expect("fresh sessions")[0].id, "s1");

        cache.fetched_at =
            Some(Instant::now() - SESSION_SUMMARY_CACHE_TTL - Duration::from_secs(1));
        assert!(cache.fresh_sessions().is_none());
    }

    #[test]
    fn transcript_cache_uses_aliases_and_fingerprint_invalidation() {
        let mut cache = TranscriptCache::default();
        let fingerprint = SessionFingerprint {
            updated_at: Some(chrono::Utc::now()),
            message_count: Some(1),
        };
        let messages = vec![HermesMessage::new("m1", HermesRole::User, "hello")];

        cache.insert(
            String::from("runtime-1"),
            vec![String::from("stored-1")],
            Some(fingerprint.clone()),
            messages.clone(),
        );

        let cached = cache
            .lookup("stored-1", Some(&fingerprint))
            .expect("cached alias");
        assert_eq!(cached.messages, messages);
        assert!(!cached.needs_refresh);

        let changed_fingerprint = SessionFingerprint {
            message_count: Some(2),
            ..fingerprint
        };
        let cached = cache
            .lookup("runtime-1", Some(&changed_fingerprint))
            .expect("cached runtime id");
        assert!(cached.needs_refresh);
    }

    #[test]
    fn transcript_cache_expires_and_evicts_oldest_entries() {
        let mut cache = TranscriptCache::default();
        for index in 0..=TRANSCRIPT_CACHE_MAX_SESSIONS {
            cache.insert(
                format!("s{index}"),
                Vec::new(),
                None,
                vec![HermesMessage::new(
                    format!("m{index}"),
                    HermesRole::User,
                    format!("message {index}"),
                )],
            );
        }

        assert!(cache.lookup("s0", None).is_none());
        assert!(cache.lookup("s1", None).is_some());

        let entry = cache.entries.get_mut("s1").expect("cached entry");
        entry.fetched_at = Instant::now() - TRANSCRIPT_CACHE_TTL - Duration::from_secs(1);
        assert!(
            cache
                .lookup("s1", None)
                .expect("expired entry still paints")
                .needs_refresh
        );
    }

    #[test]
    fn selection_generation_accepts_only_latest_selection() {
        let config_sequence = Arc::new(AtomicU64::new(0));
        let select_sequence = Arc::new(AtomicU64::new(0));
        let first = select_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let second = select_sequence.fetch_add(1, Ordering::Relaxed) + 1;

        assert!(!selection_is_current(
            &config_sequence,
            0,
            &select_sequence,
            first
        ));
        assert!(selection_is_current(
            &config_sequence,
            0,
            &select_sequence,
            second
        ));

        config_sequence.fetch_add(1, Ordering::Relaxed);
        assert!(!selection_is_current(
            &config_sequence,
            0,
            &select_sequence,
            second
        ));
    }

    #[test]
    fn parses_slash_line_for_gateway_payloads() {
        assert_eq!(
            parse_slash_line("/goal write the implementation plan"),
            ParsedSlashLine {
                command: String::from("goal write the implementation plan"),
                name: String::from("goal"),
                arg: String::from("write the implementation plan"),
            }
        );
        assert_eq!(
            parse_slash_line("  //status  "),
            ParsedSlashLine {
                command: String::from("status"),
                name: String::from("status"),
                arg: String::new(),
            }
        );
    }

    #[test]
    fn parses_desktop_command_dispatch_shapes() {
        assert_eq!(
            parse_command_dispatch(&json!({
                "type": "send",
                "notice": "Goal set",
                "message": "write the implementation plan",
            })),
            Some(CommandDispatch::Send {
                message: String::from("write the implementation plan"),
                notice: Some(String::from("Goal set")),
            })
        );
        assert_eq!(
            parse_command_dispatch(&json!({
                "type": "alias",
                "target": "status",
            })),
            Some(CommandDispatch::Alias {
                target: String::from("status"),
            })
        );
        assert_eq!(
            parse_command_dispatch(&json!({
                "type": "prefill",
                "notice": "backed up 1 turn",
                "message": "edit me",
            })),
            Some(CommandDispatch::Prefill {
                message: String::from("edit me"),
                notice: Some(String::from("backed up 1 turn")),
            })
        );
        assert_eq!(
            parse_command_dispatch(&json!({
                "type": "prefill",
                "notice": "missing message",
            })),
            None
        );
    }

    #[test]
    fn prefill_dispatch_sets_composer_prefill_without_echoing_draft() {
        let messages = Property::new(Vec::new());
        let composer_prefill = Property::new(None);

        apply_prefill_dispatch(
            &messages,
            &composer_prefill,
            10,
            "edit",
            String::from("Refine the previous answer"),
            Some(String::from("Loaded edit draft")),
        );

        assert_eq!(
            composer_prefill.get().as_deref(),
            Some("Refine the previous answer")
        );
        let messages = messages.get();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "Loaded edit draft");
        assert!(!messages[0].content.contains("Refine the previous answer"));
    }

    #[test]
    fn prefill_dispatch_reports_empty_message() {
        let messages = Property::new(Vec::new());
        let composer_prefill = Property::new(None);

        apply_prefill_dispatch(
            &messages,
            &composer_prefill,
            10,
            "edit",
            String::from("  "),
            None,
        );

        assert_eq!(composer_prefill.get(), None);
        assert_eq!(messages.get()[0].content, "/edit: empty prefill");
    }

    #[test]
    fn parses_nested_todo_payloads() {
        let todos = parse_todos_from_payload(&json!({
            "name": "todo",
            "args": {
                "todos": "[{\"id\":\"a\",\"content\":\"Read code\",\"status\":\"in_progress\"},{\"id\":\"b\",\"content\":\"Run tests\",\"status\":\"pending\"}]"
            }
        }))
        .expect("todos parse");

        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].id, "a");
        assert_eq!(todos[0].content, "Read code");
        assert_eq!(todos[0].status, TodoStatus::InProgress);
        assert_eq!(todos[1].status, TodoStatus::Pending);
    }

    #[test]
    fn dashboard_todo_event_updates_todo_store_without_tool_row() {
        let messages = Property::new(Vec::new());
        let todos = Property::new(Vec::new());
        let subagents = Property::new(Vec::new());
        apply_dashboard_tool_event(
            &messages,
            "assistant-1",
            &todos,
            &subagents,
            &DashboardRpcEvent {
                event_type: String::from("tool.progress"),
                session_id: Some(String::from("runtime-1")),
                payload: json!({
                    "name": "todo",
                    "todos": [
                        {"id": "a", "content": "Read code", "status": "completed"},
                        {"id": "b", "content": "Run tests", "status": "in_progress"}
                    ],
                }),
            },
            10,
            true,
        );

        assert!(messages.get().is_empty());
        assert_eq!(todos.get().len(), 2);
        assert_eq!(todos.get()[0].status, TodoStatus::Completed);
        assert_eq!(todos.get()[1].status, TodoStatus::InProgress);
    }

    #[test]
    fn dashboard_subagent_event_updates_subagent_store() {
        let subagents = Property::new(Vec::new());
        apply_dashboard_subagent_event(
            &subagents,
            &json!({
                "subagent_id": "agent-1",
                "goal": "Audit the code path",
                "status": "running",
                "tool_name": "grep",
                "task_count": 2,
                "task_index": 0,
                "text": "Searching"
            }),
            "subagent.progress",
        );

        let current = subagents.get();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].id, "agent-1");
        assert_eq!(current[0].goal, "Audit the code path");
        assert_eq!(current[0].status, SubagentStatus::Running);
        assert_eq!(current[0].current_tool.as_deref(), Some("grep"));
        assert_eq!(current[0].summary.as_deref(), Some("Searching"));
    }

    #[test]
    fn dashboard_delegate_task_tool_event_creates_subagents() {
        let messages = Property::new(Vec::new());
        let todos = Property::new(Vec::new());
        let subagents = Property::new(Vec::new());

        apply_dashboard_tool_event(
            &messages,
            "assistant-1",
            &todos,
            &subagents,
            &DashboardRpcEvent {
                event_type: String::from("tool.start"),
                session_id: Some(String::from("runtime-1")),
                payload: json!({
                    "name": "delegate_task",
                    "tool_id": "delegate-1",
                    "args": {
                        "tasks": [
                            {"goal": "Inspect config"},
                            {"goal": "Run tests"}
                        ]
                    },
                    "preview": "Delegating"
                }),
            },
            10,
            true,
        );

        let current = subagents.get();
        assert_eq!(current.len(), 2);
        assert_eq!(current[0].id, "delegate-tool:delegate-1:0");
        assert_eq!(current[0].goal, "Inspect config");
        assert_eq!(current[0].status, SubagentStatus::Running);
        assert_eq!(current[0].current_tool.as_deref(), Some("delegate_task"));
        assert_eq!(current[0].task_count, Some(2));
        assert_eq!(current[1].goal, "Run tests");
        assert_eq!(messages.get()[0].tool_events[0].tool, "delegate_task");
    }

    #[test]
    fn slash_exec_output_preserves_warning() {
        assert_eq!(
            slash_exec_output(
                &json!({"warning": "limited", "output": "current status"}),
                "status",
            ),
            "warning: limited\ncurrent status"
        );
        assert_eq!(slash_exec_output(&json!({}), "usage"), "/usage: no output");
    }

    #[test]
    fn parses_browser_manage_args_like_desktop() {
        assert_eq!(
            parse_browser_manage_args("").expect("status"),
            BrowserManageRequest {
                action: BrowserManageAction::Status,
                url: None,
            }
        );
        assert_eq!(
            parse_browser_manage_args("connect").expect("default connect"),
            BrowserManageRequest {
                action: BrowserManageAction::Connect,
                url: Some(String::from("http://127.0.0.1:9222")),
            }
        );
        assert_eq!(
            parse_browser_manage_args("connect http://localhost:9333").expect("custom connect"),
            BrowserManageRequest {
                action: BrowserManageAction::Connect,
                url: Some(String::from("http://localhost:9333")),
            }
        );
        assert!(parse_browser_manage_args("launch").is_err());
    }

    #[test]
    fn browser_manage_output_matches_desktop_copy() {
        let output = browser_manage_output(
            &BrowserManageRequest {
                action: BrowserManageAction::Connect,
                url: Some(String::from("http://127.0.0.1:9222")),
            },
            &json!({
                "connected": true,
                "url": "http://127.0.0.1:9222",
                "messages": ["found browser"]
            }),
        );

        assert_eq!(
            output,
            "checking Chromium-family browser remote debugging at http://127.0.0.1:9222...\nfound browser\nBrowser connected to live Chromium-family browser via CDP\nEndpoint: http://127.0.0.1:9222\nnext browser tool call will use this CDP endpoint"
        );
    }

    #[test]
    fn dashboard_endpoint_loopback_guard_accepts_localhost_only() {
        assert!(dashboard_endpoint_is_loopback("http://127.0.0.1:8642"));
        assert!(dashboard_endpoint_is_loopback("http://localhost:8642/v1"));
        assert!(!dashboard_endpoint_is_loopback("https://example.com"));
    }

    #[test]
    fn handoff_state_and_messages_match_desktop_copy() {
        assert_eq!(
            handoff_state(&json!({"state": "COMPLETED"})).as_deref(),
            Some("completed")
        );
        assert_eq!(
            handoff_error(&json!({"error": "platform unavailable"})).as_deref(),
            Some("platform unavailable")
        );
        assert_eq!(
            handoff_success_message("telegram"),
            "Handed off to telegram. Resume here anytime."
        );
        assert_eq!(
            handoff_failed_message("platform unavailable"),
            "Handoff failed: platform unavailable"
        );
        assert_eq!(
            handoff_timed_out_message(),
            "Timed out waiting for the gateway. Is `hermes gateway` running?"
        );
    }

    #[test]
    fn local_slash_suggestions_include_desktop_backend_commands() {
        let suggestions = local_slash_suggestions("/stop", &[]);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].insert_text, "/stop");
        assert_eq!(
            suggestions[0].description,
            "Stop running background processes"
        );
        assert_eq!(suggestions[0].group, "Hermes");

        let suggestions = local_slash_suggestions("/tasks", &[]);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].insert_text, "/tasks");
        assert_eq!(suggestions[0].description, "Alias for /agents");

        let suggestions = local_slash_suggestions("/status", &[]);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].insert_text, "/status");
        assert_eq!(suggestions[0].description, "Show current session status");
        assert_eq!(suggestions[0].group, "Hermes");

        let suggestions = local_slash_suggestions("/usage", &[]);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].insert_text, "/usage");
        assert_eq!(
            suggestions[0].description,
            "Show token usage for this session"
        );
        assert_eq!(suggestions[0].group, "Hermes");

        let suggestions = local_slash_suggestions("/help", &[]);
        assert!(suggestions.iter().any(|suggestion| {
            suggestion.insert_text == "/help"
                && suggestion.description == "Show desktop slash commands"
        }));

        let suggestions = local_slash_suggestions("/commands", &[]);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].insert_text, "/commands");
        assert_eq!(
            suggestions[0].description,
            "Alias for /help; show desktop slash commands"
        );
    }

    #[test]
    fn local_slash_suggestions_include_profile_command() {
        let suggestions = local_slash_suggestions("/profile", &[]);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].insert_text, "/profile");
        assert_eq!(
            suggestions[0].description,
            "Set the Hermes profile used for new dashboard chats"
        );
    }

    #[test]
    fn profile_command_result_lists_and_selects_new_chat_profile() {
        let profiles = json!({
            "profiles": [
                {"name": "default"},
                {"name": "coder"}
            ]
        });
        let active = json!({"current": "coder"});

        let list = profile_command_result(&profiles, Some(&active), Some("default"), "");
        assert!(list.content.contains("Active Hermes profile: coder"));
        assert!(list.content.contains("New chats profile: default"));
        assert!(list.content.contains("* default"));
        assert!(list.content.contains("  coder"));
        assert_eq!(list.selected_profile, None);

        let known = profile_command_result(&profiles, Some(&active), None, "default");
        assert_eq!(known.content, "New Hermes chats will use profile: default");
        assert_eq!(known.selected_profile.as_deref(), Some("default"));

        let unknown = profile_command_result(&profiles, Some(&active), None, "missing");
        assert_eq!(
            unknown.content,
            "Unknown Hermes profile: missing\nAvailable: default, coder"
        );
        assert_eq!(unknown.selected_profile, None);
    }

    #[test]
    fn parse_background_processes_maps_gateway_process_list() {
        let processes = parse_background_processes(&json!({
            "processes": [
                {
                    "session_id": "proc-1",
                    "command": "npm run dev\n-- --host",
                    "status": "running",
                    "output_tail": "ready"
                },
                {
                    "session_id": "proc-2",
                    "command": "cargo test",
                    "status": "exited",
                    "exit_code": 0
                },
                {
                    "session_id": "proc-3",
                    "command": "npm run build",
                    "status": "exited",
                    "exit_code": 2,
                    "output_tail": "failed"
                }
            ]
        }));

        assert_eq!(processes.len(), 3);
        assert_eq!(processes[0].id, "proc-1");
        assert_eq!(processes[0].title, "npm run dev");
        assert_eq!(processes[0].status, BackgroundProcessStatus::Running);
        assert_eq!(processes[0].output.as_deref(), Some("ready"));
        assert_eq!(processes[1].status, BackgroundProcessStatus::Completed);
        assert_eq!(processes[2].status, BackgroundProcessStatus::Failed);
        assert_eq!(processes[2].exit_code, Some(2));
    }

    #[test]
    fn remove_background_process_updates_property() {
        let background_processes = Property::new(vec![
            BackgroundProcessItem {
                id: String::from("proc-1"),
                title: String::from("one"),
                status: BackgroundProcessStatus::Running,
                exit_code: None,
                output: None,
            },
            BackgroundProcessItem {
                id: String::from("proc-2"),
                title: String::from("two"),
                status: BackgroundProcessStatus::Completed,
                exit_code: Some(0),
                output: None,
            },
        ]);

        remove_background_process(&background_processes, "proc-1");

        let current = background_processes.get();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].id, "proc-2");
    }

    #[test]
    fn slash_commands_catalog_text_formats_desktop_catalog() {
        let text = slash_commands_catalog_text(&json!({
            "categories": [
                {
                    "name": "Session",
                    "pairs": [
                        ["/new", "Start a new desktop chat"],
                        ["/clear", "Clear terminal screen"]
                    ]
                },
                {
                    "name": "Skills",
                    "pairs": [
                        ["/ship-it", "Run release checklist"],
                        {"command": "gif-search", "description": "Search for a gif"}
                    ]
                }
            ],
            "skill_count": 2
        }))
        .expect("catalog formats");

        assert!(text.contains("Session:\n  /new - Start a new desktop chat"));
        assert!(text.contains("Skills:\n  /ship-it - Run release checklist"));
        assert!(text.contains("  /gif-search - Search for a gif"));
        assert!(text.contains("2 skill/quick commands available."));
        assert!(!text.contains("/clear"));
    }

    #[test]
    fn slash_commands_catalog_text_formats_flat_pairs() {
        let text = slash_commands_catalog_text(&json!({
            "pairs": [
                ["/usage", "Show usage"],
                ["/deny", "Messaging approval denial"],
                ["/reload-skills", "Reload skills"],
                ["/skills", "Open skills"],
                ["/toolsets", "Configure toolsets"],
                ["/voice", "Terminal voice mode"]
            ]
        }))
        .expect("catalog formats");

        assert!(text.contains("  /usage - Show usage"));
        assert!(!text.contains("/deny"));
        assert!(!text.contains("/reload-skills"));
        assert!(!text.contains("/skills"));
        assert!(!text.contains("/toolsets"));
        assert!(!text.contains("/voice"));
    }

    #[test]
    fn catalog_slash_suggestions_formats_desktop_catalog() {
        let suggestions = catalog_slash_suggestions(
            &json!({
                "categories": [
                    {
                        "name": "Session",
                        "pairs": [
                            ["/new", "Start a new desktop chat"],
                            ["/clear", "Clear terminal screen"]
                        ]
                    },
                    {
                        "name": "Skills",
                        "pairs": [
                            {"command": "ship-it", "description": "Run release checklist"}
                        ]
                    }
                ]
            }),
            "",
        );

        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].insert_text, "/new");
        assert_eq!(suggestions[0].group, "Session");
        assert_eq!(suggestions[1].insert_text, "/ship-it");
        assert_eq!(suggestions[1].description, "Run release checklist");
    }

    #[test]
    fn complete_slash_suggestions_rewrites_argument_items() {
        let suggestions = complete_slash_suggestions(
            &json!({
                "replace_from": 13,
                "items": [
                    {"text": "planner", "display": "planner", "meta": "Planning mode"},
                    {"text": "coder", "display": "coder", "meta": [["plain", "Coding mode"]]}
                ]
            }),
            "/personality pl",
        );

        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].insert_text, "/personality planner");
        assert_eq!(suggestions[0].group, "Options");
        assert_eq!(suggestions[1].description, "Coding mode");
    }

    #[test]
    fn session_slash_suggestions_include_active_state() {
        let sessions = vec![HermesSessionSummary {
            id: String::from("session-1"),
            title: String::from("Release prep"),
            updated_at: None,
            is_active: true,
            needs_input: false,
            message_count: Some(3),
            preview: None,
            source: Some(String::from("desktop")),
        }];

        let suggestions =
            session_slash_suggestions("/resume rel", &sessions).expect("session suggestions");

        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].insert_text, "/resume session-1");
        assert_eq!(suggestions[0].display, "Release prep");
        assert_eq!(suggestions[0].description, "active - 3 msgs - desktop");
    }

    #[test]
    fn session_slash_suggestions_match_and_show_preview() {
        let sessions = vec![HermesSessionSummary {
            id: String::from("session-1"),
            title: String::from("Release prep"),
            updated_at: None,
            is_active: false,
            needs_input: false,
            message_count: Some(2),
            preview: Some(String::from("Fix the packaging release notes")),
            source: Some(String::from("desktop")),
        }];

        let suggestions =
            session_slash_suggestions("/resume packaging", &sessions).expect("session suggestions");

        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].insert_text, "/resume session-1");
        assert_eq!(
            suggestions[0].description,
            "Fix the packaging release notes - 2 msgs - desktop"
        );
    }

    async fn json_server_once(body: Value) -> (String, JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("server address");
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept request");
            let mut buffer = [0; 4096];
            let bytes = socket.read(&mut buffer).await.expect("read request");
            let request = String::from_utf8_lossy(&buffer[..bytes]).to_string();
            let body = body.to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write response");
            request
        });
        (format!("http://{address}"), handle)
    }

    async fn auto_fallback_server() -> (String, JoinHandle<Vec<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("server address");
        let handle = tokio::spawn(async move {
            let mut requests = Vec::new();
            for _ in 0..3 {
                let (mut socket, _) = listener.accept().await.expect("accept request");
                let mut buffer = [0; 4096];
                let bytes = socket.read(&mut buffer).await.expect("read request");
                let request = String::from_utf8_lossy(&buffer[..bytes]).to_string();
                let request_line = request.lines().next().unwrap_or_default().to_owned();
                let (status, body) = if request_line.starts_with("GET /v1/models ") {
                    (
                        "200 OK",
                        json!({"data": [{"id": "hermes-agent"}]}).to_string(),
                    )
                } else {
                    ("404 Not Found", json!({"message": "not found"}).to_string())
                };
                requests.push(request_line);
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("write response");
            }
            requests
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn applies_openai_delta() {
        let messages = Property::new(vec![HermesMessage::new("a", HermesRole::Assistant, "")]);
        apply_event(
            &messages,
            "a",
            SseEvent {
                event: None,
                data: String::from(
                    r#"{"object":"chat.completion.chunk","choices":[{"delta":{"content":"hi"}}]}"#,
                ),
            },
            10,
            true,
            &Property::new(None),
            None,
        )
        .expect("event applies");
        assert_eq!(messages.get()[0].content, "hi");
    }

    #[test]
    fn applies_default_message_content_delta() {
        let messages = Property::new(vec![HermesMessage::new("a", HermesRole::Assistant, "")]);
        apply_event(
            &messages,
            "a",
            SseEvent {
                event: None,
                data: String::from(r#"{"content":"hello"}"#),
            },
            10,
            true,
            &Property::new(None),
            None,
        )
        .expect("event applies");
        apply_event(
            &messages,
            "a",
            SseEvent {
                event: None,
                data: String::from(r#"{"text":" world"}"#),
            },
            10,
            true,
            &Property::new(None),
            None,
        )
        .expect("event applies");
        assert_eq!(messages.get()[0].content, "hello world");
    }

    #[test]
    fn applies_raw_text_delta_payloads() {
        let messages = Property::new(vec![HermesMessage::new("a", HermesRole::Assistant, "")]);
        apply_event(
            &messages,
            "a",
            SseEvent {
                event: Some(String::from("assistant.delta")),
                data: String::from("hello"),
            },
            10,
            true,
            &Property::new(None),
            None,
        )
        .expect("named raw delta applies");
        apply_event(
            &messages,
            "a",
            SseEvent {
                event: None,
                data: String::from(" world"),
            },
            10,
            true,
            &Property::new(None),
            None,
        )
        .expect("default raw delta applies");
        assert_eq!(messages.get()[0].content, "hello world");
    }

    #[test]
    fn updates_tool_event_by_id() {
        let messages = Property::new(vec![HermesMessage::new("a", HermesRole::Assistant, "")]);
        let approval = Property::new(None);
        apply_event(
            &messages,
            "a",
            SseEvent {
                event: Some(String::from("tool.progress")),
                data: String::from(r#"{"tool":"terminal","toolCallId":"t1","status":"running"}"#),
            },
            10,
            true,
            &approval,
            None,
        )
        .expect("first applies");
        apply_event(
            &messages,
            "a",
            SseEvent {
                event: Some(String::from("tool.completed")),
                data: String::from(r#"{"tool":"terminal","toolCallId":"t1","status":"completed"}"#),
            },
            10,
            true,
            &approval,
            None,
        )
        .expect("second applies");
        let message = messages.get().remove(0);
        assert_eq!(message.tool_events.len(), 1);
        assert_eq!(message.tool_events[0].status, "completed");
    }

    #[test]
    fn skips_tool_events_when_progress_hidden() {
        let messages = Property::new(vec![HermesMessage::new("a", HermesRole::Assistant, "")]);
        apply_event(
            &messages,
            "a",
            SseEvent {
                event: Some(String::from("tool.progress")),
                data: String::from(r#"{"tool":"terminal","toolCallId":"t1","status":"running"}"#),
            },
            10,
            false,
            &Property::new(None),
            None,
        )
        .expect("event applies");
        assert!(messages.get()[0].tool_events.is_empty());
    }

    #[test]
    fn parses_gateway_tool_progress_shapes() {
        let messages = Property::new(vec![HermesMessage::new("a", HermesRole::Assistant, "")]);
        apply_event(
            &messages,
            "a",
            SseEvent {
                event: Some(String::from("mcp.tool.started")),
                data: String::from(
                    r#"{"name":"web_search","id":"call-1","message":"Searching docs"}"#,
                ),
            },
            10,
            true,
            &Property::new(None),
            None,
        )
        .expect("event applies");

        let message = messages.get().remove(0);
        assert_eq!(message.tool_events.len(), 1);
        assert_eq!(message.tool_events[0].tool, "web_search");
        assert_eq!(message.tool_events[0].label, "Searching docs");
        assert_eq!(message.tool_events[0].status, "running");
    }

    #[test]
    fn parses_raw_tool_progress_text() {
        let messages = Property::new(vec![HermesMessage::new("a", HermesRole::Assistant, "")]);
        apply_event(
            &messages,
            "a",
            SseEvent {
                event: Some(String::from("tool.completed")),
                data: String::from("execute_code..."),
            },
            10,
            true,
            &Property::new(None),
            None,
        )
        .expect("event applies");

        let message = messages.get().remove(0);
        assert_eq!(message.tool_events.len(), 1);
        assert_eq!(message.tool_events[0].label, "execute_code...");
        assert_eq!(message.tool_events[0].status, "completed");
    }

    #[test]
    fn dashboard_delta_creates_assistant_only_after_remote_event() {
        let messages = Property::new(vec![HermesMessage::new("u", HermesRole::User, "hello")]);
        let complete = apply_dashboard_event(
            &messages,
            "a",
            DashboardRpcEvent {
                event_type: String::from("message.delta"),
                session_id: Some(String::from("sid")),
                payload: json!({"text": "hi"}),
            },
            10,
            true,
            &Property::new(None),
            &Property::new(Vec::new()),
            None,
        )
        .expect("dashboard event applies");

        let current = messages.get();
        assert!(!complete);
        assert_eq!(current.len(), 2);
        assert_eq!(current[1].id, "a");
        assert_eq!(current[1].role, HermesRole::Assistant);
        assert_eq!(current[1].content, "hi");
        assert_eq!(current[1].status, MessageStatus::Streaming);
    }

    #[test]
    fn dashboard_message_start_does_not_create_assistant_placeholder() {
        let messages = Property::new(vec![HermesMessage::new("u", HermesRole::User, "hello")]);
        apply_dashboard_event(
            &messages,
            "a",
            DashboardRpcEvent {
                event_type: String::from("message.start"),
                session_id: Some(String::from("sid")),
                payload: json!({}),
            },
            10,
            true,
            &Property::new(None),
            &Property::new(Vec::new()),
            None,
        )
        .expect("start applies");

        assert_eq!(messages.get().len(), 1);
    }

    #[test]
    fn dashboard_session_info_updates_matching_title() {
        let sessions = Property::new(vec![HermesSessionSummary {
            id: String::from("stored-1"),
            title: String::from("Hermes Chat"),
            updated_at: None,
            is_active: false,
            needs_input: false,
            message_count: None,
            preview: None,
            source: None,
        }]);

        let handled = apply_dashboard_session_info(
            &sessions,
            &Property::new(false),
            Some("stored-1"),
            &DashboardRpcEvent {
                event_type: String::from("session.info"),
                session_id: Some(String::from("runtime-1")),
                payload: json!({
                    "stored_session_id": "stored-1",
                    "title": "CEO of ZuluDesk",
                    "running": true,
                    "needs_input": true,
                }),
            },
        );

        let current = sessions.get();
        assert!(handled);
        assert_eq!(current[0].title, "CEO of ZuluDesk");
        assert!(current[0].is_active);
        assert!(current[0].needs_input);
        assert!(current[0].updated_at.is_some());
    }

    #[test]
    fn dashboard_session_info_ignores_missing_title() {
        let sessions = Property::new(vec![HermesSessionSummary {
            id: String::from("stored-1"),
            title: String::from("Hermes Chat"),
            updated_at: None,
            is_active: true,
            needs_input: true,
            message_count: None,
            preview: None,
            source: None,
        }]);

        let handled = apply_dashboard_session_info(
            &sessions,
            &Property::new(false),
            Some("stored-1"),
            &DashboardRpcEvent {
                event_type: String::from("session.info"),
                session_id: Some(String::from("runtime-1")),
                payload: json!({"running": false}),
            },
        );

        assert!(handled);
        assert_eq!(sessions.get()[0].title, "Hermes Chat");
        assert!(!sessions.get()[0].is_active);
        assert!(sessions.get()[0].needs_input);
    }

    #[test]
    fn update_session_title_locally_updates_matching_session() {
        let sessions = Property::new(vec![
            HermesSessionSummary {
                id: String::from("session-1"),
                title: String::from("Old title"),
                updated_at: None,
                is_active: false,
                needs_input: false,
                message_count: None,
                preview: None,
                source: None,
            },
            HermesSessionSummary {
                id: String::from("session-2"),
                title: String::from("Other title"),
                updated_at: None,
                is_active: false,
                needs_input: false,
                message_count: None,
                preview: None,
                source: None,
            },
        ]);

        update_session_title_locally(&sessions, &[String::from("session-1")], "New title");

        let current = sessions.get();
        assert_eq!(current[0].title, "New title");
        assert!(current[0].updated_at.is_some());
        assert_eq!(current[1].title, "Other title");
    }

    #[test]
    fn session_title_message_matches_desktop_copy() {
        assert_eq!(
            session_title_message("New title", false),
            "Session title set: New title"
        );
        assert_eq!(
            session_title_message("Fresh chat", true),
            "Session title set: Fresh chat (queued while session initializes)"
        );
        assert_eq!(session_title_message("", false), "Session title cleared.");
    }

    #[test]
    fn dashboard_yolo_active_parses_config_and_session_info_shapes() {
        assert_eq!(dashboard_yolo_active(&json!({"value": "1"})), Some(true));
        assert_eq!(dashboard_yolo_active(&json!({"value": "0"})), Some(false));
        assert_eq!(dashboard_yolo_active(&json!({"yolo": true})), Some(true));
        assert_eq!(
            dashboard_yolo_active(&json!({"session": {"yolo": false}})),
            Some(false)
        );
        assert_eq!(
            dashboard_yolo_active(&json!({"info": {"yolo": true}})),
            Some(true)
        );
    }

    #[test]
    fn dashboard_session_info_updates_yolo_state() {
        let sessions = Property::new(vec![HermesSessionSummary {
            id: String::from("stored-1"),
            title: String::from("Hermes Chat"),
            updated_at: None,
            is_active: false,
            needs_input: false,
            message_count: None,
            preview: None,
            source: None,
        }]);
        let yolo_active = Property::new(false);

        let handled = apply_dashboard_session_info(
            &sessions,
            &yolo_active,
            Some("stored-1"),
            &DashboardRpcEvent {
                event_type: String::from("session.info"),
                session_id: Some(String::from("runtime-1")),
                payload: json!({"stored_session_id": "stored-1", "yolo": true}),
            },
        );

        assert!(handled);
        assert!(yolo_active.get());
    }

    #[test]
    fn yolo_messages_match_desktop_copy() {
        assert_eq!(yolo_armed_message(true), "YOLO armed for this chat");
        assert_eq!(yolo_armed_message(false), "YOLO off");
        assert_eq!(yolo_session_message(true), "YOLO on for this session");
        assert_eq!(yolo_session_message(false), "YOLO off for this session");
    }

    #[test]
    fn branch_seed_message_uses_latest_user_or_assistant_text() {
        let messages = vec![
            HermesMessage::new("system", HermesRole::System, "context"),
            HermesMessage::new("user", HermesRole::User, " earlier prompt "),
            HermesMessage::new("assistant", HermesRole::Assistant, " latest answer "),
            HermesMessage::new("tool", HermesRole::Tool, "tool output"),
        ];

        assert_eq!(
            branch_seed_message(&messages),
            Some(BranchSeedMessage {
                role: HermesRole::Assistant,
                content: String::from("latest answer"),
            })
        );
    }

    #[test]
    fn branch_seed_message_rejects_empty_latest_text() {
        let messages = vec![
            HermesMessage::new("user", HermesRole::User, "earlier prompt"),
            HermesMessage::new("assistant", HermesRole::Assistant, "   "),
        ];

        assert_eq!(branch_seed_message(&messages), None);
    }

    #[test]
    fn branch_seed_to_messages_creates_local_branch_transcript() {
        let seed = BranchSeedMessage {
            role: HermesRole::User,
            content: String::from("branch this"),
        };

        let messages = branch_seed_to_messages(&seed);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, HermesRole::User);
        assert_eq!(messages[0].content, "branch this");
        assert!(messages[0].id.starts_with("local-branch-user-"));
    }

    #[test]
    fn completing_message_finishes_running_tool_events() {
        let mut assistant = HermesMessage::new("a", HermesRole::Assistant, "done");
        assistant.tool_events.push(ToolEvent {
            id: String::from("thinking"),
            tool: String::from("thinking"),
            label: String::from("thinking"),
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
        let messages = Property::new(vec![assistant]);

        mark_message(&messages, "a", MessageStatus::Complete, None, 10);

        let message = messages.get().remove(0);
        assert_eq!(message.status, MessageStatus::Complete);
        assert_eq!(message.tool_events[0].status, "completed");
    }

    #[test]
    fn dashboard_reasoning_events_and_complete_update_assistant() {
        let messages = Property::new(vec![HermesMessage::new("u", HermesRole::User, "hello")]);
        apply_dashboard_event(
            &messages,
            "a",
            DashboardRpcEvent {
                event_type: String::from("reasoning.delta"),
                session_id: Some(String::from("sid")),
                payload: json!({"text": "thinking"}),
            },
            10,
            true,
            &Property::new(None),
            &Property::new(Vec::new()),
            None,
        )
        .expect("reasoning applies");
        apply_dashboard_event(
            &messages,
            "a",
            DashboardRpcEvent {
                event_type: String::from("reasoning.available"),
                session_id: Some(String::from("sid")),
                payload: json!({"text": "full thinking"}),
            },
            10,
            true,
            &Property::new(None),
            &Property::new(Vec::new()),
            None,
        )
        .expect("available reasoning applies");
        let complete = apply_dashboard_event(
            &messages,
            "a",
            DashboardRpcEvent {
                event_type: String::from("message.complete"),
                session_id: Some(String::from("sid")),
                payload: json!({"text": "done", "reasoning": "final reasoning"}),
            },
            10,
            true,
            &Property::new(None),
            &Property::new(Vec::new()),
            None,
        )
        .expect("complete applies");

        let message = messages.get().remove(1);
        assert!(complete);
        assert_eq!(message.content, "done");
        assert_eq!(message.reasoning, "final reasoning");
        assert_eq!(message.status, MessageStatus::Complete);
    }

    #[test]
    fn dashboard_reasoning_available_does_not_replace_after_answer_text() {
        let messages = Property::new(vec![HermesMessage::new("u", HermesRole::User, "hello")]);
        apply_dashboard_event(
            &messages,
            "a",
            DashboardRpcEvent {
                event_type: String::from("reasoning.delta"),
                session_id: Some(String::from("sid")),
                payload: json!({"text": "initial thinking"}),
            },
            10,
            true,
            &Property::new(None),
            &Property::new(Vec::new()),
            None,
        )
        .expect("reasoning applies");
        apply_dashboard_event(
            &messages,
            "a",
            DashboardRpcEvent {
                event_type: String::from("message.delta"),
                session_id: Some(String::from("sid")),
                payload: json!({"text": "answer"}),
            },
            10,
            true,
            &Property::new(None),
            &Property::new(Vec::new()),
            None,
        )
        .expect("answer applies");
        apply_dashboard_event(
            &messages,
            "a",
            DashboardRpcEvent {
                event_type: String::from("reasoning.available"),
                session_id: Some(String::from("sid")),
                payload: json!({"text": "replacement thinking"}),
            },
            10,
            true,
            &Property::new(None),
            &Property::new(Vec::new()),
            None,
        )
        .expect("available reasoning applies");

        let message = messages.get().remove(1);
        assert_eq!(message.content, "answer");
        assert_eq!(message.reasoning, "initial thinking");
        assert_eq!(message.status, MessageStatus::Streaming);
    }

    #[test]
    fn dashboard_buffered_reasoning_publishes_with_answer() {
        let visible = Property::new(vec![HermesMessage::new("u", HermesRole::User, "hello")]);
        let buffered = Property::new(Vec::new());
        let approval = Property::new(None);
        let todos = Property::new(Vec::new());
        let subagents = Property::new(Vec::new());

        apply_dashboard_event_with_answer_buffer(
            &buffered,
            &visible,
            "a",
            DashboardRpcEvent {
                event_type: String::from("reasoning.delta"),
                session_id: Some(String::from("sid")),
                payload: json!({"text": "thinking"}),
            },
            10,
            true,
            &approval,
            &todos,
            &subagents,
            None,
        )
        .expect("reasoning applies");
        let complete = apply_dashboard_event_with_answer_buffer(
            &buffered,
            &visible,
            "a",
            DashboardRpcEvent {
                event_type: String::from("message.complete"),
                session_id: Some(String::from("sid")),
                payload: json!({"text": "done"}),
            },
            10,
            true,
            &approval,
            &todos,
            &subagents,
            None,
        )
        .expect("complete applies");

        assert!(complete);
        publish_collected_assistant(&visible, "a", &buffered, 10);
        let message = visible.get().remove(1);
        assert_eq!(message.content, "done");
        assert_eq!(message.reasoning, "thinking");
        assert_eq!(message.status, MessageStatus::Complete);
    }

    #[test]
    fn dashboard_review_summary_appends_system_message() {
        let messages = Property::new(vec![HermesMessage::new("u", HermesRole::User, "hello")]);
        let complete = apply_dashboard_event(
            &messages,
            "a",
            DashboardRpcEvent {
                event_type: String::from("review.summary"),
                session_id: Some(String::from("sid")),
                payload: json!({"text": "Self-improvement review: saved a lesson."}),
            },
            10,
            true,
            &Property::new(None),
            &Property::new(Vec::new()),
            None,
        )
        .expect("review summary applies");

        let rows = messages.get();
        assert!(!complete);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].role, HermesRole::System);
        assert_eq!(rows[1].content, "Self-improvement review: saved a lesson.");
    }

    #[test]
    fn dashboard_buffered_review_summary_appends_visible_system_message() {
        let visible = Property::new(vec![HermesMessage::new("u", HermesRole::User, "hello")]);
        let buffered = Property::new(Vec::new());
        let approval = Property::new(None);
        let todos = Property::new(Vec::new());
        let subagents = Property::new(Vec::new());

        let complete = apply_dashboard_event_with_answer_buffer(
            &buffered,
            &visible,
            "a",
            DashboardRpcEvent {
                event_type: String::from("review.summary"),
                session_id: Some(String::from("sid")),
                payload: json!({"message": "Self-improvement review: updated memory."}),
            },
            10,
            true,
            &approval,
            &todos,
            &subagents,
            None,
        )
        .expect("review summary applies");

        let rows = visible.get();
        assert!(!complete);
        assert!(buffered.get().is_empty());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].role, HermesRole::System);
        assert_eq!(rows[1].content, "Self-improvement review: updated memory.");
    }

    #[test]
    fn dashboard_thought_tool_events_are_filtered() {
        let messages = Property::new(Vec::new());
        apply_dashboard_event(
            &messages,
            "a",
            DashboardRpcEvent {
                event_type: String::from("tool.progress"),
                session_id: Some(String::from("sid")),
                payload: json!({
                    "tool": "_thinking",
                    "message": "thinking",
                    "status": "running",
                }),
            },
            10,
            true,
            &Property::new(None),
            &Property::new(Vec::new()),
            None,
        )
        .expect("thought tool applies");

        assert!(messages.get().is_empty());
    }

    #[test]
    fn dashboard_real_tool_events_are_preserved() {
        let messages = Property::new(Vec::new());
        apply_dashboard_event(
            &messages,
            "a",
            DashboardRpcEvent {
                event_type: String::from("tool.progress"),
                session_id: Some(String::from("sid")),
                payload: json!({
                    "tool": "web_search",
                    "message": "ZuluDesk CEO",
                    "status": "running",
                    "args": {
                        "query": "ZuluDesk CEO",
                        "command": "search web",
                        "path": "/tmp/search.md",
                        "url": "https://example.com/search"
                    },
                    "result": "Found current result",
                    "error": {"message": "rate limited"},
                    "inline_diff": "diff --git a/file b/file",
                }),
            },
            10,
            true,
            &Property::new(None),
            &Property::new(Vec::new()),
            None,
        )
        .expect("tool applies");

        let message = messages.get().remove(0);
        assert_eq!(message.tool_events.len(), 1);
        assert_eq!(message.tool_events[0].tool, "web_search");
        assert_eq!(message.tool_events[0].label, "ZuluDesk CEO");
        assert_eq!(
            message.tool_events[0].command.as_deref(),
            Some("search web")
        );
        assert_eq!(
            message.tool_events[0].input.as_deref(),
            Some("ZuluDesk CEO")
        );
        assert_eq!(
            message.tool_events[0].path.as_deref(),
            Some("/tmp/search.md")
        );
        assert_eq!(
            message.tool_events[0].url.as_deref(),
            Some("https://example.com/search")
        );
        assert_eq!(
            message.tool_events[0].output.as_deref(),
            Some("Found current result")
        );
        assert_eq!(
            message.tool_events[0].error.as_deref(),
            Some("rate limited")
        );
        assert!(message.tool_events[0].has_inline_diff);
        assert!(
            message.tool_events[0]
                .raw
                .as_deref()
                .is_some_and(|raw| raw.contains("web_search"))
        );
    }

    #[test]
    fn dashboard_clarify_request_preserves_kind_and_request_id() {
        let approval = Property::new(None);
        let messages = Property::new(Vec::new());
        apply_dashboard_event(
            &messages,
            "a",
            DashboardRpcEvent {
                event_type: String::from("clarify.request"),
                session_id: Some(String::from("sid")),
                payload: json!({"request_id": "clarify-1", "question": "Which file?"}),
            },
            10,
            true,
            &approval,
            &Property::new(Vec::new()),
            None,
        )
        .expect("clarify applies");

        let request = approval.get().expect("clarify request");
        assert_eq!(request.run_id, "sid");
        assert_eq!(request.approval_id.as_deref(), Some("clarify-1"));
        assert_eq!(request.prompt, "Which file?");
        assert_eq!(request.kind, ApprovalKind::Clarification);
    }

    #[test]
    fn dashboard_sensitive_input_requests_preserve_kind_and_request_id() {
        let approval = Property::new(None);
        let messages = Property::new(Vec::new());

        apply_dashboard_event(
            &messages,
            "a",
            DashboardRpcEvent {
                event_type: String::from("sudo.request"),
                session_id: Some(String::from("sid")),
                payload: json!({"request_id": "sudo-1"}),
            },
            10,
            true,
            &approval,
            &Property::new(Vec::new()),
            None,
        )
        .expect("sudo applies");

        let request = approval.get().expect("sudo request");
        assert_eq!(request.run_id, "sid");
        assert_eq!(request.approval_id.as_deref(), Some("sudo-1"));
        assert_eq!(request.kind, ApprovalKind::Sudo);
        assert!(request.prompt.contains("sudo password"));

        apply_dashboard_event(
            &messages,
            "a",
            DashboardRpcEvent {
                event_type: String::from("secret.request"),
                session_id: Some(String::from("sid")),
                payload: json!({
                    "requestId": "secret-1",
                    "envVar": "OPENAI_API_KEY",
                    "prompt": "Provide OPENAI_API_KEY."
                }),
            },
            10,
            true,
            &approval,
            &Property::new(Vec::new()),
            None,
        )
        .expect("secret applies");

        let request = approval.get().expect("secret request");
        assert_eq!(request.run_id, "sid");
        assert_eq!(request.approval_id.as_deref(), Some("secret-1"));
        assert_eq!(request.kind, ApprovalKind::Secret);
        assert!(request.prompt.contains("Provide OPENAI_API_KEY."));
        assert!(request.prompt.contains("secret response"));
    }

    #[test]
    fn stream_error_event_fails_turn() {
        let messages = Property::new(vec![HermesMessage::new("a", HermesRole::Assistant, "")]);
        let err = apply_event(
            &messages,
            "a",
            SseEvent {
                event: Some(String::from("error")),
                data: String::from(r#"{"message":"server failed"}"#),
            },
            10,
            true,
            &Property::new(None),
            None,
        )
        .expect_err("error event fails");
        assert_eq!(err.short_message(), "server failed");
    }

    #[test]
    fn approval_event_preserves_approval_id() {
        let approval = Property::new(None);
        let messages = Property::new(vec![HermesMessage::new("a", HermesRole::Assistant, "")]);
        apply_event(
            &messages,
            "a",
            SseEvent {
                event: Some(String::from("approval.required")),
                data: String::from(
                    r#"{"runId":"run-1","approvalId":"approval-1","prompt":"Allow it?"}"#,
                ),
            },
            10,
            true,
            &approval,
            None,
        )
        .expect("approval event applies");

        let request = approval.get().expect("approval request");
        assert_eq!(request.run_id, "run-1");
        assert_eq!(request.approval_id.as_deref(), Some("approval-1"));
        assert_eq!(request.prompt, "Allow it?");
    }

    #[test]
    fn approval_event_uses_run_id_hint_when_missing() {
        let approval = Property::new(None);
        let messages = Property::new(vec![HermesMessage::new("a", HermesRole::Assistant, "")]);
        apply_event(
            &messages,
            "a",
            SseEvent {
                event: Some(String::from("approval.required")),
                data: String::from(r#"{"prompt":"Allow it?"}"#),
            },
            10,
            true,
            &approval,
            Some("run-from-stream"),
        )
        .expect("approval event applies");

        let request = approval.get().expect("approval request");
        assert_eq!(request.run_id, "run-from-stream");
        assert_eq!(request.prompt, "Allow it?");
    }

    #[test]
    fn approval_event_without_run_id_fails() {
        let approval = Property::new(None);
        let messages = Property::new(vec![HermesMessage::new("a", HermesRole::Assistant, "")]);
        let err = apply_event(
            &messages,
            "a",
            SseEvent {
                event: Some(String::from("approval.required")),
                data: String::from(r#"{"prompt":"Allow it?"}"#),
            },
            10,
            true,
            &approval,
            None,
        )
        .expect_err("missing run id fails");

        assert_eq!(err.short_message(), "Unsupported server event");
        assert_eq!(approval.get(), None);
    }

    #[test]
    fn detects_missing_and_unresolved_api_keys() {
        let mut config = ConnectionConfig {
            enabled: true,
            api_key: Some(String::from("secret")),
            ..ConnectionConfig::default()
        };
        assert!(!config_has_missing_api_key(&config));

        config.api_key = None;
        assert!(config_has_missing_api_key(&config));

        config.api_key = Some(String::new());
        assert!(config_has_missing_api_key(&config));

        config.api_key = Some(String::from("$HERMES_API_SERVER_KEY"));
        assert!(config_has_missing_api_key(&config));
    }

    #[test]
    fn stream_config_cancel_scope_tracks_connection_identity() {
        let config = ConnectionConfig {
            enabled: true,
            api_key: Some(String::from("secret")),
            ..ConnectionConfig::default()
        };

        let mut next = config.clone();
        next.show_tool_progress = false;
        assert!(!stream_config_requires_cancel(&config, &next));

        let mut next = config.clone();
        next.local_history = LocalHistoryMode::Disabled;
        assert!(stream_config_requires_cancel(&config, &next));

        let mut next = config.clone();
        next.timeout_seconds = 5;
        assert!(!stream_config_requires_cancel(&config, &next));

        let mut next = config.clone();
        next.enabled = false;
        assert!(stream_config_requires_cancel(&config, &next));

        let mut next = config.clone();
        next.endpoint_url = String::from("http://127.0.0.1:9642");
        assert!(stream_config_requires_cancel(&config, &next));

        let mut next = config.clone();
        next.api_key = Some(String::from("$HERMES_API_SERVER_KEY"));
        assert!(stream_config_requires_cancel(&config, &next));

        let mut next = config.clone();
        next.dashboard_token = Some(String::from("other-dashboard-token"));
        assert!(stream_config_requires_cancel(&config, &next));

        let mut next = config.clone();
        next.session_key = Some(String::from("other-session"));
        assert!(stream_config_requires_cancel(&config, &next));

        let mut next = config.clone();
        next.transport_mode = TransportMode::Runs;
        assert!(stream_config_requires_cancel(&config, &next));
    }

    #[test]
    fn remote_state_reset_scope_tracks_server_identity() {
        let config = ConnectionConfig {
            enabled: true,
            api_key: Some(String::from("secret")),
            ..ConnectionConfig::default()
        };

        let mut next = config.clone();
        next.model = String::from("other-model");
        assert!(!remote_state_requires_reset(&config, &next));

        let mut next = config.clone();
        next.local_history = LocalHistoryMode::Disabled;
        assert!(!remote_state_requires_reset(&config, &next));

        let mut next = config.clone();
        next.show_tool_progress = false;
        assert!(!remote_state_requires_reset(&config, &next));

        let mut next = config.clone();
        next.enabled = false;
        assert!(remote_state_requires_reset(&config, &next));

        let mut next = config.clone();
        next.endpoint_url = String::from("http://127.0.0.1:9642");
        assert!(remote_state_requires_reset(&config, &next));

        let mut next = config.clone();
        next.api_key = Some(String::from("other-secret"));
        assert!(remote_state_requires_reset(&config, &next));

        let mut next = config.clone();
        next.dashboard_token = Some(String::from("other-dashboard-token"));
        assert!(remote_state_requires_reset(&config, &next));

        let mut next = config.clone();
        next.session_key = Some(String::from("other-session"));
        assert!(remote_state_requires_reset(&config, &next));

        let mut next = config.clone();
        next.transport_mode = TransportMode::Runs;
        assert!(remote_state_requires_reset(&config, &next));
    }

    #[test]
    fn disabled_connect_clears_remote_state() {
        let service = HermesChatService::new(
            ConnectionConfig {
                enabled: false,
                api_key: Some(String::from("secret")),
                ..ConnectionConfig::default()
            },
            None,
        );
        service
            .capabilities
            .set(Some(Arc::new(json!({"ok": true}))));
        service.sessions.set(vec![HermesSessionSummary {
            id: String::from("s1"),
            title: String::from("Session"),
            updated_at: None,
            is_active: false,
            needs_input: false,
            message_count: None,
            preview: None,
            source: None,
        }]);
        service.active_session_id.set(Some(String::from("s1")));
        service.approval.set(Some(ApprovalRequest {
            run_id: String::from("run-1"),
            approval_id: Some(String::from("approval-1")),
            prompt: String::from("Allow it?"),
            kind: ApprovalKind::Approval,
        }));
        service.last_error.set(Some(String::from("Previous error")));

        service.connect();

        assert_eq!(service.status.get(), HermesStatus::Disabled);
        assert_eq!(service.capabilities.get(), None);
        assert!(service.sessions.get().is_empty());
        assert_eq!(service.active_session_id.get(), None);
        assert_eq!(service.approval.get(), None);
        assert_eq!(service.last_error.get(), None);
    }

    #[test]
    fn missing_key_connect_clears_remote_state() {
        let service = HermesChatService::new(
            ConnectionConfig {
                enabled: true,
                api_key: None,
                transport_mode: TransportMode::ChatCompletions,
                ..ConnectionConfig::default()
            },
            None,
        );
        service
            .capabilities
            .set(Some(Arc::new(json!({"ok": true}))));
        service.sessions.set(vec![HermesSessionSummary {
            id: String::from("s1"),
            title: String::from("Session"),
            updated_at: None,
            is_active: false,
            needs_input: false,
            message_count: None,
            preview: None,
            source: None,
        }]);
        service.active_session_id.set(Some(String::from("s1")));
        service.approval.set(Some(ApprovalRequest {
            run_id: String::from("run-1"),
            approval_id: None,
            prompt: String::from("Allow it?"),
            kind: ApprovalKind::Approval,
        }));

        service.connect();

        assert_eq!(service.status.get(), HermesStatus::MissingApiKey);
        assert_eq!(service.capabilities.get(), None);
        assert!(service.sessions.get().is_empty());
        assert_eq!(service.active_session_id.get(), None);
        assert_eq!(service.approval.get(), None);
        assert_eq!(service.last_error.get().as_deref(), Some("Missing API key"));
    }

    #[test]
    fn update_config_clears_remote_state_when_endpoint_changes() {
        let service = HermesChatService::new(
            ConnectionConfig {
                enabled: false,
                endpoint_url: String::from("http://127.0.0.1:8642"),
                api_key: Some(String::from("secret")),
                ..ConnectionConfig::default()
            },
            None,
        );
        service
            .capabilities
            .set(Some(Arc::new(json!({"ok": true}))));
        service.sessions.set(vec![HermesSessionSummary {
            id: String::from("s1"),
            title: String::from("Session"),
            updated_at: None,
            is_active: false,
            needs_input: false,
            message_count: None,
            preview: None,
            source: None,
        }]);
        service.active_session_id.set(Some(String::from("s1")));
        service.approval.set(Some(ApprovalRequest {
            run_id: String::from("run-1"),
            approval_id: None,
            prompt: String::from("Allow it?"),
            kind: ApprovalKind::Approval,
        }));

        service.update_config(ConnectionConfig {
            enabled: false,
            endpoint_url: String::from("http://127.0.0.1:9642"),
            api_key: Some(String::from("secret")),
            ..ConnectionConfig::default()
        });

        assert_eq!(service.capabilities.get(), None);
        assert!(service.sessions.get().is_empty());
        assert_eq!(service.active_session_id.get(), None);
        assert_eq!(service.approval.get(), None);
    }

    #[test]
    fn connect_generation_accepts_only_latest_refresh() {
        let connect_sequence = Arc::new(AtomicU64::new(0));
        let first = connect_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        assert!(connect_is_current(&connect_sequence, first));

        let second = connect_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        assert!(!connect_is_current(&connect_sequence, first));
        assert!(connect_is_current(&connect_sequence, second));
    }

    #[test]
    fn config_generation_rejects_results_after_config_change() {
        let config_sequence = Arc::new(AtomicU64::new(0));
        let before_change = config_sequence.load(Ordering::Relaxed);
        assert!(config_is_current(&config_sequence, before_change));

        config_sequence.fetch_add(1, Ordering::Relaxed);
        assert!(!config_is_current(&config_sequence, before_change));

        let after_change = config_sequence.load(Ordering::Relaxed);
        assert!(config_is_current(&config_sequence, after_change));
    }

    #[test]
    fn unavailable_config_state_matches_connect_statuses() {
        let disabled = ConnectionConfig {
            enabled: false,
            api_key: Some(String::from("secret")),
            ..ConnectionConfig::default()
        };
        assert_eq!(
            unavailable_config_state(&disabled),
            Some((HermesStatus::Disabled, None))
        );

        let missing = ConnectionConfig {
            enabled: true,
            api_key: None,
            transport_mode: TransportMode::ChatCompletions,
            ..ConnectionConfig::default()
        };
        assert_eq!(
            unavailable_config_state(&missing),
            Some((
                HermesStatus::MissingApiKey,
                Some(String::from("Missing API key"))
            ))
        );

        let auto_missing = ConnectionConfig {
            enabled: true,
            api_key: None,
            transport_mode: TransportMode::Auto,
            ..ConnectionConfig::default()
        };
        assert_eq!(unavailable_config_state(&auto_missing), None);

        let ready = ConnectionConfig {
            enabled: true,
            api_key: Some(String::from("secret")),
            ..ConnectionConfig::default()
        };
        assert_eq!(unavailable_config_state(&ready), None);

        let dashboard_ready = ConnectionConfig {
            enabled: true,
            api_key: None,
            transport_mode: TransportMode::DashboardWs,
            ..ConnectionConfig::default()
        };
        assert_eq!(unavailable_config_state(&dashboard_ready), None);
    }

    #[test]
    fn auto_config_uses_discovered_transport() {
        let mut dashboard = ConnectionConfig {
            transport_mode: TransportMode::Auto,
            ..ConnectionConfig::default()
        };
        apply_discovered_transport(
            &mut dashboard,
            Some(&json!({"transport_mode": "dashboard-ws"})),
        );
        assert_eq!(dashboard.transport_mode, TransportMode::DashboardWs);

        let mut chat = ConnectionConfig {
            transport_mode: TransportMode::Auto,
            ..ConnectionConfig::default()
        };
        apply_discovered_transport(
            &mut chat,
            Some(&json!({"transport_mode": "chat-completions"})),
        );
        assert_eq!(chat.transport_mode, TransportMode::ChatCompletions);

        let mut explicit = ConnectionConfig {
            transport_mode: TransportMode::Runs,
            ..ConnectionConfig::default()
        };
        apply_discovered_transport(
            &mut explicit,
            Some(&json!({"transport_mode": "dashboard-ws"})),
        );
        assert_eq!(explicit.transport_mode, TransportMode::Runs);
    }

    #[tokio::test]
    async fn chat_completions_connect_uses_models_endpoint() {
        let (endpoint_url, request) =
            json_server_once(json!({"data": [{"id": "hermes-agent"}]})).await;

        let (capabilities, sessions) = connect_inner(ConnectionConfig {
            enabled: true,
            endpoint_url,
            api_key: Some(String::from("secret")),
            transport_mode: TransportMode::ChatCompletions,
            timeout_seconds: 1,
            ..ConnectionConfig::default()
        })
        .await
        .expect("chat completions connect");

        let request = request.await.expect("request task");
        assert!(request.starts_with("GET /v1/models "));
        assert_eq!(capabilities["transport_mode"], "chat-completions");
        assert_eq!(capabilities["models"]["data"][0]["id"], "hermes-agent");
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn auto_connect_falls_back_to_chat_completions_probe() {
        let (endpoint_url, requests) = auto_fallback_server().await;

        let (capabilities, sessions) = connect_inner(ConnectionConfig {
            enabled: true,
            endpoint_url,
            api_key: Some(String::from("secret")),
            transport_mode: TransportMode::Auto,
            timeout_seconds: 1,
            ..ConnectionConfig::default()
        })
        .await
        .expect("auto connect falls back");

        let requests = requests.await.expect("request task");
        assert_eq!(
            requests,
            vec![
                String::from("GET / HTTP/1.1"),
                String::from("GET /health HTTP/1.1"),
                String::from("GET /v1/models HTTP/1.1"),
            ]
        );
        assert_eq!(capabilities["transport_mode"], "chat-completions");
        assert_eq!(capabilities["models"]["data"][0]["id"], "hermes-agent");
        assert!(sessions.is_empty());
    }

    #[test]
    fn auto_new_session_resets_locally_after_chat_completions_fallback() {
        let service = disabled_test_service();
        make_service_ready_with_transport(&service, TransportMode::Auto);
        service.capabilities.set(Some(Arc::new(json!({
            "transport_mode": "chat-completions",
            "models": {"data": [{"id": "hermes-agent"}]},
        }))));
        service
            .active_session_id
            .set(Some(String::from("old-session")));
        service
            .messages
            .set(vec![HermesMessage::new("m1", HermesRole::User, "old")]);
        service.approval.set(Some(ApprovalRequest {
            run_id: String::from("run-1"),
            approval_id: None,
            prompt: String::from("Allow it?"),
            kind: ApprovalKind::Approval,
        }));
        service.last_error.set(Some(String::from("Previous error")));
        service.status.set(HermesStatus::Busy);

        assert!(service.should_clear_local_chat_for_new_session(&service.config()));
        service.new_session(Some(String::from("Lumen Chat")));

        assert_eq!(service.status.get(), HermesStatus::Connected);
        assert_eq!(service.active_session_id.get(), None);
        assert!(service.messages.get().is_empty());
        assert_eq!(service.approval.get(), None);
        assert_eq!(service.last_error.get(), None);
    }

    #[test]
    fn auto_new_session_keeps_session_path_without_chat_completions_marker() {
        let service = disabled_test_service();
        make_service_ready_with_transport(&service, TransportMode::Auto);
        service.capabilities.set(Some(Arc::new(json!({
            "sessions": true,
        }))));

        assert!(!service.should_clear_local_chat_for_new_session(&service.config()));
    }

    #[test]
    fn new_session_resets_local_chat_for_local_new_chat_transports() {
        for (name, transport_mode) in [
            ("runs", TransportMode::Runs),
            ("chat-completions", TransportMode::ChatCompletions),
            ("dashboard-ws", TransportMode::DashboardWs),
        ] {
            let path = temp_history_path(name);
            let service = HermesChatService::new(
                ConnectionConfig {
                    enabled: false,
                    api_key: Some(String::from("secret")),
                    ..ConnectionConfig::default()
                },
                Some(path.clone()),
            );
            make_service_ready_with_transport(&service, transport_mode);
            service
                .active_session_id
                .set(Some(String::from("old-session")));
            if let Some(store) = service.store.as_ref() {
                store.set_active_session_id(Some(String::from("old-session")));
            }
            service
                .messages
                .set(vec![HermesMessage::new("m1", HermesRole::User, "old")]);
            service.approval.set(Some(ApprovalRequest {
                run_id: String::from("run-1"),
                approval_id: Some(String::from("approval-1")),
                prompt: String::from("Allow it?"),
                kind: ApprovalKind::Approval,
            }));
            service.last_error.set(Some(String::from("Previous error")));
            service.status.set(HermesStatus::Busy);

            service.new_session(Some(String::from("Lumen Chat")));

            assert_eq!(service.status.get(), HermesStatus::Connected);
            assert_eq!(service.active_session_id.get(), None);
            assert!(service.messages.get().is_empty());
            assert_eq!(service.approval.get(), None);
            assert_eq!(service.last_error.get(), None);

            let store = LocalHistoryStore::new(path.clone());
            let (active_session_id, messages) = store.load().expect("load saved history");
            assert_eq!(active_session_id, None);
            assert!(messages.is_empty());
            remove_temp_history(&path);
        }
    }

    #[test]
    fn dashboard_session_not_found_matches_rpc_error() {
        assert!(dashboard_session_not_found(&Error::Api {
            status: 500,
            message: String::from("session not found"),
        }));
        assert!(dashboard_session_not_found(&Error::Api {
            status: 500,
            message: String::from("No session was found for that id"),
        }));
        assert!(!dashboard_session_not_found(&Error::Api {
            status: 500,
            message: String::from("authentication failed"),
        }));
        assert!(!dashboard_session_not_found(&Error::WebSocket(
            String::from("session not found")
        )));
    }

    #[test]
    fn dashboard_model_info_applies_session_create_overrides() {
        let mut params = json!({"cols": 80, "source": "desktop"});

        apply_dashboard_model_info(
            &mut params,
            &json!({
                "model": "gpt-5.5",
                "provider": "openai-codex",
                "capabilities": {"supports_tools": true},
            }),
        );

        assert_eq!(params["model"], "gpt-5.5");
        assert_eq!(params["provider"], "openai-codex");
    }

    #[test]
    fn dashboard_session_create_profile_applies_selected_profile() {
        let mut params = json!({"cols": 80, "source": "desktop"});

        apply_dashboard_session_profile(&mut params, Some(" coder "));

        assert_eq!(params["profile"], "coder");
    }

    #[test]
    fn assistant_response_text_supports_non_streaming_shapes() {
        assert_eq!(
            assistant_response_text(&json!({
                "message": {"role": "assistant", "content": "session answer"}
            }))
            .as_deref(),
            Some("session answer")
        );
        assert_eq!(
            assistant_response_text(&json!({
                "choices": [{
                    "message": {"role": "assistant", "content": "chat answer"}
                }]
            }))
            .as_deref(),
            Some("chat answer")
        );
    }

    #[test]
    fn publish_collected_assistant_reveals_buffered_message_once() {
        let messages = Property::new(vec![HermesMessage::new("u1", HermesRole::User, "Hi")]);
        let buffered = Property::new(Vec::new());
        append_delta(&buffered, "a1", "Hello", 10);
        push_tool_event(
            &buffered,
            "a1",
            ToolEvent {
                id: String::from("tool-1"),
                tool: String::from("web_search"),
                label: String::from("search"),
                status: String::from("running"),
                command: None,
                input: None,
                output: None,
                error: None,
                path: None,
                url: None,
                has_inline_diff: false,
                raw: None,
            },
            10,
        );

        assert_eq!(messages.get().len(), 1);
        publish_collected_assistant(&messages, "a1", &buffered, 10);

        let messages = messages.get();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].content, "Hello");
        assert_eq!(messages[1].status, MessageStatus::Complete);
        assert_eq!(messages[1].tool_events[0].status, "completed");
    }

    #[test]
    fn final_answer_merge_preserves_live_tool_activity() {
        let messages = Property::new(vec![HermesMessage::new("u1", HermesRole::User, "Hi")]);
        let buffered = Property::new(Vec::new());
        push_tool_event(
            &messages,
            "a1",
            ToolEvent {
                id: String::from("tool-1"),
                tool: String::from("web_search"),
                label: String::from("search"),
                status: String::from("running"),
                command: None,
                input: None,
                output: None,
                error: None,
                path: None,
                url: None,
                has_inline_diff: false,
                raw: None,
            },
            10,
        );
        append_delta(&buffered, "a1", "Final answer", 10);

        let visible = messages.get();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[1].content, "");
        assert_eq!(visible[1].tool_events[0].status, "running");

        publish_collected_assistant(&messages, "a1", &buffered, 10);

        let visible = messages.get();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[1].content, "Final answer");
        assert_eq!(visible[1].status, MessageStatus::Complete);
        assert_eq!(visible[1].tool_events[0].status, "completed");
    }

    #[test]
    fn dashboard_connect_does_not_auto_select_recent_session() {
        assert!(!auto_select_first_session_on_connect(
            TransportMode::DashboardWs
        ));
        assert!(auto_select_first_session_on_connect(
            TransportMode::Sessions
        ));
        assert!(auto_select_first_session_on_connect(TransportMode::Auto));
    }

    #[test]
    fn local_new_session_preserves_error_status() {
        for (status, last_error) in [
            (
                HermesStatus::Offline(String::from("connection refused")),
                Some(String::from("connection refused")),
            ),
            (
                HermesStatus::Error(String::from("server failed")),
                Some(String::from("server failed")),
            ),
        ] {
            let service = disabled_test_service();
            make_service_ready_with_transport(&service, TransportMode::ChatCompletions);
            service
                .messages
                .set(vec![HermesMessage::new("m1", HermesRole::User, "old")]);
            service.status.set(status.clone());
            service.last_error.set(last_error.clone());

            service.new_session(Some(String::from("Lumen Chat")));

            assert_eq!(service.status.get(), status);
            assert_eq!(service.last_error.get(), last_error);
            assert!(service.messages.get().is_empty());
        }
    }

    #[test]
    fn send_message_rejects_disabled_config_without_transcript_change() {
        let service = HermesChatService::new(
            ConnectionConfig {
                enabled: false,
                api_key: Some(String::from("secret")),
                ..ConnectionConfig::default()
            },
            None,
        );

        service.send_message(String::from("hello"));

        assert_eq!(service.status.get(), HermesStatus::Disabled);
        assert!(service.messages.get().is_empty());
        assert_eq!(service.last_error.get(), None);
    }

    #[test]
    fn select_session_rejects_missing_key_without_active_session_change() {
        let service = HermesChatService::new(
            ConnectionConfig {
                enabled: true,
                api_key: None,
                transport_mode: TransportMode::ChatCompletions,
                ..ConnectionConfig::default()
            },
            None,
        );

        service.select_session(String::from("s1"));

        assert_eq!(service.status.get(), HermesStatus::MissingApiKey);
        assert_eq!(service.active_session_id.get(), None);
        assert!(service.messages.get().is_empty());
        assert_eq!(service.last_error.get().as_deref(), Some("Missing API key"));
    }

    #[test]
    fn approval_rejects_disabled_config_and_clears_stale_prompt() {
        let service = HermesChatService::new(
            ConnectionConfig {
                enabled: false,
                api_key: Some(String::from("secret")),
                ..ConnectionConfig::default()
            },
            None,
        );
        service.approval.set(Some(ApprovalRequest {
            run_id: String::from("run-1"),
            approval_id: Some(String::from("approval-1")),
            prompt: String::from("Allow it?"),
            kind: ApprovalKind::Approval,
        }));

        service.submit_approval(true, None);

        assert_eq!(service.status.get(), HermesStatus::Disabled);
        assert_eq!(service.approval.get(), None);
    }

    #[test]
    fn approval_rejects_empty_run_id() {
        let service = disabled_test_service();
        make_service_ready(&service);
        service.approval.set(Some(ApprovalRequest {
            run_id: String::new(),
            approval_id: Some(String::from("approval-1")),
            prompt: String::from("Allow it?"),
            kind: ApprovalKind::Approval,
        }));

        service.submit_approval(true, None);

        assert_eq!(service.approval.get(), None);
        assert_eq!(
            service.last_error.get().as_deref(),
            Some("Missing approval run id")
        );
    }

    #[test]
    fn connect_status_update_changes_idle_state() {
        let status = Property::new(HermesStatus::Connecting);
        set_status_unless_busy(&status, HermesStatus::Connected);
        assert_eq!(status.get(), HermesStatus::Connected);
    }

    #[test]
    fn connect_status_update_preserves_busy_state() {
        let status = Property::new(HermesStatus::Busy);
        set_status_unless_busy(&status, HermesStatus::Connected);
        assert_eq!(status.get(), HermesStatus::Busy);
    }

    #[test]
    fn replacement_cancellation_invalidates_active_stream() {
        let service = disabled_test_service();
        let token = arm_stream(&service, 4);

        service.cancel_current_for_replacement();

        assert!(token.is_cancelled());
        assert_eq!(*service.active_stream_id.read().expect("read stream"), None);
        assert!(service.stream_token.lock().expect("read token").is_none());
    }

    #[tokio::test]
    async fn new_session_cancels_active_stream_before_create() {
        let service = disabled_test_service();
        make_service_ready(&service);
        let token = arm_stream(&service, 4);

        service.new_session(Some(String::from("Lumen Chat")));

        assert!(token.is_cancelled());
        assert_eq!(*service.active_stream_id.read().expect("read stream"), None);
    }

    #[tokio::test]
    async fn select_session_cancels_active_stream_before_switching_context() {
        let service = disabled_test_service();
        make_service_ready(&service);
        let token = arm_stream(&service, 4);

        service.select_session(String::from("s1"));

        assert!(token.is_cancelled());
        assert_eq!(*service.active_stream_id.read().expect("read stream"), None);
        assert_eq!(service.active_session_id.get().as_deref(), Some("s1"));
    }

    #[test]
    fn stale_stream_cannot_set_or_clear_active_run() {
        let active_stream_id = Arc::new(RwLock::new(Some(2)));
        let active_run_id = Arc::new(RwLock::new(Some(String::from("new-run"))));

        assert!(!stream_is_current(&active_stream_id, 1));
        set_run_if_current(
            &active_stream_id,
            &active_run_id,
            1,
            String::from("old-run"),
        );
        assert_eq!(
            active_run_id.read().expect("read run").as_deref(),
            Some("new-run")
        );

        clear_stream_if_current(&active_stream_id, &active_run_id, 1);
        assert_eq!(*active_stream_id.read().expect("read stream"), Some(2));
        assert_eq!(
            active_run_id.read().expect("read run").as_deref(),
            Some("new-run")
        );
    }

    #[test]
    fn stale_stream_cannot_publish_created_session() {
        let active_stream_id = Arc::new(RwLock::new(Some(2)));
        let active_session_id = Property::new(Some(String::from("existing-session")));
        let sessions = Property::new(vec![HermesSessionSummary {
            id: String::from("existing-session"),
            title: String::from("Existing"),
            updated_at: None,
            is_active: false,
            needs_input: false,
            message_count: None,
            preview: None,
            source: None,
        }]);

        let result = set_session_if_current(
            &active_stream_id,
            &active_session_id,
            &sessions,
            1,
            HermesSessionSummary {
                id: String::from("stale-session"),
                title: String::from("Stale"),
                updated_at: None,
                is_active: false,
                needs_input: false,
                message_count: None,
                preview: None,
                source: None,
            },
        );

        assert_eq!(result, None);
        assert_eq!(active_session_id.get().as_deref(), Some("existing-session"));
        let sessions = sessions.get();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "existing-session");
    }

    #[test]
    fn current_stream_publishes_created_session() {
        let active_stream_id = Arc::new(RwLock::new(Some(7)));
        let active_session_id = Property::new(None);
        let sessions = Property::new(Vec::new());

        let result = set_session_if_current(
            &active_stream_id,
            &active_session_id,
            &sessions,
            7,
            HermesSessionSummary {
                id: String::from("current-session"),
                title: String::from("Current"),
                updated_at: None,
                is_active: false,
                needs_input: false,
                message_count: None,
                preview: None,
                source: None,
            },
        );

        assert_eq!(result.as_deref(), Some("current-session"));
        assert_eq!(active_session_id.get().as_deref(), Some("current-session"));
        let sessions = sessions.get();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "current-session");
    }

    #[tokio::test]
    async fn next_stream_event_returns_none_when_cancelled() {
        let token = CancellationToken::new();
        token.cancel();
        let mut stream: EventStream = Box::pin(futures::stream::pending());

        assert!(next_stream_event(&mut stream, &token).await.is_none());
    }

    #[tokio::test]
    async fn next_stream_event_returns_available_event() {
        let token = CancellationToken::new();
        let mut stream: EventStream = Box::pin(futures::stream::once(async {
            Ok(SseEvent {
                event: Some(String::from("message")),
                data: String::from("hello"),
            })
        }));

        let event = next_stream_event(&mut stream, &token)
            .await
            .expect("stream event")
            .expect("event ok");
        assert_eq!(event.event.as_deref(), Some("message"));
        assert_eq!(event.data, "hello");
    }

    #[test]
    fn current_stream_sets_and_clears_active_run() {
        let active_stream_id = Arc::new(RwLock::new(Some(7)));
        let active_run_id = Arc::new(RwLock::new(None));

        assert!(stream_is_current(&active_stream_id, 7));
        set_run_if_current(
            &active_stream_id,
            &active_run_id,
            7,
            String::from("current-run"),
        );
        assert_eq!(
            active_run_id.read().expect("read run").as_deref(),
            Some("current-run")
        );

        clear_stream_if_current(&active_stream_id, &active_run_id, 7);
        assert_eq!(*active_stream_id.read().expect("read stream"), None);
        assert_eq!(active_run_id.read().expect("read run").as_deref(), None);
    }

    #[test]
    fn disabled_local_history_does_not_load_existing_history() {
        let path = temp_history_path("disabled-load");
        fs::create_dir_all(path.parent().expect("history parent")).expect("create history parent");
        let message = HermesMessage::new("m1", HermesRole::User, "private transcript");
        fs::write(
            &path,
            serde_json::to_string(&json!({
                "active_session_id": "s1",
                "messages": [message],
            }))
            .expect("serialize history"),
        )
        .expect("write history");

        let service = HermesChatService::new(
            ConnectionConfig {
                local_history: LocalHistoryMode::Disabled,
                ..ConnectionConfig::default()
            },
            Some(path.clone()),
        );

        assert_eq!(service.active_session_id.get(), None);
        assert!(service.messages.get().is_empty());
        remove_temp_history(&path);
    }

    #[test]
    fn save_local_history_respects_disabled_mode() {
        let path = temp_history_path("disabled-save");
        let store = Arc::new(LocalHistoryStore::new(path.clone()));
        let message = HermesMessage::new("m1", HermesRole::User, "private transcript");

        save_local_history(
            Some(&store),
            LocalHistoryMode::Disabled,
            Some(String::from("s1")),
            std::slice::from_ref(&message),
        );
        assert!(!path.exists());

        save_local_history(
            Some(&store),
            LocalHistoryMode::Full,
            Some(String::from("s1")),
            std::slice::from_ref(&message),
        );
        assert!(path.is_file());
        let (active_session_id, messages) = store.load().expect("load saved history");
        assert_eq!(active_session_id.as_deref(), Some("s1"));
        assert_eq!(messages.len(), 1);
        remove_temp_history(&path);
    }

    #[test]
    fn save_local_history_normalizes_streaming_messages() {
        let path = temp_history_path("streaming-save");
        let store = Arc::new(LocalHistoryStore::new(path.clone()));
        let user = HermesMessage::new("u1", HermesRole::User, "question");
        let mut empty_placeholder = HermesMessage::new("a1", HermesRole::Assistant, "");
        empty_placeholder.status = MessageStatus::Streaming;
        let mut partial = HermesMessage::new("a2", HermesRole::Assistant, "partial answer");
        partial.status = MessageStatus::Streaming;

        save_local_history(
            Some(&store),
            LocalHistoryMode::Full,
            Some(String::from("s1")),
            &[user, empty_placeholder, partial],
        );

        let (_, messages) = store.load().expect("load saved history");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].id, "u1");
        assert_eq!(messages[1].id, "a2");
        assert_eq!(messages[1].status, MessageStatus::Stopped);
        remove_temp_history(&path);
    }

    #[test]
    fn load_local_history_normalizes_legacy_streaming_messages() {
        let path = temp_history_path("streaming-load");
        fs::create_dir_all(path.parent().expect("history parent")).expect("create history parent");
        let mut empty_placeholder = HermesMessage::new("a1", HermesRole::Assistant, "");
        empty_placeholder.status = MessageStatus::Streaming;
        let mut partial = HermesMessage::new("a2", HermesRole::Assistant, "partial answer");
        partial.status = MessageStatus::Streaming;
        fs::write(
            &path,
            serde_json::to_string(&json!({
                "active_session_id": "s1",
                "messages": [empty_placeholder, partial],
            }))
            .expect("serialize history"),
        )
        .expect("write history");

        let store = LocalHistoryStore::new(path.clone());
        let (_, messages) = store.load().expect("load saved history");

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, "a2");
        assert_eq!(messages[0].status, MessageStatus::Stopped);
        remove_temp_history(&path);
    }
}
