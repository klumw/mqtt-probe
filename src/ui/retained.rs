//! Confirmation and execution of retained-message deletion.

use eframe::egui::{self, Align2, Color32, RichText};
use rust_i18n::t;
use uuid::Uuid;

use crate::app::MqttUiApp;
use crate::mqtt::Command;

use super::theme::palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetainedDeleteScope {
    Topic,
    Branch,
}

#[derive(Debug, Clone)]
pub(crate) struct RetainedDeleteRequest {
    pub connection: Uuid,
    pub path: String,
    pub scope: RetainedDeleteScope,
    pub topics: Vec<String>,
}

impl MqttUiApp {
    /// Opens the confirmation dialog for deleting retained messages. The
    /// affected topics are collected now so the dialog can state how many
    /// messages will be removed.
    pub(crate) fn request_retained_delete(
        &mut self,
        connection: Uuid,
        path: String,
        scope: RetainedDeleteScope,
    ) {
        let Some(runtime) = self.runtimes.get(&connection) else {
            return;
        };

        let topics = match scope {
            RetainedDeleteScope::Topic => runtime
                .tree
                .data(&path)
                .and_then(|data| data.latest())
                .filter(|message| message.retain)
                .map(|_| vec![path.clone()])
                .unwrap_or_default(),
            RetainedDeleteScope::Branch => runtime.tree.retained_topics_under(&path),
        };

        if topics.is_empty() {
            self.toasts.info(t!("toast.no_retained_topic"));
            return;
        }

        self.retained_confirm = Some(RetainedDeleteRequest {
            connection,
            path,
            scope,
            topics,
        });
    }

    pub(crate) fn show_retained_dialog(&mut self, ctx: &egui::Context) {
        let Some(request) = self.retained_confirm.take() else {
            return;
        };

        let pal = palette(self.config.settings.dark_mode);
        let mut open = true;
        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new(t!("retained.title"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(480.0)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                let path = display_path(&request.path);
                let question = match request.scope {
                    RetainedDeleteScope::Topic => {
                        t!("retained.topic_question", topic = path).to_string()
                    }
                    RetainedDeleteScope::Branch => t!(
                        "retained.branch_question",
                        path = path,
                        count = request.topics.len()
                    )
                    .to_string(),
                };
                ui.label(RichText::new(question).size(12.0));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(t!("retained.warning").to_string())
                        .size(11.0)
                        .color(pal.warn),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("common.cancel")).clicked() {
                        cancelled = true;
                    }
                    let delete = egui::Button::new(
                        RichText::new(t!("retained.confirm").to_string())
                            .color(Color32::WHITE)
                            .strong(),
                    )
                    .fill(pal.danger);
                    if ui.add(delete).clicked() {
                        confirmed = true;
                    }
                });
            });

        if cancelled || !open {
            return;
        }
        if confirmed {
            self.execute_retained_delete(request);
        }
    }

    fn execute_retained_delete(&mut self, request: RetainedDeleteRequest) {
        let Some(runtime) = self.runtimes.get_mut(&request.connection) else {
            self.toasts.error(t!("toast.connection_not_found"));
            return;
        };
        if !runtime.state.is_connected() {
            self.toasts.error(t!("toast.no_active_connection"));
            return;
        }

        let mut sent = 0usize;
        let mut failed = 0usize;
        for topic in &request.topics {
            let result = runtime.handle.commands.send(Command::Publish {
                topic: topic.clone(),
                payload: Vec::new(),
                qos: 1,
                retain: true,
            });
            if result.is_ok() {
                sent += 1;
                runtime.tree.clear_topic_data(topic);
                runtime.forget_subtree(topic);
            } else {
                failed += 1;
            }
        }

        if sent > 0 {
            self.toasts
                .success(t!("toast.retained_deleted", count = sent));
        }
        if failed > 0 {
            self.toasts
                .error(t!("toast.retained_delete_failed", count = failed));
        }
    }
}

fn display_path(path: &str) -> String {
    if path.is_empty() {
        "/".to_owned()
    } else {
        path.to_owned()
    }
}
