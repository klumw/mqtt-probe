//! Built-in help window with a linked table of contents.

use eframe::egui::{
    self, Align, Align2, CornerRadius, Frame, Grid, Label, Layout, Margin, RichText, ScrollArea,
    Stroke, Ui,
};
use rust_i18n::t;

use crate::app::MqttUiApp;

use super::theme::{Palette, palette};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpTopic {
    QuickStart,
    Basics,
    Overview,
    Connections,
    Tree,
    Details,
    Publish,
    Settings,
    Shortcuts,
    Troubleshooting,
    About,
}

impl HelpTopic {
    pub const ALL: [HelpTopic; 11] = [
        HelpTopic::QuickStart,
        HelpTopic::Basics,
        HelpTopic::Overview,
        HelpTopic::Connections,
        HelpTopic::Tree,
        HelpTopic::Details,
        HelpTopic::Publish,
        HelpTopic::Settings,
        HelpTopic::Shortcuts,
        HelpTopic::Troubleshooting,
        HelpTopic::About,
    ];

    fn title_key(self) -> &'static str {
        match self {
            HelpTopic::QuickStart => "help.topic.quick_start",
            HelpTopic::Basics => "help.topic.basics",
            HelpTopic::Overview => "help.topic.overview",
            HelpTopic::Connections => "help.topic.connections",
            HelpTopic::Tree => "help.topic.tree",
            HelpTopic::Details => "help.topic.details",
            HelpTopic::Publish => "help.topic.publish",
            HelpTopic::Settings => "help.topic.settings",
            HelpTopic::Shortcuts => "help.topic.shortcuts",
            HelpTopic::Troubleshooting => "help.topic.troubleshooting",
            HelpTopic::About => "help.topic.about",
        }
    }

    fn title(self) -> String {
        t!(self.title_key()).to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HelpScrollTarget {
    Top,
    Topic(HelpTopic),
}

impl MqttUiApp {
    pub(crate) fn show_help_window(&mut self, ctx: &egui::Context) {
        if !self.show_help {
            return;
        }

        let pal = palette(self.config.settings.dark_mode);
        let mut open = true;
        let mut target = self.help_scroll.take();

        egui::Window::new(t!("help.title"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(980.0, 740.0))
            .min_width(720.0)
            .min_height(460.0)
            .default_pos(ctx.content_rect().center())
            .pivot(Align2::CENTER_CENTER)
            .show(ctx, |ui| {
                ui.style_mut().visuals.hyperlink_color = pal.accent;
                ui.spacing_mut().item_spacing.y = 5.0;

                ui.horizontal(|ui| {
                    ui.label(RichText::new("MQTT").size(38.4).strong().color(pal.accent));
                    ui.label(RichText::new("Probe").size(38.4).strong().color(pal.text));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(t!("about.version", version = env!("CARGO_PKG_VERSION")))
                            .size(19.2)
                            .color(pal.muted),
                    );
                });
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                let max_height = (ctx.content_rect().height() - 220.0).max(380.0);
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(max_height)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());

                        let toc_response = table_of_contents(ui, &pal, &mut target);
                        if target == Some(HelpScrollTarget::Top) {
                            toc_response.scroll_to_me(Some(Align::TOP));
                            target = None;
                        }

                        for (index, topic) in HelpTopic::ALL.iter().enumerate() {
                            ui.add_space(16.0);
                            ui.separator();
                            ui.add_space(8.0);
                            let heading = topic_heading(ui, index + 1, *topic, &pal);
                            if target == Some(HelpScrollTarget::Topic(*topic)) {
                                heading.scroll_to_me(Some(Align::TOP));
                                target = None;
                            }
                            ui.add_space(2.0);
                            match topic {
                                HelpTopic::QuickStart => quick_start(ui, &pal, &mut target),
                                HelpTopic::Basics => basics(ui, &pal),
                                HelpTopic::Overview => overview(ui, &pal, &mut target),
                                HelpTopic::Connections => connections(ui, &pal, &mut target),
                                HelpTopic::Tree => tree(ui, &pal, &mut target),
                                HelpTopic::Details => details(ui, &pal, &mut target),
                                HelpTopic::Publish => publish(ui, &pal, &mut target),
                                HelpTopic::Settings => settings(ui, &pal, &mut target),
                                HelpTopic::Shortcuts => shortcuts(ui, &pal),
                                HelpTopic::Troubleshooting => {
                                    troubleshooting(ui, &pal, &mut target)
                                }
                                HelpTopic::About => about(ui, &pal),
                            }
                            back_to_contents(ui, &mut target);
                        }
                    });
            });

        self.show_help = open;
        self.help_scroll = target;
    }
}

fn table_of_contents(
    ui: &mut Ui,
    pal: &Palette,
    target: &mut Option<HelpScrollTarget>,
) -> egui::Response {
    Frame::new()
        .fill(pal.card)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new(t!("help.toc").to_string())
                    .size(17.6)
                    .strong()
                    .color(pal.muted),
            );
            ui.add_space(4.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            for (index, topic) in HelpTopic::ALL.iter().enumerate() {
                let label = format!("{}. {}", index + 1, topic.title());
                if ui.link(RichText::new(label).size(18.4)).clicked() {
                    *target = Some(HelpScrollTarget::Topic(*topic));
                }
            }
        })
        .response
}

fn topic_heading(ui: &mut Ui, number: usize, topic: HelpTopic, pal: &Palette) -> egui::Response {
    ui.horizontal(|ui| {
        Frame::new()
            .fill(pal.hover)
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(7, 2))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(number.to_string())
                        .size(17.6)
                        .strong()
                        .color(pal.accent),
                );
            });
        ui.label(
            RichText::new(topic.title())
                .size(24.8)
                .strong()
                .color(pal.text),
        );
    })
    .response
}

fn paragraph(ui: &mut Ui, text: String, pal: &Palette) {
    ui.add(Label::new(RichText::new(text).size(18.4).color(pal.text)).wrap());
}

fn bullets(ui: &mut Ui, pal: &Palette, items: &[String]) {
    for item in items {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("•").size(18.4).color(pal.accent));
            ui.add(Label::new(RichText::new(item).size(18.4).color(pal.text)).wrap());
        });
    }
}

fn options(ui: &mut Ui, id: &str, pal: &Palette, rows: &[(String, String)]) {
    Grid::new(id)
        .num_columns(2)
        .spacing([18.0, 5.0])
        .show(ui, |ui| {
            for (label, description) in rows {
                ui.label(RichText::new(label).size(17.6).strong().color(pal.text));
                ui.add(Label::new(RichText::new(description).size(17.6).color(pal.muted)).wrap());
                ui.end_row();
            }
        });
}

fn subheading(ui: &mut Ui, text: &str, pal: &Palette) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).size(17.6).strong().color(pal.muted));
    ui.add_space(1.0);
}

fn see_also(
    ui: &mut Ui,
    pal: &Palette,
    topics: &[HelpTopic],
    target: &mut Option<HelpScrollTarget>,
) {
    ui.add_space(5.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(t!("help.see_also").to_string())
                .size(16.0)
                .color(pal.muted),
        );
        for (index, topic) in topics.iter().enumerate() {
            if index > 0 {
                ui.label(RichText::new("·").size(16.0).color(pal.muted));
            }
            if ui.link(RichText::new(topic.title()).size(16.0)).clicked() {
                *target = Some(HelpScrollTarget::Topic(*topic));
            }
        }
    });
}

fn back_to_contents(ui: &mut Ui, target: &mut Option<HelpScrollTarget>) {
    ui.add_space(3.0);
    // Align::TOP keeps the link on the line after the section content.
    // Align::Center would center it in the remaining viewport height, leaving
    // a large empty area below the first section (Quick start).
    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
        if ui
            .link(RichText::new(format!("↑ {}", t!("help.back_to_toc"))).size(16.0))
            .clicked()
        {
            *target = Some(HelpScrollTarget::Top);
        }
    });
}

fn quick_start(ui: &mut Ui, pal: &Palette, target: &mut Option<HelpScrollTarget>) {
    paragraph(ui, t!("help.quick_start.intro").to_string(), pal);
    bullets(
        ui,
        pal,
        &[
            t!("help.quick_start.profile").to_string(),
            t!("help.quick_start.connect").to_string(),
            t!("help.quick_start.subscribe").to_string(),
            t!("help.quick_start.publish").to_string(),
        ],
    );
    see_also(
        ui,
        pal,
        &[HelpTopic::Connections, HelpTopic::Publish],
        target,
    );
}

fn basics(ui: &mut Ui, pal: &Palette) {
    paragraph(ui, t!("help.basics.intro").to_string(), pal);
    options(
        ui,
        "help-basics",
        pal,
        &[
            (
                t!("help.basics.topics_term").to_string(),
                t!("help.basics.topics").to_string(),
            ),
            (
                t!("editor.qos").to_string(),
                t!("help.basics.qos").to_string(),
            ),
            (
                t!("common.retained").to_string(),
                t!("help.basics.retained").to_string(),
            ),
            (
                t!("editor.clean_session").to_string(),
                t!("help.basics.clean_session").to_string(),
            ),
            (
                t!("editor.keep_alive").to_string(),
                t!("help.basics.keep_alive").to_string(),
            ),
            (
                t!("editor.transport").to_string(),
                t!("help.basics.transports").to_string(),
            ),
        ],
    );
}

fn overview(ui: &mut Ui, pal: &Palette, target: &mut Option<HelpScrollTarget>) {
    paragraph(ui, t!("help.overview.intro").to_string(), pal);
    bullets(
        ui,
        pal,
        &[
            t!("help.overview.areas").to_string(),
            t!("help.overview.publish").to_string(),
            t!("help.overview.toasts").to_string(),
        ],
    );
    see_also(
        ui,
        pal,
        &[
            HelpTopic::Connections,
            HelpTopic::Tree,
            HelpTopic::Details,
            HelpTopic::Publish,
            HelpTopic::Settings,
        ],
        target,
    );
}

fn connections(ui: &mut Ui, pal: &Palette, target: &mut Option<HelpScrollTarget>) {
    paragraph(ui, t!("help.connections.intro").to_string(), pal);
    subheading(ui, &t!("connections.title"), pal);
    bullets(
        ui,
        pal,
        &[
            t!("help.connections.list_row").to_string(),
            t!("help.connections.list_actions").to_string(),
            t!("help.connections.one_active").to_string(),
        ],
    );
    subheading(ui, &t!("connections.broker_info"), pal);
    paragraph(ui, t!("help.connections.broker_info").to_string(), pal);
    subheading(ui, &t!("help.connections.editor_heading"), pal);
    options(
        ui,
        "help-connections-options",
        pal,
        &[
            (
                t!("editor.name").to_string(),
                t!("help.connections.name").to_string(),
            ),
            (
                t!("editor.broker").to_string(),
                t!("help.connections.host").to_string(),
            ),
            (
                t!("editor.port").to_string(),
                t!("help.connections.port").to_string(),
            ),
            (
                t!("editor.transport").to_string(),
                t!("help.connections.transport").to_string(),
            ),
            (
                t!("editor.websocket_path").to_string(),
                t!("help.connections.ws_path").to_string(),
            ),
            (
                t!("editor.ca_certificate").to_string(),
                t!("help.connections.ca").to_string(),
            ),
            (
                t!("editor.client_certificate").to_string(),
                t!("help.connections.client_certificate").to_string(),
            ),
            (
                t!("editor.client_key").to_string(),
                t!("help.connections.client_key").to_string(),
            ),
            (
                t!("editor.username").to_string(),
                t!("help.connections.auth").to_string(),
            ),
            (
                t!("editor.client_id").to_string(),
                t!("help.connections.client_id").to_string(),
            ),
            (
                t!("editor.keep_alive").to_string(),
                t!("help.connections.keep_alive").to_string(),
            ),
            (
                t!("editor.clean_session").to_string(),
                t!("help.connections.clean_session").to_string(),
            ),
            (
                t!("editor.subscription").to_string(),
                t!("help.connections.subscription").to_string(),
            ),
            (
                t!("editor.qos").to_string(),
                t!("help.connections.subscribe_qos").to_string(),
            ),
            (
                t!("editor.auto_connect").to_string(),
                t!("help.connections.auto_connect").to_string(),
            ),
        ],
    );
    subheading(ui, &t!("help.connections.certificates_heading"), pal);
    paragraph(
        ui,
        t!("help.connections.certificate_version").to_string(),
        pal,
    );
    see_also(
        ui,
        pal,
        &[HelpTopic::Troubleshooting, HelpTopic::Basics],
        target,
    );
}

fn tree(ui: &mut Ui, pal: &Palette, target: &mut Option<HelpScrollTarget>) {
    paragraph(ui, t!("help.tree.intro").to_string(), pal);
    bullets(
        ui,
        pal,
        &[
            t!("help.tree.filter").to_string(),
            t!("help.tree.expand").to_string(),
            t!("help.tree.clear").to_string(),
            t!("help.tree.tooltip").to_string(),
            t!("help.tree.context").to_string(),
            t!("help.tree.retained_delete").to_string(),
            t!("help.tree.restore").to_string(),
            t!("help.tree.highlight").to_string(),
        ],
    );
    see_also(ui, pal, &[HelpTopic::Settings, HelpTopic::Details], target);
}

fn details(ui: &mut Ui, pal: &Palette, target: &mut Option<HelpScrollTarget>) {
    paragraph(ui, t!("help.details.intro").to_string(), pal);
    bullets(
        ui,
        pal,
        &[
            t!("help.details.header").to_string(),
            t!("help.details.value").to_string(),
            t!("help.details.history").to_string(),
            t!("help.details.diff").to_string(),
            t!("help.details.chart").to_string(),
            t!("help.details.truncation").to_string(),
        ],
    );
    see_also(ui, pal, &[HelpTopic::Settings, HelpTopic::Tree], target);
}

fn publish(ui: &mut Ui, pal: &Palette, target: &mut Option<HelpScrollTarget>) {
    paragraph(ui, t!("help.publish.intro").to_string(), pal);
    bullets(
        ui,
        pal,
        &[
            t!("help.publish.target").to_string(),
            t!("help.publish.topic").to_string(),
            t!("help.publish.payload").to_string(),
            t!("help.publish.format_history").to_string(),
            t!("help.publish.qos_retain").to_string(),
            t!("help.publish.shortcut").to_string(),
        ],
    );
    see_also(ui, pal, &[HelpTopic::Connections, HelpTopic::Tree], target);
}

fn settings(ui: &mut Ui, pal: &Palette, target: &mut Option<HelpScrollTarget>) {
    paragraph(ui, t!("help.settings.intro").to_string(), pal);
    paragraph(ui, t!("help.settings.appearance").to_string(), pal);
    paragraph(ui, t!("help.settings.language").to_string(), pal);
    options(
        ui,
        "help-settings-options",
        pal,
        &[
            (
                t!("settings.restore_previous").to_string(),
                t!("help.settings.restore").to_string(),
            ),
            (
                t!("settings.messages_per_topic").to_string(),
                t!("help.settings.max_history").to_string(),
            ),
            (
                t!("settings.max_display_bytes").to_string(),
                t!("help.settings.max_bytes").to_string(),
            ),
            (
                t!("settings.payload_preview").to_string(),
                t!("help.settings.preview").to_string(),
            ),
            (
                t!("settings.search_payload").to_string(),
                t!("help.settings.search_payload").to_string(),
            ),
            (
                t!("settings.highlight_updates").to_string(),
                t!("help.settings.highlight").to_string(),
            ),
            (
                t!("settings.highlight_duration").to_string(),
                t!("help.settings.highlight_duration").to_string(),
            ),
            (
                t!("settings.pretty_json").to_string(),
                t!("help.settings.pretty_json").to_string(),
            ),
            (
                t!("settings.utc_timestamps").to_string(),
                t!("help.settings.utc").to_string(),
            ),
        ],
    );
    paragraph(ui, t!("help.settings.config_path").to_string(), pal);
    paragraph(ui, t!("help.settings.save").to_string(), pal);
    paragraph(ui, t!("help.settings.password").to_string(), pal);
    see_also(ui, pal, &[HelpTopic::Connections, HelpTopic::Tree], target);
}

fn shortcuts(ui: &mut Ui, pal: &Palette) {
    paragraph(ui, t!("help.shortcuts.intro").to_string(), pal);
    Grid::new("help-shortcuts")
        .num_columns(2)
        .spacing([24.0, 5.0])
        .show(ui, |ui| {
            for (key, action) in [
                ("about.shortcut_search_key", "about.shortcut_search_action"),
                (
                    "about.shortcut_publish_key",
                    "about.shortcut_publish_action",
                ),
                ("about.shortcut_reset_key", "about.shortcut_reset_action"),
                ("about.shortcut_quit_key", "about.shortcut_quit_action"),
            ] {
                ui.label(
                    RichText::new(t!(key).to_string())
                        .monospace()
                        .size(17.6)
                        .color(pal.text),
                );
                ui.label(
                    RichText::new(t!(action).to_string())
                        .size(17.6)
                        .color(pal.muted),
                );
                ui.end_row();
            }
        });
}

fn troubleshooting(ui: &mut Ui, pal: &Palette, target: &mut Option<HelpScrollTarget>) {
    paragraph(ui, t!("help.troubleshooting.intro").to_string(), pal);
    bullets(
        ui,
        pal,
        &[
            t!("help.troubleshooting.subscription").to_string(),
            t!("help.troubleshooting.leading_slash").to_string(),
            t!("help.troubleshooting.host_scheme").to_string(),
            t!("help.troubleshooting.ca").to_string(),
            t!("help.troubleshooting.one_connection").to_string(),
            t!("help.troubleshooting.log").to_string(),
            t!("help.troubleshooting.config").to_string(),
        ],
    );
    see_also(
        ui,
        pal,
        &[HelpTopic::Connections, HelpTopic::Settings],
        target,
    );
}

fn about(ui: &mut Ui, pal: &Palette) {
    paragraph(ui, t!("about.description").to_string(), pal);
    ui.add_space(4.0);
    subheading(ui, &t!("about.features"), pal);
    bullets(
        ui,
        pal,
        &[
            t!("about.feature_profiles").to_string(),
            t!("about.feature_tree").to_string(),
            t!("about.feature_json").to_string(),
            t!("about.feature_charts").to_string(),
            t!("about.feature_publish").to_string(),
            t!("about.feature_languages").to_string(),
            t!("about.feature_persistence").to_string(),
        ],
    );
    subheading(ui, &t!("about.tech"), pal);
    paragraph(
        ui,
        "egui/eframe 0.37 · egui_plot 0.38 · rumqttc 0.25 · tokio · serde".to_owned(),
        pal,
    );
    ui.hyperlink_to(
        t!("about.original").to_string(),
        "https://github.com/thomasnordquist/MQTT-Explorer",
    );
    ui.add_space(4.0);
    paragraph(ui, t!("help.about.license").to_string(), pal);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_help_topic_has_a_title_in_every_locale() {
        for locale in ["en", "de", "fr", "ko", "zh", "es"] {
            for topic in HelpTopic::ALL {
                let key = topic.title_key();
                let title = t!(key, locale = locale).to_string();
                assert_ne!(title, key, "missing {key} for {locale}");
                assert!(!title.trim().is_empty(), "empty {key} for {locale}");
            }
        }
    }
}
