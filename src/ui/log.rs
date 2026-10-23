//! Connection diagnostics and log window.

use chrono::Local;
use eframe::egui::{self, Align2, CollapsingHeader, Grid, Label, RichText, ScrollArea};
use rust_i18n::t;

use crate::app::{ConnectionRuntime, MqttUiApp};
use crate::config::ConnectionProfile;
use crate::util::{format_count, format_duration};

use super::connections::transport_label;
use super::theme::palette;

impl MqttUiApp {
    pub(crate) fn show_log_window(&mut self, ctx: &egui::Context) {
        if !self.show_log {
            return;
        }

        let pal = palette(self.config.settings.dark_mode);
        let selected = self.selected_connection;
        let profile =
            selected.and_then(|id| self.config.profiles.iter().find(|profile| profile.id == id));
        let runtime = selected.and_then(|id| self.runtimes.get(&id));
        let logs: Vec<String> = runtime
            .map(|runtime| runtime.logs.iter().cloned().collect())
            .unwrap_or_default();
        let state = runtime
            .map(|runtime| runtime.state.label())
            .unwrap_or_else(|| t!("common.no_connection").to_string());
        let last_error = runtime.and_then(|runtime| runtime.last_error.clone());

        let details = profile
            .map(|profile| diagnostic_rows(profile, runtime))
            .unwrap_or_default();
        let diagnostics = build_diagnostics(profile, runtime, &details, &last_error, &logs);

        let mut open = true;
        let mut copy_logs = false;
        let mut copy_diagnostics = false;
        let mut clear = false;

        egui::Window::new(t!("log.title"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(760.0, 520.0))
            .min_width(520.0)
            .min_height(300.0)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| match profile {
                    Some(profile) => {
                        ui.label(
                            RichText::new(profile.display_name())
                                .size(12.5)
                                .strong()
                                .color(pal.text),
                        );
                        ui.label(
                            RichText::new(profile.endpoint())
                                .monospace()
                                .size(10.5)
                                .color(pal.muted),
                        );
                        ui.label(RichText::new(state.clone()).size(10.5).color(pal.muted));
                    }
                    None => {
                        ui.label(
                            RichText::new(t!("log.no_connection").to_string())
                                .size(12.0)
                                .color(pal.muted),
                        );
                    }
                });

                if let Some(error) = &last_error {
                    ui.label(RichText::new(error).size(10.5).color(pal.danger));
                }

                if !details.is_empty() {
                    ui.add_space(2.0);
                    CollapsingHeader::new(t!("log.details"))
                        .default_open(false)
                        .show(ui, |ui| {
                            diagnostics_grid(ui, &details, &pal);
                            if let Some(runtime) = runtime {
                                ui.label(
                                    RichText::new(t!(
                                        "tree.counts",
                                        topics =
                                            format_count(runtime.tree.value_topic_count as u64),
                                        messages = format_count(runtime.tree.received_messages)
                                    ))
                                    .size(10.5)
                                    .color(pal.muted),
                                );
                            }
                        });
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let has_logs = !logs.is_empty();
                    let has_diagnostics = profile.is_some() || last_error.is_some();
                    if ui
                        .add_enabled(
                            has_diagnostics,
                            egui::Button::new(t!("log.copy_diagnostics")).small(),
                        )
                        .clicked()
                    {
                        copy_diagnostics = true;
                    }
                    if ui
                        .add_enabled(has_logs, egui::Button::new(t!("log.copy_all")).small())
                        .clicked()
                    {
                        copy_logs = true;
                    }
                    if ui
                        .add_enabled(has_logs, egui::Button::new(t!("log.clear")).small())
                        .clicked()
                    {
                        clear = true;
                    }
                });
                ui.separator();

                ScrollArea::vertical()
                    .id_salt("connection-log-scroll")
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .max_height((ui.available_height() - 4.0).max(160.0))
                    .show(ui, |ui| {
                        if logs.is_empty() {
                            ui.label(
                                RichText::new(t!("log.empty").to_string())
                                    .size(11.0)
                                    .color(pal.muted),
                            );
                        } else {
                            for line in &logs {
                                ui.add(
                                    Label::new(
                                        RichText::new(line).monospace().size(10.5).color(pal.text),
                                    )
                                    .wrap(),
                                );
                            }
                        }
                    });
            });

        if copy_logs {
            ctx.copy_text(logs.join("\n"));
            self.toasts.info(t!("log.copied"));
        }
        if copy_diagnostics {
            ctx.copy_text(diagnostics);
            self.toasts.info(t!("log.diagnostics_copied"));
        }
        if clear {
            if let Some(runtime) = self
                .selected_connection
                .and_then(|id| self.runtimes.get_mut(&id))
            {
                runtime.logs.clear();
            }
            self.toasts.info(t!("log.cleared"));
        }
        self.show_log = open;
    }
}

fn diagnostics_grid(ui: &mut egui::Ui, rows: &[(String, String)], pal: &super::theme::Palette) {
    Grid::new("log-diagnostics-grid")
        .num_columns(2)
        .spacing([18.0, 4.0])
        .show(ui, |ui| {
            for (label, value) in rows {
                ui.label(RichText::new(label).size(10.5).color(pal.muted));
                ui.add(
                    Label::new(RichText::new(value).monospace().size(10.5).color(pal.text)).wrap(),
                );
                ui.end_row();
            }
        });
}

/// Builds the connection facts shown in the diagnostics section and copied by
/// "Copy diagnostics".
fn diagnostic_rows(
    profile: &ConnectionProfile,
    runtime: Option<&ConnectionRuntime>,
) -> Vec<(String, String)> {
    let mut rows = vec![
        (t!("connections.endpoint").to_string(), profile.endpoint()),
        (
            t!("editor.transport").to_string(),
            transport_label(profile.transport_kind()),
        ),
        (
            t!("editor.client_id").to_string(),
            if profile.client_id.trim().is_empty() {
                t!("log.auto").to_string()
            } else {
                profile.client_id.trim().to_owned()
            },
        ),
        (
            t!("editor.keep_alive").to_string(),
            format!("{} s", profile.keep_alive_secs),
        ),
        (
            t!("editor.clean_session").to_string(),
            if profile.clean_session {
                t!("common.yes").to_string()
            } else {
                t!("common.no").to_string()
            },
        ),
        (
            t!("connections.subscription").to_string(),
            if profile.subscribe_topic.trim().is_empty() {
                "—".to_owned()
            } else {
                format!(
                    "{} (QoS {})",
                    profile.subscribe_topic.trim(),
                    profile.subscribe_qos
                )
            },
        ),
    ];

    if !profile.ca_cert_path.trim().is_empty() {
        rows.push((
            t!("editor.ca_certificate").to_string(),
            profile.ca_cert_path.trim().to_owned(),
        ));
    }
    if !profile.client_cert_path.trim().is_empty() {
        rows.push((
            t!("editor.client_certificate").to_string(),
            profile.client_cert_path.trim().to_owned(),
        ));
    }
    if profile.websocket {
        rows.push((
            t!("editor.websocket_path").to_string(),
            profile.websocket_path.clone(),
        ));
    }
    if let Some(runtime) = runtime
        && let Some(since) = runtime.connected_at
    {
        rows.push((
            t!("connections.uptime").to_string(),
            format_duration((Local::now() - since).num_seconds()),
        ));
    }

    rows
}

fn build_diagnostics(
    profile: Option<&ConnectionProfile>,
    runtime: Option<&ConnectionRuntime>,
    details: &[(String, String)],
    last_error: &Option<String>,
    logs: &[String],
) -> String {
    let mut out = String::new();

    if let Some(profile) = profile {
        out.push_str(&profile.display_name());
        out.push('\n');
        for (label, value) in details {
            out.push_str(label);
            out.push_str(": ");
            out.push_str(value);
            out.push('\n');
        }
        if let Some(runtime) = runtime {
            out.push_str(
                t!(
                    "connections.messages_topics",
                    messages = format_count(runtime.tree.received_messages),
                    topics = format_count(runtime.tree.value_topic_count as u64)
                )
                .as_ref(),
            );
            out.push('\n');
        }
    }
    if let Some(error) = last_error {
        out.push_str(&format!("{}: {error}\n", t!("state.error")));
    }
    if !logs.is_empty() {
        out.push_str(&format!("\n--- {} ---\n", t!("log.button")));
        out.push_str(&logs.join("\n"));
        out.push('\n');
    }

    out
}
