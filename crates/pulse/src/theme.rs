//! Colours and visual style, the same as the other Showtime Suite tools:
//! dark, with the same meaning colours everywhere (green = good, blue =
//! active/selected, yellow = attention, red = error). The traffic light
//! uses exactly these; every status also has its own symbol and word, so
//! colour is never the only sign.

use egui::{
    pos2, vec2, Color32, CornerRadius, FontFamily, FontId, Painter, Rect, Shape, Stroke, TextStyle,
    Visuals,
};
use pulse_core::Status;

pub const BG: Color32 = Color32::from_rgb(12, 14, 17);
pub const PANEL: Color32 = Color32::from_rgb(17, 19, 24);
pub const CARD: Color32 = Color32::from_rgb(24, 27, 34);
pub const LINE: Color32 = Color32::from_rgb(44, 48, 58);
pub const TEXT: Color32 = Color32::from_rgb(232, 235, 239);
pub const MUTED: Color32 = Color32::from_rgb(140, 146, 160);
pub const OK: Color32 = Color32::from_rgb(34, 197, 94);
pub const ACCENT: Color32 = Color32::from_rgb(59, 130, 246);
pub const WARN: Color32 = Color32::from_rgb(250, 204, 21);
pub const ERROR: Color32 = Color32::from_rgb(239, 68, 68);
/// Grey of unknown / not checked.
pub const GREY: Color32 = Color32::from_rgb(110, 116, 130);
/// Pulse's own colour (launcher tile, icon).
pub const BRAND: Color32 = Color32::from_rgb(45, 212, 191);
/// Ink on the brand colour.
pub const ON_BRAND: Color32 = Color32::from_rgb(3, 28, 24);

pub fn status_color(s: Status) -> Color32 {
    match s {
        Status::Ok => OK,
        Status::Warn => WARN,
        Status::Down => ERROR,
        Status::Unknown => GREY,
    }
}

/// The word for a light: "OK", "WARNING", "DOWN", "NO DATA".
pub fn light_word(s: Status) -> &'static str {
    match s {
        Status::Ok => "ALL OK",
        Status::Warn => "WARNING",
        Status::Down => "DOWN",
        Status::Unknown => "NO DATA",
    }
}

/// The symbol of a status, drawn (no font glyphs): a tick in a circle, a
/// triangle with a bar, a cross in a square, a dash in a ring.
pub fn status_symbol(p: &Painter, r: Rect, s: Status) {
    let c = r.center();
    let rad = r.width().min(r.height()) / 2.0;
    let color = status_color(s);
    let w = (rad * 0.22).max(1.5);
    match s {
        Status::Ok => {
            p.circle_filled(c, rad, color);
            let ink = Stroke::new(w, BG);
            p.add(Shape::line(
                vec![
                    c + vec2(-0.45, 0.02) * rad,
                    c + vec2(-0.12, 0.35) * rad,
                    c + vec2(0.48, -0.32) * rad,
                ],
                ink,
            ));
        }
        Status::Warn => {
            let pts = vec![
                c + vec2(0.0, -1.0) * rad,
                c + vec2(1.0, 0.8) * rad,
                c + vec2(-1.0, 0.8) * rad,
            ];
            p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
            let ink = Stroke::new(w, BG);
            p.line_segment([c + vec2(0.0, -0.45) * rad, c + vec2(0.0, 0.22) * rad], ink);
            p.circle_filled(c + vec2(0.0, 0.5) * rad, w * 0.6, BG);
        }
        Status::Down => {
            let sq = Rect::from_center_size(c, vec2(rad * 1.8, rad * 1.8));
            p.rect_filled(sq, CornerRadius::same((rad * 0.3) as u8), color);
            let ink = Stroke::new(w, BG);
            let k = rad * 0.45;
            p.line_segment([c + vec2(-k, -k), c + vec2(k, k)], ink);
            p.line_segment([c + vec2(-k, k), c + vec2(k, -k)], ink);
        }
        Status::Unknown => {
            p.circle_stroke(c, rad - w / 2.0, Stroke::new(w, color));
            p.line_segment(
                [pos2(c.x - rad * 0.4, c.y), pos2(c.x + rad * 0.4, c.y)],
                Stroke::new(w, color),
            );
        }
    }
}

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();
    let mut v = Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = CARD;
    v.extreme_bg_color = Color32::from_rgb(10, 11, 14);
    v.faint_bg_color = Color32::from_rgb(28, 31, 39);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(38, 42, 52);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(52, 57, 70);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    v.window_corner_radius = CornerRadius::same(8);
    v.selection.bg_fill = Color32::from_rgb(37, 72, 140);
    style.visuals = v;
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size.y = 26.0;
    for (s, size, family) in [
        (TextStyle::Body, 14.0, FontFamily::Proportional),
        (TextStyle::Button, 14.0, FontFamily::Proportional),
        (TextStyle::Small, 11.5, FontFamily::Proportional),
        (TextStyle::Heading, 20.0, FontFamily::Proportional),
        (TextStyle::Monospace, 13.5, FontFamily::Monospace),
    ] {
        style.text_styles.insert(s, FontId::new(size, family));
    }
    ctx.set_style_of(egui::Theme::Dark, style);
    ctx.set_theme(egui::Theme::Dark);
}
