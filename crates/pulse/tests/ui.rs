//! UI tests: start screen, new profile, the demo on the monitor page (light,
//! reasons, tiles, detail, acknowledging), the inventory editor and the
//! event log. `PULSE_SHOTS=<folder>` saves pictures.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pulse::{App, Page};
use pulse_core::Status;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// All tests share one settings folder (never the real one).
fn config() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap().keep();
        std::env::set_var("PULSE_CONFIG_DIR", &dir);
        dir
    })
}

fn harness() -> Harness<'static, App> {
    config();
    let mut app = App::new(&egui::Context::default(), None);
    app.dialogs = false;
    // New profiles go to a folder of their own (never the real Documents).
    app.start.location = tempfile::tempdir().unwrap().keep();
    let mut b = Harness::builder().with_size(egui::vec2(1400.0, 860.0));
    if std::env::var_os("PULSE_SHOTS").is_some() {
        b = b.wgpu();
    }
    let mut h = b.build_ui_state(
        |ui, app: &mut App| {
            app.logic(ui.ctx());
            app.ui_root(ui);
        },
        app,
    );
    settle(&mut h);
    h
}

fn settle(h: &mut Harness<'static, App>) {
    for _ in 0..4 {
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Step until `f` holds (the monitor works in the background).
fn wait(h: &mut Harness<'static, App>, what: &str, secs: u64, f: impl Fn(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        h.step();
        if f(h.state()) {
            settle(h);
            return;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    panic!("timed out waiting for {what}");
}

fn shot(h: &mut Harness<'static, App>, name: &str) {
    let Some(dir) = std::env::var_os("PULSE_SHOTS").map(PathBuf::from) else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    match h.render() {
        Ok(img) => {
            let _ = img.save(dir.join(format!("{name}.png")));
        }
        Err(e) => eprintln!("no picture of {name}: {e}"),
    }
}

fn light(app: &App) -> Status {
    app.overview().map(|o| o.light).unwrap_or(Status::Unknown)
}

fn device_status(app: &App, name: &str) -> Status {
    app.tiles()
        .into_iter()
        .find(|t| t.name == name)
        .map(|t| t.status)
        .unwrap_or(Status::Unknown)
}

#[test]
fn start_screen_offers_new_open_and_demo() {
    let mut h = harness();
    h.get_by_label("New profile");
    h.get_by_label("Open profile");
    h.get_by_label("Try the demo");
    shot(&mut h, "start");
}

#[test]
fn a_new_profile_is_saved_and_opens_the_inventory() {
    let mut h = harness();
    h.get_by_label("New profile").click();
    settle(&mut h);
    h.get_by_label("Venue or show").type_text("School hall");
    settle(&mut h);
    shot(&mut h, "new-profile");
    h.get_by_label("Create profile").click();
    settle(&mut h);
    let path = h.state().doc.as_ref().and_then(|d| d.path.clone()).unwrap();
    assert!(path.is_file());
    assert_eq!(path.file_name().unwrap(), "School hall.pulse");
    assert_eq!(h.state().page, Page::Inventory);

    // Add a device and give it an address: saved shortly after.
    h.get_by_label("Add device").click();
    settle(&mut h);
    h.get_by_label("Stagebox").click();
    settle(&mut h);
    let addr = h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Address");
    addr.click();
    addr.type_text("127.0.0.1");
    settle(&mut h);
    wait(&mut h, "saved", 5, |a| {
        a.doc.as_ref().is_some_and(|d| !d.dirty)
    });
    let saved = pulse_core::profile::load(&path).unwrap();
    assert_eq!(saved.devices.len(), 1);
    assert_eq!(saved.devices[0].address, "127.0.0.1");
    assert_eq!(saved.devices[0].area, "Stage");
    // And it is watched at once.
    wait(&mut h, "the stagebox answers", 5, |a| {
        device_status(a, "New stagebox") == Status::Ok
    });
    shot(&mut h, "inventory-one-device");
}

#[test]
fn demo_shows_the_light_with_reasons_tiles_and_details() {
    let mut h = harness();
    h.get_by_label("Try the demo").click();
    settle(&mut h);
    assert_eq!(h.state().page, Page::Monitor);
    // The IEM rack (important) and the projector (optional) never answer: yellow.
    wait(&mut h, "the IEM rack down", 15, |a| {
        device_status(a, "IEM rack") == Status::Down
    });
    wait(&mut h, "the projector down", 5, |a| {
        device_status(a, "Projector") == Status::Down
    });
    wait(&mut h, "the rest green", 5, |a| {
        device_status(a, "FOH desk") == Status::Ok
    });
    assert_eq!(light(h.state()), Status::Warn);
    h.get_by_label_contains("Overall: Warning – IEM rack: not reachable since ");
    h.get_by_label("Device IEM rack: Down");
    h.get_by_label("Device FOH desk: OK");
    shot(&mut h, "monitor-demo");

    // Problems only.
    h.get_by_label("Problems only").click();
    settle(&mut h);
    assert!(h.query_by_label("Device FOH desk: OK").is_none());
    h.get_by_label("Device Projector: Down");
    h.get_by_label("Problems only").click();
    settle(&mut h);

    // Details and acknowledging: both known problems acknowledged -> green.
    h.get_by_label("Device IEM rack: Down").click();
    settle(&mut h);
    h.get_by_label("Check now");
    h.get_by_label_contains("Checks: Reachable");
    shot(&mut h, "detail");
    h.get_by_label("Acknowledge").click();
    wait(&mut h, "acknowledged", 3, |a| {
        a.overview().is_some_and(|o| o.acked.len() == 1)
    });
    h.get_by_label("Device IEM rack: Down (acknowledged)");
    h.get_by_label("Device Projector: Down").click();
    settle(&mut h);
    h.get_by_label("Acknowledge").click();
    wait(&mut h, "all green", 3, |a| light(a) == Status::Ok);
    shot(&mut h, "monitor-acknowledged");

    // An area from the navigation.
    h.get_by_label("Video: OK").click();
    settle(&mut h);
    assert!(h.query_by_label("Device FOH desk: OK").is_none());
    h.get_by_label("Device Camera 1 (NDI): OK");
}

#[test]
fn inventory_bulk_changes_and_delete_with_confirmation() {
    let mut h = harness();
    h.get_by_label("Try the demo").click();
    settle(&mut h);
    h.get_by_label_contains("Inventory").click();
    settle(&mut h);
    assert_eq!(h.state().page, Page::Inventory);
    h.get_by_label("Select all").click();
    settle(&mut h);
    h.get_by_label("13 devices selected");
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Optional")
        .click();
    settle(&mut h);
    let p = h.state().profile().unwrap();
    assert!(p
        .devices
        .iter()
        .all(|d| d.criticality == pulse_core::Criticality::Optional));
    shot(&mut h, "inventory-bulk");
    h.get_by_label("Delete…").click();
    settle(&mut h);
    // Nothing gone before the confirmation.
    assert_eq!(h.state().profile().unwrap().devices.len(), 13);
    h.get_by_label("Cancel").click();
    settle(&mut h);
    assert_eq!(h.state().profile().unwrap().devices.len(), 13);
    h.get_by_label("Select none").click();
    settle(&mut h);
    h.get_by_label("FOH desk").click();
    settle(&mut h);
    shot(&mut h, "inventory-edit");
    h.get_by_label("+ TCP port").click();
    settle(&mut h);
    let d = h.state().profile().unwrap().devices[0].clone();
    assert_eq!(d.checks.len(), 2);
    h.get_by_label("Delete…").click();
    settle(&mut h);
    h.get_by_label("Delete").click();
    settle(&mut h);
    let p = h.state().profile().unwrap();
    assert_eq!(p.devices.len(), 12);
    assert!(p.devices.iter().all(|d| d.name != "FOH desk"));
}

#[test]
fn event_log_lists_changes_and_exports_csv() {
    let mut h = harness();
    h.get_by_label("Try the demo").click();
    settle(&mut h);
    wait(&mut h, "the IEM rack down", 15, |a| {
        device_status(a, "IEM rack") == Status::Down
    });
    h.get_by_label_contains("Event log").click();
    settle(&mut h);
    assert_eq!(h.state().page, Page::Log);
    h.get_by_label("IEM rack");
    shot(&mut h, "log");
    let csv = h.state().log_csv();
    assert!(csv.starts_with("Time,Device,Area,Event,From,To,Cause\n"));
    assert!(
        csv.contains("IEM rack,Stage,Unknown -> Down,Unknown,Down,not reachable"),
        "{csv}"
    );
    let out = tempfile::tempdir().unwrap().keep().join("events.csv");
    assert!(h.state_mut().export_log(&out));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), csv);
}
