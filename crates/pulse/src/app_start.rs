//! The start screen: new profile (a venue or show), open, recently opened
//! and the demo. Same layout as the start screens of Show Master, Lantern
//! and Blueprint, in Pulse's teal.

use crate::app::App;
use crate::theme;
use egui::{
    pos2, Align, Color32, CornerRadius, Frame, Layout, Margin, Rect, RichText, Sense, Stroke,
    TextureHandle, Vec2,
};
use std::path::{Path, PathBuf};

/// The new-profile form of the start screen.
#[derive(Default)]
pub struct StartState {
    /// The "New profile" form is open.
    pub form: bool,
    pub name: String,
    /// The folder the profile file goes into.
    pub location: PathBuf,
    pub error: Option<String>,
    banner: Option<TextureHandle>,
}

const TEAL: Color32 = theme::BRAND;
const TEXT: Color32 = theme::TEXT;
const MUTED: Color32 = theme::MUTED;
const LINE: Color32 = Color32::from_rgb(44, 49, 58);
/// A card on the start screen: dark glass, a stroke that lights up on hover.
const GLASS: Color32 = Color32::from_rgba_premultiplied(14, 17, 23, 228);
/// Most profiles shown under "Recent".
const RECENT: usize = 8;

/// A profile name usable as a file name.
pub fn clean_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .trim()
        .to_string()
}

impl StartState {
    /// The file a new profile gets: `<location>/<name>.pulse`.
    pub fn profile_file(&self) -> PathBuf {
        let n = clean_name(&self.name);
        self.location
            .join(format!("{n}.{}", pulse_core::profile::EXTENSION))
    }

    /// Why the form cannot be sent yet.
    pub fn problem(&self) -> Option<String> {
        if clean_name(&self.name).is_empty() {
            return Some("Give the profile a name.".into());
        }
        if self.location.as_os_str().is_empty() {
            return Some("Choose where the profile goes.".into());
        }
        let f = self.profile_file();
        if f.exists() {
            return Some(format!("{} exists already.", f.display()));
        }
        None
    }
}

impl App {
    pub(crate) fn open_new_form(&mut self) {
        self.start.form = true;
        self.start.error = None;
        self.start.name.clear();
        if self.start.location.as_os_str().is_empty() {
            self.start.location = crate::settings::default_profile_dir();
        }
    }

    /// Make the profile of the form, saved at once, and start watching it.
    pub fn create_profile(&mut self) {
        if let Some(p) = self.start.problem() {
            self.start.error = Some(p);
            return;
        }
        let path = self.start.profile_file();
        let profile = pulse_core::model::Profile {
            name: self.start.name.trim().to_string(),
            ..Default::default()
        };
        if let Err(e) = pulse_core::profile::save(&profile, &path) {
            self.start.error = Some(format!("Could not save {}: {e}", path.display()));
            return;
        }
        self.settings.remember(&path);
        self.start.form = false;
        self.start.error = None;
        self.start_document(profile, Some(path));
        // An empty profile: straight to the inventory.
        self.page = crate::app::Page::Inventory;
    }

    fn backdrop(&mut self, ui: &mut egui::Ui) -> Rect {
        let rect = ui.max_rect();
        let tex = self.start.banner.get_or_insert_with(|| {
            ui.ctx().load_texture(
                "pulse_banner",
                banner_image(1280, 720),
                egui::TextureOptions::LINEAR,
            )
        });
        // Cover: fill the window, crop what is too much; keep the right side.
        let [tw, th] = tex.size().map(|v| v as f32);
        let scale = (rect.width() / tw).max(rect.height() / th);
        let size = Vec2::new(tw * scale, th * scale);
        let min = pos2(rect.right() - size.x, rect.center().y - size.y / 2.0);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let p = ui.painter();
        p.rect_filled(rect, 0.0, theme::BG);
        p.image(tex.id(), Rect::from_min_size(min, size), uv, Color32::WHITE);
        rect
    }

    pub(crate) fn start_page(&mut self, ui: &mut egui::Ui) {
        let rect = self.backdrop(ui);
        let pad = (rect.width() * 0.06).clamp(28.0, 96.0);
        let inner = rect.shrink2(Vec2::new(pad, pad * 0.8));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::top_down(Align::Min)),
        );
        let ui = &mut child;

        ui.add_space((inner.height() * 0.05).min(36.0));
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(46.0), Sense::hover());
            pulse_mark(ui.painter(), r);
            ui.add_space(8.0);
            ui.label(RichText::new("Pulse").size(46.0).strong().color(TEXT));
        });
        ui.add_space(2.0);
        ui.label(
            RichText::new("Is every device on the show network healthy?")
                .size(18.0)
                .color(Color32::from_rgb(196, 200, 210)),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "Version {}  ·  Ping, ports and protocols  ·  One traffic light with reasons  ·  Read-only",
                env!("CARGO_PKG_VERSION")
            ))
            .size(12.5)
            .color(MUTED),
        );
        ui.add_space((inner.height() * 0.06).clamp(16.0, 48.0));

        if self.start.form {
            self.new_form(ui);
        } else {
            if let Some(e) = self.start.error.clone() {
                notice(ui, theme::ERROR, &e);
                ui.add_space(10.0);
            }
            self.start_home(ui);
        }

        let y = rect.bottom() - pad * 0.45;
        ui.painter().text(
            pos2(inner.left(), y),
            egui::Align2::LEFT_CENTER,
            "Drop a .pulse file anywhere to open it",
            egui::FontId::proportional(12.5),
            MUTED,
        );
    }

    fn start_home(&mut self, ui: &mut egui::Ui) {
        let gap = 16.0;
        let w = ((ui.available_width() - 2.0 * gap) / 3.0).clamp(190.0, 300.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            if action_card(
                ui,
                w,
                Icon::New,
                "New profile",
                "A venue or show: the devices Pulse watches there.",
                true,
            )
            .clicked()
            {
                self.open_new_form();
            }
            if action_card(
                ui,
                w,
                Icon::Open,
                "Open profile",
                "A .pulse file from this computer, a USB stick or a shared folder.",
                false,
            )
            .clicked()
            {
                self.open(None);
            }
            if action_card(
                ui,
                w,
                Icon::Demo,
                "Try the demo",
                "An invented venue with simulated devices; now and then something goes wrong.",
                false,
            )
            .clicked()
            {
                self.open_demo();
            }
        });

        let recent: Vec<PathBuf> = self.settings.recent.iter().take(RECENT).cloned().collect();
        if recent.is_empty() {
            return;
        }
        ui.add_space(28.0);
        let width = 3.0 * w + 2.0 * gap;
        ui.horizontal(|ui| {
            ui.set_width(width);
            ui.label(
                RichText::new("RECENT PROFILES")
                    .size(11.5)
                    .strong()
                    .color(MUTED),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Clear list").size(11.5).color(MUTED))
                            .frame(false),
                    )
                    .clicked()
                {
                    self.settings.recent.clear();
                    self.settings.save();
                }
            });
        });
        ui.add_space(6.0);
        let mut open = None;
        Frame::new()
            .fill(GLASS)
            .stroke(Stroke::new(1.0, LINE))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::same(6))
            .show(ui, |ui| {
                ui.set_width(width - 12.0);
                let rows = ((ui.available_height() - 40.0) / 44.0).floor().max(1.0) as usize;
                for p in recent.iter().take(rows) {
                    if recent_row(ui, p).clicked() {
                        open = Some(p.clone());
                    }
                }
            });
        if let Some(p) = open {
            if p.is_file() {
                self.start.error = None;
                self.open(Some(p));
            } else {
                self.start.error = Some(format!("Not there any more: {}", p.display()));
            }
        }
    }

    fn new_form(&mut self, ui: &mut egui::Ui) {
        let dialogs = self.dialogs;
        let mut go = false;
        form_card(ui, |ui| {
            ui.label(RichText::new("New profile").size(24.0).strong().color(TEXT));
            ui.label(
                RichText::new(
                    "A profile holds the devices of one venue or show. Make one per venue and switch \
                     between them; copy the file to take it to another computer.",
                )
                .color(MUTED),
            );
            ui.add_space(16.0);
            let st = &mut self.start;
            egui::Grid::new("pulse_new_profile")
                .num_columns(2)
                .spacing([16.0, 12.0])
                .min_col_width(110.0)
                .show(ui, |ui| {
                    let l = ui.label(RichText::new("Venue or show").color(MUTED));
                    let r = ui
                        .add(
                            egui::TextEdit::singleline(&mut st.name)
                                .hint_text("e.g. School hall")
                                .desired_width(360.0),
                        )
                        .labelled_by(l.id);
                    if st.name.is_empty() && !r.has_focus() && !r.lost_focus() {
                        r.request_focus();
                    }
                    ui.end_row();
                    ui.label(RichText::new("Location").color(MUTED));
                    ui.horizontal(|ui| {
                        path_field(ui, &st.location, "No place chosen");
                        if ui.button("Choose…").clicked() && dialogs {
                            let mut d = rfd::FileDialog::new();
                            if st.location.is_dir() {
                                d = d.set_directory(&st.location);
                            }
                            if let Some(p) = d.pick_folder() {
                                st.location = p;
                            }
                        }
                    });
                    ui.end_row();
                });
            ui.add_space(10.0);
            if !clean_name(&st.name).is_empty() && !st.location.as_os_str().is_empty() {
                ui.label(
                    RichText::new(format!("Creates  {}", st.profile_file().display()))
                        .size(12.5)
                        .color(MUTED),
                );
            }
            let problem = st.problem();
            ui.add_space(14.0);
            if let Some(e) = &st.error {
                ui.label(RichText::new(e).color(theme::ERROR));
                ui.add_space(6.0);
            }
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Cancel").size(15.0))
                            .corner_radius(8)
                            .min_size(Vec2::new(110.0, 38.0)),
                    )
                    .clicked()
                {
                    st.form = false;
                    st.error = None;
                }
                ui.add_space(6.0);
                go = ui
                    .add_enabled(
                        problem.is_none(),
                        egui::Button::new(
                            RichText::new("Create profile")
                                .size(15.0)
                                .strong()
                                .color(theme::ON_BRAND),
                        )
                        .fill(TEAL)
                        .corner_radius(8)
                        .min_size(Vec2::new(180.0, 38.0)),
                    )
                    .clicked();
                if let Some(p) = &problem {
                    ui.add_space(8.0);
                    ui.label(RichText::new(p).color(MUTED));
                }
            });
            go |= problem.is_none() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                st.form = false;
                st.error = None;
            }
        });
        if go {
            self.create_profile();
        }
    }
}

// ------------------------------------------------------------ widgets

fn notice(ui: &mut egui::Ui, color: Color32, text: &str) {
    Frame::new()
        .fill(color.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, color))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(TEXT));
        });
}

#[derive(Clone, Copy)]
enum Icon {
    New,
    Open,
    Demo,
}

fn action_card(
    ui: &mut egui::Ui,
    w: f32,
    icon: Icon,
    title: &str,
    about: &str,
    primary: bool,
) -> egui::Response {
    let h = 168.0;
    let (rect, r) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, title));
    let hot = r.hovered();
    let p = ui.painter();
    let stroke = if primary {
        TEAL.gamma_multiply(if hot { 1.0 } else { 0.7 })
    } else if hot {
        Color32::from_rgb(90, 98, 112)
    } else {
        LINE
    };
    p.rect(
        rect,
        CornerRadius::same(14),
        if hot {
            Color32::from_rgba_premultiplied(24, 28, 36, 236)
        } else {
            GLASS
        },
        Stroke::new(if primary { 1.5 } else { 1.0 }, stroke),
        egui::StrokeKind::Inside,
    );
    let tile = Rect::from_min_size(rect.min + Vec2::new(20.0, 20.0), Vec2::splat(40.0));
    p.rect_filled(
        tile,
        CornerRadius::same(10),
        if primary {
            TEAL
        } else {
            Color32::from_rgb(40, 45, 54)
        },
    );
    let ink = if primary { theme::ON_BRAND } else { TEXT };
    draw_icon(p, tile.shrink(11.0), icon, ink);
    p.text(
        rect.min + Vec2::new(20.0, 78.0),
        egui::Align2::LEFT_TOP,
        title,
        egui::FontId::proportional(18.0),
        TEXT,
    );
    let galley = p.layout(
        about.to_string(),
        egui::FontId::proportional(13.0),
        MUTED,
        w - 40.0,
    );
    p.galley(rect.min + Vec2::new(20.0, 106.0), galley, MUTED);
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn draw_icon(p: &egui::Painter, r: Rect, icon: Icon, ink: Color32) {
    let s = Stroke::new(2.2, ink);
    match icon {
        Icon::New => {
            p.line_segment([r.center_top(), r.center_bottom()], s);
            p.line_segment([r.left_center(), r.right_center()], s);
        }
        Icon::Open => {
            let body = Rect::from_min_max(r.min + Vec2::new(0.0, 4.0), r.max);
            p.rect_stroke(body, CornerRadius::same(2), s, egui::StrokeKind::Middle);
            p.line_segment(
                [r.min + Vec2::new(0.0, 4.0), r.min + Vec2::new(6.0, 0.0)],
                s,
            );
            p.line_segment(
                [r.min + Vec2::new(6.0, 0.0), r.min + Vec2::new(10.0, 4.0)],
                s,
            );
        }
        Icon::Demo => {
            // A small pulse.
            let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
            p.add(egui::Shape::line(
                vec![
                    at(0.0, 0.6),
                    at(0.3, 0.6),
                    at(0.42, 0.25),
                    at(0.56, 0.95),
                    at(0.7, 0.0),
                    at(0.8, 0.6),
                    at(1.0, 0.6),
                ],
                s,
            ));
        }
    }
}

/// The app icon: a teal pulse on a dark tile (as `assets/icon.svg`).
pub fn pulse_mark(p: &egui::Painter, r: Rect) {
    p.rect(
        r,
        CornerRadius::same(10),
        Color32::from_rgb(20, 23, 28),
        Stroke::new(1.0, Color32::from_rgb(42, 47, 56)),
        egui::StrokeKind::Inside,
    );
    let u = r.width() / 1024.0;
    let at = |x: f32, y: f32| pos2(r.left() + x * u, r.top() + y * u);
    let line = vec![
        at(192.0, 540.0),
        at(384.0, 540.0),
        at(440.0, 400.0),
        at(516.0, 700.0),
        at(584.0, 300.0),
        at(640.0, 540.0),
        at(832.0, 540.0),
    ];
    p.add(egui::Shape::line(
        line.clone(),
        Stroke::new(70.0 * u, TEAL.gamma_multiply(0.18)),
    ));
    p.add(egui::Shape::line(line, Stroke::new(34.0 * u, TEAL)));
    p.circle_filled(at(832.0, 540.0), 34.0 * u, Color32::from_rgb(230, 255, 251));
}

fn recent_row(ui: &mut egui::Ui, p: &Path) -> egui::Response {
    let w = ui.available_width();
    let (rect, r) = ui.allocate_exact_size(Vec2::new(w, 44.0), Sense::click());
    let name = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    let there = p.is_file();
    let painter = ui.painter();
    if r.hovered() {
        painter.rect_filled(rect, CornerRadius::same(8), Color32::from_rgb(30, 35, 44));
    }
    let dot = pos2(rect.left() + 18.0, rect.center().y);
    painter.circle_filled(dot, 4.0, if there { TEAL } else { LINE });
    painter.text(
        pos2(rect.left() + 34.0, rect.center().y - 8.0),
        egui::Align2::LEFT_CENTER,
        &name,
        egui::FontId::proportional(15.0),
        if there { TEXT } else { MUTED },
    );
    let place = p
        .parent()
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    let place = if there {
        place
    } else {
        format!("Missing - {place}")
    };
    painter.text(
        pos2(rect.left() + 34.0, rect.center().y + 9.0),
        egui::Align2::LEFT_CENTER,
        place,
        egui::FontId::proportional(12.0),
        MUTED,
    );
    let when = std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .map(|t| {
            chrono::DateTime::<chrono::Local>::from(t)
                .format("%d %b %Y, %H:%M")
                .to_string()
        })
        .unwrap_or_default();
    painter.text(
        pos2(rect.right() - 14.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        when,
        egui::FontId::proportional(12.0),
        MUTED,
    );
    r.on_hover_text(p.display().to_string())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn path_field(ui: &mut egui::Ui, p: &Path, empty: &str) {
    let (text, color) = if p.as_os_str().is_empty() {
        (empty.to_string(), MUTED)
    } else {
        (p.display().to_string(), TEXT)
    };
    Frame::new()
        .fill(theme::BG)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.set_width(300.0);
            ui.add(egui::Label::new(RichText::new(text).color(color)).truncate());
        })
        .response
        .on_hover_text(p.display().to_string());
}

fn form_card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .fill(Color32::from_rgba_premultiplied(16, 19, 25, 242))
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::symmetric(28, 24))
        .show(ui, |ui| {
            ui.set_max_width(640.0);
            add(ui);
        });
}

/// The backdrop: a dark monitor screen, its grid fading in to the right, a
/// teal pulse trace with a glow running across it.
pub fn banner_image(w: usize, h: usize) -> egui::ColorImage {
    fn lin(c: [f32; 3]) -> [f32; 3] {
        c.map(|v| (v / 255.0).powf(2.2))
    }
    fn smooth(a: f32, b: f32, x: f32) -> f32 {
        let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
    /// Height of the trace (0..1 of the height) at x (0..1 of the width).
    fn trace(x: f32) -> f32 {
        // Beats at these places: a small bump, a spike down, a spike up.
        let mut y = 0.56;
        for b in [0.48f32, 0.74, 0.97] {
            let d = x - b;
            y += 0.05 * (-(d + 0.035).powi(2) / 0.0002).exp();
            y += 0.13 * (-(d + 0.006).powi(2) / 0.00002).exp();
            y -= 0.30 * (-(d - 0.008).powi(2) / 0.00002).exp();
            y += 0.04 * (-(d - 0.06).powi(2) / 0.0006).exp();
        }
        y
    }
    let top = lin([10.0, 18.0, 20.0]);
    let bottom = lin([4.0, 6.0, 7.0]);
    let screen = lin([10.0, 40.0, 40.0]);
    let teal = lin([45.0, 212.0, 191.0]);
    let ink = lin([220.0, 255.0, 250.0]);
    let a = w as f32 / h as f32;
    let samples: Vec<f32> = (0..w).map(|px| trace(px as f32 / w as f32)).collect();
    let mut pixels = Vec::with_capacity(w * h);
    for py in 0..h {
        let v = py as f32 / h as f32;
        for px in 0..w {
            let u = px as f32 / w as f32;
            let x = u * a;
            let base = smooth(0.0, 1.0, v);
            let mut c = [0.0f32; 3];
            for k in 0..3 {
                c[k] = top[k] + (bottom[k] - top[k]) * base;
            }
            let glass = smooth(0.30, 0.75, u) * (1.0 - 0.5 * smooth(0.6, 1.0, v));
            for k in 0..3 {
                c[k] += screen[k] * glass * 0.6;
            }
            let fine = 0.05;
            let gx = ((x / fine).fract() - 0.5).abs();
            let gy = ((v / fine).fract() - 0.5).abs();
            let grid = smooth(0.47, 0.5, gx.max(gy)) * 0.22;
            for k in 0..3 {
                c[k] += teal[k] * grid * glass * 0.35;
            }
            // Distance to the trace near this column (a few columns around).
            let mut d = f32::MAX;
            let lo = px.saturating_sub(6);
            let hi = (px + 6).min(w - 1);
            for (i, s) in samples.iter().enumerate().take(hi + 1).skip(lo) {
                let dx = (i as f32 - px as f32) / h as f32;
                let dy = s - v;
                d = d.min((dx * dx + dy * dy).sqrt());
            }
            // Fades in from the left; the newest part (right) is brightest.
            let reach = smooth(0.28, 0.6, u);
            let core = smooth(0.0028, 0.001, d);
            let glow = (-(d / 0.012).powi(2)).exp() * 0.35 + (-d / 0.07).exp() * 0.08;
            for k in 0..3 {
                c[k] += (ink[k] * core * 0.8 + teal[k] * glow) * reach;
            }
            let shade = 0.35 + 0.65 * smooth(0.0, 0.55, u);
            let rgb = c.map(|v| ((v * shade).max(0.0).powf(1.0 / 2.2) * 255.0).min(255.0) as u8);
            pixels.push(Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
        }
    }
    egui::ColorImage::new([w, h], pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_files() {
        assert_eq!(clean_name("  Aula: 2027/28 "), "Aula- 2027-28");
        let st = StartState {
            name: "School hall".into(),
            location: PathBuf::from("/tmp/x"),
            ..Default::default()
        };
        assert_eq!(st.profile_file(), PathBuf::from("/tmp/x/School hall.pulse"));
        assert!(StartState::default().problem().is_some());
    }
}
