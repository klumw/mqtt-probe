//! Central topic tree panel.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::Local;
use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Sense, Ui,
    Vec2,
};
use rust_i18n::t;

use crate::app::MqttUiApp;
use crate::model::TopicTree;
use crate::model::topic_tree::{SinceFilter, SinceMode, display_path};
use crate::util::{format_count, format_time, parse_since, truncate_chars};

use super::retained::RetainedDeleteScope;
use super::theme::{Palette, palette};
use super::widgets::flash_intensity;

const ROW_HEIGHT: f32 = 30.0;
const INDENT_STEP: f32 = 14.0;

#[derive(Debug, Clone)]
pub enum TreeAction {
    Select(String),
    Toggle(String),
    ExpandAll,
    CollapseAll,
    Clear,
    DeleteTopic(String),
    DeleteRetained(String),
    ClearRetainedBranch(String),
    CopyTopic(String),
    UseForPublish(String),
}

pub struct TreeViewState<'a> {
    pub filter: &'a str,
    pub since: Option<SinceFilter>,
    pub selected: Option<&'a str>,
    pub show_preview: bool,
    pub search_payload: bool,
    /// Flash intensity per exact topic path.
    pub active_topics: &'a HashMap<String, f32>,
    /// Flash intensity per path prefix, used for the green activity dots.
    pub active_paths: &'a HashMap<String, f32>,
    /// Paths whose expand/collapse triangles stay bright during this session.
    pub lit_paths: &'a HashSet<String>,
}

pub fn tree_ui(
    ui: &mut Ui,
    tree: &TopicTree,
    state: &TreeViewState<'_>,
    pal: &Palette,
) -> Option<TreeAction> {
    let rows = tree.visible_rows(state.filter, state.search_payload, state.since);

    if rows.is_empty() {
        let (title, hint) = if !state.filter.trim().is_empty() || state.since.is_some() {
            (
                t!("tree.no_matches_title").to_string(),
                t!("tree.no_matches_hint").to_string(),
            )
        } else if tree.value_topic_count == 0 {
            (
                t!("tree.no_messages_title").to_string(),
                t!("tree.no_messages_hint").to_string(),
            )
        } else {
            (
                t!("tree.no_visible_title").to_string(),
                t!("tree.no_visible_hint").to_string(),
            )
        };
        empty_state(ui, pal, &title, &hint);
        return None;
    }

    let mut action = None;

    let highlight = if ui.visuals().dark_mode {
        Color32::WHITE
    } else {
        Color32::BLACK
    };

    ui.spacing_mut().item_spacing.y = 2.0;
    ScrollArea::vertical()
        .id_salt("topic-tree-scroll")
        .auto_shrink([false, false])
        .show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
            for row in &rows[range] {
                let flash = row_flash(state.active_topics, row.path);
                let text = pal.text.lerp_to_gamma(highlight, flash * 0.90);
                let muted = pal.muted.lerp_to_gamma(highlight, flash * 0.95);
                let accent = pal.accent.lerp_to_gamma(highlight, flash * 0.35);

                let selected = state.selected == Some(row.path);
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), ROW_HEIGHT),
                    Sense::click(),
                );
                let hovered = response.hovered();

                if selected {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(5), pal.accent_soft);
                } else if hovered {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(5), pal.hover);
                }

                let mut row_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect.shrink2(Vec2::new(5.0, 0.0)))
                        .layout(Layout::left_to_right(Align::Center)),
                );
                row_ui.spacing_mut().item_spacing.x = 4.0;
                row_ui.add_space(row.depth as f32 * INDENT_STEP);

                let activity = state
                    .active_paths
                    .get(row.path)
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                let (dot_rect, dot_response) =
                    row_ui.allocate_exact_size(Vec2::splat(9.0), Sense::hover());
                if (row.depth == 0 || row.has_children) && activity > 0.0 {
                    let alpha = (60.0 + 195.0 * activity).round() as u8;
                    let color = Color32::from_rgba_unmultiplied(
                        pal.success.r(),
                        pal.success.g(),
                        pal.success.b(),
                        alpha,
                    );
                    row_ui
                        .painter()
                        .circle_filled(dot_rect.center(), 3.5, color);
                    dot_response.on_hover_text(t!("tree.branch_update"));
                }

                if row.has_children {
                    let arrow = if row.expanded { "▾" } else { "▸" };
                    let arrow_color = if state.lit_paths.contains(row.path) {
                        accent
                    } else {
                        muted
                    };
                    if row_ui
                        .add(
                            egui::Button::new(RichText::new(arrow).size(14.5).color(arrow_color))
                                .frame(false),
                        )
                        .clicked()
                    {
                        action = Some(TreeAction::Toggle(row.path.to_owned()));
                    }
                } else {
                    row_ui.add_space(13.0);
                }

                let segment_color = if row.has_data {
                    if row.retained { accent } else { text }
                } else {
                    muted
                };
                let segment_text = if row.segment.is_empty() {
                    "/"
                } else {
                    row.segment
                };
                if row_ui
                    .add(
                        egui::Button::new(
                            RichText::new(segment_text).size(16.25).color(segment_color),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    action = Some(TreeAction::Select(row.path.to_owned()));
                }

                if row.retained {
                    // Paint the dot with a generous hover rect: the text glyph
                    // was too small to hit reliably with the pointer.
                    let (dot_rect, dot_response) =
                        row_ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
                    row_ui
                        .painter()
                        .circle_filled(dot_rect.center(), 3.5, accent);
                    dot_response.on_hover_ui(|ui| {
                        ui.label(
                            RichText::new(t!("tree.retained_label"))
                                .strong()
                                .color(accent),
                        );
                        ui.label(t!("tree.retained_hint"));
                    });
                }

                row_ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if row.total > 0 {
                        ui.label(
                            RichText::new(format_count(row.total))
                                .size(13.5)
                                .color(muted),
                        );
                    }
                    if state.show_preview && !row.preview.is_empty() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(truncate_chars(row.preview, 70))
                                    .size(13.5)
                                    .color(muted),
                            )
                            .truncate(),
                        );
                    }
                });

                let display = display_path(row.path);
                let preview = if row.preview.is_empty() {
                    "—"
                } else {
                    row.preview
                };
                let mut tooltip = t!(
                    "tree.topic_tooltip",
                    path = display,
                    messages = format_count(row.total),
                    stored = format_count(row.stored as u64),
                    preview = preview
                )
                .to_string();
                if row.retained {
                    // Hovering the row also explains the retained marker, so
                    // the explanation is discoverable without hitting the dot.
                    let retained_label = t!("tree.retained_label");
                    let retained_hint = t!("tree.retained_hint");
                    tooltip.push_str(&format!("\n\n● {retained_label} — {retained_hint}"));
                }
                let response = response.on_hover_text(tooltip);
                // A container menu also opens when the secondary click lands
                // on a child widget such as the topic-name button.
                response.container_context_menu(|ui| {
                    if ui.button(t!("tree.copy_topic")).clicked() {
                        action = Some(TreeAction::CopyTopic(row.path.to_owned()));
                        ui.close();
                    }
                    if ui.button(t!("tree.use_as_publish")).clicked() {
                        action = Some(TreeAction::UseForPublish(row.path.to_owned()));
                        ui.close();
                    }

                    let branch_retained = if row.has_children {
                        tree.retained_topics_under(row.path).len()
                    } else {
                        0
                    };
                    if row.retained || branch_retained > 0 {
                        ui.separator();
                        if row.retained && ui.button(t!("tree.delete_retained")).clicked() {
                            action = Some(TreeAction::DeleteRetained(row.path.to_owned()));
                            ui.close();
                        }
                        if branch_retained > 0 {
                            let label = t!("tree.clear_retained_branch", count = branch_retained);
                            if ui.button(label).clicked() {
                                action = Some(TreeAction::ClearRetainedBranch(row.path.to_owned()));
                                ui.close();
                            }
                        }
                    }

                    ui.separator();
                    if ui.button(t!("tree.delete_local")).clicked() {
                        action = Some(TreeAction::DeleteTopic(row.path.to_owned()));
                        ui.close();
                    }
                });
            }
        });

    action
}

fn empty_state(ui: &mut Ui, pal: &Palette, title: &str, hint: &str) {
    ui.add_space(70.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(title).size(19.5).strong().color(pal.muted));
        ui.add_space(4.0);
        ui.label(RichText::new(hint).size(15.0).color(pal.muted));
    });
}

fn row_flash(active_topics: &HashMap<String, f32>, path: &str) -> f32 {
    active_topics
        .get(path)
        .copied()
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
}

fn tree_header_buttons(ui: &mut Ui, has_runtime: bool, action: &mut Option<TreeAction>) {
    if ui
        .add_enabled(
            has_runtime,
            egui::Button::new(t!("tree.expand_all")).small(),
        )
        .clicked()
    {
        *action = Some(TreeAction::ExpandAll);
    }
    if ui
        .add_enabled(
            has_runtime,
            egui::Button::new(t!("tree.collapse_all")).small(),
        )
        .clicked()
    {
        *action = Some(TreeAction::CollapseAll);
    }
    if ui
        .add_enabled(has_runtime, egui::Button::new(t!("tree.clear")).small())
        .clicked()
    {
        *action = Some(TreeAction::Clear);
    }
}

impl MqttUiApp {
    pub(crate) fn show_tree_panel(&mut self, ui: &mut Ui) {
        let dark = ui.visuals().dark_mode;
        let pal = palette(dark);
        let ctx = ui.ctx().clone();
        let conn_id = self.selected_connection;
        let mut action: Option<TreeAction> = None;

        let (active_topics, active_paths): (HashMap<String, f32>, HashMap<String, f32>) =
            if self.config.settings.flash_tree_on_update {
                let duration = Duration::from_secs_f32(
                    self.config.settings.flash_duration_secs.clamp(0.5, 10.0),
                );
                let mut topics = HashMap::new();
                let mut paths: HashMap<String, f32> = HashMap::new();

                if let Some(runtime) = conn_id.and_then(|id| self.runtimes.get(&id)) {
                    for (topic, updated) in &runtime.topic_updates {
                        let intensity = flash_intensity(updated.elapsed(), duration);
                        if intensity <= 0.001 {
                            continue;
                        }
                        topics.insert(topic.clone(), intensity);

                        crate::model::topic_tree::for_each_path_prefix(topic, |path| {
                            paths
                                .entry(path.to_owned())
                                .and_modify(|current| *current = current.max(intensity))
                                .or_insert(intensity);
                        });
                    }
                }

                (topics, paths)
            } else {
                (HashMap::new(), HashMap::new())
            };

        egui::CentralPanel::default_margins()
            .frame(Frame::new().fill(pal.bg).inner_margin(Margin::same(12)))
            .show(ui, |ui| {
                // Scale the panel's default text styles so the filter box and
                // header buttons grow along with the topic rows.
                let mut style = (**ui.style()).clone();
                for font_id in style.text_styles.values_mut() {
                    font_id.size *= 1.3;
                }
                ui.set_style(style);

                let has_runtime = conn_id.is_some_and(|id| self.runtimes.contains_key(&id));

                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(t!("tree.title").to_string())
                            .size(21.0)
                            .strong()
                            .color(pal.text),
                    );

                    if let Some(runtime) = conn_id.and_then(|id| self.runtimes.get(&id)) {
                        ui.label(
                            RichText::new(t!(
                                "tree.counts",
                                topics = format_count(runtime.tree.value_topic_count as u64),
                                messages = format_count(runtime.tree.received_messages)
                            ))
                            .size(13.5)
                            .color(pal.muted),
                        );
                    }
                });

                ui.add_space(6.0);

                let mut since = None;
                ui.horizontal_wrapped(|ui| {
                    let since_extra = if self.tree_since_mode == SinceMode::AnyTime {
                        0.0
                    } else {
                        180.0
                    };
                    let search_width = (ui.available_width() - 560.0 - since_extra).max(120.0);
                    let search_id = egui::Id::unique("tree-search");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.tree_filter)
                            .id(search_id)
                            .hint_text(t!("tree.filter_hint"))
                            .desired_width(search_width),
                    );
                    if self.focus_search {
                        response.request_focus();
                        self.focus_search = false;
                    }
                    if !self.tree_filter.is_empty() && ui.small_button("×").clicked() {
                        self.tree_filter.clear();
                    }

                    let since_label = match self.tree_since_mode {
                        SinceMode::AnyTime => t!("tree.since_any"),
                        SinceMode::ReceivedSince => t!("tree.since_received"),
                        SinceMode::NoMessagesSince => t!("tree.since_no_messages"),
                    };
                    egui::ComboBox::from_id_salt("tree-since-mode")
                        .width(160.0)
                        .selected_text(since_label)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.tree_since_mode,
                                SinceMode::AnyTime,
                                t!("tree.since_any"),
                            );
                            ui.selectable_value(
                                &mut self.tree_since_mode,
                                SinceMode::ReceivedSince,
                                t!("tree.since_received"),
                            );
                            ui.selectable_value(
                                &mut self.tree_since_mode,
                                SinceMode::NoMessagesSince,
                                t!("tree.since_no_messages"),
                            );
                        });

                    if self.tree_since_mode != SinceMode::AnyTime {
                        let parsed = parse_since(&self.tree_since_input, Local::now());
                        let mut input = egui::TextEdit::singleline(&mut self.tree_since_input)
                            .hint_text(t!("tree.since_hint"))
                            .desired_width(150.0);
                        if parsed.is_none() {
                            input = input.text_color(pal.danger);
                        }
                        ui.add(input).on_hover_text(match parsed {
                            Some(cutoff) => t!(
                                "tree.since_active",
                                time =
                                    format_time(&cutoff, self.config.settings.timestamps_utc, true)
                            )
                            .to_string(),
                            None => t!("tree.since_invalid_hint").to_string(),
                        });
                        since = parsed.map(|cutoff| SinceFilter {
                            mode: self.tree_since_mode,
                            cutoff,
                        });
                    }

                    tree_header_buttons(ui, has_runtime, &mut action);
                });

                if since.is_some() {
                    // Relative expressions such as "15m" move with the clock.
                    ui.ctx().request_repaint_after(Duration::from_secs(1));
                }

                ui.add_space(4.0);

                if let Some(runtime) = conn_id.and_then(|id| self.runtimes.get(&id)) {
                    let state = TreeViewState {
                        filter: &self.tree_filter,
                        since,
                        selected: self.selected_topic.as_deref(),
                        show_preview: self.config.settings.show_payload_preview,
                        search_payload: self.config.settings.search_in_payload,
                        active_topics: &active_topics,
                        active_paths: &active_paths,
                        lit_paths: &runtime.lit_paths,
                    };
                    if let Some(next) = tree_ui(ui, &runtime.tree, &state, &pal) {
                        action = Some(next);
                    }
                } else {
                    let (title, hint) = if conn_id.is_some() {
                        (
                            t!("tree.not_connected_title").to_string(),
                            t!("tree.not_connected_hint").to_string(),
                        )
                    } else {
                        (
                            t!("tree.no_connection_title").to_string(),
                            t!("tree.no_connection_hint").to_string(),
                        )
                    };
                    ui.add_space(50.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(title).size(19.5).strong().color(pal.muted));
                        ui.add_space(4.0);
                        ui.label(RichText::new(hint).size(15.0).color(pal.muted));
                    });
                }
            });

        let Some(id) = self.selected_connection else {
            return;
        };
        let Some(action) = action else {
            return;
        };

        match action {
            TreeAction::Select(path) => {
                if self.selected_topic.as_deref() != Some(path.as_str()) {
                    self.selected_topic = Some(path);
                    self.history_selection = None;
                    self.chart_field = None;
                }
            }
            TreeAction::Toggle(path) => {
                if let Some(runtime) = self.runtimes.get_mut(&id) {
                    runtime.tree.toggle_expanded(&path);
                }
            }
            TreeAction::ExpandAll => {
                if let Some(runtime) = self.runtimes.get_mut(&id) {
                    runtime.tree.set_all_expanded(true);
                }
            }
            TreeAction::CollapseAll => {
                if let Some(runtime) = self.runtimes.get_mut(&id) {
                    runtime.tree.set_all_expanded(false);
                }
            }
            TreeAction::Clear => {
                if let Some(runtime) = self.runtimes.get_mut(&id) {
                    runtime.tree.clear();
                    runtime.forget_all();
                }
                self.selected_topic = None;
                self.history_selection = None;
                self.chart_field = None;
                self.toasts.info(t!("tree.cleared_toast"));
            }
            TreeAction::DeleteTopic(path) => {
                if let Some(runtime) = self.runtimes.get_mut(&id) {
                    runtime.tree.remove_topic(&path);
                    runtime.forget_subtree(&path);
                }
                if self.selected_topic.as_deref() == Some(path.as_str()) {
                    self.selected_topic = None;
                    self.history_selection = None;
                    self.chart_field = None;
                }
                self.toasts
                    .info(t!("tree.removed_toast", path = display_path(&path)));
            }
            TreeAction::DeleteRetained(path) => {
                self.request_retained_delete(id, path, RetainedDeleteScope::Topic);
            }
            TreeAction::ClearRetainedBranch(path) => {
                self.request_retained_delete(id, path, RetainedDeleteScope::Branch);
            }
            TreeAction::CopyTopic(path) => {
                ctx.copy_text(display_path(&path).to_owned());
                self.toasts.info(t!("tree.copied_toast"));
            }
            TreeAction::UseForPublish(path) => {
                self.publish.topic = display_path(&path).to_owned();
                self.publish.open = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_updated_topic_is_highlighted() {
        let mut topics = HashMap::new();
        topics.insert("esp32/airquality/sensor".to_owned(), 1.0);

        assert_eq!(row_flash(&topics, "esp32/airquality/sensor"), 1.0);
        assert_eq!(row_flash(&topics, "esp32/airquality"), 0.0);
        assert_eq!(row_flash(&topics, "esp32/airquality/other"), 0.0);
        assert_eq!(row_flash(&topics, "home/kitchen/temperature"), 0.0);
        assert_eq!(row_flash(&topics, "esp32x/airquality/sensor"), 0.0);
    }
}
