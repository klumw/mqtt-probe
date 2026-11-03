//! Reusable small widgets.

use std::time::Duration;

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

/// Intensity of the update highlight at `elapsed` into a flash of `total`.
///
/// The highlight holds at full strength for the first half and then fades out
/// with a quadratic curve.
pub fn flash_intensity(elapsed: Duration, total: Duration) -> f32 {
    let total = total.as_secs_f32().max(0.001);
    let progress = (elapsed.as_secs_f32() / total).clamp(0.0, 1.0);
    const HOLD: f32 = 0.5;

    if progress <= HOLD {
        1.0
    } else {
        let fade = (progress - HOLD) / (1.0 - HOLD);
        (1.0 - fade) * (1.0 - fade)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::flash_intensity;

    #[test]
    fn update_flash_holds_then_fades_out() {
        let total = Duration::from_secs(3);

        assert!((flash_intensity(Duration::ZERO, total) - 1.0).abs() < 0.001);
        assert!((flash_intensity(Duration::from_millis(1_500), total) - 1.0).abs() < 0.001);
        assert_eq!(flash_intensity(total, total), 0.0);
        assert_eq!(
            flash_intensity(total + Duration::from_millis(50), total),
            0.0
        );

        let mid_fade = flash_intensity(Duration::from_millis(2_250), total);
        assert!(mid_fade > 0.0 && mid_fade < 1.0);
    }
}
