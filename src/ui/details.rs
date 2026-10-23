//! Right-hand detail panel: value, history, diff and chart views.

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Frame, Label, Layout, Margin, RichText, ScrollArea,
    Stroke, Ui,
};
use rust_i18n::t;
use similar::ChangeTag;

use crate::app::MqttUiApp;
use crate::config::AppSettings;
use crate::model::{MessageRecord, TopicData};
use crate::util::{format_bytes, format_count, format_time, hex_dump, human_qos, truncate_chars};

use super::theme::{Palette, palette};
use super::widgets::{badge, section_heading};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Value,
    History,
    Diff,
    Chart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueMode {
    Text,
    Json,
    Hex,
}

#[derive(Debug, Clone)]
pub enum DetailAction {
    DeleteTopic,
    ClearHistory,
    UseTopicForPublish,
    UsePayloadForPublish(String),
}

// The detail view needs access to several independent pieces of UI state;
// grouping them into a struct would only move the argument list elsewhere.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    path: &str,
    data: Option<&TopicData>,
    settings: &AppSettings,
    tab: &mut DetailTab,
    value_mode: &mut ValueMode,
    history_selection: &mut Option<MessageRecord>,
    chart_field: &mut Option<String>,
) -> Vec<DetailAction> {
    let pal = palette(ui.visuals().dark_mode);
    let mut actions = Vec::new();

    ui.horizontal(|ui| {
        section_heading(ui, &t!("details.title"), &pal);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .small_button(t!("details.delete_topic"))
                .on_hover_text(t!("details.delete_topic_tooltip"))
                .clicked()
            {
                actions.push(DetailAction::DeleteTopic);
            }
            if data.is_some_and(|data| !data.history.is_empty())
                && ui
                    .small_button(t!("details.clear_history"))
                    .on_hover_text(t!("details.clear_history_tooltip"))
                    .clicked()
            {
                actions.push(DetailAction::ClearHistory);
            }
            if ui
                .small_button(t!("publish.button"))
                .on_hover_text(t!("details.publish_tooltip"))
                .clicked()
            {
                actions.push(DetailAction::UseTopicForPublish);
            }
        });
    });

    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(path)
                .monospace()
                .size(13.5)
                .strong()
                .color(pal.text),
        );
        if ui.small_button(t!("common.copy")).clicked() {
            ui.ctx().copy_text(path.to_owned());
        }
    });

    let latest = data.and_then(|data| data.latest());
    if let Some(latest) = latest {
        ui.horizontal_wrapped(|ui| {
            if latest.retain {
                badge(ui, &t!("common.retained"), pal.accent);
            }
            badge(ui, &format!("QoS {}", latest.qos), pal.muted);
            badge(ui, &format_bytes(latest.size() as u64), pal.muted);
            if latest.text().is_none() {
                badge(ui, &t!("common.binary"), pal.warn);
            }
            if let Some(data) = data {
                ui.label(
                    RichText::new(t!(
                        "details.stats",
                        stored = format_count(data.history.len() as u64),
                        received = format_count(data.total)
                    ))
                    .size(10.5)
                    .color(pal.muted),
                );
                ui.label(
                    RichText::new(t!(
                        "details.since",
                        time = format_time(&data.first_seen, settings.timestamps_utc, true)
                    ))
                    .size(10.5)
                    .color(pal.muted),
                );
            }
        });
        ui.label(
            RichText::new(format_time(
                &latest.received_at,
                settings.timestamps_utc,
                true,
            ))
            .size(10.5)
            .color(pal.muted),
        );
        ui.label(
            RichText::new(human_qos(latest.qos))
                .size(10.0)
                .color(pal.muted),
        );
    }

    let Some(data) = data else {
        ui.add_space(12.0);
        empty_hint(
            ui,
            &pal,
            &t!("details.intermediate_title"),
            &t!("details.intermediate_hint"),
        );
        return actions;
    };

    ui.add_space(8.0);
    let mut auto_fit_chart = false;
    ui.horizontal(|ui| {
        for (value, label) in [
            (DetailTab::Value, t!("details.tab.value").to_string()),
            (DetailTab::History, t!("details.tab.history").to_string()),
            (DetailTab::Diff, t!("details.tab.diff").to_string()),
            (DetailTab::Chart, t!("details.tab.chart").to_string()),
        ] {
            let response = ui.selectable_value(tab, value, label);
            if value == DetailTab::Chart && response.clicked() {
                auto_fit_chart = true;
            }
        }
    });
    ui.separator();

    match *tab {
        DetailTab::Value => {
            value_tab(ui, data, settings, value_mode, &pal, &mut actions);
        }
        DetailTab::History => {
            history_tab(ui, data, settings, history_selection, &pal, &mut actions);
        }
        DetailTab::Diff => {
            diff_tab(ui, data, settings, &pal);
        }
        DetailTab::Chart => {
            chart_tab(ui, data, settings, chart_field, &pal, auto_fit_chart);
        }
    }

    actions
}

pub fn empty_state(ui: &mut Ui) {
    let pal = palette(ui.visuals().dark_mode);
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(t!("details.no_topic_title").to_string())
                .size(15.0)
                .strong()
                .color(pal.muted),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(t!("details.no_topic_hint").to_string())
                .size(11.5)
                .color(pal.muted),
        );
    });
}

fn value_tab(
    ui: &mut Ui,
    data: &TopicData,
    settings: &AppSettings,
    value_mode: &mut ValueMode,
    pal: &Palette,
    actions: &mut Vec<DetailAction>,
) {
    let Some(latest) = data.latest() else {
        empty_hint(
            ui,
            pal,
            &t!("details.no_value_title"),
            &t!("details.no_value_hint"),
        );
        return;
    };

    let is_json = latest.is_json();
    if *value_mode == ValueMode::Json && !is_json {
        *value_mode = ValueMode::Text;
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.selectable_value(value_mode, ValueMode::Text, t!("common.text"));
        ui.add_enabled_ui(is_json, |ui| {
            ui.selectable_value(value_mode, ValueMode::Json, "JSON");
        });
        ui.selectable_value(value_mode, ValueMode::Hex, "Hex");

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.small_button(t!("details.copy_value")).clicked() {
                ui.ctx().copy_text(latest.text_lossy());
            }
            if ui.small_button(t!("details.use_for_publish")).clicked() {
                actions.push(DetailAction::UsePayloadForPublish(latest.text_lossy()));
            }
        });
    });
    ui.add_space(6.0);

    let max_bytes = settings.max_display_bytes;
    match *value_mode {
        ValueMode::Text => {
            let text = truncate_bytes(&latest.text_lossy(), max_bytes);
            code_view(ui, &text, pal, false, "value-text");
        }
        ValueMode::Json => {
            let raw = latest.pretty_json().unwrap_or_else(|| latest.text_lossy());
            let pretty = if settings.json_pretty {
                raw
            } else {
                latest.text_lossy()
            };
            let job = json_highlight(&pretty, pal);
            Frame::new()
                .fill(pal.code_bg)
                .stroke(Stroke::new(1.0, pal.border))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    let height = (ui.available_height() - 10.0).max(160.0);
                    ScrollArea::both()
                        .id_salt("value-json")
                        .auto_shrink([false, false])
                        .max_height(height)
                        .show(ui, |ui| {
                            ui.add(Label::new(job).selectable(true));
                        });
                });
        }
        ValueMode::Hex => {
            let dump = hex_dump(&latest.payload, max_bytes);
            code_view(ui, &dump, pal, true, "value-hex");
        }
    }
}

fn history_tab(
    ui: &mut Ui,
    data: &TopicData,
    settings: &AppSettings,
    history_selection: &mut Option<MessageRecord>,
    pal: &Palette,
    actions: &mut Vec<DetailAction>,
) {
    if data.history.is_empty() {
        empty_hint(
            ui,
            pal,
            &t!("details.no_history_title"),
            &t!("details.no_history_hint"),
        );
        return;
    }

    let list_height = (ui.available_height() * 0.55).max(150.0);
    ui.add_space(6.0);
    ScrollArea::vertical()
        .id_salt("history-list")
        .auto_shrink([false, false])
        .max_height(list_height)
        .show(ui, |ui| {
            for message in data.history.iter().rev() {
                let selected = history_selection
                    .as_ref()
                    .is_some_and(|selected| selected.seq == message.seq);
                let text = format!(
                    "{}   {:>10}   {}",
                    message.timestamp_string(settings.timestamps_utc),
                    format_bytes(message.size() as u64),
                    truncate_chars(&message.preview(70), 70)
                );
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(text)
                                .monospace()
                                .size(11.0)
                                .color(if selected { pal.accent } else { pal.text }),
                        )
                        .frame(selected)
                        .fill(if selected {
                            pal.accent_soft
                        } else {
                            Color32::TRANSPARENT
                        })
                        .min_size(egui::vec2(ui.available_width(), 20.0)),
                    )
                    .clicked()
                {
                    *history_selection = Some(message.clone());
                }
            }
        });

    ui.separator();

    if let Some(message) = history_selection.as_ref() {
        let still_in_history = data
            .history
            .iter()
            .any(|candidate| candidate.seq == message.seq);
        ui.horizontal(|ui| {
            let suffix = if still_in_history {
                String::new()
            } else {
                t!("details.not_in_history").to_string()
            };
            ui.label(
                RichText::new(t!(
                    "details.message_title",
                    time = message.full_timestamp_string(settings.timestamps_utc),
                    suffix = suffix
                ))
                .size(10.5)
                .color(pal.muted),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button(t!("common.copy")).clicked() {
                    ui.ctx().copy_text(message.text_lossy());
                }
                if ui.small_button(t!("details.use_for_publish")).clicked() {
                    actions.push(DetailAction::UsePayloadForPublish(message.text_lossy()));
                }
            });
        });
        ui.add_space(4.0);
        let text = truncate_bytes(&message.text_lossy(), settings.max_display_bytes);
        code_view(ui, &text, pal, false, "history-preview");
        return;
    }

    ui.add_space(8.0);
    ui.label(
        RichText::new(t!("details.select_history_message").to_string())
            .size(10.5)
            .color(pal.muted),
    );
}

fn diff_tab(ui: &mut Ui, data: &TopicData, settings: &AppSettings, pal: &Palette) {
    let (Some(previous), Some(latest)) = (data.previous(), data.latest()) else {
        empty_hint(
            ui,
            pal,
            &t!("details.no_diff_title"),
            &t!("details.no_diff_hint"),
        );
        return;
    };

    let old = truncate_bytes(&previous.text_lossy(), settings.max_display_bytes);
    let new = truncate_bytes(&latest.text_lossy(), settings.max_display_bytes);
    let diff = similar::TextDiff::from_lines(&old, &new);

    let mut additions = 0usize;
    let mut deletions = 0usize;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => additions += 1,
            ChangeTag::Delete => deletions += 1,
            ChangeTag::Equal => {}
        }
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "{}  →  {}",
                previous.timestamp_string(settings.timestamps_utc),
                latest.timestamp_string(settings.timestamps_utc)
            ))
            .monospace()
            .size(10.5)
            .color(pal.muted),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("+{additions} / -{deletions}"))
                    .monospace()
                    .size(10.5)
                    .color(if additions + deletions == 0 {
                        pal.muted
                    } else {
                        pal.accent
                    }),
            );
        });
    });
    ui.add_space(4.0);

    if old == new {
        empty_hint(
            ui,
            pal,
            &t!("details.no_differences_title"),
            &t!("details.no_differences_hint"),
        );
        return;
    }

    Frame::new()
        .fill(pal.code_bg)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            let height = (ui.available_height() - 8.0).max(180.0);
            ScrollArea::both()
                .id_salt("diff-view")
                .auto_shrink([false, false])
                .max_height(height)
                .show(ui, |ui| {
                    for (shown, change) in diff.iter_all_changes().enumerate() {
                        if shown >= 4_000 {
                            ui.label(
                                RichText::new(t!("details.diff_truncated").to_string())
                                    .size(10.5)
                                    .color(pal.muted),
                            );
                            break;
                        }

                        let (marker, color) = match change.tag() {
                            ChangeTag::Delete => ("-", pal.danger),
                            ChangeTag::Insert => ("+", pal.success),
                            ChangeTag::Equal => (" ", pal.muted),
                        };
                        let old_line = change
                            .old_index()
                            .map(|index| format!("{:>4}", index + 1))
                            .unwrap_or_else(|| "    ".to_owned());
                        let new_line = change
                            .new_index()
                            .map(|index| format!("{:>4}", index + 1))
                            .unwrap_or_else(|| "    ".to_owned());

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(
                                RichText::new(format!("{old_line} {new_line}"))
                                    .monospace()
                                    .size(10.0)
                                    .color(pal.muted),
                            );
                            ui.label(
                                RichText::new(marker)
                                    .monospace()
                                    .size(11.5)
                                    .strong()
                                    .color(color),
                            );
                            ui.label(
                                RichText::new(change.to_string().trim_end())
                                    .monospace()
                                    .size(11.5)
                                    .color(color),
                            );
                        });
                    }
                });
        });
}

/// The sentinel field name for a plain numeric payload is shown with a
/// localized label.
fn field_display_name(field: &str) -> String {
    if field == "value" {
        t!("details.plain_value").to_string()
    } else {
        crate::model::message::field_label(field)
    }
}

fn chart_tab(
    ui: &mut Ui,
    data: &TopicData,
    settings: &AppSettings,
    chart_field: &mut Option<String>,
    pal: &Palette,
    auto_fit: bool,
) {
    let mut auto_fit = auto_fit;
    let mut fields: Vec<String> = Vec::new();
    for message in &data.history {
        for (field, _) in message.numeric_fields() {
            if !fields.iter().any(|existing| existing == &field) {
                fields.push(field);
            }
            if fields.len() >= 32 {
                break;
            }
        }
        if fields.len() >= 32 {
            break;
        }
    }

    if fields.is_empty() {
        empty_hint(
            ui,
            pal,
            &t!("details.no_numeric_title"),
            &t!("details.no_numeric_hint"),
        );
        return;
    }

    fields.sort();
    if chart_field
        .as_ref()
        .is_none_or(|selected| !fields.iter().any(|field| field == selected))
    {
        *chart_field = Some(fields[0].clone());
    }
    let field = chart_field.clone().unwrap_or_else(|| fields[0].clone());
    let field_before = field.clone();

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(t!("details.field").to_string())
                .size(10.5)
                .color(pal.muted),
        );
        let mut selected = field.clone();
        egui::ComboBox::from_id_salt("chart-field")
            .width(200.0)
            .selected_text(field_display_name(&selected))
            .show_ui(ui, |ui| {
                for candidate in &fields {
                    ui.selectable_value(
                        &mut selected,
                        candidate.clone(),
                        field_display_name(candidate),
                    );
                }
            });
        *chart_field = Some(selected);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format_count(data.history.len() as u64))
                    .size(10.5)
                    .color(pal.muted),
            );
            ui.label(
                RichText::new(t!("details.messages").to_string())
                    .size(10.5)
                    .color(pal.muted),
            );
        });
    });

    let field = chart_field.clone().unwrap_or_else(|| fields[0].clone());
    if field != field_before {
        auto_fit = true;
    }
    let start = data.history.front().map(|message| message.received_at);
    let mut points: Vec<[f64; 2]> = Vec::new();
    if let Some(start) = start {
        for message in &data.history {
            if let Some(value) = message.numeric_at(&field) {
                let seconds = (message.received_at - start).num_milliseconds() as f64 / 1000.0;
                points.push([seconds, value]);
            }
        }
    }

    if points.is_empty() {
        empty_hint(
            ui,
            pal,
            &t!("details.no_points_title"),
            &t!("details.no_points_hint"),
        );
        return;
    }

    let latest = points.last().map(|point| point[1]).unwrap_or_default();
    let min = points
        .iter()
        .map(|point| point[1])
        .fold(f64::INFINITY, f64::min);
    let max = points
        .iter()
        .map(|point| point[1])
        .fold(f64::NEG_INFINITY, f64::max);
    let average = points.iter().map(|point| point[1]).sum::<f64>() / points.len() as f64;
    let x_min = points
        .iter()
        .map(|point| point[0])
        .fold(f64::INFINITY, f64::min);
    let x_max = points
        .iter()
        .map(|point| point[0])
        .fold(f64::NEG_INFINITY, f64::max);

    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        for (label, value) in [
            (t!("details.current").to_string(), latest),
            (t!("details.minimum").to_string(), min),
            (t!("details.maximum").to_string(), max),
            (t!("details.average").to_string(), average),
        ] {
            Frame::new()
                .fill(pal.card)
                .stroke(Stroke::new(1.0, pal.border))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(8))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(label).size(9.5).color(pal.muted));
                        ui.label(
                            RichText::new(format_number(value))
                                .size(15.0)
                                .strong()
                                .color(pal.text),
                        );
                    });
                });
        }
    });

    ui.add_space(8.0);

    let mut plot_points = points;
    if plot_points.len() > 2_000 {
        let step = plot_points.len() / 2_000 + 1;
        plot_points = plot_points.into_iter().step_by(step).collect();
    }

    let height = (ui.available_height() - 8.0).max(240.0);
    let (y_min, y_max) = padded_range(min, max);
    let (x_min, x_max) = padded_range(x_min, x_max);
    egui_plot::Plot::new("numeric-chart")
        .height(height)
        .allow_scroll(false)
        .allow_drag([true, true])
        .allow_zoom([true, true])
        .show(ui, |plot_ui| {
            // Fit both axes when the chart tab is opened or the field changes;
            // afterwards the user can pan and zoom either axis.
            if auto_fit {
                plot_ui.set_plot_bounds_x(x_min..=x_max);
                plot_ui.set_plot_bounds_y(y_min..=y_max);
            }
            plot_ui.line(
                egui_plot::Line::new(field.clone(), egui_plot::PlotPoints::from(plot_points))
                    .stroke(Stroke::new(1.8, pal.accent)),
            );
        });

    let _ = settings;
}

/// Pads the value range slightly so points never touch the chart edges. A
/// constant series would otherwise have a zero-height range.
fn padded_range(min: f64, max: f64) -> (f64, f64) {
    let span = (max - min).abs();
    let padding = if span > f64::EPSILON {
        span * 0.08
    } else {
        max.abs().max(1.0) * 0.1
    };
    (min - padding, max + padding)
}

fn code_view(ui: &mut Ui, text: &str, pal: &Palette, horizontal: bool, id: &str) {
    Frame::new()
        .fill(pal.code_bg)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            let height = (ui.available_height() - 10.0).max(150.0);
            let area = if horizontal {
                ScrollArea::both()
            } else {
                ScrollArea::vertical()
            };
            area.id_salt(id)
                .auto_shrink([false, false])
                .max_height(height)
                .show(ui, |ui| {
                    ui.add(
                        Label::new(RichText::new(text).monospace().size(12.0).color(pal.text))
                            .wrap()
                            .selectable(true),
                    );
                });
        });
}

fn empty_hint(ui: &mut Ui, pal: &Palette, title: &str, hint: &str) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(title).size(14.0).strong().color(pal.muted));
        ui.add_space(4.0);
        ui.label(RichText::new(hint).size(11.0).color(pal.muted));
    });
}

fn truncate_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }

    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n\n{}",
        &text[..end],
        t!("details.bytes_truncated", bytes = text.len() - end)
    )
}

fn format_number(value: f64) -> String {
    if value.fract().abs() < 1e-9 {
        format!("{value:.0}")
    } else {
        format!("{value:.3}")
    }
}

fn json_highlight(text: &str, pal: &Palette) -> LayoutJob {
    const MAX_CHARS: usize = 32_000;

    let font = FontId::monospace(12.0);
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;

    let chars: Vec<char> = text.chars().take(MAX_CHARS).collect();
    let mut index = 0usize;

    while index < chars.len() {
        let character = chars[index];

        if character == '"' {
            let start = index;
            index += 1;
            let mut escaped = false;
            while index < chars.len() {
                let current = chars[index];
                if escaped {
                    escaped = false;
                } else if current == '\\' {
                    escaped = true;
                } else if current == '"' {
                    index += 1;
                    break;
                }
                index += 1;
            }

            let mut lookahead = index;
            while lookahead < chars.len() && chars[lookahead].is_whitespace() {
                lookahead += 1;
            }
            let is_key = lookahead < chars.len() && chars[lookahead] == ':';
            let token: String = chars[start..index].iter().collect();
            append_json_token(
                &mut job,
                &font,
                &token,
                if is_key {
                    pal.json_key
                } else {
                    pal.json_string
                },
            );
        } else if character == '-' || character.is_ascii_digit() {
            let start = index;
            while index < chars.len()
                && matches!(chars[index], '0'..='9' | '.' | 'e' | 'E' | '+' | '-')
            {
                index += 1;
            }
            let token: String = chars[start..index].iter().collect();
            append_json_token(&mut job, &font, &token, pal.json_number);
        } else if character.is_ascii_alphabetic() {
            let start = index;
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric() || chars[index] == '_')
            {
                index += 1;
            }
            let token: String = chars[start..index].iter().collect();
            append_json_token(&mut job, &font, &token, pal.json_literal);
        } else {
            append_json_token(&mut job, &font, &character.to_string(), pal.json_punct);
            index += 1;
        }
    }

    if text.chars().count() > MAX_CHARS {
        append_json_token(&mut job, &font, &t!("details.display_truncated"), pal.muted);
    }

    job
}

fn append_json_token(job: &mut LayoutJob, font: &FontId, text: &str, color: Color32) {
    job.append(
        text,
        0.0,
        TextFormat {
            font_id: font.clone(),
            color,
            ..Default::default()
        },
    );
}

impl MqttUiApp {
    pub(crate) fn show_details_panel(&mut self, ui: &mut Ui) {
        let pal = palette(ui.visuals().dark_mode);
        let conn_id = self.selected_connection;
        let path = self.selected_topic.clone();
        let mut actions = Vec::new();

        egui::Panel::right("details-panel")
            .default_size(470.0)
            .min_size(340.0)
            .max_size(760.0)
            .resizable(true)
            .frame(
                Frame::new()
                    .fill(pal.panel)
                    .stroke(Stroke::new(1.0, pal.border))
                    .inner_margin(Margin::same(14)),
            )
            .show(ui, |ui| match (conn_id, path.as_deref()) {
                (Some(id), Some(path)) => {
                    let runtime = self.runtimes.get(&id);
                    let exists = runtime.is_some_and(|runtime| runtime.tree.node(path).is_some());
                    if exists {
                        let data = runtime.and_then(|runtime| runtime.tree.data(path));
                        actions = show(
                            ui,
                            path,
                            data,
                            &self.config.settings,
                            &mut self.detail_tab,
                            &mut self.value_mode,
                            &mut self.history_selection,
                            &mut self.chart_field,
                        );
                    } else {
                        empty_state(ui);
                    }
                }
                _ => empty_state(ui),
            });

        for action in actions {
            self.apply_detail_action(action);
        }

        if let (Some(id), Some(path)) = (self.selected_connection, self.selected_topic.clone())
            && self
                .runtimes
                .get(&id)
                .is_none_or(|runtime| runtime.tree.node(&path).is_none())
        {
            self.selected_topic = None;
            self.history_selection = None;
            self.chart_field = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::padded_range;

    #[test]
    fn constant_series_gets_a_visible_range() {
        let (min, max) = padded_range(42.0, 42.0);
        assert!(min < 42.0);
        assert!(max > 42.0);
        assert!(max - min > 1.0);
    }

    #[test]
    fn varying_series_is_padded_on_both_sides() {
        let (min, max) = padded_range(10.0, 20.0);
        assert!(min < 10.0);
        assert!(max > 20.0);
        assert!((max - min - 11.6).abs() < 1e-9);
    }
}
