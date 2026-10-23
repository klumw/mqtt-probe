//! Publish composer at the bottom of the window.

use eframe::egui::{self, Align, Color32, Frame, Layout, Margin, RichText, Stroke, Ui};
use rust_i18n::t;

use crate::app::MqttUiApp;
use crate::util::{format_bytes, truncate_chars};

use super::theme::palette;
use super::widgets::badge;

pub struct PublishDraft {
    pub topic: String,
    pub payload: String,
    pub qos: u8,
    pub retain: bool,
    pub open: bool,
}

impl Default for PublishDraft {
    fn default() -> Self {
        Self {
            topic: String::new(),
            payload: "{\n  \"hello\": \"mqtt\"\n}".to_owned(),
            qos: 0,
            retain: false,
            open: true,
        }
    }
}

impl MqttUiApp {
    pub(crate) fn show_publish_panel(&mut self, ui: &mut Ui) {
        let pal = palette(ui.visuals().dark_mode);
        let connected = self.connected_target();
        let target_name = connected.and_then(|id| {
            self.config
                .profiles
                .iter()
                .find(|profile| profile.id == id)
                .map(|profile| profile.display_name().to_owned())
        });
        let open = self.publish.open;
        let mut do_publish = false;

        // A separate id for the collapsed state keeps the stored height of the
        // expanded panel intact when it is toggled closed.
        let panel_id = if open {
            "publish-panel"
        } else {
            "publish-panel-collapsed"
        };
        let mut panel = egui::Panel::bottom(panel_id).frame(
            Frame::new()
                .fill(pal.panel)
                .stroke(Stroke::new(1.0, pal.border))
                .inner_margin(Margin::symmetric(14, 8)),
        );
        panel = if open {
            panel
                .resizable(true)
                .default_size(255.0)
                .min_size(175.0)
                .max_size(560.0)
        } else {
            panel.exact_size(44.0)
        };

        panel.show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(if open { "▾" } else { "▸" })
                                .size(12.0)
                                .color(pal.muted),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    self.publish.open = !open;
                    self.config.settings.publish_open = self.publish.open;
                    self.config_dirty = true;
                }
                ui.label(
                    RichText::new(t!("publish.title").to_string())
                        .size(13.5)
                        .strong()
                        .color(pal.text),
                );

                if let Some(name) = &target_name {
                    badge(ui, &format!("→ {name}"), pal.success);
                } else {
                    badge(ui, &t!("common.no_connection"), pal.danger);
                }

                if !open && !self.publish.topic.trim().is_empty() {
                    ui.label(
                        RichText::new(format!(
                            "{}  ·  {}",
                            truncate_chars(&self.publish.topic, 50),
                            format_bytes(self.publish.payload.len() as u64)
                        ))
                        .monospace()
                        .size(10.5)
                        .color(pal.muted),
                    );
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(t!("about.shortcut_publish_key").to_string())
                            .monospace()
                            .size(10.0)
                            .color(pal.muted),
                    );
                });
            });

            if !open {
                return;
            }

            ui.separator();

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t!("publish.topic").to_string())
                        .size(10.5)
                        .color(pal.muted),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.publish.topic)
                        .hint_text(t!("publish.topic_hint"))
                        .desired_width((ui.available_width() - 130.0).max(120.0)),
                );
                if ui
                    .add_enabled(
                        self.selected_topic.is_some(),
                        egui::Button::new(t!("publish.from_selection")),
                    )
                    .on_hover_text(t!("publish.use_selected_tooltip"))
                    .clicked()
                    && let Some(topic) = &self.selected_topic
                {
                    self.publish.topic = topic.clone();
                }
            });

            ui.add_space(3.0);
            ui.add(
                egui::TextEdit::multiline(&mut self.publish.payload)
                    .code_editor()
                    .desired_rows(4)
                    .desired_width(f32::INFINITY)
                    .hint_text("{ \"temperature\": 21.5 }"),
            );

            ui.add_space(4.0);
            let payload_len = self.publish.payload.len();
            let json_valid =
                serde_json::from_str::<serde_json::Value>(&self.publish.payload).is_ok();

            ui.horizontal(|ui| {
                // Reserve the width of a five-digit content length so the
                // following JSON/TEXT badge does not move while typing.
                let content_length = t!("publish.content_length", bytes = payload_len).to_string();
                let reserved_text = t!("publish.content_length", bytes = 99_999).to_string();
                let mut font_id = ui.style().text_styles[&egui::TextStyle::Body].clone();
                font_id.size = 10.5;
                let reserved_width = ui.ctx().fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(reserved_text, font_id, pal.muted)
                        .size()
                        .x
                });
                ui.allocate_ui_with_layout(
                    egui::vec2(reserved_width + 4.0, ui.spacing().interact_size.y),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.label(RichText::new(content_length).size(10.5).color(pal.muted));
                    },
                );
                badge(
                    ui,
                    if json_valid { "JSON" } else { "TEXT" },
                    if json_valid { pal.success } else { pal.muted },
                );

                if ui
                    .add_enabled(json_valid, egui::Button::new(t!("publish.format")))
                    .on_hover_text(t!("publish.format_tooltip"))
                    .clicked()
                    && let Ok(value) =
                        serde_json::from_str::<serde_json::Value>(&self.publish.payload)
                    && let Ok(pretty) = serde_json::to_string_pretty(&value)
                {
                    self.publish.payload = pretty;
                }

                let mut load_index = None;
                egui::ComboBox::from_id_salt("publish-history")
                    .width(150.0)
                    .selected_text(t!("publish.history"))
                    .show_ui(ui, |ui| {
                        if self.config.publish_history.is_empty() {
                            ui.label(
                                RichText::new(t!("publish.no_entries").to_string())
                                    .color(pal.muted),
                            );
                        }
                        for (index, entry) in self.config.publish_history.iter().enumerate() {
                            let label = format!(
                                "{}  ·  {}",
                                truncate_chars(&entry.topic, 38),
                                format_bytes(entry.payload.len() as u64)
                            );
                            let retain = if entry.retain {
                                t!("common.yes").to_string()
                            } else {
                                t!("common.no").to_string()
                            };
                            if ui
                                .button(label)
                                .on_hover_text(t!(
                                    "publish.history_tooltip",
                                    topic = entry.topic,
                                    qos = entry.qos,
                                    retain = retain
                                ))
                                .clicked()
                            {
                                load_index = Some(index);
                                ui.close();
                            }
                        }
                    });
                if let Some(index) = load_index
                    && let Some(entry) = self.config.publish_history.get(index).cloned()
                {
                    self.publish.topic = entry.topic;
                    self.publish.payload = entry.payload;
                    self.publish.qos = entry.qos;
                    self.publish.retain = entry.retain;
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let enabled = connected.is_some() && !self.publish.topic.trim().is_empty();
                    let button = egui::Button::new(
                        RichText::new(t!("publish.button").to_string())
                            .color(Color32::WHITE)
                            .strong(),
                    )
                    .fill(pal.accent)
                    .stroke(Stroke::new(1.0, pal.accent))
                    .min_size(egui::vec2(130.0, 27.0));
                    if ui.add_enabled(enabled, button).clicked() {
                        do_publish = true;
                    }

                    ui.checkbox(&mut self.publish.retain, "Retain")
                        .on_hover_text(t!("publish.retain_tooltip"));

                    let mut qos = self.publish.qos;
                    egui::ComboBox::from_id_salt("publish-qos")
                        .width(78.0)
                        .selected_text(format!("QoS {qos}"))
                        .show_ui(ui, |ui| {
                            for value in 0..=2u8 {
                                ui.selectable_value(&mut qos, value, format!("QoS {value}"));
                            }
                        });
                    self.publish.qos = qos;
                });
            });
        });

        if do_publish {
            self.publish_now();
        }
    }
}
