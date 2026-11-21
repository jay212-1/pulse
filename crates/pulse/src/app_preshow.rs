//! The pre-show check: every check runs at once, more thoroughly (five
//! echo requests per ping), the results are listed problems first, known
//! problems can be acknowledged, and at the end someone releases the
//! technology with name and time. That writes a report (HTML and CSV).

use crate::app::{clock, date_time, now_ms, App};
use crate::theme;
use egui::{vec2, RichText, Sense};
use pulse_core::report::PreShowReport;
use pulse_core::status::Ms;
use pulse_core::Status;
use pulse_net::Mode;
use std::path::PathBuf;

#[derive(Default)]
pub struct PreShowState {
    /// When the running (or last) check started; `None` = not started.
    pub started: Option<Ms>,
    /// All checks have run once since the start.
    pub finished: Option<Ms>,
    pub name: String,
    /// "Sign off with open problems" was ticked.
    pub accept_open: bool,
    /// The last sign-off: who, when, verdict, the report files.
    pub signed: Option<SignOff>,
}

#[derive(Clone, Debug)]
pub struct SignOff {
    pub by: String,
    pub at: Ms,
    pub verdict: String,
    pub html: Option<PathBuf>,
    pub csv: Option<PathBuf>,
    pub error: Option<String>,
}

/// One device row of the checklist.
struct Row {
    id: pulse_core::Id,
    name: String,
    area: String,
    status: Status,
    cause: String,
    acked: bool,
    /// Checks that have run since the start / all of them.
    done: usize,
    total: usize,
}

impl App {
    /// Run every check now, thoroughly.
    pub fn start_preshow(&mut self) {
        let Some(m) = &self.monitor else { return };
        m.set_mode(Mode::Thorough);
        m.check_now(None);
        self.preshow.started = Some(now_ms());
        self.preshow.finished = None;
        self.preshow.accept_open = false;
    }

    fn preshow_rows(&self, since: Ms) -> Vec<Row> {
        let Some(m) = &self.monitor else {
            return Vec::new();
        };
        m.with(|s| {
            let mut rows: Vec<Row> = s
                .profile
                .devices
                .iter()
                .filter(|d| d.enabled)
                .map(|d| {
                    let st = s.engine.device(d.id);
                    let checks: Vec<_> = d.checks.iter().filter(|c| c.enabled).collect();
                    let done = checks
                        .iter()
                        .filter(|c| {
                            st.and_then(|x| x.checks.get(&c.id))
                                .and_then(|x| x.last_at)
                                .is_some_and(|t| t >= since)
                        })
                        .count();
                    Row {
                        id: d.id,
                        name: d.name.clone(),
                        area: d.area.clone(),
                        status: st.map(|x| x.status).unwrap_or_default(),
                        cause: st.map(|x| x.cause.clone()).unwrap_or_default(),
                        acked: st.is_some_and(|x| x.ack.is_some()),
                        done,
                        total: checks.len(),
                    }
                })
                .collect();
            rows.sort_by(|a, b| {
                b.status
                    .rank()
                    .cmp(&a.status.rank())
                    .then_with(|| a.area.cmp(&b.area))
                    .then_with(|| a.name.cmp(&b.name))
            });
            rows
        })
    }

    /// Where reports go: the chosen folder, next to the profile, or Documents/Pulse/Reports.
    fn report_dir(&self) -> PathBuf {
        if let Some(d) = &self.report_dir {
            return d.clone();
        }
        self.doc
            .as_ref()
            .and_then(|d| d.path.as_ref())
            .and_then(|p| p.parent())
            .map(|p| p.join("Reports"))
            .unwrap_or_else(|| crate::settings::default_profile_dir().join("Reports"))
    }

    /// Release the technology: name and time, and the report.
    pub fn sign_off(&mut self) {
        let by = self.preshow.name.trim().to_string();
        let (Some(m), Some(started)) = (&self.monitor, self.preshow.started) else {
            return;
        };
        if by.is_empty() {
            return;
        }
        let at = now_ms();
        let report = m.with(|s| {
            PreShowReport::build(
                &s.profile,
                &s.engine,
                &s.overview,
                &date_time(started),
                &date_time(at),
                &by,
                &format!("Pulse {}", env!("CARGO_PKG_VERSION")),
            )
        });
        let dir = self.report_dir();
        let title = self
            .doc
            .as_ref()
            .map(|d| crate::app_start::clean_name(&d.title()))
            .unwrap_or_else(|| "Pulse".into());
        let stem = format!("{title} pre-show {}", date_time(at).replace(':', ""));
        let html = dir.join(format!("{stem}.html"));
        let csv = dir.join(format!("{stem}.csv"));
        let written = std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .and_then(|_| {
                pulse_core::profile::atomic_write(&html, report.html().as_bytes())
                    .map_err(|e| e.to_string())
            })
            .and_then(|_| {
                pulse_core::profile::atomic_write(&csv, report.csv().as_bytes())
                    .map_err(|e| e.to_string())
            });
        let verdict = report.verdict();
        self.preshow.signed = Some(match written {
            Ok(()) => SignOff {
                by: by.clone(),
                at,
                verdict: verdict.clone(),
                html: Some(html),
                csv: Some(csv),
                error: None,
            },
            Err(e) => SignOff {
                by: by.clone(),
                at,
                verdict: verdict.clone(),
                html: None,
                csv: None,
                error: Some(format!(
                    "The report could not be written to {}: {e}",
                    dir.display()
                )),
            },
        });
        self.preshow.started = None;
        self.preshow.finished = None;
        m.set_mode(Mode::Monitor);
        self.say(format!("{verdict} by {by}"), theme::OK);
    }

    pub(crate) fn preshow_page(&mut self, ui: &mut egui::Ui) {
        let now = now_ms();
        ui.horizontal(|ui| {
            ui.heading("Pre-show check");
            ui.label(
                RichText::new("Every check at once and more thoroughly; at the end someone releases the technology.")
                    .color(theme::MUTED),
            );
        });
        ui.add_space(8.0);
        if let Some(s) = self.preshow.signed.clone() {
            self.signed_box(ui, &s);
            ui.add_space(8.0);
        }
        let Some(started) = self.preshow.started else {
            ui.label("Runs every check of every device now, with five echo requests per ping instead of one. Takes a few seconds; the light updates as results come in.");
            ui.add_space(6.0);
            if ui
                .add(
                    egui::Button::new(RichText::new("Start pre-show check").size(16.0))
                        .min_size(vec2(220.0, 36.0)),
                )
                .clicked()
            {
                self.start_preshow();
            }
            return;
        };
        let rows = self.preshow_rows(started);
        let total: usize = rows.iter().map(|r| r.total).sum();
        let done: usize = rows.iter().map(|r| r.done).sum();
        if done >= total && self.preshow.finished.is_none() {
            self.preshow.finished = Some(now);
            if let Some(m) = &self.monitor {
                m.set_mode(Mode::Monitor);
            }
        }
        let finished = self.preshow.finished.is_some();
        ui.horizontal(|ui| {
            let frac = if total == 0 {
                1.0
            } else {
                done as f32 / total as f32
            };
            ui.add(
                egui::ProgressBar::new(frac)
                    .desired_width(260.0)
                    .text(format!("{done} of {total} checks")),
            );
            ui.label(
                RichText::new(if finished {
                    format!("done, started {}", clock(started))
                } else {
                    format!("running since {}", clock(started))
                })
                .color(theme::MUTED),
            );
            if ui.button("Run again").clicked() {
                self.start_preshow();
            }
            if ui.button("Cancel").clicked() {
                self.preshow.started = None;
                self.preshow.finished = None;
                if let Some(m) = &self.monitor {
                    m.set_mode(Mode::Monitor);
                }
            }
        });
        ui.add_space(8.0);
        let problems: Vec<&Row> = rows
            .iter()
            .filter(|r| matches!(r.status, Status::Warn | Status::Down) && !r.acked)
            .collect();
        let unknown = rows.iter().filter(|r| r.status == Status::Unknown).count();

        // Sign-off.
        egui::Frame::new()
            .fill(theme::CARD)
            .stroke(egui::Stroke::new(1.0, theme::LINE))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Release").strong());
                ui.horizontal(|ui| {
                    let l = ui.label(RichText::new("Your name").color(theme::MUTED));
                    ui.add(egui::TextEdit::singleline(&mut self.preshow.name).desired_width(200.0))
                        .labelled_by(l.id);
                    if !problems.is_empty() {
                        ui.checkbox(
                            &mut self.preshow.accept_open,
                            format!(
                                "Release with {} open problem{} (listed in the report)",
                                problems.len(),
                                if problems.len() == 1 { "" } else { "s" }
                            ),
                        );
                    }
                });
                let ready = finished
                    && !self.preshow.name.trim().is_empty()
                    && (problems.is_empty() || self.preshow.accept_open);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            ready,
                            egui::Button::new(RichText::new("Technology released").strong()),
                        )
                        .on_hover_text(
                            "Signs off with your name and the time, and writes the report",
                        )
                        .clicked()
                    {
                        self.sign_off();
                    }
                    let why = if !finished {
                        "Waiting for every check to run once."
                    } else if self.preshow.name.trim().is_empty() {
                        "Type your name to sign off."
                    } else if !problems.is_empty() && !self.preshow.accept_open {
                        "Acknowledge the known problems below, or release with them open."
                    } else {
                        ""
                    };
                    if !why.is_empty() {
                        ui.label(RichText::new(why).color(theme::MUTED));
                    }
                });
                if unknown > 0 && finished {
                    ui.label(
                        RichText::new(format!(
                            "{unknown} device(s) unknown: their checks could not run (see below)."
                        ))
                        .small()
                        .color(theme::WARN),
                    );
                }
            });
        ui.add_space(10.0);

        // The checklist.
        let mut ack = None;
        egui::ScrollArea::vertical()
            .id_salt("preshow_list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("preshow_grid")
                    .striped(true)
                    .num_columns(6)
                    .spacing([14.0, 6.0])
                    .show(ui, |ui| {
                        for h in ["", "Device", "Area", "Checks", "Result", ""] {
                            ui.label(RichText::new(h).small().color(theme::MUTED));
                        }
                        ui.end_row();
                        for r in &rows {
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                            theme::status_symbol(ui.painter(), rect, r.status);
                            ui.label(RichText::new(&r.name).strong());
                            ui.label(&r.area);
                            ui.label(
                                RichText::new(format!("{} of {}", r.done, r.total))
                                    .small()
                                    .color(if r.done >= r.total {
                                        theme::MUTED
                                    } else {
                                        theme::ACCENT
                                    }),
                            );
                            let text = match (r.status, r.acked) {
                                (Status::Ok, _) => "all checks passed".to_string(),
                                (_, true) => format!("{} (acknowledged)", r.cause),
                                (Status::Unknown, _) if r.done < r.total => "checking…".to_string(),
                                _ => r.cause.clone(),
                            };
                            ui.label(RichText::new(text).color(theme::status_color(r.status)));
                            if matches!(r.status, Status::Warn | Status::Down | Status::Unknown)
                                && !r.acked
                                && r.done >= r.total
                            {
                                if ui
                                    .small_button(format!("Known: {}", r.name))
                                    .on_hover_text(
                                        "Acknowledge as a known problem (stays in the report)",
                                    )
                                    .clicked()
                                {
                                    ack = Some(r.id);
                                }
                            } else {
                                ui.label("");
                            }
                            ui.end_row();
                        }
                    });
            });
        if let (Some(id), Some(m)) = (ack, &self.monitor) {
            m.ack(id, "known at the pre-show check");
        }
    }

    fn signed_box(&mut self, ui: &mut egui::Ui, s: &SignOff) {
        let color = if s.error.is_some() {
            theme::WARN
        } else {
            theme::OK
        };
        egui::Frame::new()
            .fill(color.gamma_multiply(0.10))
            .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.7)))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(format!("{} by {} at {}", s.verdict, s.by, date_time(s.at)))
                        .strong()
                        .color(color),
                );
                if let Some(e) = &s.error {
                    ui.label(e);
                }
                if let Some(h) = &s.html {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Report: {}", h.display()))
                                .small()
                                .color(theme::MUTED),
                        );
                        if self.dialogs && ui.small_button("Open report").clicked() {
                            crate::platform::open(h);
                        }
                        if self.dialogs && ui.small_button("Show in folder").clicked() {
                            if let Some(dir) = h.parent() {
                                crate::platform::open(dir);
                            }
                        }
                    });
                }
            });
    }
}
