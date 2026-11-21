//! The event log: every status change and acknowledgement, newest first,
//! with a search, status and area filters, and a CSV export.

use crate::app::{clock, date_time, App};
use crate::theme;
use egui::{vec2, RichText, Sense};
use pulse_core::events::{EventKind, EventLog};
use pulse_core::Status;
use std::path::Path;

impl App {
    /// The filtered events as CSV.
    pub fn log_csv(&self) -> String {
        let Some(m) = &self.monitor else {
            return String::new();
        };
        let f = self.log_filter.clone();
        m.with(|s| EventLog::to_csv(s.log.filtered(&f), date_time))
    }

    /// Write the filtered events to `path`.
    pub fn export_log(&mut self, path: &Path) -> bool {
        let csv = self.log_csv();
        match pulse_core::profile::atomic_write(path, csv.as_bytes()) {
            Ok(()) => {
                self.say(format!("Exported {}", path.display()), theme::OK);
                true
            }
            Err(e) => {
                self.say(format!("Could not export: {e}"), theme::ERROR);
                false
            }
        }
    }

    pub(crate) fn log_page(&mut self, ui: &mut egui::Ui) {
        let Some(m) = &self.monitor else { return };
        let f = self.log_filter.clone();
        let (events, total, areas) = m.with(|s| {
            let ev: Vec<_> = s.log.filtered(&f).rev().cloned().collect();
            (ev, s.log.len(), s.profile.areas())
        });
        ui.horizontal(|ui| {
            ui.heading("Event log");
            ui.label(RichText::new(format!("{} of {total}", events.len())).color(theme::MUTED));
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.log_filter.text)
                    .hint_text("Search device, area, cause")
                    .desired_width(220.0),
            );
            for s in [Status::Down, Status::Warn, Status::Ok, Status::Unknown] {
                let mut on = self.log_filter.to.contains(&s);
                if ui
                    .toggle_value(&mut on, s.label())
                    .on_hover_text(format!("Only changes to {}", s.label()))
                    .changed()
                {
                    if on {
                        self.log_filter.to.push(s);
                    } else {
                        self.log_filter.to.retain(|x| *x != s);
                    }
                }
            }
            egui::ComboBox::from_id_salt("log_area")
                .selected_text(
                    self.log_filter
                        .area
                        .clone()
                        .unwrap_or_else(|| "All areas".into()),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.log_filter.area, None, "All areas");
                    for a in &areas {
                        ui.selectable_value(&mut self.log_filter.area, Some(a.clone()), a);
                    }
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(!events.is_empty(), egui::Button::new("Export CSV…"))
                    .on_hover_text("The events shown, as a spreadsheet file")
                    .clicked()
                    && self.dialogs
                {
                    let name = format!(
                        "Pulse events {}.csv",
                        date_time(crate::app::now_ms()).replace(':', "-")
                    );
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("CSV", &["csv"])
                        .set_file_name(name)
                        .save_file()
                    {
                        self.export_log(&p);
                    }
                }
            });
        });
        ui.add_space(8.0);
        if events.is_empty() {
            ui.label(
                RichText::new("No events yet. Status changes and acknowledgements appear here.")
                    .color(theme::MUTED),
            );
            return;
        }
        let row_h = 24.0;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_h, events.len(), |ui, range| {
                for e in &events[range] {
                    ui.horizontal(|ui| {
                        ui.set_height(row_h);
                        ui.label(RichText::new(clock(e.at)).monospace().color(theme::MUTED));
                        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                        match e.kind {
                            EventKind::Status { to, .. } => {
                                theme::status_symbol(ui.painter(), r, to)
                            }
                            _ => {
                                ui.painter().circle_stroke(
                                    r.center(),
                                    6.0,
                                    egui::Stroke::new(1.5, theme::ACCENT),
                                );
                            }
                        }
                        cell(ui, 170.0, row_h, RichText::new(&e.name).strong());
                        cell(ui, 80.0, row_h, RichText::new(&e.area).color(theme::MUTED));
                        cell(ui, 170.0, row_h, RichText::new(e.kind.label()));
                        ui.label(RichText::new(&e.cause).color(theme::MUTED));
                    });
                }
            });
    }
}

/// A left-aligned cell of a fixed width.
fn cell(ui: &mut egui::Ui, w: f32, h: f32, text: RichText) {
    ui.allocate_ui_with_layout(
        vec2(w, h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_width(w);
            ui.add(egui::Label::new(text).truncate());
        },
    );
}
