//! Lightweight toast notifications.

use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, CornerRadius, Frame, Id, Margin, Order, RichText, Stroke, Vec2};

use super::theme::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

struct Toast {
    message: String,
    kind: ToastKind,
    created: Instant,
    ttl: Duration,
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

impl Toasts {
    const MAX_VISIBLE: usize = 4;

    pub fn info(&mut self, message: impl Into<String>) {
        self.push(ToastKind::Info, message.into());
    }

    pub fn success(&mut self, message: impl Into<String>) {
        self.push(ToastKind::Success, message.into());
    }

    pub fn warning(&mut self, message: impl Into<String>) {
        self.push(ToastKind::Warning, message.into());
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.push(ToastKind::Error, message.into());
    }

    fn push(&mut self, kind: ToastKind, message: String) {
        self.items.push(Toast {
            message,
            kind,
            created: Instant::now(),
            ttl: match kind {
                ToastKind::Error => Duration::from_secs(7),
                _ => Duration::from_secs(4),
            },
        });
        if self.items.len() > Self::MAX_VISIBLE {
            self.items.remove(0);
        }
    }

    pub fn tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        self.items
            .retain(|toast| now.duration_since(toast.created) < toast.ttl);
        if let Some(toast) = self.items.last() {
            let remaining = toast.ttl.saturating_sub(now.duration_since(toast.created));
            ctx.request_repaint_after(remaining);
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, pal: &Palette) {
        if self.items.is_empty() {
            return;
        }

        let mut dismiss = None;
        egui::Area::new(Id::unique("mqtt-probe-toasts"))
            .anchor(Align2::RIGHT_TOP, Vec2::new(-16.0, 64.0))
            .order(Order::Foreground)
            .show(ctx, |ui| {
                ui.set_max_width(380.0);
                for (index, toast) in self.items.iter().enumerate() {
                    let color = match toast.kind {
                        ToastKind::Info => pal.accent,
                        ToastKind::Success => pal.success,
                        ToastKind::Warning => pal.warn,
                        ToastKind::Error => pal.danger,
                    };

                    Frame::new()
                        .fill(pal.card)
                        .stroke(Stroke::new(1.0, color.gamma_multiply(0.65)))
                        .corner_radius(CornerRadius::same(9))
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (rect, _) =
                                    ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                                ui.painter().circle_filled(rect.center(), 4.0, color);
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&toast.message).size(12.0).color(pal.text),
                                    )
                                    .wrap(),
                                );
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("×").size(14.0).color(pal.muted),
                                        )
                                        .frame(false),
                                    )
                                    .clicked()
                                {
                                    dismiss = Some(index);
                                }
                            });
                        });
                    ui.add_space(6.0);
                }
            });

        if let Some(index) = dismiss {
            self.items.remove(index);
        }
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.items.clear();
    }
}
