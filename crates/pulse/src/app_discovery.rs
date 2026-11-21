//! The discovery page: devices that announce themselves (mDNS, NDI) or
//! answer (ArtPoll, console queries, an optional ping sweep), held against
//! the inventory. New ones are added with one click, with the preset that
//! suits them; devices the inventory expects but nobody saw are listed too.

use crate::app::{ago, now_ms, App};
use crate::theme;
use egui::RichText;
use pulse_core::discovery::{Candidate, Report};
use pulse_core::Preset;
use pulse_net::found::Listener;
use pulse_net::Nic;
use std::collections::BTreeMap;
use std::net::Ipv4Addr;

#[derive(Default)]
pub struct DiscoveryState {
    /// The preset chosen for a new device (else the suggested one).
    pub presets: BTreeMap<Ipv4Addr, Preset>,
    /// Interface for the sweep (name).
    pub sweep_nic: String,
    /// "Ping … addresses?" is asked.
    pub confirm_sweep: bool,
}

/// What the page shows, read from the monitor in one go.
struct View {
    report: Report,
    mdns: Listener,
    artnet: Listener,
    last_scan: Option<u64>,
    sweep: Option<pulse_net::found::Sweep>,
    nics: Vec<Nic>,
}

fn listener_text(l: &Listener, on: &str) -> (String, egui::Color32) {
    match l {
        Listener::Off => ("off".into(), theme::MUTED),
        Listener::On(_) => (on.into(), theme::OK),
        Listener::Failed(e) => (e.clone(), theme::WARN),
    }
}

impl App {
    /// New devices found (for the navigation badge).
    pub fn discovered_new(&self) -> usize {
        let (Some(m), Some(p)) = (&self.monitor, self.profile()) else {
            return 0;
        };
        m.found(|f| f.report(p).new.len())
    }

    /// The discovery report right now.
    pub fn discovery_report(&self) -> Option<Report> {
        let m = self.monitor.as_ref()?;
        let p = self.profile()?;
        Some(m.found(|f| f.report(p)))
    }

    fn add_found(&mut self, cands: &[Candidate]) {
        let presets = self.discovery.presets.clone();
        let mut names = Vec::new();
        self.edit(|p| {
            for c in cands {
                let mut c = c.clone();
                if let Some(pr) = presets.get(&c.address) {
                    c.preset = *pr;
                }
                p.add_candidate(&c);
                names.push(c.name.clone());
            }
        });
        match names.as_slice() {
            [] => {}
            [one] => self.say(format!("Added {one} to the inventory"), theme::OK),
            many => self.say(
                format!("Added {} devices to the inventory", many.len()),
                theme::OK,
            ),
        }
    }

    pub(crate) fn discovery_page(&mut self, ui: &mut egui::Ui) {
        let Some(m) = &self.monitor else { return };
        let Some(profile) = self.profile().cloned() else {
            return;
        };
        let v = m.found(|f| View {
            report: f.report(&profile),
            mdns: f.mdns_state.clone(),
            artnet: f.artnet_state.clone(),
            last_scan: f.last_scan,
            sweep: f.sweep.clone(),
            nics: f.nics.clone(),
        });
        let now = now_ms();

        ui.horizontal(|ui| {
            ui.heading("Discovery");
            ui.label(
                RichText::new(
                    "Devices that announce themselves or answer. Add them with one click.",
                )
                .color(theme::MUTED),
            );
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let (t, c) = listener_text(&v.mdns, "listening");
            ui.label(RichText::new("mDNS / NDI / Dante:").color(theme::MUTED));
            ui.label(RichText::new(t).color(c));
            ui.add_space(12.0);
            let (t, c) = listener_text(&v.artnet, "listening");
            ui.label(RichText::new("Art-Net:").color(theme::MUTED));
            ui.label(RichText::new(t).color(c));
            ui.add_space(12.0);
            ui.label(RichText::new("Consoles:").color(theme::MUTED));
            ui.label(match v.last_scan {
                Some(t) => format!("asked {}", ago(now, t)),
                None => "asking…".into(),
            });
            ui.add_space(12.0);
            if ui
                .button("Search again")
                .on_hover_text("Asks consoles and Art-Net nodes again (one broadcast each)")
                .clicked()
            {
                if let Some(m) = &self.monitor {
                    m.scan();
                }
            }
        });
        ui.add_space(8.0);
        self.sweep_box(ui, &v);
        ui.add_space(10.0);

        egui::ScrollArea::vertical()
            .id_salt("discovery_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.new_list(ui, &v.report.new, now);
                ui.add_space(12.0);
                known_list(ui, &v.report, &profile);
                ui.add_space(12.0);
                missing_list(ui, &v.report);
            });
    }

    fn sweep_box(&mut self, ui: &mut egui::Ui, v: &View) {
        let lan: Vec<&Nic> = v.nics.iter().filter(|n| n.up && !n.loopback).collect();
        if self.discovery.sweep_nic.is_empty()
            || !lan.iter().any(|n| n.name == self.discovery.sweep_nic)
        {
            if let Some(n) = lan.first() {
                self.discovery.sweep_nic = n.name.clone();
            }
        }
        let nic = lan
            .iter()
            .find(|n| n.name == self.discovery.sweep_nic)
            .copied();
        egui::Frame::new()
            .fill(theme::CARD)
            .stroke(egui::Stroke::new(1.0, theme::LINE))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let running = v.sweep.as_ref().is_some_and(|s| s.finished.is_none());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Ping sweep").strong());
                    ui.label(
                        RichText::new("Finds devices that announce nothing: pings every address of one subnet once, 20 per second.")
                            .small()
                            .color(theme::MUTED),
                    );
                });
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("sweep_nic")
                        .selected_text(nic.map(|n| n.label()).unwrap_or_else(|| "no network".into()))
                        .show_ui(ui, |ui| {
                            for n in &lan {
                                ui.selectable_value(&mut self.discovery.sweep_nic, n.name.clone(), n.label());
                            }
                        });
                    if running {
                        if ui.button("Stop sweep").clicked() {
                            if let Some(m) = &self.monitor {
                                m.cancel_sweep();
                            }
                        }
                    } else if ui
                        .add_enabled(nic.is_some(), egui::Button::new("Scan subnet…"))
                        .clicked()
                    {
                        self.discovery.confirm_sweep = true;
                    }
                    if let Some(s) = &v.sweep {
                        let frac = if s.total == 0 { 1.0 } else { s.done as f32 / s.total as f32 };
                        ui.add(
                            egui::ProgressBar::new(frac)
                                .desired_width(160.0)
                                .text(format!("{} of {}", s.done, s.total)),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{}: {} answered{}",
                                s.net.label(),
                                s.answered.len(),
                                if s.cancelled { ", stopped" } else { "" }
                            ))
                            .color(theme::MUTED),
                        );
                    }
                });
                if let (true, Some(n)) = (self.discovery.confirm_sweep && !running, nic) {
                    let net = n.net();
                    let hosts = net.hosts().len();
                    ui.add_space(4.0);
                    egui::Frame::new()
                        .fill(theme::WARN.gamma_multiply(0.10))
                        .stroke(egui::Stroke::new(1.0, theme::WARN.gamma_multiply(0.7)))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.label(format!(
                                "Ping {hosts} addresses of {} from {}? It takes about {:.0} s. Only scan networks you are allowed to.",
                                net.label(),
                                n.name,
                                hosts as f64 / pulse_net::discovery::SWEEP_RATE
                            ));
                            ui.horizontal(|ui| {
                                if ui.button("Start sweep").clicked() {
                                    if let Some(m) = &self.monitor {
                                        m.sweep(net, Some(n.ip));
                                    }
                                    self.discovery.confirm_sweep = false;
                                }
                                if ui.button("Cancel").clicked() {
                                    self.discovery.confirm_sweep = false;
                                }
                            });
                        });
                }
            });
    }

    fn new_list(&mut self, ui: &mut egui::Ui, new: &[Candidate], now: u64) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("NEW ({})", new.len()))
                    .small()
                    .color(theme::MUTED),
            );
            if new.len() > 1 && ui.button(format!("Add all {} new", new.len())).clicked() {
                self.add_found(new);
            }
        });
        if new.is_empty() {
            ui.label(
                RichText::new("Nothing new: every device found is in the inventory.")
                    .color(theme::MUTED),
            );
            return;
        }
        let mut add = None;
        egui::Grid::new("discovery_new")
            .striped(true)
            .num_columns(6)
            .spacing([14.0, 6.0])
            .show(ui, |ui| {
                for h in ["Name", "Address", "Found via", "Details", "Add as", ""] {
                    ui.label(RichText::new(h).small().color(theme::MUTED));
                }
                ui.end_row();
                for c in new {
                    ui.label(RichText::new(&c.name).strong());
                    ui.label(RichText::new(c.address.to_string()).monospace());
                    ui.label(c.via_label());
                    ui.label(
                        RichText::new(format!(
                            "{} · seen {}",
                            c.details.join("; "),
                            ago(now, c.last)
                        ))
                        .small()
                        .color(theme::MUTED),
                    );
                    let chosen = self
                        .discovery
                        .presets
                        .get(&c.address)
                        .copied()
                        .unwrap_or(c.preset);
                    let mut pick = chosen;
                    egui::ComboBox::from_id_salt(("preset", u32::from(c.address)))
                        .selected_text(pick.label())
                        .width(210.0)
                        .show_ui(ui, |ui| {
                            for pr in Preset::ALL {
                                ui.selectable_value(&mut pick, pr, pr.label())
                                    .on_hover_text(pr.hint());
                            }
                        });
                    if pick != chosen {
                        self.discovery.presets.insert(c.address, pick);
                    }
                    if ui
                        .button("Add")
                        .on_hover_text(format!("Add {} to the inventory", c.name))
                        .clicked()
                    {
                        add = Some(c.clone());
                    }
                    ui.end_row();
                }
            });
        if let Some(c) = add {
            self.add_found(&[c]);
        }
    }
}

fn known_list(ui: &mut egui::Ui, r: &Report, p: &pulse_core::Profile) {
    ui.label(
        RichText::new(format!("IN THE INVENTORY ({})", r.known.len()))
            .small()
            .color(theme::MUTED),
    );
    if r.known.is_empty() {
        ui.label(
            RichText::new("None of the inventory's devices was found yet.").color(theme::MUTED),
        );
        return;
    }
    egui::Grid::new("discovery_known")
        .striped(true)
        .num_columns(3)
        .spacing([14.0, 6.0])
        .show(ui, |ui| {
            for c in &r.known {
                let name = c
                    .device
                    .and_then(|id| p.device(id))
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| c.name.clone());
                ui.label(name);
                ui.label(RichText::new(c.address.to_string()).monospace());
                ui.label(RichText::new(c.via_label()).color(theme::MUTED));
                ui.end_row();
            }
        });
}

fn missing_list(ui: &mut egui::Ui, r: &Report) {
    ui.label(
        RichText::new(format!("EXPECTED, NOT SEEN ({})", r.missing.len()))
            .small()
            .color(theme::MUTED),
    );
    if r.missing.is_empty() {
        ui.label(
            RichText::new("Every device that should announce itself or answer was seen.")
                .color(theme::MUTED),
        );
        return;
    }
    egui::Grid::new("discovery_missing")
        .striped(true)
        .num_columns(3)
        .spacing([14.0, 6.0])
        .show(ui, |ui| {
            for m in &r.missing {
                ui.label(RichText::new(&m.name).color(theme::WARN));
                ui.label(RichText::new(&m.address).monospace());
                ui.label(RichText::new(format!("expected via {}", m.expected)).color(theme::MUTED));
                ui.end_row();
            }
        });
}
