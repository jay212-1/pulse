//! The monitor against real sockets on this computer: localhost answers,
//! 192.0.2.1 (TEST-NET-1, never routed) does not, and TCP ports we open or
//! close ourselves.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pulse_core::model::{Check, CheckKind, Criticality, DeviceKind, Hysteresis, Id, Profile};
use pulse_core::status::Status;
use pulse_core::EventKind;
use pulse_net::{Method, Monitor, Options, Shared};
use std::time::{Duration, Instant};

/// Never answers (RFC 5737).
const NOWHERE: &str = "192.0.2.1";

fn wait(m: &Monitor, what: &str, secs: u64, f: impl Fn(&Shared) -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if m.with(&f) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let state = m.with(|s| {
        format!(
            "{:?}\n{:?}",
            s.overview.reasons,
            s.log.iter().collect::<Vec<_>>()
        )
    });
    panic!("timed out waiting for {what}\n{state}");
}

fn status(s: &Shared, d: Id) -> Status {
    s.engine
        .device(d)
        .map(|x| x.status)
        .unwrap_or(Status::Unknown)
}

/// A device with fast checks (0.2 s apart, short timeouts, red after 3).
fn fast(p: &mut Profile, name: &str, address: &str, crit: Criticality, kind: CheckKind) -> Id {
    let id = p.add_device(DeviceKind::Other, name, address);
    let check = p.alloc_id();
    let d = p.device_mut(id).unwrap();
    d.criticality = crit;
    d.hysteresis = Hysteresis {
        fail_after: 3,
        ok_after: 2,
    };
    d.checks = vec![Check {
        id: check,
        kind,
        interval_s: Some(0.2),
        timeout_ms: Some(250),
        ..Check::default()
    }];
    id
}

#[test]
fn localhost_is_green_and_a_dead_critical_device_turns_the_light_red() {
    let mut p = Profile::default();
    let lo = fast(
        &mut p,
        "Loopback",
        "127.0.0.1",
        Criticality::Critical,
        CheckKind::Ping,
    );
    let dead = fast(
        &mut p,
        "Stagebox SL",
        NOWHERE,
        Criticality::Critical,
        CheckKind::Ping,
    );
    let started = Instant::now();
    let m = Monitor::start(p, Options::default()).unwrap();
    wait(&m, "loopback green", 5, |s| status(s, lo) == Status::Ok);
    wait(&m, "dead device red", 10, |s| {
        status(s, dead) == Status::Down
    });
    // 3 failures 0.2 s apart (+ timeouts): well within the configured time.
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    m.with(|s| {
        assert_eq!(s.overview.light, Status::Down);
        let r = &s.overview.reasons[0];
        assert_eq!(r.name, "Stagebox SL");
        assert_eq!(r.text(""), "Stagebox SL: not reachable");
        let cs = s.engine.device(lo).unwrap().checks.values().next().unwrap();
        let last = cs.last.as_ref().unwrap();
        assert!(last.metric(pulse_core::Metric::LatencyMs).is_some());
        assert!(!last.via.is_empty());
        assert!(s.log.iter().any(|e| e.device == dead
            && e.kind
                == EventKind::Status {
                    from: Status::Unknown,
                    to: Status::Down
                }));
        assert!(s.load.packets > 0 && s.load.bytes > 0);
        assert_eq!(s.load.crashes, 0);
    });
}

#[test]
fn tcp_open_port_passes_closed_port_fails_as_refused() {
    let open = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let open_port = open.local_addr().unwrap().port();
    let closed_port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut p = Profile::default();
    let a = fast(
        &mut p,
        "Desk OSC",
        "127.0.0.1",
        Criticality::Important,
        CheckKind::Tcp { port: open_port },
    );
    let b = fast(
        &mut p,
        "Remote app",
        "localhost",
        Criticality::Important,
        CheckKind::Tcp { port: closed_port },
    );
    let m = Monitor::start(p, Options::default()).unwrap();
    wait(&m, "open port green", 5, |s| status(s, a) == Status::Ok);
    wait(&m, "closed port red", 5, |s| status(s, b) == Status::Down);
    m.with(|s| {
        let cause = &s.engine.device(b).unwrap().cause;
        assert!(cause.contains("connection refused"), "{cause}");
        // Important down: yellow, not red.
        assert_eq!(s.overview.light, Status::Warn);
    });
    drop(open);
}

#[test]
fn tcp_fallback_counts_a_refused_connection_as_reachable() {
    let mut p = Profile::default();
    let lo = fast(
        &mut p,
        "Loopback",
        "127.0.0.1",
        Criticality::Critical,
        CheckKind::Ping,
    );
    let opts = Options {
        method: Some(Method::Tcp {
            reason: "test".into(),
        }),
        ..Options::default()
    };
    let m = Monitor::start(p, opts).unwrap();
    wait(&m, "loopback green over TCP", 5, |s| {
        status(s, lo) == Status::Ok
    });
    m.with(|s| {
        assert!(s.method.label().starts_with("TCP 80"));
        let cs = s.engine.device(lo).unwrap().checks.values().next().unwrap();
        assert!(cs.last.as_ref().unwrap().via.starts_with("TCP 80"));
    });
}

#[test]
fn profile_changes_acknowledgements_and_check_now_reach_the_monitor() {
    let mut p = Profile::default();
    let dead = fast(
        &mut p,
        "Tablet 2",
        NOWHERE,
        Criticality::Critical,
        CheckKind::Ping,
    );
    let m = Monitor::start(p.clone(), Options::default()).unwrap();
    wait(&m, "red", 10, |s| s.overview.light == Status::Down);
    m.ack(dead, "off on purpose");
    wait(&m, "acknowledged", 2, |s| {
        s.overview.light == Status::Ok && s.overview.acked.len() == 1
    });
    m.unack(dead);
    wait(&m, "red again", 2, |s| s.overview.light == Status::Down);
    // Disabled in the inventory: grey, and nothing lights up.
    p.device_mut(dead).unwrap().enabled = false;
    m.set_profile(p.clone());
    wait(&m, "disabled", 2, |s| {
        status(s, dead) == Status::Unknown && s.overview.light == Status::Unknown
    });
    let runs = m.with(|s| s.load.runs);
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(m.with(|s| s.load.runs), runs, "disabled checks do not run");
    // Back on, with a long interval: "check now" runs it at once.
    let d = p.device_mut(dead).unwrap();
    d.enabled = true;
    d.checks[0].interval_s = Some(3600.0);
    m.set_profile(p);
    m.check_now(Some(dead));
    wait(&m, "checked now", 3, |s| s.load.runs > runs);
}

#[test]
fn the_packet_limit_holds_with_many_devices() {
    let mut p = Profile::default();
    for i in 0..100 {
        fast(
            &mut p,
            &format!("Dev {i}"),
            "127.0.0.1",
            Criticality::Optional,
            CheckKind::Ping,
        );
    }
    let opts = Options {
        max_rate: 40.0,
        ..Options::default()
    };
    let m = Monitor::start(p, opts).unwrap();
    std::thread::sleep(Duration::from_secs(3));
    m.with(|s| {
        let rate = s.load.packet_rate(pulse_net::monitor::now_ms());
        // 100 checks every 0.2 s would be 500/s; the limit keeps it near 40 (+ the first burst).
        assert!(rate < 60.0, "{rate} packets/s");
        assert!(s.load.throttled > 0);
        assert_eq!(s.load.crashes, 0);
    });
}
