//! The inventory editor: a list of all devices (several can be selected,
//! for bulk changes), and for one device its fields, hysteresis and checks.
//! Every change goes to the monitor at once and is saved shortly after.

use crate::app::App;
use crate::theme;
use egui::{vec2, RichText, Sense};
use pulse_core::model::{Check, CheckKind, Criticality, Device, DeviceKind, Id, Profile, AREAS};
use pulse_core::Status;
use std::collections::BTreeSet;

#[derive(Default)]
pub struct InventoryState {
    pub selection: BTreeSet<Id>,
    /// "Delete …?" is asked.
    pub confirm_delete: bool,
    /// Area typed for the selected devices.
    pub bulk_area: String,
}

impl InventoryState {
    pub fn select_only(&mut self, id: Id) {
        self.selection.clear();
        self.selection.insert(id);
        self.confirm_delete = false;
    }
}

/// The areas to choose from: the usual ones and the profile's own.
fn area_choices(p: &Profile) -> Vec<String> {
    let mut out: Vec<String> = AREAS.iter().map(|a| a.to_string()).collect();
    for a in p.areas() {
        if !out.contains(&a) {
            out.push(a);
        }
    }
    out
}

impl App {
    pub(crate) fn inventory_page(&mut self, ui: &mut egui::Ui) {
        let Some(profile) = self.profile().cloned() else {
            return;
        };
        let statuses: Vec<(Id, Status)> = self
            .monitor
            .as_ref()
            .map(|m| {
                m.with(|s| {
                    s.profile
                        .devices
                        .iter()
                        .map(|d| {
                            (
                                d.id,
                                s.engine.device(d.id).map(|x| x.status).unwrap_or_default(),
                            )
                        })
                        .collect()
                })
            })
            .unwrap_or_default();
        self.inventory
            .selection
            .retain(|id| profile.device(*id).is_some());

        ui.horizontal(|ui| {
            ui.heading("Inventory");
            ui.label(
                RichText::new(format!("{} devices", profile.devices.len())).color(theme::MUTED),
            );
        });
        ui.add_space(6.0);
        self.inventory_toolbar(ui, &profile);
        ui.add_space(8.0);

        let editor_w = 420.0_f32.min(ui.available_width() * 0.45);
        egui::Panel::right("inventory_editor")
            .resizable(false)
            .exact_size(editor_w)
            .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                left: 14,
                ..Default::default()
            }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("inventory_editor_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.inventory_editor(ui, &profile));
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                egui::ScrollArea::both()
                    .id_salt("inventory_list")
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.inventory_list(ui, &profile, &statuses));
            });
    }

    fn inventory_toolbar(&mut self, ui: &mut egui::Ui, profile: &Profile) {
        let sel: Vec<Id> = self.inventory.selection.iter().copied().collect();
        ui.horizontal(|ui| {
            ui.menu_button("Add device", |ui| {
                for k in DeviceKind::ALL {
                    if ui.button(k.label()).clicked() {
                        ui.close();
                        let mut new = 0;
                        self.edit(|p| {
                            new = p.add_device(k, &format!("New {}", k.label().to_lowercase()), "")
                        });
                        if new != 0 {
                            self.inventory.select_only(new);
                        }
                    }
                }
            });
            let any = !sel.is_empty();
            if ui
                .add_enabled(any, egui::Button::new("Duplicate"))
                .on_hover_text("A copy of each selected device, with its checks")
                .clicked()
            {
                let mut made = Vec::new();
                self.edit(|p| {
                    for id in &sel {
                        if let Some(n) = p.duplicate(*id) {
                            made.push(n);
                        }
                    }
                });
                self.inventory.selection = made.into_iter().collect();
            }
            if ui.add_enabled(any, egui::Button::new("Delete…")).clicked() {
                self.inventory.confirm_delete = true;
            }
            if ui
                .add_enabled(!profile.devices.is_empty(), egui::Button::new("Select all"))
                .clicked()
            {
                self.inventory.selection = profile.devices.iter().map(|d| d.id).collect();
            }
            if any && ui.button("Select none").clicked() {
                self.inventory.selection.clear();
                self.inventory.confirm_delete = false;
            }
        });
        if self.inventory.confirm_delete && !sel.is_empty() {
            ui.add_space(6.0);
            egui::Frame::new()
                .fill(theme::ERROR.gamma_multiply(0.12))
                .stroke(egui::Stroke::new(1.0, theme::ERROR))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let n = sel.len();
                        ui.label(format!(
                            "Delete {n} device{} with their checks?",
                            if n == 1 { "" } else { "s" }
                        ));
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Delete").color(theme::TEXT))
                                    .fill(theme::ERROR.gamma_multiply(0.6)),
                            )
                            .clicked()
                        {
                            self.edit(|p| p.devices.retain(|d| !sel.contains(&d.id)));
                            if self.selected.is_some_and(|s| sel.contains(&s)) {
                                self.selected = None;
                            }
                            self.inventory.selection.clear();
                            self.inventory.confirm_delete = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.inventory.confirm_delete = false;
                        }
                    });
                });
        }
    }

    fn inventory_list(&mut self, ui: &mut egui::Ui, profile: &Profile, statuses: &[(Id, Status)]) {
        if profile.devices.is_empty() {
            ui.add_space(20.0);
            ui.label(
                RichText::new("No devices yet. Add one with \"Add device\".").color(theme::MUTED),
            );
            return;
        }
        let mut click: Option<(Id, bool)> = None;
        let mut toggle: Option<Id> = None;
        egui::Grid::new("inventory_grid")
            .striped(true)
            .num_columns(9)
            .spacing([14.0, 6.0])
            .show(ui, |ui| {
                for h in [
                    "",
                    "",
                    "Name",
                    "Kind",
                    "Area",
                    "Address",
                    "Criticality",
                    "Checks",
                    "On",
                ] {
                    ui.label(RichText::new(h).small().color(theme::MUTED));
                }
                ui.end_row();
                for d in &profile.devices {
                    let on = self.inventory.selection.contains(&d.id);
                    let mut c = on;
                    if ui
                        .checkbox(&mut c, "")
                        .on_hover_text("Select for bulk changes")
                        .changed()
                    {
                        toggle = Some(d.id);
                    }
                    let st = statuses
                        .iter()
                        .find(|(id, _)| *id == d.id)
                        .map(|(_, s)| *s)
                        .unwrap_or_default();
                    let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                    theme::status_symbol(ui.painter(), r, st);
                    let name = if d.name.trim().is_empty() {
                        "(no name)"
                    } else {
                        &d.name
                    };
                    let r = ui.selectable_label(on, name);
                    if r.clicked() {
                        let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                        click = Some((d.id, add));
                    }
                    ui.label(d.kind.label());
                    ui.label(&d.area);
                    ui.label(RichText::new(&d.address).monospace());
                    ui.label(d.criticality.label());
                    let checks: Vec<String> = d
                        .checks
                        .iter()
                        .filter(|c| c.enabled)
                        .map(|c| c.kind.label())
                        .collect();
                    ui.label(RichText::new(checks.join(", ")).small());
                    ui.label(if d.enabled { "yes" } else { "no" });
                    ui.end_row();
                }
            });
        if let Some(id) = toggle {
            if !self.inventory.selection.remove(&id) {
                self.inventory.selection.insert(id);
            }
            self.inventory.confirm_delete = false;
        }
        if let Some((id, add)) = click {
            if add {
                if !self.inventory.selection.remove(&id) {
                    self.inventory.selection.insert(id);
                }
            } else {
                self.inventory.select_only(id);
            }
        }
    }

    fn inventory_editor(&mut self, ui: &mut egui::Ui, profile: &Profile) {
        let sel: Vec<Id> = self.inventory.selection.iter().copied().collect();
        match sel.as_slice() {
            [] => {
                ui.label(
                    RichText::new("Select a device to edit it, or several for bulk changes.")
                        .color(theme::MUTED),
                );
            }
            [id] => {
                if let Some(d) = profile.device(*id) {
                    let mut d = d.clone();
                    device_form(ui, &mut d, profile);
                    if profile.device(*id) != Some(&d) {
                        self.edit(|p| {
                            if let Some(x) = p.device_mut(d.id) {
                                *x = d;
                            }
                        });
                    }
                    ui.add_space(10.0);
                    self.check_list(ui, *id, profile);
                }
            }
            many => self.bulk_form(ui, many, profile),
        }
    }

    fn check_list(&mut self, ui: &mut egui::Ui, id: Id, profile: &Profile) {
        let Some(d) = profile.device(id) else { return };
        ui.label(RichText::new("CHECKS").small().color(theme::MUTED));
        let mut remove = None;
        for c in &d.checks {
            let mut c2 = c.clone();
            let gone = check_form(ui, &mut c2);
            if gone {
                remove = Some(c.id);
            } else if c2 != *c {
                self.edit(|p| {
                    if let Some(x) = p
                        .device_mut(id)
                        .and_then(|d| d.checks.iter_mut().find(|x| x.id == c2.id))
                    {
                        *x = c2;
                    }
                });
            }
        }
        if let Some(cid) = remove {
            self.edit(|p| {
                if let Some(d) = p.device_mut(id) {
                    d.checks.retain(|c| c.id != cid);
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.button("+ Ping").clicked() {
                self.edit(|p| {
                    p.add_check(id, CheckKind::Ping);
                });
            }
            if ui
                .button("+ TCP port")
                .on_hover_text("Opens a connection and closes it again at once; nothing is sent")
                .clicked()
            {
                self.edit(|p| {
                    p.add_check(id, CheckKind::Tcp { port: 80 });
                });
            }
        });
    }

    fn bulk_form(&mut self, ui: &mut egui::Ui, sel: &[Id], profile: &Profile) {
        ui.label(
            RichText::new(format!("{} devices selected", sel.len()))
                .size(17.0)
                .strong(),
        );
        ui.label(RichText::new("Changes apply to all of them.").color(theme::MUTED));
        ui.add_space(10.0);
        let apply = |app: &mut App, f: &dyn Fn(&mut Device)| {
            app.edit(|p| {
                for d in p.devices.iter_mut().filter(|d| sel.contains(&d.id)) {
                    f(d);
                }
            });
        };
        ui.label(RichText::new("Area").color(theme::MUTED));
        ui.horizontal_wrapped(|ui| {
            for a in area_choices(profile) {
                if ui.button(&a).clicked() {
                    apply(self, &|d| d.area = a.clone());
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.inventory.bulk_area)
                    .hint_text("Other area")
                    .desired_width(160.0),
            );
            let a = self.inventory.bulk_area.trim().to_string();
            if ui
                .add_enabled(!a.is_empty(), egui::Button::new("Set area"))
                .clicked()
            {
                apply(self, &|d| d.area = a.clone());
                self.inventory.bulk_area.clear();
            }
        });
        ui.add_space(8.0);
        ui.label(RichText::new("Criticality").color(theme::MUTED));
        ui.horizontal(|ui| {
            for c in Criticality::ALL {
                if ui.button(c.label()).clicked() {
                    apply(self, &|d| d.criticality = c);
                }
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Enable").clicked() {
                apply(self, &|d| d.enabled = true);
            }
            if ui
                .button("Disable")
                .on_hover_text("Not checked, grey, never colours the light")
                .clicked()
            {
                apply(self, &|d| d.enabled = false);
            }
        });
        ui.add_space(8.0);
        ui.label(
            RichText::new("Wi-Fi or wired thresholds for all their checks").color(theme::MUTED),
        );
        ui.horizontal(|ui| {
            if ui.button("Wired").clicked() {
                apply(self, &|d| {
                    d.checks
                        .iter_mut()
                        .for_each(|c| c.thresholds = pulse_core::model::Thresholds::wired())
                });
            }
            if ui.button("Wi-Fi").clicked() {
                apply(self, &|d| {
                    d.checks
                        .iter_mut()
                        .for_each(|c| c.thresholds = pulse_core::model::Thresholds::wifi())
                });
            }
        });
    }
}

/// The fields of one device.
fn device_form(ui: &mut egui::Ui, d: &mut Device, profile: &Profile) {
    ui.label(
        RichText::new(if d.name.is_empty() { "Device" } else { &d.name })
            .size(17.0)
            .strong(),
    );
    ui.add_space(6.0);
    egui::Grid::new(("device_form", d.id))
        .num_columns(2)
        .spacing([12.0, 8.0])
        .min_col_width(90.0)
        .show(ui, |ui| {
            let l = ui.label(RichText::new("Name").color(theme::MUTED));
            ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(240.0)).labelled_by(l.id);
            ui.end_row();
            ui.label(RichText::new("Kind").color(theme::MUTED));
            egui::ComboBox::from_id_salt(("kind", d.id))
                .selected_text(d.kind.label())
                .show_ui(ui, |ui| {
                    for k in DeviceKind::ALL {
                        ui.selectable_value(&mut d.kind, k, k.label());
                    }
                });
            ui.end_row();
            ui.label(RichText::new("Area").color(theme::MUTED));
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(("area", d.id))
                    .selected_text(d.area.clone())
                    .show_ui(ui, |ui| {
                        for a in area_choices(profile) {
                            let v = a.clone();
                            ui.selectable_value(&mut d.area, v, a);
                        }
                    });
                ui.add(egui::TextEdit::singleline(&mut d.area).desired_width(100.0).hint_text("or type"));
            });
            ui.end_row();
            let l = ui.label(RichText::new("Address").color(theme::MUTED));
            ui.add(
                egui::TextEdit::singleline(&mut d.address)
                    .hint_text("IP address or host name")
                    .desired_width(240.0)
                    .font(egui::TextStyle::Monospace),
            )
            .labelled_by(l.id);
            ui.end_row();
            ui.label(RichText::new("MAC").color(theme::MUTED));
            ui.add(egui::TextEdit::singleline(&mut d.mac).hint_text("optional").desired_width(240.0));
            ui.end_row();
            ui.label(RichText::new("Interface").color(theme::MUTED));
            ui.add(
                egui::TextEdit::singleline(&mut d.interface)
                    .hint_text("the system's choice")
                    .desired_width(240.0),
            )
            .on_hover_text("Network interface to check from (e.g. en1). Used from phase 4 on.");
            ui.end_row();
            ui.label(RichText::new("Criticality").color(theme::MUTED));
            ui.horizontal(|ui| {
                for c in Criticality::ALL {
                    ui.selectable_value(&mut d.criticality, c, c.label());
                }
            })
            .response
            .on_hover_text("Critical down: red. Important down or critical degraded: yellow. Optional: at most yellow.");
            ui.end_row();
            ui.label(RichText::new("Checked").color(theme::MUTED));
            ui.checkbox(&mut d.enabled, "Enabled");
            ui.end_row();
            ui.label(RichText::new("Debounce").color(theme::MUTED));
            ui.horizontal(|ui| {
                ui.label("red after");
                ui.add(egui::DragValue::new(&mut d.hysteresis.fail_after).range(1..=20));
                ui.label("failures, green after");
                ui.add(egui::DragValue::new(&mut d.hysteresis.ok_after).range(1..=20));
            });
            ui.end_row();
            ui.label(RichText::new("Notes").color(theme::MUTED));
            ui.add(egui::TextEdit::multiline(&mut d.notes).desired_rows(2).desired_width(240.0));
            ui.end_row();
        });
}

/// One check's settings; true if it should be removed.
fn check_form(ui: &mut egui::Ui, c: &mut Check) -> bool {
    let mut remove = false;
    egui::Frame::new()
        .fill(theme::CARD)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                match &mut c.kind {
                    CheckKind::Ping => {
                        ui.label(RichText::new("Ping").strong());
                    }
                    CheckKind::Tcp { port } => {
                        ui.label(RichText::new("TCP port").strong());
                        ui.add(egui::DragValue::new(port).range(1..=65535));
                    }
                }
                ui.checkbox(&mut c.enabled, "on");
                ui.checkbox(&mut c.critical, "critical")
                    .on_hover_text("Down here means the device is down (otherwise only degraded while other checks pass)");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Remove").clicked() {
                        remove = true;
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("every").color(theme::MUTED));
                let mut i = c.interval();
                if ui
                    .add(egui::DragValue::new(&mut i).range(0.5..=3600.0).speed(0.1).suffix(" s"))
                    .changed()
                {
                    c.interval_s = Some(i);
                }
                ui.label(RichText::new("timeout").color(theme::MUTED));
                let mut t = c.timeout_ms();
                if ui
                    .add(egui::DragValue::new(&mut t).range(50..=30_000).speed(10).suffix(" ms"))
                    .changed()
                {
                    c.timeout_ms = Some(t);
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("yellow over").color(theme::MUTED));
                ui.add(egui::DragValue::new(&mut c.thresholds.latency_ms).range(0.1..=5000.0).speed(0.5).suffix(" ms"))
                    .on_hover_text("Latency");
                if c.kind == CheckKind::Ping {
                    ui.add(egui::DragValue::new(&mut c.thresholds.jitter_ms).range(0.1..=5000.0).speed(0.5).suffix(" ms jitter"));
                    ui.add(egui::DragValue::new(&mut c.thresholds.loss_pct).range(0.0..=100.0).speed(0.5).suffix(" % loss"));
                }
            });
        });
    ui.add_space(4.0);
    remove
}
