//! The detail panel of a device: its status and why, acknowledging,
//! "check now", every check with its depth, message and measurements, the
//! raw data of the last run and the device's latest events.

use crate::app::{ago, clock, now_ms, App, Page};
use crate::theme;
use egui::{vec2, RichText, Sense};
use pulse_core::events::EventKind;
use pulse_core::model::{Check, Device};
use pulse_core::status::{CheckState, DeviceState};
use pulse_core::Status;

impl App {
    pub(crate) fn detail(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.selected else { return };
        let Some(m) = &self.monitor else { return };
        let data: Option<(Device, DeviceState, Vec<pulse_core::Event>)> = m.with(|s| {
            let d = s.profile.device(id)?.clone();
            let st = s.engine.device(id)?.clone();
            let ev: Vec<_> = s
                .log
                .iter()
                .rev()
                .filter(|e| e.device == id)
                .take(8)
                .cloned()
                .collect();
            Some((d, st, ev))
        });
        let Some((d, st, events)) = data else { return };
        let now = now_ms();

        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
            theme::status_symbol(ui.painter(), r, st.status);
            ui.label(RichText::new(&d.name).size(19.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    self.selected = None;
                }
            });
        });
        ui.label(
            RichText::new(format!(
                "{} · {} · {} · {}",
                d.kind.label(),
                d.area,
                d.criticality.label(),
                if d.address.is_empty() {
                    "no address"
                } else {
                    &d.address
                }
            ))
            .color(theme::MUTED),
        );
        ui.add_space(8.0);

        // Status, why and since when.
        let color = theme::status_color(st.status);
        egui::Frame::new()
            .fill(color.gamma_multiply(0.10))
            .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.7)))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(st.status.label()).strong().color(color));
                if !st.cause.is_empty() {
                    ui.label(&st.cause);
                }
                ui.label(
                    RichText::new(format!(
                        "since {} ({})",
                        clock(st.since),
                        ago(now, st.since)
                    ))
                    .small()
                    .color(theme::MUTED),
                );
                match st.last_seen {
                    Some(t) => ui.label(
                        RichText::new(format!("Last answer {}", ago(now, t)))
                            .small()
                            .color(theme::MUTED),
                    ),
                    None => ui.label(RichText::new("No answer yet").small().color(theme::MUTED)),
                };
                if st.flapping {
                    ui.label(
                        RichText::new("Flapping: changes often between good and bad.")
                            .small()
                            .color(theme::WARN),
                    );
                }
            });
        ui.add_space(8.0);

        // Acknowledge.
        match &st.ack {
            Some(a) => {
                ui.label(
                    RichText::new(format!(
                        "Acknowledged at {}{}",
                        clock(a.at),
                        if a.note.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", a.note)
                        }
                    ))
                    .color(theme::MUTED),
                );
                ui.label(
                    RichText::new("Still shown, but no longer colours the light. Ends by itself when the device is OK again.")
                        .small()
                        .color(theme::MUTED),
                );
                if ui.button("Remove acknowledgement").clicked() {
                    if let Some(m) = &self.monitor {
                        m.unack(id);
                    }
                }
            }
            None if matches!(st.status, Status::Warn | Status::Down | Status::Unknown) => {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.ack_note)
                            .hint_text("Why, e.g. off on purpose")
                            .desired_width(200.0),
                    );
                    if ui
                        .button("Acknowledge")
                        .on_hover_text(
                            "A known problem: it stays visible but no longer colours the light",
                        )
                        .clicked()
                    {
                        if let Some(m) = &self.monitor {
                            m.ack(id, self.ack_note.trim());
                        }
                        self.ack_note.clear();
                    }
                });
            }
            None => {}
        }
        ui.horizontal(|ui| {
            if ui.button("Check now").clicked() {
                if let Some(m) = &self.monitor {
                    m.check_now(Some(id));
                }
            }
            if ui.button("Edit device").clicked() {
                self.page = Page::Inventory;
                self.inventory.select_only(id);
            }
        });
        ui.add_space(10.0);

        ui.label(RichText::new("CHECKS").small().color(theme::MUTED));
        for c in d.checks.iter() {
            check_card(ui, c, st.checks.get(&c.id), now);
        }
        if d.checks.is_empty() {
            ui.label(RichText::new("No checks: add one in the inventory.").color(theme::MUTED));
        }

        if !events.is_empty() {
            ui.add_space(10.0);
            ui.label(RichText::new("LATEST EVENTS").small().color(theme::MUTED));
            for e in &events {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(clock(e.at))
                            .monospace()
                            .small()
                            .color(theme::MUTED),
                    );
                    if let EventKind::Status { to, .. } = e.kind {
                        let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                        theme::status_symbol(ui.painter(), r, to);
                    }
                    ui.label(RichText::new(e.kind.label()).small());
                    if !e.cause.is_empty() {
                        ui.label(RichText::new(&e.cause).small().color(theme::MUTED));
                    }
                });
            }
        }
        if !d.notes.is_empty() {
            ui.add_space(10.0);
            ui.label(RichText::new("NOTES").small().color(theme::MUTED));
            ui.label(&d.notes);
        }
    }
}

/// One check: status, depth (what a pass proves), message, values, raw data.
fn check_card(ui: &mut egui::Ui, c: &Check, st: Option<&CheckState>, now: u64) {
    let status = if c.enabled {
        st.map(|s| s.status).unwrap_or_default()
    } else {
        Status::Unknown
    };
    egui::Frame::new()
        .fill(theme::CARD)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                theme::status_symbol(ui.painter(), r, status);
                ui.label(RichText::new(c.kind.label()).strong());
                if c.critical {
                    ui.label(RichText::new("critical").small().color(theme::MUTED))
                        .on_hover_text("Down here means the device is down");
                }
                if !c.enabled {
                    ui.label(RichText::new("off").small().color(theme::MUTED));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("every {:.1} s", c.interval()))
                            .small()
                            .color(theme::MUTED),
                    );
                });
            });
            let Some(r) = st.and_then(|s| s.last.as_ref()) else {
                ui.label(RichText::new("Not run yet").small().color(theme::MUTED));
                return;
            };
            ui.label(&r.message);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("Checks: {}", r.depth.label()))
                        .small()
                        .color(theme::ACCENT),
                )
                .on_hover_text(r.depth.meaning());
                if !r.via.is_empty() {
                    ui.label(
                        RichText::new(format!("via {}", r.via))
                            .small()
                            .color(theme::MUTED),
                    );
                }
                if let Some(at) = st.and_then(|s| s.last_at) {
                    ui.label(RichText::new(ago(now, at)).small().color(theme::MUTED));
                }
            });
            if !r.metrics.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for (k, v) in &r.metrics {
                        ui.label(
                            RichText::new(format!("{} {}", k.label(), k.format(*v)))
                                .monospace()
                                .small(),
                        );
                    }
                });
            }
            if !r.raw.is_empty() {
                egui::CollapsingHeader::new(RichText::new("Raw data").small().color(theme::MUTED))
                    .id_salt(("raw", c.id))
                    .show(ui, |ui| {
                        ui.label(RichText::new(&r.raw).monospace().small());
                    });
            }
        });
    ui.add_space(4.0);
}
