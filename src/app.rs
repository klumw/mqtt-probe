//! Application state and eframe integration.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Local};
use eframe::egui::{self, Ui};
use rust_i18n::t;
use uuid::Uuid;

use crate::config::{
    AppConfig, ConfigStore, ConnectionProfile, ConnectionSnapshot, MAX_PUBLISH_HISTORY,
    PublishHistoryEntry, SavedTopic,
};
use crate::model::topic_tree::{InsertOutcome, TopicNode};
use crate::model::{MessageRecord, TopicTree};
use crate::mqtt::{self, Command, ConnectionHandle, ConnectionState, WorkerEvent};
use crate::ui;
use crate::ui::connections::ProfileEditor;
use crate::ui::details::{DetailAction, DetailTab, ValueMode};
use crate::ui::help::HelpScrollTarget;
use crate::ui::publish::PublishDraft;
use crate::ui::retained::RetainedDeleteRequest;
use crate::ui::toasts::Toasts;

/// How long per-topic update timestamps are retained for the highlight effect.
const MAX_UPDATE_RETENTION: Duration = Duration::from_secs(10);
/// Ignore the initial retained synchronization after (re)connecting.
const INITIAL_HIGHLIGHT_SUPPRESSION: Duration = Duration::from_secs(2);

/// A handle for a runtime that has no worker. Any events from an older worker
/// are discarded together with its receiver when the handle is replaced.
fn disconnected_handle() -> ConnectionHandle {
    let (commands, _command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_event_tx, events) = std::sync::mpsc::channel();
    ConnectionHandle {
        commands,
        events,
        join: None,
    }
}

/// Stops a running worker and immediately marks the runtime as disconnected.
/// Replacing the handle also discards events that the old worker may still
/// have queued.
fn stop_runtime(runtime: &mut ConnectionRuntime) {
    let _ = runtime.handle.commands.send(Command::Disconnect);
    runtime.handle = disconnected_handle();
    runtime.state = ConnectionState::Disconnected;
    runtime.connected_at = None;
}

/// Builds one snapshot per tree, including empty trees. An explicit empty
/// snapshot tells the next session that no topics were displayed at exit.
fn snapshots_for<'a>(
    trees: impl IntoIterator<Item = (Uuid, &'a TopicTree)>,
) -> Vec<ConnectionSnapshot> {
    let mut snapshots = Vec::new();
    for (profile_id, tree) in trees {
        snapshots.push(ConnectionSnapshot {
            profile_id,
            topics: snapshot_tree(tree),
        });
    }
    snapshots.sort_by_key(|snapshot| snapshot.profile_id);
    snapshots
}

fn snapshot_tree(tree: &TopicTree) -> Vec<SavedTopic> {
    fn walk(node: &TopicNode, out: &mut Vec<SavedTopic>) {
        if let Some(data) = &node.data
            && let Some(latest) = data.latest()
        {
            out.push(SavedTopic {
                topic: node.path.clone(),
                payload_base64: BASE64.encode(&latest.payload),
                qos: latest.qos,
                retain: latest.retain,
                received_at: latest.received_at.to_owned(),
                total: data.total,
            });
        }
        for child in node.children.values() {
            walk(child, out);
        }
    }

    let mut topics = Vec::new();
    walk(&tree.root, &mut topics);
    topics
}

fn restore_tree(snapshot: &ConnectionSnapshot, max_history: usize) -> TopicTree {
    let mut tree = TopicTree::default();
    for saved in &snapshot.topics {
        let payload = BASE64.decode(&saved.payload_base64).unwrap_or_default();
        let mut message = MessageRecord::new(saved.topic.clone(), payload, saved.qos, saved.retain);
        message.received_at = saved.received_at.to_owned();
        tree.insert(message, max_history);
        tree.set_total(&saved.topic, saved.total);
    }
    // Restored branches stay collapsed, matching a fresh connection.
    tree.set_all_expanded(false);
    tree
}

pub struct ConnectionRuntime {
    pub handle: ConnectionHandle,
    pub state: ConnectionState,
    pub tree: TopicTree,
    pub connected_at: Option<DateTime<Local>>,
    pub last_error: Option<String>,
    pub subscribed_topic: Option<String>,
    pub logs: VecDeque<String>,
    /// Last update time per exact topic path.
    pub topic_updates: HashMap<String, Instant>,
    /// Topics that have received data during this session.
    pub lit_topics: HashSet<String>,
    /// All path prefixes of [`Self::lit_topics`], used for bright arrows.
    pub lit_paths: HashSet<String>,
    pub last_update_prune: Instant,
    /// Do not mark highlights before this instant (initial retained flood).
    pub suppress_highlights_until: Instant,
    /// Ignore retained messages for topics that are not already in the tree.
    /// Enabled after restoring the topic set of the previous session.
    pub restore_filter: bool,
}

impl ConnectionRuntime {
    pub(crate) fn mark_topic_lit(&mut self, topic: &str) {
        self.lit_topics.insert(topic.to_owned());
        crate::model::topic_tree::insert_path_prefixes(topic, &mut self.lit_paths);
    }

    fn rebuild_lit_paths(&mut self) {
        self.lit_paths.clear();
        for topic in &self.lit_topics {
            crate::model::topic_tree::insert_path_prefixes(topic, &mut self.lit_paths);
        }
    }

    pub(crate) fn unmark_topic_lit(&mut self, topic: &str) {
        if self.lit_topics.remove(topic) {
            self.rebuild_lit_paths();
        }
    }

    pub(crate) fn forget_subtree(&mut self, path: &str) {
        let prefix = format!("{path}/");
        self.lit_topics
            .retain(|topic| topic != path && !topic.starts_with(&prefix));
        self.rebuild_lit_paths();
        self.topic_updates
            .retain(|topic, _| topic != path && !topic.starts_with(&prefix));
    }

    pub(crate) fn forget_all(&mut self) {
        self.lit_topics.clear();
        self.lit_paths.clear();
        self.topic_updates.clear();
    }
}

pub struct MqttUiApp {
    pub(crate) config: AppConfig,
    pub(crate) store: ConfigStore,
    pub(crate) runtimes: HashMap<Uuid, ConnectionRuntime>,
    /// Topic trees from the last session, applied only after a connection is
    /// established. Until then they must not be displayed.
    pub(crate) pending_restores: HashMap<Uuid, ConnectionSnapshot>,
    pub(crate) selected_connection: Option<Uuid>,
    /// Scroll the selected profile card into view on the next sidebar frame.
    pub(crate) scroll_selected_profile: bool,
    pub(crate) top_combo_was_open: bool,
    pub(crate) scroll_top_combo_to_selection: bool,
    pub(crate) selected_topic: Option<String>,
    pub(crate) tree_filter: String,
    pub(crate) focus_search: bool,
    pub(crate) detail_tab: DetailTab,
    pub(crate) value_mode: ValueMode,
    pub(crate) history_selection: Option<MessageRecord>,
    pub(crate) chart_field: Option<String>,
    pub(crate) publish: PublishDraft,
    pub(crate) profile_editor: Option<ProfileEditor>,
    pub(crate) show_settings: bool,
    /// Whether the settings window was already open in the previous frame.
    pub(crate) settings_was_open: bool,
    pub(crate) show_help: bool,
    pub(crate) help_scroll: Option<HelpScrollTarget>,
    /// Pending retained-message deletion shown in a confirmation dialog.
    pub(crate) retained_confirm: Option<RetainedDeleteRequest>,
    pub(crate) show_log: bool,
    pub(crate) toasts: Toasts,
    pub(crate) config_dirty: bool,
    pub(crate) save_error_shown: bool,
    pub(crate) last_save: Instant,
    pub(crate) applied_dark: Option<bool>,
    pub(crate) delete_profile_confirm: Option<Uuid>,
}

impl MqttUiApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        ui::theme::install(&cc.egui_ctx);
        let store = ConfigStore::new();
        let mut config = store.load();
        rust_i18n::set_locale(config.settings.language.code());
        let selected_connection = config
            .topic_snapshots
            .iter()
            .find(|snapshot| {
                config
                    .profiles
                    .iter()
                    .any(|profile| profile.id == snapshot.profile_id)
            })
            .map(|snapshot| snapshot.profile_id)
            .or_else(|| config.profiles.first().map(|profile| profile.id));

        // Keep the last session's trees in memory, but do not display them
        // before a successful connection is established.
        let pending_restores: HashMap<Uuid, ConnectionSnapshot> =
            std::mem::take(&mut config.topic_snapshots)
                .into_iter()
                .filter(|snapshot| {
                    config
                        .profiles
                        .iter()
                        .any(|profile| profile.id == snapshot.profile_id)
                })
                .map(|snapshot| (snapshot.profile_id, snapshot))
                .collect();

        let mut app = Self {
            config,
            store,
            runtimes: HashMap::new(),
            pending_restores,
            selected_connection,
            scroll_selected_profile: false,
            top_combo_was_open: false,
            scroll_top_combo_to_selection: false,
            selected_topic: None,
            tree_filter: String::new(),
            focus_search: false,
            detail_tab: DetailTab::Value,
            value_mode: ValueMode::Json,
            history_selection: None,
            chart_field: None,
            publish: PublishDraft::default(),
            profile_editor: None,
            show_settings: false,
            settings_was_open: false,
            show_help: false,
            help_scroll: None,
            retained_confirm: None,
            show_log: false,
            toasts: Toasts::default(),
            config_dirty: false,
            save_error_shown: false,
            last_save: Instant::now(),
            applied_dark: None,
            delete_profile_confirm: None,
        };
        app.publish.open = app.config.settings.publish_open;

        // Configurations are normalized on load, so at most one profile is
        // marked; pick it deterministically in profile order.
        let auto_connect = app
            .config
            .profiles
            .iter()
            .find(|profile| profile.auto_connect)
            .map(|profile| profile.id);
        if let Some(id) = auto_connect {
            app.connect_profile(id, &cc.egui_ctx);
        }

        app
    }

    pub(crate) fn select_connection(&mut self, id: Uuid) {
        if self.selected_connection != Some(id) {
            self.selected_connection = Some(id);
            self.scroll_selected_profile = true;
            self.selected_topic = None;
            self.history_selection = None;
            self.chart_field = None;
        }
    }

    pub(crate) fn connect_profile(&mut self, id: Uuid, ctx: &egui::Context) {
        let Some(profile) = self
            .config
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
        else {
            return;
        };

        // Only one broker connection may be active at a time. Stop any other
        // running worker immediately so the old profile shows "Connect".
        for (other_id, runtime) in &mut self.runtimes {
            if *other_id != id && runtime.state.is_active() {
                stop_runtime(runtime);
            }
        }

        let handle = mqtt::spawn(profile.clone(), ctx.clone());

        if let Some(runtime) = self.runtimes.get_mut(&id) {
            let _ = runtime.handle.commands.send(Command::Shutdown);
            runtime.handle = handle;
            runtime.state = ConnectionState::Connecting;
            runtime.connected_at = None;
            runtime.last_error = None;
            runtime.subscribed_topic = None;
            runtime.logs.clear();
            runtime.suppress_highlights_until = Instant::now() + INITIAL_HIGHLIGHT_SUPPRESSION;
        } else {
            self.runtimes.insert(
                id,
                ConnectionRuntime {
                    handle,
                    state: ConnectionState::Connecting,
                    tree: TopicTree::default(),
                    connected_at: None,
                    last_error: None,
                    subscribed_topic: None,
                    logs: VecDeque::new(),
                    topic_updates: HashMap::new(),
                    lit_topics: HashSet::new(),
                    lit_paths: HashSet::new(),
                    last_update_prune: Instant::now(),
                    suppress_highlights_until: Instant::now() + INITIAL_HIGHLIGHT_SUPPRESSION,
                    restore_filter: false,
                },
            );
        }

        self.select_connection(id);
        self.config_dirty = true;
        // Ensure the other profiles immediately render their disconnected
        // state instead of waiting for the next input event.
        ctx.request_repaint();
        self.toasts.info(t!(
            "toast.connecting",
            name = profile.display_name(),
            endpoint = profile.endpoint()
        ));
    }

    /// Restores the previous session's topic tree into a runtime once its
    /// connection has been established. Nothing is shown before that point.
    fn apply_pending_restore(&mut self, id: Uuid, max_history: usize) {
        if !self.runtimes.contains_key(&id) {
            return;
        }
        let Some(snapshot) = self.pending_restores.remove(&id) else {
            return;
        };
        if !self.config.settings.restore_previous_topics {
            return;
        }
        let Some(runtime) = self.runtimes.get_mut(&id) else {
            return;
        };

        runtime.tree = restore_tree(&snapshot, max_history);
        runtime.topic_updates.clear();
        runtime.lit_topics.clear();
        runtime.lit_paths.clear();
        // A configured (possibly empty) snapshot also filters the broker's
        // retained backlog down to the topics that were visible at exit.
        runtime.restore_filter = true;
        runtime.suppress_highlights_until = Instant::now() + INITIAL_HIGHLIGHT_SUPPRESSION;
    }

    /// Stores the topic trees that are currently displayed so they can be
    /// restored when the application is started again.
    pub(crate) fn capture_topic_snapshots(&mut self) {
        let pending_ids: HashSet<Uuid> = self.pending_restores.keys().copied().collect();
        let mut snapshots = snapshots_for(
            self.runtimes
                .iter()
                .filter(|(id, _)| !pending_ids.contains(id))
                .map(|(id, runtime)| (*id, &runtime.tree)),
        );

        // A snapshot that was never applied (for example because the
        // connection could not be established) is carried forward unchanged.
        for (id, snapshot) in &self.pending_restores {
            if self.config.profiles.iter().any(|profile| profile.id == *id) {
                snapshots.push(snapshot.clone());
            }
        }

        snapshots.sort_by_key(|snapshot| snapshot.profile_id);
        self.config.topic_snapshots = snapshots;
    }

    pub(crate) fn disconnect_profile(&mut self, id: Uuid) {
        let Some(runtime) = self.runtimes.get_mut(&id) else {
            return;
        };
        if runtime.state.is_active() {
            stop_runtime(runtime);
            self.toasts.info(t!("toast.disconnected"));
        }
    }

    pub(crate) fn delete_profile(&mut self, id: Uuid) {
        if let Some(runtime) = self.runtimes.remove(&id) {
            let _ = runtime.handle.commands.send(Command::Shutdown);
        }
        self.pending_restores.remove(&id);
        self.config.profiles.retain(|profile| profile.id != id);
        self.delete_profile_confirm = None;

        if self.selected_connection == Some(id) {
            self.selected_connection = self.config.profiles.first().map(|profile| profile.id);
            self.selected_topic = None;
            self.history_selection = None;
            self.chart_field = None;
        }

        self.config_dirty = true;
        self.toasts.info(t!("toast.profile_deleted"));
    }

    pub(crate) fn save_profile(&mut self, profile: ConnectionProfile) {
        let auto_connect = profile.auto_connect;
        let id = profile.id;
        if let Some(existing) = self
            .config
            .profiles
            .iter_mut()
            .find(|existing| existing.id == profile.id)
        {
            *existing = profile;
        } else {
            self.config.profiles.push(profile);
        }
        if auto_connect {
            self.config.set_auto_connect_profile(id);
        }
        self.config_dirty = true;
        self.toasts.success(t!("toast.profile_saved"));
    }

    fn profile_name(&self, id: Uuid) -> String {
        self.config
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .map(|profile| profile.display_name().to_owned())
            .unwrap_or_else(|| t!("common.broker").to_string())
    }

    pub(crate) fn connected_target(&self) -> Option<Uuid> {
        if let Some(id) = self.selected_connection
            && self
                .runtimes
                .get(&id)
                .is_some_and(|runtime| runtime.state.is_connected())
        {
            return Some(id);
        }
        self.runtimes
            .iter()
            .find(|(_, runtime)| runtime.state.is_connected())
            .map(|(id, _)| *id)
    }

    pub(crate) fn publish_now(&mut self) {
        let Some(target) = self.connected_target() else {
            self.toasts.error(t!("toast.no_active_connection"));
            return;
        };

        let topic = self.publish.topic.trim().to_owned();
        if topic.is_empty() {
            self.toasts.error(t!("toast.topic_required"));
            return;
        }
        if topic.contains('#') || topic.contains('+') {
            self.toasts.error(t!("toast.wildcards"));
            return;
        }

        let payload = self.publish.payload.clone().into_bytes();
        let qos = self.publish.qos.min(2);
        let retain = self.publish.retain;

        let sent = self
            .runtimes
            .get(&target)
            .map(|runtime| {
                runtime.handle.commands.send(Command::Publish {
                    topic: topic.clone(),
                    payload,
                    qos,
                    retain,
                })
            })
            .transpose();

        match sent {
            Ok(Some(())) => {
                self.record_publish_history(PublishHistoryEntry {
                    topic: topic.clone(),
                    payload: self.publish.payload.clone(),
                    qos,
                    retain,
                });
                let retained = if retain {
                    t!("toast.retained_suffix").to_string()
                } else {
                    String::new()
                };
                self.toasts.success(t!(
                    "toast.published",
                    topic = topic,
                    bytes = crate::util::format_bytes(self.publish.payload.len() as u64),
                    qos = qos,
                    retained = retained
                ));
            }
            Ok(None) => self.toasts.error(t!("toast.connection_not_found")),
            Err(err) => self.toasts.error(t!("toast.publish_failed", error = err)),
        }
    }

    fn record_publish_history(&mut self, entry: PublishHistoryEntry) {
        self.config.publish_history.retain(|existing| {
            !(existing.topic == entry.topic
                && existing.payload == entry.payload
                && existing.qos == entry.qos
                && existing.retain == entry.retain)
        });
        self.config.publish_history.insert(0, entry);
        self.config.publish_history.truncate(MAX_PUBLISH_HISTORY);
        self.config_dirty = true;
    }

    fn poll_events(&mut self, ctx: &egui::Context) {
        let max_history = self.config.settings.max_history;
        let mut events = Vec::new();

        for (id, runtime) in &self.runtimes {
            for _ in 0..5_000 {
                match runtime.handle.events.try_recv() {
                    Ok(event) => events.push((*id, event)),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }
        }

        let limit_reached = events.len() >= 5_000;
        if limit_reached {
            ctx.request_repaint();
        }

        let mut notifications: Vec<(ToastLevel, String)> = Vec::new();
        let mut auto_select: Option<(Uuid, String)> = None;

        for (id, event) in events {
            let display_name = self.profile_name(id);

            // The previous session's topics become visible only after the
            // connection (and preferably its subscription) is established.
            if matches!(
                &event,
                WorkerEvent::State(ConnectionState::Connected) | WorkerEvent::Subscribed(_)
            ) {
                self.apply_pending_restore(id, max_history);
            }

            let Some(runtime) = self.runtimes.get_mut(&id) else {
                continue;
            };

            match event {
                WorkerEvent::State(state) => {
                    let previous = runtime.state.clone();
                    runtime.state = state.clone();
                    if previous == state {
                        continue;
                    }
                    match state {
                        ConnectionState::Connected => {
                            // A fresh connection always starts with a fully
                            // collapsed tree.
                            runtime.tree.set_all_expanded(false);
                            runtime.connected_at = Some(Local::now());
                            runtime.last_error = None;
                            runtime.suppress_highlights_until =
                                Instant::now() + INITIAL_HIGHLIGHT_SUPPRESSION;
                            notifications.push((
                                ToastLevel::Success,
                                t!("toast.connected", name = display_name).to_string(),
                            ));
                        }
                        ConnectionState::Reconnecting => {
                            notifications.push((
                                ToastLevel::Warning,
                                t!("toast.reconnecting", name = display_name).to_string(),
                            ));
                        }
                        ConnectionState::Disconnected => {
                            runtime.connected_at = None;
                            if previous.is_active() {
                                notifications.push((
                                    ToastLevel::Info,
                                    t!("toast.connection_disconnected", name = display_name)
                                        .to_string(),
                                ));
                            }
                        }
                        ConnectionState::Error(message) => {
                            runtime.last_error = Some(message.clone());
                            notifications
                                .push((ToastLevel::Error, format!("{display_name}: {message}")));
                        }
                        ConnectionState::Connecting => {}
                    }
                }
                WorkerEvent::Message(message) => {
                    // Retained messages are the broker's backlog replayed on
                    // (re)subscribe. Keep only those for topics that are part
                    // of the restored view; live messages (retain = false)
                    // always pass and can extend the view.
                    if !should_accept_message(
                        runtime.restore_filter,
                        message.retain,
                        runtime.tree.data(&message.topic).is_some(),
                    ) {
                        continue;
                    }

                    let was_empty = runtime.tree.value_topic_count == 0;
                    let topic = message.topic.clone();
                    let now = Instant::now();
                    let outcome = runtime.tree.insert(message, max_history);
                    match outcome {
                        InsertOutcome::NewTopic | InsertOutcome::Updated => {
                            if should_mark_update(outcome, now, runtime.suppress_highlights_until) {
                                runtime.topic_updates.insert(topic.clone(), now);
                                runtime.mark_topic_lit(&topic);
                            }
                            if was_empty
                                && auto_select.is_none()
                                && self.selected_connection == Some(id)
                            {
                                auto_select = Some((id, topic));
                            }
                        }
                        InsertOutcome::Removed => {
                            runtime.topic_updates.remove(&topic);
                            runtime.unmark_topic_lit(&topic);
                        }
                        InsertOutcome::Ignored => {}
                    }
                }
                WorkerEvent::Log(line) => {
                    log::info!("MQTT: {line}");
                    runtime
                        .logs
                        .push_back(format!("{} {line}", Local::now().format("%H:%M:%S")));
                    while runtime.logs.len() > 200 {
                        runtime.logs.pop_front();
                    }
                }
                WorkerEvent::Subscribed(topic) => {
                    runtime.subscribed_topic = Some(topic);
                    runtime.last_error = None;
                }
                WorkerEvent::SubscribeFailed(message) => {
                    runtime.subscribed_topic = None;
                    runtime.last_error = Some(message.clone());
                    notifications.push((ToastLevel::Error, format!("{display_name}: {message}")));
                }
            }
        }

        let now = Instant::now();
        for runtime in self.runtimes.values_mut() {
            if runtime.last_update_prune.elapsed() >= Duration::from_secs(1) {
                runtime
                    .topic_updates
                    .retain(|_, updated| now.duration_since(*updated) < MAX_UPDATE_RETENTION);
                runtime.last_update_prune = now;
            }
        }

        for (level, message) in notifications {
            match level {
                ToastLevel::Info => self.toasts.info(message),
                ToastLevel::Success => self.toasts.success(message),
                ToastLevel::Warning => self.toasts.warning(message),
                ToastLevel::Error => self.toasts.error(message),
            }
        }

        // Select the very first topic of a fresh connection so the details
        // panel is immediately useful.
        if let Some((id, topic)) = auto_select
            && self.selected_connection == Some(id)
            && self.selected_topic.is_none()
        {
            self.selected_topic = Some(topic);
            self.history_selection = None;
            self.chart_field = None;
        }

        // Drop selections whose topic disappeared (e.g. after a retained delete).
        if let Some(path) = self.selected_topic.clone() {
            let exists = self
                .selected_connection
                .and_then(|id| self.runtimes.get(&id))
                .is_some_and(|runtime| runtime.tree.node(&path).is_some());
            if !exists {
                self.selected_topic = None;
                self.history_selection = None;
                self.chart_field = None;
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let (publish, search, escape, quit) = ctx.input(|input| {
            (
                input.modifiers.command && input.key_pressed(egui::Key::Enter),
                input.modifiers.command && input.key_pressed(egui::Key::F),
                input.key_pressed(egui::Key::Escape),
                input.modifiers.command && input.key_pressed(egui::Key::Q),
            )
        });

        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if search {
            self.focus_search = true;
            ctx.request_repaint();
        }
        if escape && !self.tree_filter.is_empty() {
            self.tree_filter.clear();
        }
        if publish {
            self.publish_now();
        }
    }

    fn save_if_dirty(&mut self) {
        if !self.config_dirty || self.last_save.elapsed() < Duration::from_millis(500) {
            return;
        }

        self.last_save = Instant::now();
        match self.store.save(&self.config) {
            Ok(()) => {
                self.config_dirty = false;
                self.save_error_shown = false;
            }
            Err(err) => {
                log::error!("{}", t!("toast.config_save_log", error = &err));
                if !self.save_error_shown {
                    self.toasts
                        .error(t!("toast.config_save_failed", error = err));
                    self.save_error_shown = true;
                }
            }
        }
    }

    fn show_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();

        self.show_top_bar(ui);
        self.show_publish_panel(ui);
        self.show_connections_panel(ui);
        self.show_details_panel(ui);
        self.show_tree_panel(ui);
        self.show_windows(&ctx);

        let pal = ui::theme::palette(ui.visuals().dark_mode);
        self.toasts.show(&ctx, &pal);
    }

    fn show_windows(&mut self, ctx: &egui::Context) {
        self.show_profile_editor(ctx);
        self.show_settings_window(ctx);
        self.show_help_window(ctx);
        self.show_log_window(ctx);
        self.show_retained_dialog(ctx);
    }

    pub(crate) fn apply_detail_action(&mut self, action: DetailAction) {
        let Some(path) = self.selected_topic.clone() else {
            return;
        };
        match action {
            DetailAction::DeleteTopic => {
                if let Some(runtime) = self
                    .selected_connection
                    .and_then(|id| self.runtimes.get_mut(&id))
                {
                    runtime.tree.remove_topic(&path);
                    runtime.forget_subtree(&path);
                }
                self.selected_topic = None;
                self.history_selection = None;
                self.chart_field = None;
                self.toasts.info(t!("tree.removed_toast", path = path));
            }
            DetailAction::ClearHistory => {
                if let Some(runtime) = self
                    .selected_connection
                    .and_then(|id| self.runtimes.get_mut(&id))
                {
                    runtime.tree.clear_history(&path);
                }
                self.history_selection = None;
                self.chart_field = None;
                self.toasts.info(t!("toast.history_cleared"));
            }
            DetailAction::UseTopicForPublish => {
                self.publish.topic = path;
                self.publish.open = true;
            }
            DetailAction::UsePayloadForPublish(payload) => {
                self.publish.payload = payload;
                self.publish.open = true;
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn selected_runtime(&self) -> Option<&ConnectionRuntime> {
        self.selected_connection
            .and_then(|id| self.runtimes.get(&id))
    }
}

enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

impl eframe::App for MqttUiApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_events(ctx);
        self.handle_shortcuts(ctx);

        let dark = self.config.settings.dark_mode;
        if self.applied_dark != Some(dark) {
            ui::theme::apply_mode(ctx, dark);
            self.applied_dark = Some(dark);
            ctx.request_repaint();
        }

        self.toasts.tick(ctx);
        self.save_if_dirty();

        if self.config_dirty {
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        let flash_duration =
            Duration::from_secs_f32(self.config.settings.flash_duration_secs.clamp(0.5, 10.0));
        if self.config.settings.flash_tree_on_update
            && self
                .selected_connection
                .and_then(|id| self.runtimes.get(&id))
                .is_some_and(|runtime| {
                    runtime
                        .topic_updates
                        .values()
                        .any(|updated| updated.elapsed() < flash_duration)
                })
        {
            // 20 fps is enough for a smooth fade and avoids burning CPU on busy brokers.
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        if self
            .runtimes
            .values()
            .any(|runtime| runtime.state.is_active())
        {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show_ui(ui);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        let color = ui::theme::palette(visuals.dark_mode).bg;
        [
            color.r() as f32 / 255.0,
            color.g() as f32 / 255.0,
            color.b() as f32 / 255.0,
            1.0,
        ]
    }
}

impl Drop for MqttUiApp {
    fn drop(&mut self) {
        // Persist exactly the trees that are visible right now, including a
        // locally cleared tree (which produces no snapshot at all).
        self.capture_topic_snapshots();
        for runtime in self.runtimes.values() {
            let _ = runtime.handle.commands.send(Command::Shutdown);
        }
        let _ = self.store.save(&self.config);
    }
}

fn should_mark_update(outcome: InsertOutcome, now: Instant, suppress_until: Instant) -> bool {
    matches!(outcome, InsertOutcome::NewTopic | InsertOutcome::Updated) && now >= suppress_until
}

/// Decides whether an incoming message may extend a restored topic tree.
///
/// The broker replays retained messages when a subscription is established.
/// After a restore, those messages are limited to topics that are already in
/// the tree. Live messages (`retained == false`) are always accepted.
fn should_accept_message(restore_filter: bool, retained: bool, topic_in_tree: bool) -> bool {
    !restore_filter || !retained || topic_in_tree
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_embedded_locales_resolve_core_translations() {
        for (locale, expected) in [
            ("en", "Connect"),
            ("de", "Verbinden"),
            ("fr", "Connecter"),
            ("ko", "연결"),
            ("zh", "连接"),
            ("es", "Conectar"),
        ] {
            assert_eq!(
                t!("common.connect", locale = locale).to_string(),
                expected,
                "locale {locale}"
            );
        }
    }

    #[test]
    fn translations_interpolate_values() {
        assert_eq!(
            t!("tree.removed_toast", locale = "de", path = "/a/b").to_string(),
            "Thema entfernt: /a/b"
        );
        assert_eq!(
            t!("details.stats", locale = "fr", stored = 1, received = 2).to_string(),
            "1 stockés · 2 reçus"
        );
    }

    #[test]
    fn only_real_updates_after_the_initial_sync_are_highlighted() {
        let now = Instant::now();
        let suppress_until = now + Duration::from_secs(2);

        assert!(!should_mark_update(
            InsertOutcome::Removed,
            now + Duration::from_secs(5),
            suppress_until
        ));
        assert!(!should_mark_update(
            InsertOutcome::Ignored,
            now + Duration::from_secs(5),
            suppress_until
        ));
        assert!(!should_mark_update(
            InsertOutcome::NewTopic,
            now + Duration::from_secs(1),
            suppress_until
        ));
        assert!(!should_mark_update(
            InsertOutcome::Updated,
            now + Duration::from_secs(1),
            suppress_until
        ));
        assert!(should_mark_update(
            InsertOutcome::NewTopic,
            now + Duration::from_secs(3),
            suppress_until
        ));
        assert!(should_mark_update(
            InsertOutcome::Updated,
            now + Duration::from_secs(3),
            suppress_until
        ));
    }

    #[test]
    fn snapshot_roundtrip_keeps_topics_payloads_and_totals() {
        let mut tree = TopicTree::default();
        tree.insert(
            MessageRecord::new("home/value".to_owned(), vec![0, 1, 2, 255], 1, true),
            100,
        );
        tree.insert(
            MessageRecord::new("home/value".to_owned(), vec![0, 1, 2, 255], 1, true),
            100,
        );
        tree.insert(
            MessageRecord::new("/demo/text".to_owned(), b"hello".to_vec(), 0, false),
            100,
        );

        let snapshot = ConnectionSnapshot {
            profile_id: Uuid::new_v4(),
            topics: snapshot_tree(&tree),
        };
        assert_eq!(snapshot.topics.len(), 2);

        let restored = restore_tree(&snapshot, 100);
        assert_eq!(restored.data("home/value").unwrap().total, 2);
        assert_eq!(
            restored
                .data("home/value")
                .unwrap()
                .latest()
                .unwrap()
                .payload,
            vec![0, 1, 2, 255]
        );
        assert_eq!(
            restored
                .data("/demo/text")
                .unwrap()
                .latest()
                .unwrap()
                .text(),
            Some("hello")
        );
        assert_eq!(restored.value_topic_count, 2);
    }

    #[test]
    fn empty_tree_produces_no_snapshot_topics() {
        let mut tree = TopicTree::default();
        assert!(snapshot_tree(&tree).is_empty());

        tree.insert(
            MessageRecord::new("demo/value".to_owned(), b"1".to_vec(), 0, false),
            100,
        );
        tree.clear();
        assert!(snapshot_tree(&tree).is_empty());
    }

    #[test]
    fn a_locally_cleared_tree_is_captured_as_an_empty_snapshot() {
        let visible_id = Uuid::new_v4();
        let cleared_id = Uuid::new_v4();

        let mut visible = TopicTree::default();
        visible.insert(
            MessageRecord::new("home/temperature".to_owned(), b"21".to_vec(), 0, false),
            100,
        );

        let mut cleared = TopicTree::default();
        cleared.insert(
            MessageRecord::new("old/value".to_owned(), b"42".to_vec(), 0, false),
            100,
        );
        cleared.clear();

        let snapshots = snapshots_for([(visible_id, &visible), (cleared_id, &cleared)]);
        assert_eq!(snapshots.len(), 2);

        let visible_snapshot = snapshots
            .iter()
            .find(|snapshot| snapshot.profile_id == visible_id)
            .unwrap();
        assert_eq!(visible_snapshot.topics.len(), 1);
        assert_eq!(visible_snapshot.topics[0].topic, "home/temperature");

        let cleared_snapshot = snapshots
            .iter()
            .find(|snapshot| snapshot.profile_id == cleared_id)
            .unwrap();
        assert!(cleared_snapshot.topics.is_empty());
    }

    #[test]
    fn retained_backlog_is_filtered_but_live_messages_extend_the_tree() {
        // Retained message for a topic outside the restored view: ignored.
        assert!(!should_accept_message(true, true, false));
        // Retained message for a restored topic: accepted.
        assert!(should_accept_message(true, true, true));
        // Live message for a new topic: accepted.
        assert!(should_accept_message(true, false, false));
        // Without a restored view, everything is accepted.
        assert!(should_accept_message(false, true, false));
    }

    #[test]
    fn restored_topics_are_collapsed() {
        let mut tree = TopicTree::default();
        tree.insert(
            MessageRecord::new("a/b/c/d".to_owned(), b"1".to_vec(), 0, false),
            100,
        );

        let snapshot = ConnectionSnapshot {
            profile_id: Uuid::new_v4(),
            topics: snapshot_tree(&tree),
        };
        let restored = restore_tree(&snapshot, 100);
        let rows = restored.visible_rows("", false);
        assert!(rows.iter().any(|row| row.path == "a"));
        assert!(!rows.iter().any(|row| row.path == "a/b/c/d"));
    }
}
