//! Pulse: the network and device monitor of the Showtime Suite. This crate
//! is the window; the logic lives in `pulse-core`, the probes and the
//! scheduler in `pulse-net`.

pub mod app;
mod app_detail;
pub mod app_diagnose;
pub mod app_discovery;
pub mod app_inventory;
mod app_log;
pub mod app_monitor;
pub mod app_preshow;
pub mod app_show;
pub mod app_start;
mod open_files;
mod platform;
pub mod settings;
pub mod theme;

pub use app::{App, Mode, Page, Sort};

use std::path::PathBuf;

fn icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../../../assets/icon.png"))
        .ok()?
        .to_rgba8();
    Some(egui::IconData {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    })
}

struct Window(App);

impl eframe::App for Window {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.0.logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui_root(ui);
    }
}

/// Open the window and run until it is closed. Main thread only.
pub fn run(open: Option<PathBuf>) -> eframe::Result {
    // Early, so a document that launched the app is caught (macOS).
    open_files::install();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Pulse")
        .with_app_id("pulse")
        .with_inner_size([1400.0, 860.0])
        .with_min_inner_size([900.0, 600.0])
        .with_drag_and_drop(true);
    if let Some(icon) = icon() {
        viewport = viewport.with_icon(icon);
    }
    eframe::run_native(
        "Pulse",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(|cc| {
            open_files::install();
            open_files::set_repaint_context(&cc.egui_ctx);
            Ok(Box::new(Window(App::new(&cc.egui_ctx, open))))
        }),
    )
}
