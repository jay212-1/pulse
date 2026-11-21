//! The monitor page: the overall light with its reasons on top, then a
//! tile per device, grouped by area (or sorted by status or name), with a
//! search, "problems only" and the area chosen in the navigation.

use crate::app::{ago, clock, now_ms, App, Sort};
use crate::theme;
use egui::{pos2, vec2, Color32, CornerRadius, Rect, RichText, Sense, Stroke};
use pulse_core::model::{Criticality, Id};
use pulse_core::status::{Ms, Overview};
use pulse_core::{Metric, Status};

/// What a tile shows (read from the monitor in one go).
#[derive(Clone, Debug)]
pub struct Tile {
    pub id: Id,
    pub name: String,
    pub area: String,
    pub address: String,
    pub criticality: Criticality,
    pub enabled: bool,
    pub status: Status,
    pub cause: String,
    pub since: Ms,
    pub last_seen: Option<Ms>,
    pub acked: bool,
    /// The key measurement: "0.4 ms".
    pub metric: Option<String>,
}

pub const TILE_W: f32 = 232.0;
pub const TILE_H: f32 = 96.0;
const GAP: f32 = 10.0;

impl App {
    /// Tiles of all devices, in inventory order.
    pub fn tiles(&self) -> Vec<Tile> {
        let Some(m) = &self.monitor else {
            return Vec::new();
        };
        m.with(|s| {
            s.profile
                .devices
                .iter()
                .map(|d| {
                    let st = s.engine.device(d.id);
                    let metric = st.and_then(|st| {
                        d.checks
                            .iter()
                            .filter(|c| c.enabled)
                            .filter_map(|c| st.checks.get(&c.id)?.last.as_ref())
                            .find_map(|r| {
                                [Metric::LatencyMs, Metric::ConnectMs]
                                    .into_iter()
                                    .find_map(|k| r.metric(k).map(|v| k.format(v)))
                            })
                    });
                    Tile {
                        id: d.id,
                        name: d.name.clone(),
                        area: d.area.clone(),
                        address: d.address.clone(),
                        criticality: d.criticality,
                        enabled: d.enabled,
                        status: st.map(|x| x.status).unwrap_or_default(),
                        cause: st.map(|x| x.cause.clone()).unwrap_or_default(),
                        since: st.map(|x| x.since).unwrap_or(0),
                        last_seen: st.and_then(|x| x.last_seen),
                        acked: st.is_some_and(|x| x.ack.is_some()),
                        metric,
                    }
                })
                .collect()
        })
    }

    pub fn overview(&self) -> Option<Overview> {
        self.monitor
            .as_ref()
            .map(|m| m.with(|s| s.overview.clone()))
    }

    pub(crate) fn monitor_page(&mut self, ui: &mut egui::Ui) {
        let Some(o) = self.overview() else {
            ui.label(RichText::new("Monitoring is not running.").color(theme::ERROR));
            return;
        };
        let now = now_ms();
        self.light_header(ui, &o, now);
        ui.add_space(12.0);
        self.tile_filter(ui);
        ui.add_space(8.0);

        let mut tiles = self.tiles();
        let f = &self.tiles;
        let q = f.text.trim().to_lowercase();
        tiles.retain(|t| {
            (q.is_empty()
                || t.name.to_lowercase().contains(&q)
                || t.address.to_lowercase().contains(&q)
                || t.area.to_lowercase().contains(&q))
                && (!f.problems_only || matches!(t.status, Status::Warn | Status::Down))
                && f.area.as_ref().is_none_or(|a| *a == t.area)
        });
        if tiles.is_empty() {
            let empty = self.profile().is_some_and(|p| p.devices.is_empty());
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                if empty {
                    ui.label(
                        RichText::new("No devices yet.")
                            .size(16.0)
                            .color(theme::MUTED),
                    );
                    if ui.button("Add devices in the inventory").clicked() {
                        self.page = crate::app::Page::Inventory;
                    }
                } else if f.problems_only {
                    ui.label(RichText::new("No problems.").size(16.0).color(theme::OK));
                } else {
                    ui.label(
                        RichText::new("Nothing matches the filter.")
                            .size(16.0)
                            .color(theme::MUTED),
                    );
                }
            });
            return;
        }
        let sort = self.tiles.sort;
        let areas: Vec<(String, Status)> =
            o.areas.iter().map(|a| (a.area.clone(), a.light)).collect();
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match sort {
                Sort::Area => {
                    for (area, light) in &areas {
                        let group: Vec<&Tile> = tiles.iter().filter(|t| t.area == *area).collect();
                        if group.is_empty() {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                            theme::status_symbol(ui.painter(), r, *light);
                            ui.label(
                                RichText::new(area.to_uppercase())
                                    .strong()
                                    .color(theme::TEXT),
                            );
                            ui.label(
                                RichText::new(light.label())
                                    .small()
                                    .color(theme::status_color(*light)),
                            );
                        });
                        ui.add_space(4.0);
                        if let Some(id) = self.tile_grid(ui, &group, now) {
                            clicked = Some(id);
                        }
                        ui.add_space(14.0);
                    }
                }
                Sort::Status | Sort::Name => {
                    let mut list: Vec<&Tile> = tiles.iter().collect();
                    if sort == Sort::Status {
                        list.sort_by(|a, b| {
                            b.status
                                .rank()
                                .cmp(&a.status.rank())
                                .then(a.criticality.cmp(&b.criticality))
                                .then(a.since.cmp(&b.since))
                        });
                    } else {
                        list.sort_by_key(|t| t.name.to_lowercase());
                    }
                    if let Some(id) = self.tile_grid(ui, &list, now) {
                        clicked = Some(id);
                    }
                }
            });
        if let Some(id) = clicked {
            self.selected = if self.selected == Some(id) {
                None
            } else {
                Some(id)
            };
            self.ack_note.clear();
        }
    }

    /// The overall light: symbol and word, the reasons, the counts.
    fn light_header(&mut self, ui: &mut egui::Ui, o: &Overview, now: Ms) {
        let color = theme::status_color(o.light);
        let frame = egui::Frame::new()
            .fill(color.gamma_multiply(0.10))
            .stroke(Stroke::new(1.5, color.gamma_multiply(0.8)))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(egui::Margin::symmetric(18, 14));
        let headline = o.reasons.first().map(|r| r.text(&clock(r.since)));
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, resp) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::Role::Image,
                        true,
                        format!(
                            "Overall: {}{}",
                            o.light.label(),
                            headline
                                .as_ref()
                                .map(|h| format!(" – {h}"))
                                .unwrap_or_default()
                        ),
                    )
                });
                theme::status_symbol(ui.painter(), r, o.light);
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(theme::light_word(o.light))
                            .size(30.0)
                            .strong()
                            .color(color),
                    );
                    match &headline {
                        Some(h) => {
                            ui.label(RichText::new(h).size(16.0).color(theme::TEXT));
                        }
                        None if o.light == Status::Unknown => {
                            ui.label(
                                RichText::new("Waiting for the first results.").color(theme::MUTED),
                            );
                        }
                        None if !o.acked.is_empty() => {
                            ui.label(
                                RichText::new("No problems apart from the acknowledged ones.")
                                    .color(theme::MUTED),
                            );
                        }
                        None => {
                            ui.label(
                                RichText::new("Every device answers as expected.")
                                    .color(theme::MUTED),
                            );
                        }
                    }
                    let more = o.reasons.len().saturating_sub(1);
                    for r in o.reasons.iter().skip(1).take(3) {
                        ui.label(
                            RichText::new(r.text(&clock(r.since)))
                                .size(13.0)
                                .color(theme::MUTED),
                        );
                    }
                    if more > 3 {
                        ui.label(
                            RichText::new(format!("and {} more", more - 3))
                                .size(13.0)
                                .color(theme::MUTED),
                        );
                    }
                    if !o.acked.is_empty() {
                        let names: Vec<_> = o.acked.iter().map(|r| r.name.as_str()).collect();
                        ui.label(
                            RichText::new(format!(
                                "Acknowledged (not counted): {}",
                                names.join(", ")
                            ))
                            .size(12.5)
                            .color(theme::MUTED),
                        );
                    }
                });
                ui.add_space((ui.available_width() - 130.0).max(0.0));
                {
                    let c = o.counts;
                    ui.vertical(|ui| {
                        for (n, s) in [
                            (c.ok, Status::Ok),
                            (c.warn, Status::Warn),
                            (c.down, Status::Down),
                            (c.unknown, Status::Unknown),
                        ] {
                            ui.horizontal(|ui| {
                                let (r, _) =
                                    ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                                theme::status_symbol(ui.painter(), r, s);
                                ui.label(
                                    RichText::new(format!("{n} {}", s.label()))
                                        .size(12.5)
                                        .color(theme::MUTED),
                                );
                            });
                        }
                    });
                }
            });
        });
        let _ = now;
    }

    fn tile_filter(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.tiles.text)
                    .hint_text("Search name, address, area")
                    .desired_width(240.0),
            );
            ui.checkbox(&mut self.tiles.problems_only, "Problems only");
            egui::ComboBox::from_id_salt("tile_sort")
                .selected_text(self.tiles.sort.label())
                .show_ui(ui, |ui| {
                    for s in Sort::ALL {
                        ui.selectable_value(&mut self.tiles.sort, s, s.label());
                    }
                });
            if let Some(a) = self.tiles.area.clone() {
                if ui
                    .button(format!("Area: {a}  x"))
                    .on_hover_text("Show all areas")
                    .clicked()
                {
                    self.tiles.area = None;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Check all now").clicked() {
                    if let Some(m) = &self.monitor {
                        m.check_now(None);
                    }
                }
            });
        });
    }

    /// Tiles in rows; returns the clicked one.
    fn tile_grid(&self, ui: &mut egui::Ui, tiles: &[&Tile], now: Ms) -> Option<Id> {
        let w = ui.available_width();
        let cols = ((w + GAP) / (TILE_W + GAP)).floor().max(1.0) as usize;
        let tw = ((w + GAP) / cols as f32 - GAP).max(TILE_W * 0.8);
        let mut clicked = None;
        for row in tiles.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                for t in row {
                    if tile(ui, t, tw, self.selected == Some(t.id), now).clicked() {
                        clicked = Some(t.id);
                    }
                }
            });
            ui.add_space(GAP - ui.spacing().item_spacing.y);
        }
        clicked
    }
}

/// One device tile: status bar and symbol, name, address, key value, and
/// "seen …" or "down since …".
fn tile(ui: &mut egui::Ui, t: &Tile, w: f32, selected: bool, now: Ms) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(vec2(w, TILE_H), Sense::click());
    let label = format!(
        "Device {}: {}{}",
        t.name,
        t.status.label(),
        if t.acked { " (acknowledged)" } else { "" }
    );
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &label));
    let p = ui.painter();
    let color = theme::status_color(t.status);
    let fill = if r.hovered() {
        Color32::from_rgb(30, 34, 43)
    } else {
        theme::CARD
    };
    let stroke = if selected {
        Stroke::new(2.0, theme::ACCENT)
    } else if matches!(t.status, Status::Down | Status::Warn) && !t.acked {
        Stroke::new(1.0, color.gamma_multiply(0.7))
    } else {
        Stroke::new(1.0, theme::LINE)
    };
    p.rect(
        rect,
        CornerRadius::same(10),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    // Status bar on the left (dimmed when acknowledged).
    let bar = Rect::from_min_size(rect.min + vec2(1.0, 1.0), vec2(5.0, rect.height() - 2.0));
    p.rect_filled(
        bar,
        CornerRadius {
            nw: 9,
            sw: 9,
            ne: 0,
            se: 0,
        },
        if t.acked {
            color.gamma_multiply(0.4)
        } else {
            color
        },
    );
    let x = rect.left() + 16.0;
    let sym = Rect::from_center_size(pos2(x + 8.0, rect.top() + 20.0), vec2(16.0, 16.0));
    theme::status_symbol(p, sym, t.status);
    let name_w = w - 16.0 - 26.0 - 48.0;
    let name = p.layout(
        t.name.clone(),
        egui::FontId::proportional(15.0),
        if t.enabled { theme::TEXT } else { theme::MUTED },
        name_w,
    );
    let name_galley = if name.rows.len() > 1 {
        // Keep it to one line.
        let mut job = egui::text::LayoutJob::simple_singleline(
            t.name.clone(),
            egui::FontId::proportional(15.0),
            theme::TEXT,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(name_w);
        p.layout_job(job)
    } else {
        name
    };
    p.galley(pos2(x + 24.0, rect.top() + 11.0), name_galley, theme::TEXT);
    let tag = match (t.acked, t.criticality) {
        (true, _) => Some(("ACK", theme::MUTED)),
        (false, Criticality::Critical) => Some(("CRIT", Color32::from_rgb(200, 205, 215))),
        (false, Criticality::Optional) => Some(("OPT", theme::MUTED)),
        _ => None,
    };
    if let Some((tag, c)) = tag {
        p.text(
            pos2(rect.right() - 10.0, rect.top() + 12.0),
            egui::Align2::RIGHT_TOP,
            tag,
            egui::FontId::proportional(10.5),
            c,
        );
    }
    p.text(
        pos2(x, rect.top() + 40.0),
        egui::Align2::LEFT_TOP,
        if t.address.is_empty() {
            "no address"
        } else {
            &t.address
        },
        egui::FontId::monospace(12.0),
        theme::MUTED,
    );
    // Bottom line: the value, or why it is not OK.
    let (left, lc) = match t.status {
        Status::Ok => (t.metric.clone().unwrap_or_default(), theme::TEXT),
        Status::Unknown => (t.cause.clone(), theme::MUTED),
        _ => (t.cause.clone(), color),
    };
    let mut job =
        egui::text::LayoutJob::simple_singleline(left, egui::FontId::proportional(13.0), lc);
    job.wrap = egui::text::TextWrapping::truncate_at_width(w - 16.0 - 90.0);
    let g = p.layout_job(job);
    p.galley(pos2(x, rect.bottom() - 26.0), g, lc);
    let right = match t.status {
        Status::Down | Status::Warn => format!("since {}", clock(t.since)),
        _ => t
            .last_seen
            .map(|s| format!("seen {}", ago(now, s)))
            .unwrap_or_default(),
    };
    p.text(
        pos2(rect.right() - 10.0, rect.bottom() - 18.0),
        egui::Align2::RIGHT_CENTER,
        right,
        egui::FontId::proportional(11.5),
        theme::MUTED,
    );
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}
