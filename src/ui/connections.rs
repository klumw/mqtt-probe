//! Connection sidebar and profile editor.

use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

use chrono::Local;
use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Stroke, Ui,
};
use rust_i18n::t;
use uuid::Uuid;

use crate::app::MqttUiApp;
use crate::config::{ConnectionProfile, TransportKind, is_default_profile_name, new_client_id};
use crate::mqtt::ConnectionState;
use crate::util::{format_duration, truncate_chars};

use super::theme::palette;
use super::widgets::{badge, section_heading, status_color, status_dot};

/// File extensions accepted by the certificate file pickers.
const CERTIFICATE_EXTENSIONS: [&str; 8] = ["pem", "crt", "cer", "der", "PEM", "CRT", "CER", "DER"];

/// File extensions accepted by the private key file picker.
const PRIVATE_KEY_EXTENSIONS: [&str; 6] = ["pem", "key", "der", "PEM", "KEY", "DER"];

/// Reads or writes a GTK setting through `gsettings`.
///
/// Returns the trimmed command output on success. Missing binaries or schemas
/// yield `None`, so the caller keeps the native dialog behaviour.
#[cfg(target_os = "linux")]
fn gsettings(args: &[&str]) -> Option<String> {
    ["/usr/bin/gsettings", "gsettings"]
        .into_iter()
        .find_map(|program| {
            let output = Command::new(program)
                .args(args)
                .stderr(Stdio::null())
                .output()
                .ok()?;
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        })
}

/// Opens the file dialog with hidden (dot-prefixed) entries included.
///
/// The Linux portal dialog follows the GTK preference
/// `org.gtk.Settings.FileChooser show-hidden`, so the preference is enabled
/// for the lifetime of the dialog and restored afterwards. On other
/// platforms the native dialog decides which hidden entries it lists.
fn pick_path(dialog: rfd::FileDialog) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let hidden_was_off = gsettings(&["get", "org.gtk.Settings.FileChooser", "show-hidden"])
            .as_deref()
            == Some("false");
        if hidden_was_off {
            gsettings(&["set", "org.gtk.Settings.FileChooser", "show-hidden", "true"]);
        }
        let picked = dialog.pick_file();
        if hidden_was_off {
            gsettings(&[
                "set",
                "org.gtk.Settings.FileChooser",
                "show-hidden",
                "false",
            ]);
        }
        picked
    }
    #[cfg(not(target_os = "linux"))]
    {
        dialog.pick_file()
    }
}

/// Opens the platform file dialog and returns the path the user picked.
///
/// The dialog starts in the folder of the current value when that folder
/// exists and otherwise uses the platform default location.
fn pick_file(
    title: &str,
    current_path: &str,
    filter_name: &str,
    extensions: &[&str],
) -> Option<String> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(title)
        .add_filter(filter_name, extensions);
    if let Some(directory) = Path::new(current_path.trim()).parent()
        && directory.is_dir()
    {
        dialog = dialog.set_directory(directory);
    }
    pick_path(dialog).map(|path| path.display().to_string())
}

/// Renders a path input with a button that opens the platform file dialog.
///
/// Field and button are only interactive when `enabled` is set, so the TLS
/// settings stay inactive for transports that do not use TLS.
fn path_input(
    ui: &mut Ui,
    value: &mut String,
    title: &str,
    hint: &str,
    filter_name: &str,
    extensions: &[&str],
    enabled: bool,
) {
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(value)
                    .desired_width(220.0)
                    .hint_text(hint),
            );
            if ui
                .add(egui::Button::new(RichText::new("📂").size(14.0)))
                .on_hover_text(t!("editor.browse_tooltip"))
                .clicked()
                && let Some(path) = pick_file(title, value, filter_name, extensions)
            {
                *value = path;
                ui.ctx().request_repaint();
            }
        });
    });
}

pub struct ProfileEditor {
    pub draft: ConnectionProfile,
    pub is_new: bool,
    pub error: Option<String>,
}

impl ProfileEditor {
    pub fn new() -> Self {
        let draft = ConnectionProfile {
            name: t!("connections.new_profile_name").to_string(),
            port: 1883,
            ..ConnectionProfile::default()
        };
        Self {
            draft,
            is_new: true,
            error: None,
        }
    }

    pub fn from_profile(profile: &ConnectionProfile) -> Self {
        let mut draft = profile.clone();
        if is_default_profile_name(&draft.name) {
            draft.name = t!("connections.default_profile_name").to_string();
        }
        Self {
            draft,
            is_new: false,
            error: None,
        }
    }
}

struct ProfileSnapshot {
    id: Uuid,
    name: String,
    endpoint: String,
    transport: TransportKind,
    state_label: String,
    color: Color32,
    connected: bool,
    auto_connect: bool,
    uptime: Option<i64>,
    subscribe_topic: String,
    subscribed: Option<String>,
    last_error: Option<String>,
}

enum ConnectionAction {
    New,
    Select(Uuid),
    Connect(Uuid),
    Disconnect(Uuid),
    Edit(Uuid),
    AskDelete(Uuid),
    Delete(Uuid),
    CancelDelete,
}

impl MqttUiApp {
    pub(crate) fn show_connections_panel(&mut self, ui: &mut Ui) {
        let pal = palette(ui.visuals().dark_mode);
        let ctx = ui.ctx().clone();
        let selected = self.selected_connection;
        let scroll_selected = self.scroll_selected_profile;
        let delete_confirm = self.delete_profile_confirm;
        let mut action: Option<ConnectionAction> = None;

        let profiles: Vec<ProfileSnapshot> = self
            .config
            .profiles
            .iter()
            .map(|profile| {
                let runtime = self.runtimes.get(&profile.id);
                let state = runtime
                    .map(|runtime| runtime.state.clone())
                    .unwrap_or(ConnectionState::Disconnected);
                let uptime = runtime
                    .and_then(|runtime| runtime.connected_at)
                    .map(|since| (Local::now() - since).num_seconds());
                ProfileSnapshot {
                    id: profile.id,
                    name: profile.display_name().to_owned(),
                    endpoint: profile.endpoint(),
                    transport: profile.transport_kind(),
                    state_label: state.label().to_owned(),
                    color: status_color(&state, &pal),
                    connected: state.is_connected(),
                    auto_connect: profile.auto_connect,
                    uptime,
                    subscribe_topic: if profile.subscribe_topic.trim().is_empty() {
                        "—".to_owned()
                    } else {
                        profile.subscribe_topic.clone()
                    },
                    subscribed: runtime.and_then(|runtime| runtime.subscribed_topic.clone()),
                    last_error: runtime.and_then(|runtime| runtime.last_error.clone()),
                }
            })
            .collect();

        egui::Panel::left("connections-panel")
            .default_size(330.0)
            .min_size(270.0)
            .max_size(480.0)
            .resizable(true)
            .frame(
                Frame::new()
                    .fill(pal.panel)
                    .stroke(Stroke::new(1.0, pal.border))
                    .inner_margin(Margin::same(12)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    section_heading(ui, &t!("connections.title"), &pal);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(t!("connections.new")).clicked() {
                            action = Some(ConnectionAction::New);
                        }
                    });
                });
                ui.add_space(6.0);

                egui::Panel::bottom("broker-info-panel")
                    .resizable(false)
                    .show_separator_line(false)
                    .default_size(150.0)
                    .frame(Frame::new().inner_margin(Margin::symmetric(0, 6)))
                    .show(ui, |ui| {
                        if let Some(profile) =
                            profiles.iter().find(|profile| Some(profile.id) == selected)
                        {
                            Frame::new()
                                .fill(pal.card)
                                .stroke(Stroke::new(1.0, pal.border))
                                .corner_radius(CornerRadius::same(10))
                                .inner_margin(Margin::same(10))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        section_heading(ui, &t!("connections.broker_info"), &pal);
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                badge(ui, &profile.state_label, profile.color);
                                            },
                                        );
                                    });
                                    egui::Grid::new("broker-info-grid")
                                        .num_columns(2)
                                        .spacing([12.0, 3.0])
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(
                                                    t!("connections.endpoint").to_string(),
                                                )
                                                .size(10.5)
                                                .color(pal.muted),
                                            );
                                            ui.label(
                                                RichText::new(&profile.endpoint)
                                                    .monospace()
                                                    .size(10.5)
                                                    .color(pal.text),
                                            );
                                            ui.end_row();

                                            ui.label(
                                                RichText::new(
                                                    t!("connections.subscription").to_string(),
                                                )
                                                .size(10.5)
                                                .color(pal.muted),
                                            );
                                            ui.label(
                                                RichText::new(
                                                    profile
                                                        .subscribed
                                                        .as_ref()
                                                        .unwrap_or(&profile.subscribe_topic),
                                                )
                                                .monospace()
                                                .size(10.5)
                                                .color(pal.text),
                                            );
                                            ui.end_row();

                                            if let Some(uptime) = profile.uptime {
                                                ui.label(
                                                    RichText::new(
                                                        t!("connections.uptime").to_string(),
                                                    )
                                                    .size(10.5)
                                                    .color(pal.muted),
                                                );
                                                ui.label(
                                                    RichText::new(format_duration(uptime))
                                                        .monospace()
                                                        .size(10.5)
                                                        .color(pal.text),
                                                );
                                                ui.end_row();
                                            }
                                        });

                                    if let Some(error) = &profile.last_error {
                                        ui.add_space(3.0);
                                        ui.label(
                                            RichText::new(truncate_chars(error, 90))
                                                .size(10.0)
                                                .color(pal.danger),
                                        );
                                    }
                                });
                        } else {
                            ui.label(
                                RichText::new(t!("connections.select_profile").to_string())
                                    .size(10.5)
                                    .color(pal.muted),
                            );
                        }
                    });

                Frame::new()
                    .fill(pal.panel)
                    .stroke(Stroke::new(1.0, pal.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        ScrollArea::vertical()
                            .id_salt("connections-scroll")
                            .auto_shrink([false, false])
                            .max_height(ui.available_height())
                            .show(ui, |ui| {
                                for profile in &profiles {
                                    let is_selected = selected == Some(profile.id);
                                    let card = Frame::new()
                                        .fill(if is_selected { pal.hover } else { pal.card })
                                        .stroke(Stroke::new(
                                            1.0,
                                            if is_selected { pal.accent } else { pal.border },
                                        ))
                                        .corner_radius(CornerRadius::same(10))
                                        .inner_margin(Margin::same(10))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                status_dot(ui, profile.color);
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new(truncate_chars(
                                                                &profile.name,
                                                                30,
                                                            ))
                                                            .strong()
                                                            .color(pal.text),
                                                        )
                                                        .frame(false),
                                                    )
                                                    .on_hover_text(&profile.name)
                                                    .clicked()
                                                {
                                                    action =
                                                        Some(ConnectionAction::Select(profile.id));
                                                }
                                                if profile.auto_connect {
                                                    badge(ui, "AUTO", pal.muted);
                                                }
                                            });

                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(&profile.endpoint)
                                                        .monospace()
                                                        .size(10.5)
                                                        .color(pal.muted),
                                                );
                                                match profile.transport {
                                                    TransportKind::Tcp => {}
                                                    TransportKind::Tls => {
                                                        badge(ui, "TLS", pal.success)
                                                    }
                                                    TransportKind::Ws => badge(ui, "WS", pal.muted),
                                                    TransportKind::Wss => {
                                                        badge(ui, "WSS", pal.success)
                                                    }
                                                }
                                            });

                                            if !profile.connected
                                                && let Some(error) = &profile.last_error
                                            {
                                                ui.label(
                                                    RichText::new(truncate_chars(error, 54))
                                                        .size(10.5)
                                                        .color(pal.danger),
                                                );
                                            }

                                            ui.add_space(3.0);
                                            ui.horizontal(|ui| {
                                                if profile.connected {
                                                    if ui
                                                        .small_button(t!("common.disconnect"))
                                                        .clicked()
                                                    {
                                                        action =
                                                            Some(ConnectionAction::Disconnect(
                                                                profile.id,
                                                            ));
                                                    }
                                                } else {
                                                    let button = egui::Button::new(
                                                        RichText::new(
                                                            t!("common.connect").to_string(),
                                                        )
                                                        .color(Color32::WHITE)
                                                        .strong()
                                                        .size(11.5),
                                                    )
                                                    .fill(pal.accent)
                                                    .stroke(Stroke::new(1.0, pal.accent));
                                                    if ui.add(button).clicked() {
                                                        action = Some(ConnectionAction::Connect(
                                                            profile.id,
                                                        ));
                                                    }
                                                }
                                                if ui.small_button(t!("common.edit")).clicked() {
                                                    action =
                                                        Some(ConnectionAction::Edit(profile.id));
                                                }
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        if ui
                                                            .small_button("×")
                                                            .on_hover_text(t!(
                                                                "connections.delete_profile_tooltip"
                                                            ))
                                                            .clicked()
                                                        {
                                                            action = if delete_confirm
                                                                == Some(profile.id)
                                                            {
                                                                Some(ConnectionAction::Delete(
                                                                    profile.id,
                                                                ))
                                                            } else {
                                                                Some(ConnectionAction::AskDelete(
                                                                    profile.id,
                                                                ))
                                                            };
                                                        }
                                                    },
                                                );
                                            });

                                            if delete_confirm == Some(profile.id) {
                                                ui.add_space(4.0);
                                                Frame::new()
                                                    .fill(Color32::from_rgba_unmultiplied(
                                                        pal.danger.r(),
                                                        pal.danger.g(),
                                                        pal.danger.b(),
                                                        24,
                                                    ))
                                                    .stroke(Stroke::new(1.0, pal.danger))
                                                    .corner_radius(CornerRadius::same(7))
                                                    .inner_margin(Margin::same(7))
                                                    .show(ui, |ui| {
                                                        ui.label(
                                                            RichText::new(
                                                                t!("connections.delete_confirm")
                                                                    .to_string(),
                                                            )
                                                            .size(10.5)
                                                            .color(pal.danger),
                                                        );
                                                        ui.horizontal(|ui| {
                                                            if ui
                                                                .small_button(t!("common.delete"))
                                                                .clicked()
                                                            {
                                                                action =
                                                                    Some(ConnectionAction::Delete(
                                                                        profile.id,
                                                                    ));
                                                            }
                                                            if ui
                                                                .small_button(t!("common.cancel"))
                                                                .clicked()
                                                            {
                                                                action = Some(
                                                                    ConnectionAction::CancelDelete,
                                                                );
                                                            }
                                                        });
                                                    });
                                            }
                                        });
                                    if is_selected && scroll_selected {
                                        card.response.scroll_to_me(None);
                                    }
                                    ui.add_space(7.0);
                                }

                                if profiles.is_empty() {
                                    ui.add_space(20.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            RichText::new(
                                                t!("connections.no_profiles").to_string(),
                                            )
                                            .color(pal.muted),
                                        );
                                        ui.label(
                                            RichText::new(
                                                t!("connections.create_first").to_string(),
                                            )
                                            .size(10.5)
                                            .color(pal.muted),
                                        );
                                    });
                                }
                            });
                    });
            });

        if scroll_selected {
            self.scroll_selected_profile = false;
        }

        match action {
            Some(ConnectionAction::New) => {
                self.profile_editor = Some(ProfileEditor::new());
            }
            Some(ConnectionAction::Select(id)) => self.select_connection(id),
            Some(ConnectionAction::Connect(id)) => self.connect_profile(id, &ctx),
            Some(ConnectionAction::Disconnect(id)) => self.disconnect_profile(id),
            Some(ConnectionAction::Edit(id)) => {
                if let Some(profile) = self.config.profiles.iter().find(|profile| profile.id == id)
                {
                    self.profile_editor = Some(ProfileEditor::from_profile(profile));
                }
            }
            Some(ConnectionAction::AskDelete(id)) => {
                self.delete_profile_confirm = Some(id);
            }
            Some(ConnectionAction::Delete(id)) => self.delete_profile(id),
            Some(ConnectionAction::CancelDelete) => {
                self.delete_profile_confirm = None;
            }
            None => {}
        }
    }

    pub(crate) fn show_profile_editor(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.profile_editor.take() else {
            return;
        };

        let pal = palette(self.config.settings.dark_mode);
        let title = if editor.is_new {
            t!("editor.new_title").to_string()
        } else {
            t!("editor.edit_title").to_string()
        };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;

        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                egui::Grid::new("profile-editor-grid")
                    .num_columns(2)
                    .spacing([14.0, 7.0])
                    .show(ui, |ui| {
                        ui.label(t!("editor.name"));
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.draft.name)
                                .desired_width(320.0)
                                .hint_text(t!("editor.name_hint")),
                        );
                        ui.end_row();

                        ui.label(t!("editor.broker"));
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut editor.draft.host)
                                    .desired_width(230.0)
                                    .hint_text("localhost"),
                            );
                            ui.label(t!("editor.port"));
                            ui.add(
                                egui::DragValue::new(&mut editor.draft.port).range(1..=65_535u16),
                            );
                        });
                        ui.end_row();

                        ui.label(t!("editor.transport"));
                        ui.vertical(|ui| {
                            let current = editor.draft.transport_kind();
                            let mut selected = current;
                            egui::ComboBox::from_id_salt("editor-transport")
                                .width(220.0)
                                .selected_text(transport_label(current))
                                .show_ui(ui, |ui| {
                                    for kind in [
                                        TransportKind::Tcp,
                                        TransportKind::Tls,
                                        TransportKind::Ws,
                                        TransportKind::Wss,
                                    ] {
                                        ui.selectable_value(
                                            &mut selected,
                                            kind,
                                            transport_label(kind),
                                        );
                                    }
                                });
                            if selected != current {
                                editor.draft.set_transport_kind(selected);
                            }
                            if matches!(selected, TransportKind::Ws | TransportKind::Wss) {
                                ui.horizontal(|ui| {
                                    ui.label(t!("editor.websocket_path"));
                                    ui.add(
                                        egui::TextEdit::singleline(
                                            &mut editor.draft.websocket_path,
                                        )
                                        .desired_width(160.0)
                                        .hint_text(t!("editor.websocket_path_hint")),
                                    );
                                });
                                ui.label(
                                    RichText::new(t!("editor.websocket_hint").to_string())
                                        .size(10.0)
                                        .color(pal.muted),
                                );
                            }
                        });
                        ui.end_row();

                        ui.label(t!("editor.ca_certificate"));
                        ui.vertical(|ui| {
                            path_input(
                                ui,
                                &mut editor.draft.ca_cert_path,
                                &t!("editor.ca_certificate"),
                                &t!("editor.ca_certificate_hint"),
                                &t!("editor.filter_certificates"),
                                &CERTIFICATE_EXTENSIONS,
                                editor.draft.tls,
                            );
                            ui.label(
                                RichText::new(t!("editor.ca_certificate_help").to_string())
                                    .size(10.0)
                                    .color(pal.muted),
                            );
                        });
                        ui.end_row();

                        ui.label(t!("editor.client_certificate"));
                        ui.vertical(|ui| {
                            path_input(
                                ui,
                                &mut editor.draft.client_cert_path,
                                &t!("editor.client_certificate"),
                                &t!("editor.client_certificate_hint"),
                                &t!("editor.filter_certificates"),
                                &CERTIFICATE_EXTENSIONS,
                                editor.draft.tls,
                            );
                            ui.label(
                                RichText::new(t!("editor.client_certificate_help").to_string())
                                    .size(10.0)
                                    .color(pal.muted),
                            );
                        });
                        ui.end_row();

                        ui.label(t!("editor.client_key"));
                        ui.vertical(|ui| {
                            path_input(
                                ui,
                                &mut editor.draft.client_key_path,
                                &t!("editor.client_key"),
                                &t!("editor.client_key_hint"),
                                &t!("editor.filter_keys"),
                                &PRIVATE_KEY_EXTENSIONS,
                                editor.draft.tls,
                            );
                            ui.label(
                                RichText::new(t!("editor.client_key_help").to_string())
                                    .size(10.0)
                                    .color(pal.muted),
                            );
                        });
                        ui.end_row();

                        ui.label(t!("editor.username"));
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.draft.username)
                                .desired_width(320.0)
                                .hint_text(t!("common.optional")),
                        );
                        ui.end_row();

                        ui.label(t!("editor.password"));
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.draft.password)
                                .password(true)
                                .desired_width(320.0)
                                .hint_text(t!("common.optional")),
                        );
                        ui.end_row();

                        ui.label("");
                        ui.vertical(|ui| {
                            ui.checkbox(
                                &mut editor.draft.save_password,
                                t!("editor.save_password"),
                            );
                            if editor.draft.save_password {
                                ui.label(
                                    RichText::new(t!("editor.save_password_warning").to_string())
                                        .size(10.0)
                                        .color(pal.warn),
                                );
                            }
                        });
                        ui.end_row();

                        ui.label(t!("editor.client_id"));
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut editor.draft.client_id)
                                    .desired_width(230.0),
                            );
                            if ui.small_button(t!("common.new")).clicked() {
                                editor.draft.client_id = new_client_id();
                            }
                        });
                        ui.end_row();

                        ui.label(t!("editor.keep_alive"));
                        ui.add(
                            egui::DragValue::new(&mut editor.draft.keep_alive_secs)
                                .range(5..=3_600u64)
                                .suffix(" s"),
                        );
                        ui.end_row();

                        ui.label(t!("editor.session"));
                        ui.checkbox(&mut editor.draft.clean_session, t!("editor.clean_session"));
                        ui.end_row();

                        ui.label(t!("editor.subscription"));
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut editor.draft.subscribe_topic)
                                    .desired_width(210.0)
                                    .hint_text("#"),
                            );
                            ui.label(t!("editor.qos"));
                            let mut qos = editor.draft.subscribe_qos;
                            egui::ComboBox::from_id_salt("editor-subscribe-qos")
                                .width(70.0)
                                .selected_text(format!("QoS {qos}"))
                                .show_ui(ui, |ui| {
                                    for value in 0..=2u8 {
                                        ui.selectable_value(
                                            &mut qos,
                                            value,
                                            format!("QoS {value}"),
                                        );
                                    }
                                });
                            editor.draft.subscribe_qos = qos;
                        });
                        ui.end_row();

                        ui.label(t!("editor.start"));
                        ui.vertical(|ui| {
                            ui.checkbox(&mut editor.draft.auto_connect, t!("editor.auto_connect"));
                            ui.label(
                                RichText::new(t!("editor.auto_connect_hint").to_string())
                                    .size(10.0)
                                    .color(pal.muted),
                            );
                        });
                        ui.end_row();
                    });

                if let Some(error) = &editor.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(error).size(11.0).color(pal.danger));
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let save_button = egui::Button::new(
                        RichText::new(if editor.is_new {
                            t!("editor.create").to_string()
                        } else {
                            t!("common.save").to_string()
                        })
                        .color(Color32::WHITE)
                        .strong(),
                    )
                    .fill(pal.accent)
                    .stroke(Stroke::new(1.0, pal.accent));
                    if ui.add(save_button).clicked() {
                        save = true;
                    }
                    if ui.button(t!("common.cancel")).clicked() {
                        cancel = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(t!("editor.password_note").to_string())
                                .size(10.0)
                                .color(pal.muted),
                        );
                    });
                });
            });

        if save {
            match validate_profile(&mut editor.draft) {
                Ok(()) => self.save_profile(editor.draft),
                Err(error) => {
                    editor.error = Some(error);
                    self.profile_editor = Some(editor);
                }
            }
        } else if open && !cancel {
            self.profile_editor = Some(editor);
        }
    }
}

pub(crate) fn transport_label(kind: TransportKind) -> String {
    match kind {
        TransportKind::Tcp => t!("editor.transport_tcp").to_string(),
        TransportKind::Tls => t!("editor.transport_tls").to_string(),
        TransportKind::Ws => t!("editor.transport_ws").to_string(),
        TransportKind::Wss => t!("editor.transport_wss").to_string(),
    }
}

fn validate_profile(profile: &mut ConnectionProfile) -> Result<(), String> {
    profile.name = profile.name.trim().to_owned();
    profile.host = profile.host.trim().to_owned();
    profile.username = profile.username.trim().to_owned();
    profile.ca_cert_path = profile.ca_cert_path.trim().to_owned();
    profile.client_cert_path = profile.client_cert_path.trim().to_owned();
    profile.client_key_path = profile.client_key_path.trim().to_owned();
    profile.subscribe_topic = profile.subscribe_topic.trim().to_owned();
    profile.websocket_path = crate::mqtt::websocket::normalize_path(&profile.websocket_path);

    if profile.host.is_empty() {
        return Err(t!("validation.host_required").to_string());
    }
    if profile.host.contains("://") {
        return Err(t!("validation.host_scheme").to_string());
    }
    if profile.port == 0 {
        return Err(t!("validation.port").to_string());
    }
    if profile.websocket && profile.websocket_path.chars().any(char::is_whitespace) {
        return Err(t!("validation.websocket_path").to_string());
    }
    if profile.tls && !profile.ca_cert_path.is_empty() {
        crate::mqtt::validate_ca_certificate(&profile.ca_cert_path).map_err(|err| {
            t!(
                "validation.ca_certificate",
                path = profile.ca_cert_path,
                error = err
            )
            .to_string()
        })?;
    }
    if profile.tls && (!profile.client_cert_path.is_empty() || !profile.client_key_path.is_empty())
    {
        crate::mqtt::validate_client_credentials(
            &profile.client_cert_path,
            &profile.client_key_path,
        )
        .map_err(|err| {
            t!(
                "validation.client_credentials",
                certificate = profile.client_cert_path,
                key = profile.client_key_path,
                error = err
            )
            .to_string()
        })?;
    }
    if profile.client_id.trim().is_empty() {
        profile.client_id = new_client_id();
    }
    if profile.subscribe_qos > 2 {
        profile.subscribe_qos = 0;
    }
    if profile.keep_alive_secs < 5 {
        profile.keep_alive_secs = 5;
    }
    Ok(())
}
