//! Show mode: a calm view during the show. The overall light large, only
//! the devices with a problem as big tiles (the unacknowledged ones blink
//! slowly), everything else as one quiet line per area. No dialogs, an
//! optional alert sound (off by default). And the traffic light as a window
//! of its own for a second monitor.

use crate::app::{clock, now_ms, App};
use crate::theme;
use egui::{vec2, Color32, RichText, Sense, Stroke};
use pulse_core::status::{Ms, Overview};
use pulse_core::{Id, Status};
use std::collections::BTreeSet;

#[derive(Default)]
pub struct ShowState {
    /// The light window is open.
    pub light_window: bool,
    /// What the light was last frame (for the alert sound).
    pub last_light: Option<Status>,
    pub last_problems: BTreeSet<Id>,
}

/// Unacknowledged devices that colour the light.
fn problems(o: &Overview) -> BTreeSet<Id> {
    o.reasons.iter().map(|r| r.device).collect()
}

/// Slow blink: 0.35 ..= 1.0 over two seconds.
fn blink(ui: &egui::Ui) -> f32 {
    let t = ui.input(|i| i.time) as f32;
    0.675 + 0.325 * (t * std::f32::consts::PI).cos()
}

/// The overall light filling `ui`: symbol, word, the first reasons.
pub fn big_light(ui: &mut egui::Ui, o: Option<&Overview>, title: &str) {
    let rect = ui.max_rect();
    let light = o.map(|o| o.light).unwrap_or(Status::Unknown);
    let color = theme::status_color(light);
    let p = ui.painter();
    p.rect_filled(rect, 0.0, theme::BG);
    p.rect_filled(rect.shrink(6.0), 18.0, color.gamma_multiply(0.14));
    p.rect_stroke(
        rect.shrink(6.0),
        18.0,
        Stroke::new(3.0, color.gamma_multiply(0.9)),
        egui::StrokeKind::Inside,
    );
    let size = (rect.height() * 0.42).min(rect.width() * 0.35).max(40.0);
    let center = egui::pos2(rect.center().x, rect.top() + rect.height() * 0.36);
    let sym = egui::Rect::from_center_size(center, vec2(size, size));
    theme::status_symbol(p, sym, light);
    let word_y = sym.bottom() + size * 0.18;
    p.text(
        egui::pos2(rect.center().x, word_y),
        egui::Align2::CENTER_TOP,
        theme::light_word(light),
        egui::FontId::proportional((size * 0.34).max(22.0)),
        color,
    );
    let mut y = word_y + (size * 0.34).max(22.0) + 18.0;
    let lines: Vec<String> = match o {
        Some(o) if !o.reasons.is_empty() => o
            .reasons
            .iter()
            .take(4)
            .map(|r| r.text(&clock(r.since)))
            .collect(),
        Some(o) if o.light == Status::Ok => vec!["Every device answers as expected.".into()],
        _ => vec!["Waiting for the first results.".into()],
    };
    for l in lines {
        p.text(
            egui::pos2(rect.center().x, y),
            egui::Align2::CENTER_TOP,
            l,
            egui::FontId::proportional((size * 0.11).clamp(15.0, 30.0)),
            theme::TEXT,
        );
        y += (size * 0.11).clamp(15.0, 30.0) + 8.0;
    }
    p.text(
        rect.left_bottom() + vec2(20.0, -18.0),
        egui::Align2::LEFT_BOTTOM,
        format!("Pulse · {title}"),
        egui::FontId::proportional(13.0),
        theme::MUTED,
    );
    let r = ui.allocate_rect(rect, Sense::hover());
    r.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Image,
            true,
            format!("Light window: {}", light.label()),
        )
    });
}

impl App {
    /// Sound when the light gets worse or a new device has a problem
    /// (show mode, only when switched on). Called every frame.
    pub(crate) fn show_alerts(&mut self) {
        let Some(o) = self.overview() else {
            self.show.last_light = None;
            self.show.last_problems.clear();
            return;
        };
        let now = problems(&o);
        let worse = self
            .show
            .last_light
            .is_some_and(|l| o.light.rank() > l.rank());
        let new = now.iter().any(|id| !self.show.last_problems.contains(id));
        let first = self.show.last_light.is_none();
        if !first && (worse || new) && self.mode == crate::app::Mode::Show && self.settings.sound {
            crate::platform::alert();
        }
        self.show.last_light = Some(o.light);
        self.show.last_problems = now;
    }

    /// The light window on its own viewport (or an inner window where viewports are embedded).
    pub(crate) fn light_window(&mut self, ctx: &egui::Context) {
        if !self.show.light_window {
            return;
        }
        let o = self.overview();
        let title = self.doc.as_ref().map(|d| d.title()).unwrap_or_default();
        let mut close = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("pulse_light"),
            egui::ViewportBuilder::default()
                .with_title(format!("Pulse light · {title}"))
                .with_inner_size([900.0, 600.0])
                .with_min_inner_size([320.0, 220.0]),
            |ui, class| {
                if class == egui::ViewportClass::EmbeddedWindow {
                    ui.set_min_size(vec2(520.0, 360.0));
                }
                big_light(ui, o.as_ref(), &title);
                if ui.input(|i| i.viewport().close_requested()) {
                    close = true;
                }
                if ui.input(|i| i.key_pressed(egui::Key::F11) || i.key_pressed(egui::Key::F)) {
                    let full = ui.input(|i| i.viewport().fullscreen.unwrap_or(false));
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
                }
            },
        );
        if close {
            self.show.light_window = false;
        }
        if self.show.light_window {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    pub(crate) fn show_page(&mut self, ui: &mut egui::Ui) {
        let Some(o) = self.overview() else { return };
        let now = now_ms();
        let tiles = self.tiles();
        // The light, large.
        let color = theme::status_color(o.light);
        egui::Frame::new()
            .fill(color.gamma_multiply(0.10))
            .stroke(Stroke::new(2.0, color.gamma_multiply(0.85)))
            .corner_radius(14)
            .inner_margin(egui::Margin::symmetric(22, 16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (r, resp) = ui.allocate_exact_size(vec2(84.0, 84.0), Sense::hover());
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::Role::Image,
                            true,
                            format!("Show: {}", o.light.label()),
                        )
                    });
                    theme::status_symbol(ui.painter(), r, o.light);
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(theme::light_word(o.light))
                                .size(44.0)
                                .strong()
                                .color(color),
                        );
                        match o.reasons.first() {
                            Some(r) => {
                                ui.label(RichText::new(r.text(&clock(r.since))).size(20.0));
                            }
                            None => {
                                ui.label(
                                    RichText::new(if o.light == Status::Unknown {
                                        "Waiting for the first results."
                                    } else {
                                        "Every device answers as expected."
                                    })
                                    .size(18.0)
                                    .color(theme::MUTED),
                                );
                            }
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        let mut sound = self.settings.sound;
                        if ui
                            .checkbox(&mut sound, "Sound on new problems")
                            .on_hover_text("A short alert sound when the light gets worse or a device gets a problem. Off by default.")
                            .changed()
                        {
                            self.settings.sound = sound;
                            self.settings.save();
                        }
                    });
                });
            });
        ui.add_space(12.0);

        // The problems, large.
        let mut bad: Vec<_> = tiles
            .iter()
            .filter(|t| t.enabled && matches!(t.status, Status::Warn | Status::Down))
            .collect();
        bad.sort_by(|a, b| {
            a.acked
                .cmp(&b.acked)
                .then(b.status.rank().cmp(&a.status.rank()))
                .then(a.since.cmp(&b.since))
        });
        let mut click = None;
        if bad.is_empty() {
            ui.label(RichText::new("No problems.").size(18.0).color(theme::MUTED));
        } else {
            let pulse = blink(ui);
            let w = 340.0_f32.min(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                for t in &bad {
                    if show_tile(ui, t, w, if t.acked { 1.0 } else { pulse }, now).clicked() {
                        click = Some(t.id);
                    }
                }
            });
            if bad.iter().any(|t| !t.acked) {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(80));
            }
        }
        if let Some(id) = click {
            self.selected = if self.selected == Some(id) {
                None
            } else {
                Some(id)
            };
        }
        ui.add_space(16.0);

        // Everything else, quietly.
        ui.label(RichText::new("AREAS").small().color(theme::MUTED));
        for a in &o.areas {
            let in_area: Vec<_> = tiles
                .iter()
                .filter(|t| t.area == a.area && t.enabled)
                .collect();
            let ok = in_area.iter().filter(|t| t.status == Status::Ok).count();
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                theme::status_symbol(ui.painter(), r, a.light);
                ui.label(RichText::new(&a.area).strong());
                ui.label(
                    RichText::new(format!("{ok} of {} OK", in_area.len())).color(theme::MUTED),
                );
            });
        }
    }
}

/// A big tile for a device with a problem; `alpha` makes it blink.
fn show_tile(
    ui: &mut egui::Ui,
    t: &crate::app_monitor::Tile,
    w: f32,
    alpha: f32,
    now: Ms,
) -> egui::Response {
    let color = theme::status_color(t.status);
    let (rect, r) = ui.allocate_exact_size(vec2(w, 96.0), Sense::click());
    r.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!(
                "Problem {}: {}{}",
                t.name,
                t.status.label(),
                if t.acked { " (acknowledged)" } else { "" }
            ),
        )
    });
    let p = ui.painter();
    let fill = if t.acked {
        theme::CARD
    } else {
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (40.0 * alpha) as u8)
    };
    p.rect_filled(rect, 12.0, fill);
    p.rect_stroke(
        rect,
        12.0,
        Stroke::new(
            if t.acked { 1.0 } else { 2.5 },
            color.gamma_multiply(if t.acked { 0.4 } else { alpha }),
        ),
        egui::StrokeKind::Inside,
    );
    let sym = egui::Rect::from_min_size(rect.min + vec2(14.0, 14.0), vec2(30.0, 30.0));
    theme::status_symbol(p, sym, t.status);
    p.text(
        rect.min + vec2(56.0, 12.0),
        egui::Align2::LEFT_TOP,
        &t.name,
        egui::FontId::proportional(20.0),
        theme::TEXT,
    );
    p.text(
        rect.min + vec2(56.0, 40.0),
        egui::Align2::LEFT_TOP,
        if t.cause.is_empty() {
            t.status.label()
        } else {
            &t.cause
        },
        egui::FontId::proportional(14.5),
        color,
    );
    p.text(
        rect.min + vec2(56.0, 66.0),
        egui::Align2::LEFT_TOP,
        format!(
            "{} · since {}{}",
            t.area,
            clock(t.since),
            if t.acked { " · acknowledged" } else { "" }
        ),
        egui::FontId::proportional(12.5),
        theme::MUTED,
    );
    let _ = now;
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}
