//! The window: the open profile, its monitor, the pages (monitor,
//! inventory, event log), the File menu, the navigation and the status bar.

use crate::app_diagnose::DiagnoseState;
use crate::app_discovery::DiscoveryState;
use crate::app_inventory::InventoryState;
use crate::app_preshow::PreShowState;
use crate::app_show::ShowState;
use crate::app_start::StartState;
use crate::settings::Settings;
use crate::theme;
use egui::{RichText, ViewportCommand};
use pulse_core::events::Filter;
use pulse_core::model::{Id, Profile};
use pulse_core::status::Ms;
use pulse_core::Status;
use pulse_net::{Monitor, Options};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A change is written this long after the last edit.
const SAVE_AFTER: Duration = Duration::from_millis(800);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Monitor,
    PreShow,
    Inventory,
    Discovery,
    Log,
}

/// CHECK (set up and check), SHOW (calm, during the show), DIAGNOSE (graphs and tools).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Check,
    Show,
    Diagnose,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Check, Mode::Show, Mode::Diagnose];
    pub fn label(self) -> &'static str {
        match self {
            Mode::Check => "CHECK",
            Mode::Show => "SHOW",
            Mode::Diagnose => "DIAGNOSE",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Mode::Check => "Set up and check: monitor, pre-show check, inventory, discovery, log",
            Mode::Show => "During the show: calm, only problems large, no dialogs",
            Mode::Diagnose => "One device in depth: graphs, raw data, hints, port test, traceroute",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Area,
    Status,
    Name,
}

impl Sort {
    pub const ALL: [Sort; 3] = [Sort::Area, Sort::Status, Sort::Name];
    pub fn label(self) -> &'static str {
        match self {
            Sort::Area => "By area",
            Sort::Status => "Problems first",
            Sort::Name => "By name",
        }
    }
}

/// What the monitor page shows.
#[derive(Clone, Debug, Default)]
pub struct TileFilter {
    pub text: String,
    pub problems_only: bool,
    pub area: Option<String>,
    pub sort: Sort,
}

/// The open profile.
pub struct Doc {
    pub profile: Profile,
    /// `None`: not saved yet (the demo).
    pub path: Option<PathBuf>,
    pub dirty: bool,
    changed: Instant,
}

impl Doc {
    pub fn title(&self) -> String {
        if !self.profile.name.trim().is_empty() {
            return self.profile.name.clone();
        }
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".into())
    }
}

pub struct App {
    pub settings: Settings,
    pub doc: Option<Doc>,
    pub monitor: Option<Monitor>,
    pub page: Page,
    pub mode: Mode,
    /// Device in the detail panel.
    pub selected: Option<Id>,
    pub tiles: TileFilter,
    pub log_filter: Filter,
    pub start: StartState,
    pub inventory: InventoryState,
    pub discovery: DiscoveryState,
    pub preshow: PreShowState,
    pub show: ShowState,
    pub diagnose: DiagnoseState,
    /// Reports go here (default: next to the profile, in "Reports").
    pub report_dir: Option<PathBuf>,
    /// The monitor was told the discovery page is open.
    discovering: bool,
    /// This computer's interfaces (refreshed now and then).
    nics: (Vec<pulse_net::Nic>, Option<Instant>),
    /// Note typed for an acknowledgement.
    pub ack_note: String,
    /// File dialogs (off in tests).
    pub dialogs: bool,
    /// The demo's simulated devices.
    sim: Option<pulse_sim::Sim>,
    /// The demo's scenario (something goes wrong now and then; off in tests).
    pub demo_events: bool,
    /// Discovery on this computer's networks (mDNS, broadcasts; off in tests).
    pub lan: bool,
    /// A short message in the status bar: text, colour, when.
    pub notice: Option<(String, egui::Color32, Instant)>,
    /// The status page, while switched on.
    pub status_page: Option<pulse_net::status_page::StatusPage>,
    /// What the status page shows (the open profile's monitor).
    page_source: pulse_net::status_page::Source,
    ctx: egui::Context,
}

impl App {
    pub fn new(ctx: &egui::Context, open: Option<PathBuf>) -> Self {
        theme::apply(ctx);
        let mut app = App {
            settings: Settings::load(),
            doc: None,
            monitor: None,
            page: Page::Monitor,
            mode: Mode::Check,
            selected: None,
            tiles: TileFilter::default(),
            log_filter: Filter::default(),
            start: StartState::default(),
            inventory: InventoryState::default(),
            discovery: DiscoveryState::default(),
            preshow: PreShowState::default(),
            show: ShowState::default(),
            diagnose: DiagnoseState::default(),
            report_dir: None,
            discovering: false,
            nics: (Vec::new(), None),
            ack_note: String::new(),
            dialogs: true,
            lan: true,
            sim: None,
            demo_events: true,
            notice: None,
            status_page: None,
            page_source: Arc::default(),
            ctx: ctx.clone(),
        };
        if app.settings.status_page {
            app.set_status_page(true);
        }
        if let Some(p) = open {
            app.open(Some(p));
        }
        app
    }

    // ------------------------------------------------------------ profile

    /// Watch `profile` (stops the previous monitor).
    pub fn start_document(&mut self, profile: Profile, path: Option<PathBuf>) {
        self.monitor = None;
        self.sim = None;
        self.discovering = false;
        self.mode = Mode::Check;
        self.preshow = PreShowState::default();
        self.show = ShowState::default();
        let ctx = self.ctx.clone();
        let mut opts = Options {
            on_change: Some(Arc::new(move || ctx.request_repaint())),
            ..Options::default()
        };
        if !self.lan {
            opts.lan = false;
            opts.mdns = false;
            opts.artnet_port = 0;
            opts.sacn_port = 0;
        }
        self.monitor = match Monitor::start(profile.clone(), opts) {
            Ok(m) => Some(m),
            Err(e) => {
                self.say(format!("Could not start monitoring: {e}"), theme::ERROR);
                None
            }
        };
        self.sync_page_source();
        self.doc = Some(Doc {
            profile,
            path,
            dirty: false,
            changed: Instant::now(),
        });
        self.page = Page::Monitor;
        self.selected = None;
        self.tiles = TileFilter::default();
        self.log_filter = Filter::default();
        self.inventory = InventoryState::default();
    }

    /// The demo venue on the simulator (without it: the plain demo profile).
    pub fn open_demo(&mut self) {
        match pulse_sim::demo::start() {
            Ok((profile, sim)) => {
                self.start_document(profile, None);
                if let Some(m) = &self.monitor {
                    sim.attach(m.found_handle());
                }
                if self.demo_events {
                    let (steps, period) = pulse_sim::demo::scenario();
                    sim.scenario(steps, period);
                }
                self.sim = Some(sim);
            }
            Err(e) => {
                self.start_document(pulse_core::demo::profile(), None);
                self.say(format!("Simulator not available: {e}"), theme::WARN);
            }
        }
    }

    /// Open a profile (`None`: ask for one).
    pub fn open(&mut self, path: Option<PathBuf>) {
        let path = match path {
            Some(p) => p,
            None => {
                if !self.dialogs {
                    return;
                }
                let mut d = rfd::FileDialog::new()
                    .add_filter("Pulse profile", &[pulse_core::profile::EXTENSION]);
                if let Some(dir) = &self.settings.last_dir {
                    d = d.set_directory(dir);
                }
                match d.pick_file() {
                    Some(p) => p,
                    None => return,
                }
            }
        };
        match pulse_core::profile::load(&path) {
            Ok(p) => {
                self.settings.remember(&path);
                self.start.error = None;
                self.start_document(p, Some(path));
            }
            Err(e) => {
                let msg = format!("Could not open {}: {e}", path.display());
                if self.doc.is_none() {
                    self.start.error = Some(msg);
                } else {
                    self.say(msg, theme::ERROR);
                }
            }
        }
    }

    /// Write the profile to `path` (or its own path). True if saved.
    pub fn save_to(&mut self, path: Option<&Path>) -> bool {
        let Some(doc) = &mut self.doc else {
            return false;
        };
        let Some(path) = path.map(Path::to_path_buf).or_else(|| doc.path.clone()) else {
            return false;
        };
        match pulse_core::profile::save(&doc.profile, &path) {
            Ok(()) => {
                doc.path = Some(path.clone());
                doc.dirty = false;
                self.settings.remember(&path);
                true
            }
            Err(e) => {
                self.say(format!("Could not save: {e}"), theme::ERROR);
                false
            }
        }
    }

    pub fn save(&mut self) {
        if self.doc.as_ref().is_some_and(|d| d.path.is_none()) {
            self.save_as();
        } else if self.save_to(None) {
            self.say("Saved".into(), theme::OK);
        }
    }

    /// Save under a new name (also: export the profile).
    pub fn save_as(&mut self) {
        if !self.dialogs {
            return;
        }
        let Some(doc) = &self.doc else { return };
        let name = format!(
            "{}.{}",
            crate::app_start::clean_name(&doc.title()),
            pulse_core::profile::EXTENSION
        );
        let dir = doc
            .path
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(|| self.settings.last_dir.clone())
            .unwrap_or_else(crate::settings::default_profile_dir);
        let _ = std::fs::create_dir_all(&dir);
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Pulse profile", &[pulse_core::profile::EXTENSION])
            .set_directory(dir)
            .set_file_name(name)
            .save_file()
        {
            if self.save_to(Some(&p)) {
                self.say(format!("Saved as {}", p.display()), theme::OK);
            }
        }
    }

    pub fn close_profile(&mut self) {
        if self
            .doc
            .as_ref()
            .is_some_and(|d| d.dirty && d.path.is_some())
        {
            self.save_to(None);
        }
        self.monitor = None;
        self.sim = None;
        self.doc = None;
        self.sync_page_source();
    }

    /// The status page shows the monitor of the open profile.
    fn sync_page_source(&self) {
        if let Ok(mut s) = self.page_source.lock() {
            *s = self.monitor.as_ref().map(|m| m.shared_handle());
        }
    }

    /// Switch the status page on or off (and remember it). On this
    /// computer's networks on TCP 7470; without LAN (tests) on 127.0.0.1.
    pub fn set_status_page(&mut self, on: bool) {
        if on && self.status_page.is_none() {
            let (bind, port) = if self.lan {
                (
                    std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
                    pulse_net::status_page::PORT,
                )
            } else {
                (std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 0)
            };
            match pulse_net::status_page::StatusPage::start(
                bind,
                port,
                self.page_source.clone(),
                self.lan,
            ) {
                Ok(p) => self.status_page = Some(p),
                Err(e) => {
                    self.say(
                        format!("Status page: cannot listen on TCP {port}: {e}"),
                        theme::ERROR,
                    );
                    return;
                }
            }
        } else if !on {
            self.status_page = None;
        }
        if self.settings.status_page != on {
            self.settings.status_page = on;
            self.settings.save();
        }
    }

    /// This computer's interfaces (re-read every few seconds).
    pub fn nics(&mut self) -> Vec<pulse_net::Nic> {
        if self
            .nics
            .1
            .is_none_or(|t| t.elapsed() > Duration::from_secs(3))
        {
            self.nics = (pulse_net::interfaces::list(), Some(Instant::now()));
        }
        self.nics.0.clone()
    }

    /// Take over the devices of a Showtime exchange file (from Blueprint).
    /// `None`: ask for the file.
    pub fn import_exchange(&mut self, path: Option<PathBuf>) {
        let path = match path {
            Some(p) => p,
            None => {
                if !self.dialogs {
                    return;
                }
                let mut d = rfd::FileDialog::new()
                    .add_filter("Showtime exchange file", &["json"])
                    .set_title("Import devices from Blueprint");
                if let Some(dir) = &self.settings.last_dir {
                    d = d.set_directory(dir);
                }
                match d.pick_file() {
                    Some(p) => p,
                    None => return,
                }
            }
        };
        let read = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| pulse_core::exchange::read(&t));
        match read {
            Ok(x) => {
                let mut r = pulse_core::exchange::Imported::default();
                self.edit(|p| r = p.import_exchange(&x));
                self.say(format!("Blueprint: {}", r.summary()), theme::MUTED);
            }
            Err(e) => self.say(format!("Cannot import: {e}"), theme::ERROR),
        }
    }

    /// Change the inventory: marks it unsaved and hands it to the monitor.
    pub fn edit(&mut self, f: impl FnOnce(&mut Profile)) {
        let Some(doc) = &mut self.doc else { return };
        let before = doc.profile.clone();
        f(&mut doc.profile);
        if doc.profile == before {
            return;
        }
        doc.dirty = true;
        doc.changed = Instant::now();
        if let Some(m) = &self.monitor {
            m.set_profile(doc.profile.clone());
        }
    }

    pub fn profile(&self) -> Option<&Profile> {
        self.doc.as_ref().map(|d| &d.profile)
    }

    pub(crate) fn say(&mut self, text: String, color: egui::Color32) {
        self.notice = Some((text, color, Instant::now()));
    }

    // ------------------------------------------------------------ frame

    /// Per frame, before drawing: shortcuts, dropped files, saving.
    pub fn logic(&mut self, ctx: &egui::Context) {
        let cmd = egui::Modifiers::COMMAND;
        let (save, open, new) = ctx.input_mut(|i| {
            (
                i.consume_key(cmd, egui::Key::S),
                i.consume_key(cmd, egui::Key::O),
                i.consume_key(cmd, egui::Key::N),
            )
        });
        if save {
            self.save();
        }
        if open {
            self.open(None);
        }
        if new {
            self.close_profile();
            self.start.form = true;
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if let Some(p) = dropped.into_iter().find(|p| {
            p.extension()
                .is_some_and(|e| e == pulse_core::profile::EXTENSION)
        }) {
            self.close_profile();
            self.open(Some(p));
        }
        // Edits are written shortly after the last one.
        let due = self
            .doc
            .as_ref()
            .is_some_and(|d| d.dirty && d.path.is_some() && d.changed.elapsed() >= SAVE_AFTER);
        if due {
            self.save_to(None);
        }
        if self
            .doc
            .as_ref()
            .is_some_and(|d| d.dirty && d.path.is_some())
        {
            ctx.request_repaint_after(SAVE_AFTER);
        }
        self.show_alerts();
        self.light_window(ctx);
        // The monitor listens to everything while the discovery page is open.
        let want = self.mode == Mode::Check && self.page == Page::Discovery && self.doc.is_some();
        if want != self.discovering {
            if let Some(m) = &self.monitor {
                m.set_discovery(want);
            }
            self.discovering = want;
        }
        if self.doc.is_some() {
            // "seen 3 s ago" keeps counting.
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.doc.is_some() {
            self.close_profile();
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    pub fn ui_root(&mut self, ui: &mut egui::Ui) {
        if self.doc.is_none() {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(theme::BG))
                .show(ui, |ui| self.start_page(ui));
            return;
        }
        egui::Panel::top("top_bar")
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status_bar")
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ui, |ui| self.status_bar(ui));
        if self.mode == Mode::Check {
            egui::Panel::left("nav")
                .resizable(false)
                .exact_size(200.0)
                .frame(
                    egui::Frame::NONE
                        .fill(theme::PANEL)
                        .inner_margin(egui::Margin::symmetric(10, 12)),
                )
                .show(ui, |ui| self.nav(ui));
        }
        let detail_here = match self.mode {
            Mode::Check => self.page == Page::Monitor,
            Mode::Show => true,
            Mode::Diagnose => false,
        };
        if detail_here
            && self
                .selected
                .is_some_and(|id| self.profile().is_some_and(|p| p.device(id).is_some()))
        {
            egui::Panel::right("detail")
                .resizable(false)
                .exact_size(380.0)
                .frame(
                    egui::Frame::NONE
                        .fill(theme::PANEL)
                        .inner_margin(egui::Margin::symmetric(14, 12)),
                )
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.detail(ui));
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(18, 14)),
            )
            .show(ui, |ui| match (self.mode, self.page) {
                (Mode::Show, _) => self.show_page(ui),
                (Mode::Diagnose, _) => self.diagnose_page(ui),
                (Mode::Check, Page::Monitor) => self.monitor_page(ui),
                (Mode::Check, Page::PreShow) => self.preshow_page(ui),
                (Mode::Check, Page::Inventory) => self.inventory_page(ui),
                (Mode::Check, Page::Discovery) => self.discovery_page(ui),
                (Mode::Check, Page::Log) => self.log_page(ui),
            });
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Pulse").strong().color(theme::BRAND).size(16.0));
            ui.separator();
            ui.menu_button("File", |ui| self.file_menu(ui));
            let Some(doc) = &self.doc else { return };
            ui.label(RichText::new(doc.title()).strong().size(15.0));
            if doc.path.is_none() {
                ui.label(RichText::new("not saved").small().color(theme::MUTED));
            }
            ui.add_space(16.0);
            for m in Mode::ALL {
                let on = self.mode == m;
                let b = egui::Button::new(RichText::new(m.label()).strong().color(if on {
                    theme::ON_BRAND
                } else {
                    theme::MUTED
                }))
                .fill(if on { theme::BRAND } else { theme::CARD })
                .selected(on);
                if ui.add(b).on_hover_text(m.hint()).clicked() {
                    self.mode = m;
                }
            }
            ui.add_space(8.0);
            let on = self.show.light_window;
            if ui
                .add(egui::Button::new("Light window").selected(on))
                .on_hover_text("The traffic light in a window of its own, e.g. for a second monitor (F: fullscreen)")
                .clicked()
            {
                self.show.light_window = !on;
            }
            let method = self.monitor.as_ref().map(|m| m.with(|s| s.method.clone()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(m) = method {
                    let (text, color) = match &m {
                        pulse_net::Method::Icmp => ("Ping: ICMP".to_string(), theme::MUTED),
                        pulse_net::Method::Tcp { .. } => ("Ping: TCP fallback".to_string(), theme::WARN),
                    };
                    ui.label(RichText::new(text).small().color(color)).on_hover_text(format!(
                        "How Pulse pings: {}.\nICMP needs no administrator rights here; without it, Pulse tries a TCP connection to port 80 (a refused connection also proves the device is there).",
                        m.label()
                    ));
                }
            });
        });
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(220.0);
        if ui.button("Start screen").clicked() {
            ui.close();
            self.close_profile();
        }
        ui.separator();
        if ui
            .add(egui::Button::new("New profile…").shortcut_text("Cmd/Ctrl+N"))
            .clicked()
        {
            ui.close();
            self.close_profile();
            self.start.form = true;
        }
        if ui
            .add(egui::Button::new("Open…").shortcut_text("Cmd/Ctrl+O"))
            .clicked()
        {
            ui.close();
            self.open(None);
        }
        let recent = self.settings.recent.clone();
        ui.add_enabled_ui(!recent.is_empty(), |ui| {
            ui.menu_button("Open recent", |ui| {
                for p in &recent {
                    let name = p
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let r = ui
                        .add_enabled(p.is_file(), egui::Button::new(name))
                        .on_hover_text(p.display().to_string());
                    if r.clicked() {
                        ui.close();
                        self.close_profile();
                        self.open(Some(p.clone()));
                    }
                }
            });
        });
        ui.separator();
        if ui
            .add(egui::Button::new("Save").shortcut_text("Cmd/Ctrl+S"))
            .clicked()
        {
            ui.close();
            self.save();
        }
        if ui
            .button("Save as / export…")
            .on_hover_text("A copy of the profile for another computer or venue")
            .clicked()
        {
            ui.close();
            self.save_as();
        }
        ui.separator();
        if ui.button("Close profile").clicked() {
            ui.close();
            self.close_profile();
        }
    }

    fn nav(&mut self, ui: &mut egui::Ui) {
        let (light, areas, devices, events) = match &self.monitor {
            Some(m) => m.with(|s| {
                (
                    s.overview.light,
                    s.overview
                        .areas
                        .iter()
                        .map(|a| (a.area.clone(), a.light))
                        .collect::<Vec<_>>(),
                    s.profile.devices.len(),
                    s.log.len(),
                )
            }),
            None => (Status::Unknown, Vec::new(), 0, 0),
        };
        let section = |ui: &mut egui::Ui, t: &str| {
            ui.add_space(8.0);
            ui.label(RichText::new(t).small().color(theme::MUTED));
        };
        section(ui, "SHOW");
        if nav_item(ui, self.page == Page::Monitor, Some(light), "Monitor") {
            self.page = Page::Monitor;
            self.tiles.area = None;
        }
        let preshow = match (&self.preshow.started, &self.preshow.finished) {
            (Some(_), None) => "Pre-show check  (running)".to_string(),
            (Some(_), Some(_)) => "Pre-show check  (done)".to_string(),
            _ => "Pre-show check".to_string(),
        };
        if nav_item(ui, self.page == Page::PreShow, None, &preshow) {
            self.page = Page::PreShow;
        }
        if nav_item(
            ui,
            self.page == Page::Inventory,
            None,
            &format!("Inventory  ({devices})"),
        ) {
            self.page = Page::Inventory;
        }
        let new = self.discovered_new();
        if nav_item(
            ui,
            self.page == Page::Discovery,
            None,
            &if new > 0 {
                format!("Discovery  ({new} new)")
            } else {
                "Discovery".to_string()
            },
        ) {
            self.page = Page::Discovery;
        }
        if nav_item(
            ui,
            self.page == Page::Log,
            None,
            &format!("Event log  ({events})"),
        ) {
            self.page = Page::Log;
        }
        if !areas.is_empty() {
            section(ui, "AREAS");
            for (area, l) in areas {
                let on =
                    self.page == Page::Monitor && self.tiles.area.as_deref() == Some(area.as_str());
                if nav_item(ui, on, Some(l), &area) {
                    self.page = Page::Monitor;
                    self.tiles.area = if on { None } else { Some(area) };
                }
            }
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let now = now_ms();
        let urls = match self.status_page.is_some() {
            true if self.lan => {
                let nics = self.nics();
                self.status_page
                    .as_ref()
                    .map(|p| p.urls(&nics))
                    .unwrap_or_default()
            }
            true => self
                .status_page
                .as_ref()
                .map(|p| vec![format!("http://127.0.0.1:{}/", p.port)])
                .unwrap_or_default(),
            false => Vec::new(),
        };
        ui.horizontal(|ui| {
            if let Some(m) = &self.monitor {
                let (devices, running, load, counts) = m.with(|s| {
                    (
                        s.profile.devices.len(),
                        s.running,
                        s.load,
                        s.overview.counts,
                    )
                });
                let small = |ui: &mut egui::Ui, t: String| {
                    ui.label(RichText::new(t).small().color(theme::MUTED));
                };
                small(ui, format!("{devices} devices"));
                small(
                    ui,
                    format!(
                        "{} OK · {} warning · {} down · {} unknown",
                        counts.ok, counts.warn, counts.down, counts.unknown
                    ),
                );
                let secs = (now.saturating_sub(load.since) as f64 / 1000.0).max(1.0);
                small(
                    ui,
                    format!(
                        "Network: {:.1} packets/s, {:.1} kB/s",
                        load.packet_rate(now),
                        load.bytes as f64 / 1024.0 / secs
                    ),
                );
                if running > 0 {
                    small(
                        ui,
                        format!(
                            "{running} check{} running",
                            if running == 1 { "" } else { "s" }
                        ),
                    );
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut on = self.status_page.is_some();
                let hover = if on {
                    if urls.is_empty() {
                        "Status page on, but this computer has no network address right now.".to_string()
                    } else {
                        format!(
                            "Read-only status page for phones and other computers:\n{}",
                            urls.join("\n")
                        )
                    }
                } else {
                    "A read-only status page on TCP 7470 for phones and other computers of the crew. Nothing can be changed there.".to_string()
                };
                if ui
                    .toggle_value(&mut on, RichText::new("Status page").small())
                    .on_hover_text(hover)
                    .changed()
                {
                    self.set_status_page(on);
                }
                if let Some(u) = urls.first() {
                    ui.label(RichText::new(u).small().monospace().color(theme::MUTED));
                }
                if let Some((text, color, at)) = &self.notice {
                    if at.elapsed() < Duration::from_secs(6) {
                        ui.label(RichText::new(text).small().color(*color));
                    }
                }
                if let Some(d) = &self.doc {
                    let t = match (&d.path, d.dirty) {
                        (None, _) => "Demo – Save as… keeps a copy",
                        (Some(_), true) => "Saving…",
                        (Some(_), false) => "Saved",
                    };
                    ui.label(RichText::new(t).small().color(theme::MUTED));
                }
            });
        });
    }
}

/// A navigation entry with an optional status symbol; true if clicked.
fn nav_item(ui: &mut egui::Ui, on: bool, status: Option<Status>, label: &str) -> bool {
    let w = ui.available_width();
    let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 28.0), egui::Sense::click());
    let text = match status {
        Some(s) => format!("{label}: {}", s.label()),
        None => label.to_string(),
    };
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, on, &text));
    let p = ui.painter();
    if on {
        p.rect_filled(rect, 6.0, theme::ACCENT.gamma_multiply(0.25));
    } else if r.hovered() {
        p.rect_filled(rect, 6.0, egui::Color32::from_rgb(30, 34, 42));
    }
    let mut x = rect.left() + 8.0;
    if let Some(s) = status {
        let sym = egui::Rect::from_center_size(
            egui::pos2(x + 7.0, rect.center().y),
            egui::vec2(14.0, 14.0),
        );
        theme::status_symbol(p, sym, s);
        x += 22.0;
    }
    p.text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(14.0),
        if on {
            theme::TEXT
        } else {
            egui::Color32::from_rgb(200, 204, 212)
        },
    );
    r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Unix time now, in milliseconds.
pub fn now_ms() -> Ms {
    pulse_net::monitor::now_ms()
}

/// Local clock time: "21:42:05".
pub fn clock(ms: Ms) -> String {
    local(ms)
        .map(|t| t.format("%H:%M:%S").to_string())
        .unwrap_or_default()
}

/// Local date and time for exports: "2026-11-21 21:42:05".
pub fn date_time(ms: Ms) -> String {
    local(ms)
        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

fn local(ms: Ms) -> Option<chrono::DateTime<chrono::Local>> {
    let t = chrono::DateTime::from_timestamp_millis(i64::try_from(ms).ok()?)?;
    Some(t.with_timezone(&chrono::Local))
}

/// "3 s ago", "4 min ago", "2 h ago".
pub fn ago(now: Ms, then: Ms) -> String {
    let s = now.saturating_sub(then) / 1000;
    match s {
        0..=59 => format!("{s} s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        _ => format!("{} h ago", s / 3600),
    }
}
