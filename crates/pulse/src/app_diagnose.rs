//! Diagnosis: one device at a time, with graphs of every measurement over
//! the last minutes (latency, jitter, loss, connect time, frame rate), the
//! raw data, "check now", plain-language hints, a test of one TCP port and
//! traceroute. Tools run only on a click.

use crate::app::{ago, now_ms, App};
use crate::theme;
use egui::{pos2, vec2, Color32, RichText, Sense, Stroke};
use pulse_core::discovery::Net;
use pulse_core::history::Series;
use pulse_core::model::{Check, CheckKind, Device};
use pulse_core::probe::{Metric, ProbeResult};
use pulse_core::status::{DeviceState, Ms};
use pulse_core::{Id, Status};
use pulse_net::tools::Trace;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct DiagnoseState {
    /// Minutes shown in the graphs.
    pub minutes: u32,
    pub port: u16,
    /// The port test running or done: device, its answer.
    pub port_result: Option<(Id, Arc<Mutex<Option<String>>>)>,
    pub trace: Option<(Id, Trace)>,
}

impl Default for DiagnoseState {
    fn default() -> Self {
        DiagnoseState {
            minutes: 5,
            port: 80,
            port_result: None,
            trace: None,
        }
    }
}

/// The measurements a kind of check records, with its limit (and whether
/// the limit is a maximum).
fn metrics(c: &Check) -> Vec<(Metric, Option<(f64, bool)>)> {
    let t = c.thresholds;
    match &c.kind {
        CheckKind::Ping => vec![
            (Metric::LatencyMs, Some((t.latency_ms, true))),
            (Metric::JitterMs, Some((t.jitter_ms, true))),
            (Metric::LossPct, Some((t.loss_pct, true))),
        ],
        CheckKind::Tcp { .. } => vec![(Metric::ConnectMs, Some((t.latency_ms, true)))],
        CheckKind::Ndi { .. } => vec![(Metric::ConnectMs, None)],
        CheckKind::Console { .. } | CheckKind::Snmp { .. } | CheckKind::Agent { .. } => {
            vec![(Metric::AnswerMs, None)]
        }
        CheckKind::ArtNetUniverse { min_fps, .. } | CheckKind::SacnUniverse { min_fps, .. } => {
            vec![(Metric::Fps, Some((*min_fps, false)))]
        }
        _ => Vec::new(),
    }
}

/// A line graph of `s` from `since` to `now`; gaps where results are missing.
pub fn graph(
    ui: &mut egui::Ui,
    m: Metric,
    s: Option<&Series>,
    limit: Option<(f64, bool)>,
    since: Ms,
    now: Ms,
) {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 96.0), Sense::hover());
    let pts: Vec<(Ms, f64)> = s
        .map(|s| s.since(since).copied().collect())
        .unwrap_or_default();
    // Fresh data fills the graph instead of a sliver at its right edge.
    let since = pts
        .first()
        .map(|(t, _)| since.max(t.saturating_sub(2000)))
        .unwrap_or(since)
        .min(now.saturating_sub(10_000));
    let last = pts.last().map(|(_, v)| m.format(*v));
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Image,
            true,
            format!(
                "Graph {}: {}",
                m.label(),
                last.clone().unwrap_or_else(|| "no data".into())
            ),
        )
    });
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 6.0, theme::CARD);
    p.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, theme::LINE),
        egui::StrokeKind::Inside,
    );
    let plot = rect.shrink2(vec2(10.0, 18.0));
    let hi = pts
        .iter()
        .map(|(_, v)| *v)
        .chain(limit.map(|(l, _)| l))
        .fold(0.0_f64, f64::max);
    let top = match m {
        Metric::LossPct => hi.clamp(10.0, 100.0),
        _ => (hi * 1.2).max(1.0),
    };
    let x = |t: Ms| {
        plot.left()
            + plot.width()
                * (t.saturating_sub(since) as f32 / now.saturating_sub(since).max(1) as f32)
    };
    let y = |v: f64| plot.bottom() - plot.height() * (v / top).clamp(0.0, 1.0) as f32;
    // Grid and labels.
    for f in [0.0, 0.5, 1.0] {
        let yy = plot.bottom() - plot.height() * f;
        p.line_segment(
            [pos2(plot.left(), yy), pos2(plot.right(), yy)],
            Stroke::new(1.0, theme::LINE),
        );
    }
    p.text(
        rect.left_top() + vec2(8.0, 3.0),
        egui::Align2::LEFT_TOP,
        format!("{} · max {}", m.label(), m.format(top)),
        egui::FontId::proportional(11.0),
        theme::MUTED,
    );
    if let Some(l) = &last {
        p.text(
            rect.right_top() + vec2(-8.0, 3.0),
            egui::Align2::RIGHT_TOP,
            format!("now {l}"),
            egui::FontId::proportional(11.0),
            theme::TEXT,
        );
    }
    p.text(
        rect.left_bottom() + vec2(8.0, -2.0),
        egui::Align2::LEFT_BOTTOM,
        {
            let span = now.saturating_sub(since) / 1000;
            if span < 120 {
                format!("-{span} s")
            } else {
                format!("-{} min", span / 60)
            }
        },
        egui::FontId::proportional(10.0),
        theme::MUTED,
    );
    if let Some((l, upper)) = limit {
        let yy = y(l);
        let dashes = egui::Shape::dashed_line(
            &[pos2(plot.left(), yy), pos2(plot.right(), yy)],
            Stroke::new(1.0, theme::WARN.gamma_multiply(0.8)),
            6.0,
            4.0,
        );
        p.extend(dashes);
        p.text(
            pos2(plot.right() - 2.0, yy - 2.0),
            egui::Align2::RIGHT_BOTTOM,
            format!(
                "{} {}",
                if upper { "limit" } else { "minimum" },
                m.format(l)
            ),
            egui::FontId::proportional(10.0),
            theme::WARN,
        );
    }
    if pts.is_empty() {
        p.text(
            plot.center(),
            egui::Align2::CENTER_CENTER,
            "no data yet",
            egui::FontId::proportional(12.0),
            theme::MUTED,
        );
        return;
    }
    // Gaps: more than three times the usual step between two results.
    let mut steps: Vec<Ms> = pts
        .windows(2)
        .map(|w| w[1].0.saturating_sub(w[0].0))
        .collect();
    steps.sort_unstable();
    let step = steps.get(steps.len() / 2).copied().unwrap_or(2000).max(500);
    let mut line: Vec<egui::Pos2> = Vec::new();
    let bad = |v: f64| limit.is_some_and(|(l, upper)| if upper { v > l } else { v < l });
    let mut prev: Option<Ms> = None;
    for (t, v) in &pts {
        if prev.is_some_and(|pt| t.saturating_sub(pt) > step * 3) && !line.is_empty() {
            p.add(egui::Shape::line(
                std::mem::take(&mut line),
                Stroke::new(1.6, theme::ACCENT),
            ));
        }
        let pt = pos2(x(*t), y(*v));
        line.push(pt);
        if bad(*v) {
            p.circle_filled(pt, 2.2, theme::WARN);
        }
        prev = Some(*t);
    }
    if line.len() == 1 {
        p.circle_filled(line[0], 2.0, theme::ACCENT);
    } else {
        p.add(egui::Shape::line(line, Stroke::new(1.6, theme::ACCENT)));
    }
}

impl App {
    pub(crate) fn diagnose_page(&mut self, ui: &mut egui::Ui) {
        let Some(m) = &self.monitor else { return };
        let tiles = self.tiles();
        // Problems first.
        let mut list: Vec<_> = tiles.iter().collect();
        list.sort_by(|a, b| {
            b.status
                .rank()
                .cmp(&a.status.rank())
                .then(a.name.cmp(&b.name))
        });
        if self.selected.is_none() {
            self.selected = list.first().map(|t| t.id);
        }
        let id = self.selected;
        egui::Panel::left("diagnose_list")
            .resizable(false)
            .exact_size(230.0)
            .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                right: 12,
                ..Default::default()
            }))
            .show(ui, |ui| {
                ui.label(RichText::new("DEVICES").small().color(theme::MUTED));
                egui::ScrollArea::vertical()
                    .id_salt("diagnose_devices")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for t in &list {
                            ui.horizontal(|ui| {
                                let (r, _) =
                                    ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                                theme::status_symbol(ui.painter(), r, t.status);
                                if ui.selectable_label(id == Some(t.id), &t.name).clicked() {
                                    self.selected = Some(t.id);
                                }
                            });
                        }
                    });
            });
        let Some(id) = self.selected else {
            ui.label(RichText::new("No devices.").color(theme::MUTED));
            return;
        };
        let data = m.with(|s| {
            let d = s.profile.device(id)?.clone();
            let st = s.engine.device(id)?.clone();
            let series: Vec<(Id, Metric, Option<Series>)> = d
                .checks
                .iter()
                .flat_map(|c| {
                    metrics(c)
                        .into_iter()
                        .map(|(mm, _)| (c.id, mm, s.history.series(c.id, mm).cloned()))
                        .collect::<Vec<_>>()
                })
                .collect();
            Some((d, st, series))
        });
        let Some((d, st, series)) = data else { return };
        let nets: Vec<Net> = self
            .nics()
            .iter()
            .filter(|n| n.up && !n.loopback)
            .map(|n| n.net())
            .collect();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("diagnose_detail")
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.diagnose_device(ui, &d, &st, &series, &nets));
            });
    }

    fn diagnose_device(
        &mut self,
        ui: &mut egui::Ui,
        d: &Device,
        st: &DeviceState,
        series: &[(Id, Metric, Option<Series>)],
        nets: &[Net],
    ) {
        let now = now_ms();
        let color = theme::status_color(st.status);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
            theme::status_symbol(ui.painter(), r, st.status);
            ui.label(RichText::new(&d.name).size(20.0).strong());
            ui.label(RichText::new(st.status.label()).strong().color(color));
            if !st.cause.is_empty() {
                ui.label(RichText::new(&st.cause).color(color));
            }
        });
        ui.label(
            RichText::new(format!(
                "{} · {} · {}{}",
                d.kind.label(),
                d.area,
                if d.address.is_empty() {
                    "no address"
                } else {
                    &d.address
                },
                if d.interface.is_empty() {
                    String::new()
                } else {
                    format!(" · via {}", d.interface)
                }
            ))
            .color(theme::MUTED),
        );
        ui.horizontal(|ui| {
            if ui.button("Check now").clicked() {
                if let Some(m) = &self.monitor {
                    m.check_now(Some(d.id));
                }
            }
            ui.label(RichText::new("Graphs:").color(theme::MUTED));
            for mins in [5u32, 15, 60] {
                ui.selectable_value(&mut self.diagnose.minutes, mins, format!("{mins} min"));
            }
        });
        ui.add_space(8.0);

        // Hints first: the plain-language part.
        let lasts: Vec<Option<ProbeResult>> = d
            .checks
            .iter()
            .map(|c| st.checks.get(&c.id).and_then(|x| x.last.clone()))
            .collect();
        let refs: Vec<Option<&ProbeResult>> = lasts.iter().map(|r| r.as_ref()).collect();
        let hints = pulse_core::hints::hints(d, &refs, nets);
        if !hints.is_empty() {
            egui::Frame::new()
                .fill(theme::ACCENT.gamma_multiply(0.08))
                .stroke(Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.5)))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("WHAT TO LOOK AT").small().color(theme::MUTED));
                    for h in &hints {
                        ui.label(format!("\u{2022} {h}"));
                    }
                });
            ui.add_space(8.0);
        }

        let since = now.saturating_sub(u64::from(self.diagnose.minutes) * 60_000);
        for c in d.checks.iter().filter(|c| c.enabled) {
            let cs = st.checks.get(&c.id);
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(Stroke::new(1.0, theme::LINE))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let s = cs.map(|x| x.status).unwrap_or(Status::Unknown);
                        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                        theme::status_symbol(ui.painter(), r, s);
                        ui.label(RichText::new(c.kind.label()).strong());
                        if let Some(r) = cs.and_then(|x| x.last.as_ref()) {
                            ui.label(&r.message);
                            ui.label(
                                RichText::new(format!(
                                    "checks: {} · via {}",
                                    r.depth.label(),
                                    r.via
                                ))
                                .small()
                                .color(theme::MUTED),
                            );
                        }
                        if let Some(at) = cs.and_then(|x| x.last_at) {
                            ui.label(RichText::new(ago(now, at)).small().color(theme::MUTED));
                        }
                    });
                    for (mm, limit) in metrics(c) {
                        let s = series
                            .iter()
                            .find(|(cid, m2, _)| *cid == c.id && *m2 == mm)
                            .and_then(|(_, _, s)| s.as_ref());
                        graph(ui, mm, s, limit, since, now);
                        ui.add_space(4.0);
                    }
                    if let Some(r) = cs
                        .and_then(|x| x.last.as_ref())
                        .filter(|r| !r.raw.is_empty())
                    {
                        egui::CollapsingHeader::new(
                            RichText::new("Raw data").small().color(theme::MUTED),
                        )
                        .id_salt(("diag_raw", c.id))
                        .show(ui, |ui| {
                            ui.label(RichText::new(&r.raw).monospace().small());
                        });
                    }
                });
            ui.add_space(6.0);
        }

        // Tools.
        ui.add_space(6.0);
        ui.label(RichText::new("TOOLS").small().color(theme::MUTED));
        let host = d.address.trim().to_string();
        let usable = pulse_net::tools::valid_host(&host);
        ui.horizontal(|ui| {
            ui.label("TCP port");
            ui.add(egui::DragValue::new(&mut self.diagnose.port).range(1..=65535));
            if ui
                .add_enabled(usable, egui::Button::new("Test port"))
                .on_hover_text("Opens one connection and closes it again; nothing is sent")
                .clicked()
            {
                let out = Arc::new(Mutex::new(None));
                let o = out.clone();
                let (h, port) = (host.clone(), self.diagnose.port);
                let ctx = ui.ctx().clone();
                let _ = std::thread::Builder::new()
                    .name("pulse-port-test".into())
                    .spawn(move || {
                        let r = pulse_net::tools::port_test(&h, port, Duration::from_secs(2));
                        if let Ok(mut g) = o.lock() {
                            *g = Some(r);
                        }
                        ctx.request_repaint();
                    });
                self.diagnose.port_result = Some((d.id, out));
            }
            if let Some((pid, r)) = &self.diagnose.port_result {
                if *pid == d.id {
                    let text = r
                        .lock()
                        .ok()
                        .and_then(|g| g.clone())
                        .unwrap_or_else(|| "testing…".into());
                    ui.label(RichText::new(text).monospace());
                }
            }
        });
        ui.horizontal(|ui| {
            let running = self
                .diagnose
                .trace
                .as_ref()
                .is_some_and(|(_, t)| !t.is_done());
            if ui
                .add_enabled(usable && !running, egui::Button::new("Traceroute"))
                .on_hover_text("The route to the device, hop by hop (the system's traceroute)")
                .clicked()
            {
                let ctx = ui.ctx().clone();
                let t =
                    pulse_net::tools::traceroute(&host, Arc::new(move || ctx.request_repaint()));
                self.diagnose.trace = Some((d.id, t));
            }
            if running {
                ui.spinner();
            }
        });
        if let Some((tid, t)) = &self.diagnose.trace {
            if *tid == d.id {
                let text = t.text();
                egui::Frame::new()
                    .fill(Color32::from_rgb(12, 14, 18))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new(if text.is_empty() {
                                "starting…"
                            } else {
                                &text
                            })
                            .monospace()
                            .small(),
                        );
                    });
            }
        }
    }
}
