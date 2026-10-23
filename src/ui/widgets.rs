//! Reusable small widgets.

use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText, Stroke, Ui, Vec2};

use crate::mqtt::ConnectionState;

use super::theme::Palette;

pub fn badge(ui: &mut Ui, text: &str, color: Color32) {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(
            color.r(),
            color.g(),
            color.b(),
            28,
        ))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.55)))
        .corner_radius(CornerRadius::same(7))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(10.5).color(color).strong());
        });
}

pub fn status_dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
    ui.painter().circle_stroke(
        rect.center(),
        4.0,
        Stroke::new(1.0, color.gamma_multiply(1.35)),
    );
}

pub fn status_color(state: &ConnectionState, pal: &Palette) -> Color32 {
    match state {
        ConnectionState::Connected => pal.success,
        ConnectionState::Connecting | ConnectionState::Reconnecting => pal.warn,
        ConnectionState::Disconnected => pal.muted,
        ConnectionState::Error(_) => pal.danger,
    }
}

pub fn section_heading(ui: &mut Ui, text: &str, pal: &Palette) {
    ui.label(RichText::new(text).size(11.0).strong().color(pal.muted));
}
