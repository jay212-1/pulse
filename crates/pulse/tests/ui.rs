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
    app.lan = false;
    app.demo_events = false;
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
    h.get_by_label("14 devices selected");
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
    assert_eq!(h.state().profile().unwrap().devices.len(), 14);
    h.get_by_label("Cancel").click();
    settle(&mut h);
    assert_eq!(h.state().profile().unwrap().devices.len(), 14);
    h.get_by_label("Select none").click();
    settle(&mut h);
    h.get_by_label("FOH desk").click();
    settle(&mut h);
    shot(&mut h, "inventory-edit");
    h.get_by_label("Add check").click();
    settle(&mut h);
    h.get_by_label("TCP port").click();
    settle(&mut h);
    let d = h.state().profile().unwrap().devices[0].clone();
    assert_eq!(
        d.checks.len(),
        3,
        "ping, console query and the new TCP port"
    );
    h.get_by_label("Delete…").click();
    settle(&mut h);
    h.get_by_label("Delete").click();
    settle(&mut h);
    let p = h.state().profile().unwrap();
    assert_eq!(p.devices.len(), 13);
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

#[test]
fn presets_bring_their_checks_and_new_check_kinds_can_be_edited() {
    let mut h = harness();
    h.get_by_label("Try the demo").click();
    settle(&mut h);
    h.get_by_label_contains("Inventory").click();
    settle(&mut h);
    h.get_by_label("Add from preset").click();
    settle(&mut h);
    h.get_by_label("Behringer X32 / Midas M32").click();
    settle(&mut h);
    let p = h.state().profile().unwrap();
    let d = p.devices.last().unwrap();
    assert_eq!(d.kind, pulse_core::DeviceKind::Console);
    assert_eq!(
        d.checks[1].kind,
        pulse_core::CheckKind::Console {
            model: pulse_core::ConsoleModel::X32,
            port: None
        }
    );
    h.get_by_label("Console query");
    h.get_by_label("Add check").click();
    settle(&mut h);
    h.get_by_label("Art-Net universe").click();
    settle(&mut h);
    let d = h.state().profile().unwrap().devices.last().unwrap().clone();
    assert!(matches!(
        d.checks[2].kind,
        pulse_core::CheckKind::ArtNetUniverse { universe: 0, .. }
    ));
    h.get_by_label_contains("0 (0:0:0)");
    shot(&mut h, "inventory-preset");
}

#[test]
fn discovery_lists_new_known_and_missing_devices_and_adds_with_one_click() {
    let mut h = harness();
    h.get_by_label("Try the demo").click();
    settle(&mut h);
    // An X32 nobody entered, and an Art-Net node at the demo's address.
    let now = pulse_net::monitor::now_ms();
    h.state().monitor.as_ref().unwrap().inject(|f| {
        let ip = std::net::Ipv4Addr::new(192, 0, 2, 99);
        f.consoles.insert(
            (ip, pulse_core::ConsoleModel::X32),
            pulse_net::found::ConsoleAnswer {
                model: pulse_core::ConsoleModel::X32,
                ip,
                name: "Monitor X32".into(),
                detail: "X32 · firmware 4.06".into(),
                at: now,
            },
        );
        f.artnet.nodes.insert(
            std::net::Ipv4Addr::LOCALHOST,
            (
                pulse_core::artnet::PollReply {
                    ip: std::net::Ipv4Addr::LOCALHOST,
                    short_name: "Node DS".into(),
                    ..Default::default()
                },
                now,
            ),
        );
    });
    settle(&mut h);
    h.get_by_label("Discovery  (1 new)").click();
    settle(&mut h);
    assert_eq!(h.state().page, Page::Discovery);
    h.get_by_label("Monitor X32");
    h.get_by_label("NEW (1)");
    h.get_by_label("IN THE INVENTORY (1)");
    h.get_by_label("Scan subnet…");
    shot(&mut h, "discovery");
    h.get_by_label("Add").click();
    settle(&mut h);
    let p = h.state().profile().unwrap();
    let d = p.devices.last().unwrap();
    assert_eq!(d.name, "Monitor X32");
    assert_eq!(d.address, "192.0.2.99");
    assert_eq!(d.checks.len(), 2);
    h.get_by_label("NEW (0)");
    h.get_by_role_and_label(egui::accesskit::Role::Tab, "Discovery");
    // The new desk is expected to answer its query: it is listed as not seen
    // once its answer is older than a minute.
    h.get_by_label_contains("EXPECTED, NOT SEEN");
}

#[test]
fn pre_show_check_signs_off_and_writes_the_report() {
    let mut h = harness();
    let reports = tempfile::tempdir().unwrap().keep();
    h.state_mut().report_dir = Some(reports.clone());
    h.get_by_label("Try the demo").click();
    settle(&mut h);
    wait(&mut h, "the dead devices down", 20, |a| {
        device_status(a, "IEM rack") == Status::Down
            && device_status(a, "Projector") == Status::Down
    });
    h.get_by_label("Pre-show check").click();
    settle(&mut h);
    assert_eq!(h.state().page, Page::PreShow);
    h.get_by_label("Start pre-show check").click();
    settle(&mut h);
    wait(&mut h, "every check ran", 30, |a| {
        a.preshow.finished.is_some()
    });
    h.get_by_label("Pre-show check  (done)");
    h.get_by_label("Your name").click();
    h.get_by_label("Your name").type_text("Test Person");
    settle(&mut h);
    // Two open problems: one acknowledged as known, the other released open.
    h.get_by_label("Known: IEM rack").click();
    wait(&mut h, "acknowledged", 3, |a| {
        a.overview().is_some_and(|o| o.acked.len() == 1)
    });
    h.get_by_label("Release with 1 open problem (listed in the report)")
        .click();
    settle(&mut h);
    shot(&mut h, "preshow");
    h.get_by_label("Technology released").click();
    settle(&mut h);
    let s = h.state().preshow.signed.clone().unwrap();
    assert_eq!(s.by, "Test Person");
    assert!(s.error.is_none(), "{:?}", s.error);
    let html = std::fs::read_to_string(s.html.unwrap()).unwrap();
    assert!(html.contains("Technology released by <b>Test Person</b>"));
    assert!(html.contains("Released with 2 known problems"), "{html}");
    assert!(html.contains("Acknowledged as known: known at the pre-show check"));
    let csv = std::fs::read_to_string(s.csv.unwrap()).unwrap();
    assert!(csv.lines().count() >= 14);
    assert!(s.at > 0);
    h.get_by_label_contains("Report: ");
    assert!(h.state().preshow.started.is_none());
    shot(&mut h, "preshow-signed");
}

#[test]
fn show_mode_is_calm_and_the_light_window_opens() {
    let mut h = harness();
    h.get_by_label("Try the demo").click();
    settle(&mut h);
    wait(&mut h, "the IEM rack down", 15, |a| {
        device_status(a, "IEM rack") == Status::Down
    });
    h.get_by_role_and_label(egui::accesskit::Role::Button, "SHOW")
        .click();
    settle(&mut h);
    assert_eq!(h.state().mode, pulse::Mode::Show);
    h.get_by_label("Show: Warning");
    h.get_by_label("Problem IEM rack: Down");
    // OK devices are not shown one by one, and there is no navigation.
    assert!(h.query_by_label("Device FOH desk: OK").is_none());
    assert!(h.query_by_label("Inventory  (14)").is_none());
    h.get_by_label("Sound on new problems");
    assert!(!h.state().settings.sound, "the sound is off by default");
    shot(&mut h, "show");
    h.get_by_label("Light window").click();
    settle(&mut h);
    h.get_by_label("Light window: Warning");
    shot(&mut h, "light-window");
    // In tests the window is embedded over the main one; closing it there:
    h.state_mut().show.light_window = false;
    settle(&mut h);
    assert!(h.query_by_label("Light window: Warning").is_none());
    h.get_by_role_and_label(egui::accesskit::Role::Button, "CHECK")
        .click();
    settle(&mut h);
    h.get_by_label("Inventory  (14)");
}

#[test]
fn diagnose_shows_graphs_hints_and_tests_a_port() {
    let mut h = harness();
    h.get_by_label("Try the demo").click();
    settle(&mut h);
    wait(&mut h, "the IEM rack down", 15, |a| {
        device_status(a, "IEM rack") == Status::Down
    });
    h.get_by_role_and_label(egui::accesskit::Role::Button, "DIAGNOSE")
        .click();
    settle(&mut h);
    // Problems first: the IEM rack (TEST-NET, outside this computer's networks).
    h.get_by_label_contains("No answer at all");
    h.get_by_label("FOH desk").click();
    settle(&mut h);
    wait(&mut h, "a few results", 10, |a| {
        a.monitor.as_ref().unwrap().with(|s| {
            let d = s
                .profile
                .devices
                .iter()
                .find(|d| d.name == "FOH desk")
                .unwrap();
            s.history
                .series(d.checks[0].id, pulse_core::Metric::LatencyMs)
                .is_some_and(|x| x.len() >= 3)
        })
    });
    h.get_by_label_contains("Graph Latency: ");
    h.get_by_label_contains("Graph Loss: ");
    assert!(h.query_by_label("Graph Latency: no data").is_none());
    shot(&mut h, "diagnose");
    h.get_by_label("Test port").click();
    wait(&mut h, "the port test", 5, |a| {
        a.diagnose
            .port_result
            .as_ref()
            .and_then(|(_, r)| r.lock().unwrap().clone())
            .is_some()
    });
    h.get_by_label_contains("Port 80");
}

/// A plain GET (the status page closes after each answer).
fn http_get(url: &str) -> String {
    use std::io::{Read, Write};
    let rest = url.strip_prefix("http://").unwrap();
    let (host, path) = rest.split_once('/').unwrap();
    let mut s = std::net::TcpStream::connect(host).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    write!(
        s,
        "GET /{path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

#[test]
fn the_status_page_shows_the_open_profile() {
    let mut h = harness();
    h.get_by_label("Try the demo").click();
    settle(&mut h);
    wait(&mut h, "the IEM rack down", 15, |a| {
        device_status(a, "IEM rack") == Status::Down
    });
    h.get_by_label("Status page").click();
    settle(&mut h);
    let port = h.state().status_page.as_ref().unwrap().port;
    let url = format!("http://127.0.0.1:{port}/");
    h.get_by_label(&url);
    assert!(http_get(&url).contains("Pulse status page"));
    let api = http_get(&format!("{url}api/status"));
    assert!(
        api.contains("\"profile\":\"Demo: Riverside Hall\""),
        "{api}"
    );
    assert!(api.contains("\"name\":\"IEM rack\""), "{api}");
    // Read-only.
    {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(
            s,
            "POST /api/status HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n"
        )
        .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        assert!(out.starts_with("HTTP/1.1 405"), "{out}");
    }
    shot(&mut h, "status_page_on");
    h.get_by_label("Status page").click();
    settle(&mut h);
    assert!(h.state().status_page.is_none());
    assert!(!h.state().settings.status_page);
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
}

#[test]
fn devices_come_over_from_a_blueprint_exchange_file() {
    let mut h = harness();
    h.get_by_label("Try the demo").click();
    settle(&mut h);
    h.state_mut().page = Page::Inventory;
    settle(&mut h);
    h.get_by_label("Import from Blueprint…");
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Harbour.showtime.json");
    std::fs::write(
        &file,
        r#"{"format":"showtime-exchange","version":1,"production":{"name":"Harbour","venue":"x"},
           "devices":[
             {"name":"Si Impact","kind":"console","location":"FOH","model":"Soundcraft Si Impact","address":"127.0.0.1"},
             {"name":"MA3","kind":"console","location":"Light","model":"grandMA3 light","address":""},
             {"name":"FOH desk","kind":"console","location":"FOH","model":"X32","address":"10.0.0.1"}
           ]}"#,
    )
    .unwrap();
    h.state_mut().import_exchange(Some(file));
    settle(&mut h);
    h.get_by_label("Blueprint: 2 devices added, 1 without an address (switched off)");
    let p = h.state().profile().unwrap();
    assert_eq!(p.devices.len(), 16);
    let si = p.devices.iter().find(|d| d.name == "Si Impact").unwrap();
    assert!(si
        .checks
        .iter()
        .any(|c| c.kind == pulse_core::CheckKind::Tcp { port: 3804 }));
    // The demo's own FOH desk keeps its address.
    let foh = p.devices.iter().find(|d| d.name == "FOH desk").unwrap();
    assert_eq!(foh.address, "127.0.0.1");
    h.state_mut()
        .import_exchange(Some(dir.path().join("missing.json")));
    settle(&mut h);
    h.get_by_label_contains("Cannot import: ");
}
