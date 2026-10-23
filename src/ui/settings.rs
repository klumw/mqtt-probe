//! Settings and about windows.

use eframe::egui::{self, CornerRadius, Frame, Margin, RichText, Stroke};
use rust_i18n::t;

use crate::app::MqttUiApp;
use crate::config::Language;

use super::theme::{Palette, palette};
use super::widgets::section_heading;

impl MqttUiApp {
    pub(crate) fn show_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }

        let pal = palette(self.config.settings.dark_mode);
        let mut open = true;
        let mut close = false;
        let mut save_now = false;
        let mut dirty = false;
        let center_next = !self.settings_was_open;
        self.settings_was_open = true;
        let window_id = egui::Id::unique("mqtt-probe-settings");
        let center = ctx.content_rect().center();
        if center_next {
            // Reset the remembered position so the dialog always opens
            // centered, while remaining freely movable afterwards.
            ctx.memory_mut(|mem| {
                mem.areas_mut().set_state(
                    egui::LayerId::new(egui::Order::Middle, window_id),
                    egui::AreaState {
                        pivot: egui::Align2::CENTER_CENTER,
                        pivot_pos: Some(center),
                        ..Default::default()
                    },
                );
            });
        }

        let window = egui::Window::new(t!("settings.title"))
            .id(window_id)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .movable(true)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(center)
            .default_width(820.0)
            .min_width(820.0)
            .max_width(820.0);
        window.show(ctx, |ui| {
            ui.spacing_mut().slider_width = 280.0;
            ui.spacing_mut().item_spacing.y = 4.0;
            section_heading(ui, &t!("settings.appearance"), &pal);
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(t!("settings.color_scheme"));
                if ui
                    .selectable_value(&mut self.config.settings.dark_mode, true, t!("common.dark"))
                    .changed()
                {
                    dirty = true;
                }
                if ui
                    .selectable_value(
                        &mut self.config.settings.dark_mode,
                        false,
                        t!("common.light"),
                    )
                    .changed()
                {
                    dirty = true;
                }
            });

            ui.add_space(8.0);
            section_heading(ui, &t!("settings.language"), &pal);
            ui.add_space(2.0);
            let mut language = self.config.settings.language;
            egui::ComboBox::from_id_salt("settings-language")
                .width(220.0)
                .selected_text(language.native_name())
                .show_ui(ui, |ui| {
                    for option in Language::ALL {
                        ui.selectable_value(&mut language, option, option.native_name());
                    }
                });
            if language != self.config.settings.language {
                self.config.settings.language = language;
                rust_i18n::set_locale(language.code());
                dirty = true;
            }
            ui.label(
                RichText::new(t!("settings.language_hint").to_string())
                    .size(10.0)
                    .color(pal.muted),
            );

            ui.add_space(8.0);
            section_heading(ui, &t!("settings.topic_tree"), &pal);
            ui.add_space(2.0);
            if ui
                .checkbox(
                    &mut self.config.settings.restore_previous_topics,
                    t!("settings.restore_previous"),
                )
                .changed()
            {
                let enabled = self.config.settings.restore_previous_topics;
                for runtime in self.runtimes.values_mut() {
                    runtime.restore_filter = enabled;
                }
                dirty = true;
            }
            ui.label(
                RichText::new(t!("settings.restore_previous_hint").to_string())
                    .size(10.0)
                    .color(pal.muted),
            );
            if settings_slider(
                ui,
                &pal,
                true,
                egui::Slider::new(&mut self.config.settings.max_history, 20..=2_000)
                    .text(t!("settings.messages_per_topic").to_string()),
            )
            .changed()
            {
                dirty = true;
            }
            if settings_slider(
                ui,
                &pal,
                true,
                egui::Slider::new(
                    &mut self.config.settings.max_display_bytes,
                    4_096..=1_048_576,
                )
                .logarithmic(true)
                .text(t!("settings.max_display_bytes").to_string()),
            )
            .changed()
            {
                dirty = true;
            }
            if ui
                .checkbox(
                    &mut self.config.settings.show_payload_preview,
                    t!("settings.payload_preview"),
                )
                .changed()
            {
                dirty = true;
            }
            if ui
                .checkbox(
                    &mut self.config.settings.search_in_payload,
                    t!("settings.search_payload"),
                )
                .changed()
            {
                dirty = true;
            }
            if ui
                .checkbox(
                    &mut self.config.settings.flash_tree_on_update,
                    t!("settings.highlight_updates"),
                )
                .changed()
            {
                dirty = true;
            }
            if settings_slider(
                ui,
                &pal,
                self.config.settings.flash_tree_on_update,
                egui::Slider::new(&mut self.config.settings.flash_duration_secs, 0.5..=10.0)
                    .step_by(0.5)
                    .text(t!("settings.highlight_duration").to_string()),
            )
            .changed()
            {
                dirty = true;
            }
            ui.label(
                RichText::new(t!("settings.highlight_hint").to_string())
                    .size(10.0)
                    .color(pal.muted),
            );
            if ui
                .checkbox(
                    &mut self.config.settings.json_pretty,
                    t!("settings.pretty_json"),
                )
                .changed()
            {
                dirty = true;
            }
            if ui
                .checkbox(
                    &mut self.config.settings.timestamps_utc,
                    t!("settings.utc_timestamps"),
                )
                .changed()
            {
                dirty = true;
            }

            ui.add_space(8.0);
            section_heading(ui, &t!("settings.configuration"), &pal);
            ui.add_space(2.0);
            Frame::new()
                .fill(pal.card)
                .stroke(Stroke::new(1.0, pal.border))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(self.store.path().display().to_string())
                                .monospace()
                                .size(10.5)
                                .color(pal.muted),
                        );
                        if ui.small_button(t!("common.copy")).clicked() {
                            ctx.copy_text(self.store.path().display().to_string());
                        }
                    });
                });
            ui.add_space(4.0);
            ui.label(
                RichText::new(t!("settings.password_note").to_string())
                    .size(10.5)
                    .color(pal.muted),
            );

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(t!("settings.save_now")).clicked() {
                    save_now = true;
                }
                if ui.button(t!("common.close")).clicked() {
                    close = true;
                }
            });
        });

        if dirty {
            self.config_dirty = true;
        }
        if save_now {
            match self.store.save(&self.config) {
                Ok(()) => {
                    self.config_dirty = false;
                    self.toasts.success(t!("settings.saved_toast"));
                }
                Err(err) => {
                    self.toasts.error(t!("settings.save_failed", error = err));
                }
            }
        }
        self.show_settings = open && !close;
        if !self.show_settings {
            self.settings_was_open = false;
        }
    }
}

/// Slider rails use egui's inactive background fill, which defaults to the
/// window background in this theme. Give the rail its own visible color while
/// leaving the DragValue button background unchanged.
fn settings_slider(
    ui: &mut egui::Ui,
    pal: &Palette,
    enabled: bool,
    slider: egui::Slider<'_>,
) -> egui::Response {
    ui.scope(|ui| {
        ui.visuals_mut().widgets.inactive.bg_fill = pal.slider_rail;
        ui.add_enabled(enabled, slider)
    })
    .inner
}
