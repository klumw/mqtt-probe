//! Color palette and egui styling for MQTT Probe.

use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle, Theme};

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub card: Color32,
    pub input: Color32,
    pub code_bg: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub slider_rail: Color32,
    pub hover: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub success: Color32,
    pub warn: Color32,
    pub danger: Color32,
    pub selection: Color32,
    pub json_key: Color32,
    pub json_string: Color32,
    pub json_number: Color32,
    pub json_literal: Color32,
    pub json_punct: Color32,
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            bg: Color32::from_rgb(12, 15, 21),
            panel: Color32::from_rgb(19, 23, 31),
            card: Color32::from_rgb(25, 30, 40),
            input: Color32::from_rgb(15, 18, 25),
            code_bg: Color32::from_rgb(13, 16, 23),
            border: Color32::from_rgb(42, 50, 64),
            border_strong: Color32::from_rgb(60, 72, 92),
            slider_rail: Color32::from_rgb(68, 82, 104),
            hover: Color32::from_rgb(32, 39, 51),
            text: Color32::from_rgb(229, 234, 243),
            muted: Color32::from_rgb(137, 148, 168),
            accent: Color32::from_rgb(77, 163, 255),
            accent_soft: Color32::from_rgba_unmultiplied(77, 163, 255, 40),
            success: Color32::from_rgb(61, 220, 151),
            warn: Color32::from_rgb(242, 193, 78),
            danger: Color32::from_rgb(255, 107, 107),
            selection: Color32::from_rgba_unmultiplied(51, 105, 170, 150),
            json_key: Color32::from_rgb(125, 180, 255),
            json_string: Color32::from_rgb(126, 214, 160),
            json_number: Color32::from_rgb(245, 190, 120),
            json_literal: Color32::from_rgb(200, 150, 255),
            json_punct: Color32::from_rgb(130, 142, 160),
        }
    } else {
        Palette {
            bg: Color32::from_rgb(254, 253, 240),
            panel: Color32::from_rgb(254, 253, 240),
            card: Color32::from_rgb(234, 239, 246),
            input: Color32::from_rgb(249, 251, 254),
            code_bg: Color32::from_rgb(240, 244, 249),
            border: Color32::from_rgb(190, 200, 214),
            border_strong: Color32::from_rgb(158, 171, 191),
            slider_rail: Color32::from_rgb(150, 165, 185),
            hover: Color32::from_rgb(222, 230, 241),
            text: Color32::from_rgb(17, 23, 35),
            muted: Color32::from_rgb(64, 74, 92),
            accent: Color32::from_rgb(20, 92, 210),
            accent_soft: Color32::from_rgba_unmultiplied(20, 92, 210, 35),
            success: Color32::from_rgb(8, 106, 74),
            warn: Color32::from_rgb(142, 88, 0),
            danger: Color32::from_rgb(176, 40, 30),
            selection: Color32::from_rgba_unmultiplied(20, 92, 210, 55),
            json_key: Color32::from_rgb(20, 92, 210),
            json_string: Color32::from_rgb(6, 104, 63),
            json_number: Color32::from_rgb(146, 74, 0),
            json_literal: Color32::from_rgb(108, 42, 158),
            json_punct: Color32::from_rgb(82, 93, 110),
        }
    }
}

/// Installs the custom dark and light styles and activates the dark variant.
/// Loads system fonts for CJK fallback and for a heavier, more readable
/// proportional font in the light theme.
const READABLE_FAMILY: &str = "mqtt-probe-readable";

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut cjk_proportional = Vec::new();
    let mut cjk_monospace = Vec::new();

    // (font path, proportional face indices, monospace face indices)
    const CJK_CANDIDATES: &[(&str, &[u32], &[u32])] = &[
        (
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            &[2, 1, 0],
            &[7, 6, 5],
        ),
        (
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            &[2, 1, 0],
            &[7, 6, 5],
        ),
        (
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            &[2, 1, 0],
            &[7, 6, 5],
        ),
        (
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            &[2, 1, 0],
            &[7, 6, 5],
        ),
        (
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            &[0],
            &[0],
        ),
        ("/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc", &[0], &[1]),
        ("C:\\Windows\\Fonts\\msyh.ttc", &[0], &[0]),
        ("C:\\Windows\\Fonts\\malgun.ttf", &[0], &[0]),
        ("/System/Library/Fonts/PingFang.ttc", &[0], &[0]),
    ];

    for (path, proportional_faces, monospace_faces) in CJK_CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let blob: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(bytes);

        for (prefix, faces, names) in [
            (
                "cjk-proportional",
                *proportional_faces,
                &mut cjk_proportional,
            ),
            ("cjk-monospace", *monospace_faces, &mut cjk_monospace),
        ] {
            for &index in faces {
                let name = format!("{prefix}-{index}");
                fonts.font_data.insert(
                    name.clone(),
                    Arc::new(egui::FontData::from_blob(blob.clone(), index)),
                );
                names.push(name);
            }
        }
        break;
    }

    for name in cjk_proportional {
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push(name);
    }
    for name in cjk_monospace {
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push(name);
    }

    // A regular-weight system font is easier to read on the light background
    // than egui's bundled Ubuntu-Light. Keep the default proportional fonts
    // as fallbacks for symbols and CJK.
    let mut readable_family = Vec::new();
    if let Some((name, data)) = load_readable_font() {
        fonts.font_data.insert(name.clone(), Arc::new(data));
        readable_family.push(name);
    }
    if let Some(proportional) = fonts.families.get(&egui::FontFamily::Proportional) {
        readable_family.extend(proportional.iter().cloned());
    }
    fonts.families.insert(
        egui::FontFamily::Name(READABLE_FAMILY.into()),
        readable_family,
    );

    ctx.set_fonts(fonts);
}

fn load_readable_font() -> Option<(String, egui::FontData)> {
    const CANDIDATES: &[&str] = &[
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
        "/System/Library/Fonts/Helvetica.ttc",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
    ];

    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            return Some((
                "readable-regular".to_owned(),
                egui::FontData::from_owned(bytes),
            ));
        }
    }
    None
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);

    for dark in [true, false] {
        let theme = if dark { Theme::Dark } else { Theme::Light };
        let mut style = (*ctx.style_of(theme)).clone();
        let pal = palette(dark);
        let mut visuals = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        visuals.dark_mode = dark;
        visuals.panel_fill = pal.panel;
        visuals.window_fill = pal.card;
        visuals.extreme_bg_color = pal.input;
        visuals.text_edit_bg_color = Some(pal.input);
        visuals.faint_bg_color = pal.card;
        visuals.code_bg_color = pal.code_bg;
        visuals.weak_text_color = Some(pal.muted);
        visuals.hyperlink_color = pal.accent;
        visuals.warn_fg_color = pal.warn;
        visuals.error_fg_color = pal.danger;
        visuals.selection.bg_fill = pal.selection;
        visuals.selection.stroke = Stroke::new(1.0, pal.accent);
        visuals.window_corner_radius = CornerRadius::same(10);
        visuals.menu_corner_radius = CornerRadius::same(8);

        let radius = CornerRadius::same(6);
        visuals.widgets.noninteractive.bg_fill = pal.panel;
        visuals.widgets.noninteractive.weak_bg_fill = pal.panel;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal.border);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal.text);
        visuals.widgets.noninteractive.corner_radius = radius;

        visuals.widgets.inactive.bg_fill = pal.card;
        visuals.widgets.inactive.weak_bg_fill = pal.card;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, pal.border);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, pal.text);
        visuals.widgets.inactive.corner_radius = radius;

        visuals.widgets.hovered.bg_fill = pal.hover;
        visuals.widgets.hovered.weak_bg_fill = pal.hover;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, pal.border_strong);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, pal.text);
        visuals.widgets.hovered.corner_radius = radius;

        visuals.widgets.active.bg_fill = pal.accent;
        visuals.widgets.active.weak_bg_fill = pal.accent;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, pal.accent);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        visuals.widgets.active.corner_radius = radius;

        visuals.widgets.open.bg_fill = pal.card;
        visuals.widgets.open.weak_bg_fill = pal.card;
        visuals.widgets.open.bg_stroke = Stroke::new(1.0, pal.border_strong);
        visuals.widgets.open.fg_stroke = Stroke::new(1.0, pal.text);
        visuals.widgets.open.corner_radius = radius;

        style.visuals = visuals;
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(9.0, 4.0);
        style.spacing.interact_size = egui::vec2(40.0, 24.0);
        style.spacing.indent = 18.0;
        style.spacing.combo_width = 110.0;
        style.text_styles = if dark {
            [
                (TextStyle::Heading, FontId::proportional(19.0)),
                (TextStyle::Body, FontId::proportional(13.5)),
                (TextStyle::Monospace, FontId::monospace(12.5)),
                (TextStyle::Button, FontId::proportional(13.0)),
                (TextStyle::Small, FontId::proportional(11.0)),
            ]
            .into()
        } else {
            let readable = egui::FontFamily::Name(READABLE_FAMILY.into());
            [
                (TextStyle::Heading, FontId::new(19.0, readable.clone())),
                (TextStyle::Body, FontId::new(13.5, readable.clone())),
                (TextStyle::Monospace, FontId::monospace(12.5)),
                (TextStyle::Button, FontId::new(13.0, readable.clone())),
                (TextStyle::Small, FontId::new(11.0, readable)),
            ]
            .into()
        };

        ctx.set_style_of(theme, style);
    }

    apply_mode(ctx, true);
}

pub fn apply_mode(ctx: &egui::Context, dark: bool) {
    ctx.set_theme(if dark { Theme::Dark } else { Theme::Light });
}
