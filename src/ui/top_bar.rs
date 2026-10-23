//! Top application bar: branding, active connection and global actions.

use eframe::egui::{self, Align, Frame, Layout, Margin, RichText, Stroke, Ui};
use rust_i18n::t;
use uuid::Uuid;

use crate::app::MqttUiApp;
use crate::mqtt::ConnectionState;

use super::help::HelpScrollTarget;
use super::theme::{Palette, palette};

struct ConnectionRow {
    id: Uuid,
    name: String,
    state_label: String,
}

enum TopAction {
    Select(Uuid),
    Settings,
    Help,
    Log,
}

impl MqttUiApp {
    pub(crate) fn show_top_bar(&mut self, ui: &mut Ui) {
        let pal = palette(ui.visuals().dark_mode);
        let mut action: Option<TopAction> = None;

        let profiles: Vec<ConnectionRow> = self
            .config
            .profiles
            .iter()
            .map(|profile| {
                let state = self
                    .runtimes
                    .get(&profile.id)
                    .map(|runtime| runtime.state.clone())
                    .unwrap_or(ConnectionState::Disconnected);
                ConnectionRow {
                    id: profile.id,
                    name: profile.display_name().to_owned(),
                    state_label: state.label().to_owned(),
                }
            })
            .collect();

        let selected = self.selected_connection;
        let scroll_to_selected = self.scroll_top_combo_to_selection;
        let mut combo_open_after = None;
        let selected_row = profiles.iter().find(|row| Some(row.id) == selected);
        let selected_name = selected_row
            .map(|row| row.name.clone())
            .unwrap_or_else(|| t!("common.no_connection").to_string());

        egui::Panel::top("top-bar")
            .exact_size(56.0)
            .frame(
                Frame::new()
                    .fill(pal.panel)
                    .stroke(Stroke::new(1.0, pal.border))
                    .inner_margin(Margin::symmetric(16, 8)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("MQTT").size(20.0).strong().color(pal.accent));
                    ui.label(RichText::new("Probe").size(20.0).strong().color(pal.text));
                    ui.add_space(20.0);

                    let mut choice = selected;
                    let combo_id = ui.make_persistent_id("active-connection");
                    egui::ComboBox::from_id_salt("active-connection")
                        .width(200.0)
                        .selected_text(selected_name)
                        .show_ui(ui, |ui| {
                            for row in &profiles {
                                let response = ui.selectable_value(
                                    &mut choice,
                                    Some(row.id),
                                    format!("{} · {}", row.name, row.state_label),
                                );
                                if scroll_to_selected && Some(row.id) == selected {
                                    response.scroll_to_me(None);
                                }
                            }
                            if profiles.is_empty() {
                                ui.label(
                                    RichText::new(t!("top.no_profiles").to_string())
                                        .color(pal.muted),
                                );
                            }
                        });
                    combo_open_after = Some(egui::ComboBox::is_open(ui.ctx(), combo_id));
                    if choice != selected
                        && let Some(id) = choice
                    {
                        action = Some(TopAction::Select(id));
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Right-to-left layout: add the rightmost button first
                        // so the visual order is Settings, Log, Help.
                        if ui.button(t!("common.help")).clicked() {
                            action = Some(TopAction::Help);
                        }
                        if ui.button(t!("log.button")).clicked() {
                            action = Some(TopAction::Log);
                        }
                        if ui.button(t!("common.settings")).clicked() {
                            action = Some(TopAction::Settings);
                        }
                    });
                });
            });

        if let Some(open) = combo_open_after {
            self.scroll_top_combo_to_selection = open && !self.top_combo_was_open;
            self.top_combo_was_open = open;
        }

        match action {
            Some(TopAction::Select(id)) => self.select_connection(id),
            Some(TopAction::Settings) => self.show_settings = true,
            Some(TopAction::Help) => {
                self.show_help = true;
                // The help window keeps its scroll position across closings;
                // every fresh open starts at the top of the table of contents.
                self.help_scroll = Some(HelpScrollTarget::Top);
            }
            Some(TopAction::Log) => self.show_log = true,
            None => {}
        }
    }
}

#[allow(dead_code)]
fn _palette_is_send(_pal: Palette) {}
