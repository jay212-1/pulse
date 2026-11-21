//! Pulse against the simulator: every kind of check passes, faults turn the
//! right devices yellow or red with the right cause and recover, the demo
//! scenario runs, and 100 devices stay within the packet limit.
//!
//! `cargo test -p pulse-sim --test sim -- --ignored --nocapture` runs the
//! one-minute network load measurement (see `docs/netzlast.md`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pulse_core::model::{CheckKind, ConsoleModel, Hysteresis, Profile};
use pulse_core::status::Status;
use pulse_core::Id;
use pulse_net::{Monitor, Options, Shared};
use pulse_sim::{demo, Action, Kind, Sim, Spec, Step};
use std::time::{Duration, Instant};

fn opts() -> Options {
    Options {
        mdns: false,
        lan: false,
        artnet_port: 0,
        sacn_port: 0,
        ..Options::default()
    }
}

fn wait(m: &Monitor, what: &str, secs: u64, f: impl Fn(&Shared) -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if m.with(&f) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let state = m.with(|s| {
        s.profile
            .devices
            .iter()
            .map(|d| {
                let st = s.engine.device(d.id).unwrap();
                format!("{}: {:?} {}", d.name, st.status, st.cause)
            })
            .collect::<Vec<_>>()
            .join("\n")
    });
    panic!("timed out waiting for {what}\n{state}");
}

fn id(s: &Shared, name: &str) -> Id {
    s.profile
        .devices
        .iter()
        .find(|d| d.name == name)
        .unwrap()
        .id
}

fn status(s: &Shared, name: &str) -> Status {
    s.engine.device(id(s, name)).unwrap().status
}

fn cause(s: &Shared, name: &str) -> String {
    s.engine.device(id(s, name)).unwrap().cause.clone()
}

/// Fast checks: twice a second, short timeouts, red after 2.
fn quick(p: &mut Profile) {
    // The tests break devices on purpose, often: that is no flapping.
    p.rules.flapping.changes = 100;
    for d in &mut p.devices {
        d.hysteresis = Hysteresis {
            fail_after: 2,
            ok_after: 1,
        };
        for c in &mut d.checks {
            c.interval_s = Some(0.4);
            c.timeout_ms = Some(400);
        }
    }
}

fn start_demo() -> (Monitor, Sim) {
    let (mut p, sim) = demo::start().unwrap();
    quick(&mut p);
    let m = Monitor::start(p, opts()).unwrap();
    sim.attach(m.found_handle());
    (m, sim)
}

#[test]
fn every_simulated_device_passes_its_checks() {
    let (m, _sim) = start_demo();
    wait(&m, "all simulated devices OK", 15, |s| {
        s.profile
            .devices
            .iter()
            .filter(|d| !d.address.starts_with("192.0.2."))
            .all(|d| {
                let st = s.engine.device(d.id).unwrap();
                // Every check's last run passed, not only the device light
                // (one miss before the simulator was attached does not count).
                st.status == Status::Ok
                    && st.checks.values().all(|c| {
                        c.last
                            .as_ref()
                            .is_some_and(|r| r.outcome == pulse_core::Outcome::Ok)
                    })
            })
    });
    m.with(|s| {
        // The checks went as deep as they claim.
        let d = s
            .profile
            .devices
            .iter()
            .find(|d| d.name == "FOH desk")
            .unwrap();
        let st = s.engine.device(d.id).unwrap();
        let q = st
            .checks
            .get(&d.checks[1].id)
            .unwrap()
            .last
            .as_ref()
            .unwrap();
        assert_eq!(q.depth, pulse_core::Depth::Answers);
        assert!(
            q.message.contains("\"FOH X32\", X32 · firmware 4.06"),
            "{}",
            q.message
        );
        let cam = s
            .profile
            .devices
            .iter()
            .find(|d| d.name == "Camera 1 (NDI)")
            .unwrap();
        let n = s
            .engine
            .device(cam.id)
            .unwrap()
            .checks
            .get(&cam.checks[1].id)
            .unwrap();
        assert!(
            n.last.as_ref().unwrap().message.contains("port"),
            "{:?}",
            n.last
        );
        assert_eq!(status(s, "IEM rack"), Status::Down);
        assert_eq!(s.overview.light, Status::Warn);
    });
}

#[test]
fn faults_turn_devices_yellow_or_red_and_they_recover() {
    let (m, sim) = start_demo();
    wait(&m, "the stageboxes OK", 15, |s| {
        status(s, "Stagebox SR") == Status::Ok && status(s, "Light desk") == Status::Ok
    });

    // A Dante stagebox stops announcing itself: yellow (ping still answers).
    sim.set_down("Stagebox SR", true);
    // The light desk sends only 5 frames per second.
    sim.set_fps("Light desk", Some(5.0));
    // Show Master stops (its port is critical): red.
    sim.set_closed("Show Master Mac", true);
    // The desk answers too late.
    sim.set_slow("FOH desk", 1000);
    // The NDI camera is announced but takes no connections.
    sim.set_closed("Camera 1 (NDI)", true);
    wait(&m, "the faults show", 15, |s| {
        status(s, "Stagebox SR") == Status::Warn
            && status(s, "Light desk") == Status::Warn
            && status(s, "Show Master Mac") == Status::Down
            && status(s, "FOH desk") == Status::Warn
            && status(s, "Camera 1 (NDI)") == Status::Warn
    });
    m.with(|s| {
        assert!(
            cause(s, "Stagebox SR").contains("\"SB-SR\" is not announced"),
            "{}",
            cause(s, "Stagebox SR")
        );
        assert!(
            cause(s, "Light desk").contains("frames per second"),
            "{}",
            cause(s, "Light desk")
        );
        assert!(
            cause(s, "Show Master Mac").contains("connection refused"),
            "{}",
            cause(s, "Show Master Mac")
        );
        assert!(
            cause(s, "FOH desk").contains("no answer to the X32/M32 query"),
            "{}",
            cause(s, "FOH desk")
        );
        assert!(
            cause(s, "Camera 1 (NDI)").contains("does not accept connections"),
            "{}",
            cause(s, "Camera 1 (NDI)")
        );
        assert_eq!(s.overview.light, Status::Down);
        assert_eq!(s.overview.reasons[0].name, "Show Master Mac");
    });

    for n in ["Stagebox SR", "FOH desk"] {
        sim.set_down(n, false);
    }
    sim.set_slow("FOH desk", 0);
    sim.set_fps("Light desk", None);
    sim.set_closed("Show Master Mac", false);
    sim.set_closed("Camera 1 (NDI)", false);
    wait(&m, "everything recovers", 15, |s| {
        [
            "Stagebox SR",
            "Light desk",
            "Show Master Mac",
            "FOH desk",
            "Camera 1 (NDI)",
        ]
        .iter()
        .all(|n| status(s, n) == Status::Ok)
    });
    // The event log tells the story.
    m.with(|s| {
        let sm = id(s, "Show Master Mac");
        assert!(s.log.iter().any(|e| e.device == sm
            && e.kind
                == pulse_core::EventKind::Status {
                    from: Status::Down,
                    to: Status::Ok
                }));
    });
}

#[test]
fn a_scenario_breaks_and_heals_a_device() {
    let (m, sim) = start_demo();
    wait(&m, "the node OK", 15, |s| {
        status(s, "Art-Net node DS") == Status::Ok
    });
    let step = |at: u64, action| Step {
        at: Duration::from_millis(at),
        device: "Art-Net node DS".into(),
        action,
    };
    sim.scenario(
        vec![
            step(100, Action::Down(true)),
            step(12_000, Action::Down(false)),
        ],
        Duration::from_secs(600),
    );
    // ArtPollReplies count for 10 s: then the node is missed.
    wait(&m, "the node missed", 20, |s| {
        status(s, "Art-Net node DS") == Status::Warn
    });
    wait(&m, "the node back", 15, |s| {
        status(s, "Art-Net node DS") == Status::Ok
    });
}

/// 100 devices: 70 ping only, 20 with a TCP service, 5 consoles, 5 Dante
/// devices, all at default intervals. Returns the monitor after `secs`.
fn hundred(secs: u64) -> (Monitor, Sim, Instant) {
    let mut specs = Vec::new();
    for i in 0..20 {
        specs.push(Spec::new(&format!("Service {i}"), Kind::Tcp));
    }
    for i in 0..5 {
        specs.push(Spec::new(
            &format!("Desk {i}"),
            Kind::Console {
                model: ConsoleModel::X32,
                name: format!("Desk {i}"),
                firmware: "4.06".into(),
            },
        ));
    }
    for i in 0..5 {
        specs.push(Spec::new(
            &format!("Stagebox {i}"),
            Kind::Announce {
                service: pulse_core::model::DANTE_SERVICE.into(),
                instance: format!("SB-{i}"),
            },
        ));
    }
    let sim = Sim::start(specs).unwrap();
    let mut p = Profile::default();
    for i in 0..70 {
        p.add_preset(
            pulse_core::Preset::Generic,
            &format!("Device {i}"),
            "127.0.0.1",
        );
    }
    for i in 0..20 {
        let d = p.add_preset(
            pulse_core::Preset::Generic,
            &format!("Service {i}"),
            "127.0.0.1",
        );
        let port = sim.device(&format!("Service {i}")).unwrap().port;
        p.add_check(d, CheckKind::Tcp { port });
    }
    for i in 0..5 {
        let d = p.add_preset(
            pulse_core::Preset::Generic,
            &format!("Desk {i}"),
            "127.0.0.1",
        );
        let port = sim.device(&format!("Desk {i}")).unwrap().port;
        p.add_check(
            d,
            CheckKind::Console {
                model: ConsoleModel::X32,
                port: Some(port),
            },
        );
    }
    for i in 0..5 {
        let d = p.add_preset(
            pulse_core::Preset::DanteDevice,
            &format!("Stagebox {i}"),
            "127.0.0.1",
        );
        if let Some(dev) = p.device_mut(d) {
            for c in &mut dev.checks {
                if let CheckKind::Mdns { instance, .. } = &mut c.kind {
                    *instance = format!("SB-{i}");
                }
            }
        }
    }
    assert_eq!(p.devices.len(), 100);
    let start = Instant::now();
    let m = Monitor::start(p, opts()).unwrap();
    sim.attach(m.found_handle());
    std::thread::sleep(Duration::from_secs(secs));
    (m, sim, start)
}

#[test]
fn a_hundred_devices_stay_within_the_packet_limit() {
    let (m, _sim, _) = hundred(12);
    m.with(|s| {
        let now = pulse_net::monitor::now_ms();
        let rate = s.load.packet_rate(now);
        // The global limit is 50 packets per second (bursts of one second on top).
        assert!(rate <= 56.0, "{rate:.1} packets/s");
        assert_eq!(s.load.crashes, 0);
        assert_eq!(s.load.timeouts, 0);
        assert_eq!(s.overview.counts.ok, 100, "{:?}", s.overview.reasons);
    });
}

/// The measurement for `docs/netzlast.md`.
#[test]
#[ignore]
fn measure_network_load_with_a_hundred_devices() {
    let cpu = || {
        let out = std::process::Command::new("ps")
            .args(["-o", "time=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
        // [[dd-]hh:]mm:ss(.ss)
        let mut secs = 0.0;
        for part in t.split(':') {
            secs = secs * 60.0 + part.parse::<f64>().ok()?;
        }
        Some(secs)
    };
    let cpu0 = cpu();
    let (m, sim, start) = hundred(60);
    let wall = start.elapsed().as_secs_f64();
    let cpu1 = cpu();
    m.with(|s| {
        let now = pulse_net::monitor::now_ms();
        let secs = now.saturating_sub(s.load.since) as f64 / 1000.0;
        println!("devices: {}", s.profile.devices.len());
        println!(
            "checks run: {} ({:.1}/s)",
            s.load.runs,
            s.load.runs as f64 / secs
        );
        println!(
            "packets sent: {} ({:.1}/s)",
            s.load.packets,
            s.load.packets as f64 / secs
        );
        println!(
            "bytes sent (estimate): {} ({:.2} kB/s)",
            s.load.bytes,
            s.load.bytes as f64 / 1024.0 / secs
        );
        let wanted: f64 = s
            .profile
            .devices
            .iter()
            .flat_map(|d| &d.checks)
            .map(|c| 1.0 / c.interval())
            .sum();
        println!("checks wanted: {wanted:.1}/s");
        println!(
            "postponed by the limit: {} runs, {:.0} ms late on average",
            s.load.throttled,
            s.load.delayed_ms as f64 / s.load.throttled.max(1) as f64
        );
        println!("timeouts: {}, crashes: {}", s.load.timeouts, s.load.crashes);
        println!("status: {:?}", s.overview.counts);
    });
    println!(
        "simulated devices sent: {} packets",
        sim.sent.load(std::sync::atomic::Ordering::Relaxed)
    );
    if let (Some(a), Some(b)) = (cpu0, cpu1) {
        println!(
            "CPU (whole test process, incl. simulator): {:.1} s in {wall:.0} s = {:.1} %",
            b - a,
            (b - a) / wall * 100.0
        );
    }
}

#[test]
fn sacn_snmp_and_the_agent_report_problems_and_recover() {
    let (m, sim) = start_demo();
    wait(&m, "the new checks OK", 15, |s| {
        ["Media server", "Core switch", "Backup playback"]
            .iter()
            .all(|n| {
                let st = s.engine.device(id(s, n)).unwrap();
                st.status == Status::Ok
                    && st.checks.values().all(|c| {
                        c.last.as_ref().is_some_and(|r| {
                            r.outcome == pulse_core::Outcome::Ok && !r.message.contains("measuring")
                        })
                    })
            })
    });
    m.with(|s| {
        let msg = |name: &str, i: usize| {
            let d = s.profile.devices.iter().find(|d| d.name == name).unwrap();
            let c = s.engine.device(d.id).unwrap();
            c.checks
                .get(&d.checks[i].id)
                .unwrap()
                .last
                .clone()
                .map(|r| format!("{} | {}", r.message, r.raw))
                .unwrap()
        };
        assert!(
            msg("Media server", 1).contains("from \"Media server\" (127.0.0.1) at priority 100"),
            "{}",
            msg("Media server", 1)
        );
        assert!(
            msg("Core switch", 1).starts_with("\"core-sw1\": port 1 (Uplink FOH) is up |"),
            "{}",
            msg("Core switch", 1)
        );
        assert!(
            msg("Backup playback", 2).starts_with("backup-mac answers"),
            "{}",
            msg("Backup playback", 2)
        );
    });

    sim.set_fps("Media server", Some(4.0));
    sim.set_closed("Core switch", true);
    sim.set_closed("Backup agent", true);
    wait(&m, "the problems show", 15, |s| {
        ["Media server", "Core switch", "Backup playback"]
            .iter()
            .all(|n| status(s, n) == Status::Warn)
    });
    m.with(|s| {
        assert!(
            cause(s, "Media server").contains("frames per second"),
            "{}",
            cause(s, "Media server")
        );
        assert!(
            cause(s, "Core switch").contains("port 1 (Uplink FOH) is down"),
            "{}",
            cause(s, "Core switch")
        );
        assert!(
            cause(s, "Backup playback").contains("Lantern is not running on backup-mac"),
            "{}",
            cause(s, "Backup playback")
        );
    });

    sim.set_fps("Media server", None);
    sim.set_closed("Core switch", false);
    sim.set_closed("Backup agent", false);
    wait(&m, "they recover", 15, |s| {
        ["Media server", "Core switch", "Backup playback"]
            .iter()
            .all(|n| status(s, n) == Status::Ok)
    });

    // Errors rising on a port, run after run: yellow.
    let stop = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|sc| {
        sc.spawn(|| {
            let end = Instant::now() + Duration::from_secs(15);
            while !stop.load(std::sync::atomic::Ordering::Relaxed) && Instant::now() < end {
                sim.add_errors("Core switch", 3);
                std::thread::sleep(Duration::from_millis(150));
            }
        });
        wait(&m, "port errors show", 15, |s| {
            status(s, "Core switch") == Status::Warn
        });
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    m.with(|s| {
        assert!(
            cause(s, "Core switch").contains("new errors since the last check"),
            "{}",
            cause(s, "Core switch")
        )
    });
}

#[test]
fn the_agent_answers_over_http() {
    let sim = Sim::start(vec![Spec::new(
        "Playback",
        Kind::Agent {
            host: "playback-1".into(),
            processes: vec!["Show Master".into(), "QLab".into()],
        },
    )])
    .unwrap();
    let port = sim.device("Playback").unwrap().port;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (code, body) = rt
        .block_on(pulse_net::http::get(
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            port,
            &pulse_core::agent::path(&["Show Master".into(), "Lantern".into()]),
            None,
            Duration::from_secs(2),
        ))
        .unwrap();
    assert_eq!(code, 200);
    let st: pulse_core::agent::Status = serde_json::from_str(&body).unwrap();
    assert_eq!(st.host, "playback-1");
    assert_eq!(
        st.processes,
        vec![
            pulse_core::agent::Process {
                name: "Show Master".into(),
                running: true
            },
            pulse_core::agent::Process {
                name: "Lantern".into(),
                running: false
            },
        ]
    );
}
