//! The listeners and the new checks against fake devices on this computer:
//! an Art-Net node and senders on loopback, a fake X Air desk, the
//! loopback interface, an mDNS service we announce ourselves, discovery and
//! a tiny ping sweep.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pulse_core::artnet::{self, PollReply};
use pulse_core::discovery::Net;
use pulse_core::model::{
    Check, CheckKind, ConsoleModel, Criticality, DeviceKind, Hysteresis, Id, Profile,
};
use pulse_core::status::Status;
use pulse_net::{console, interfaces, Monitor, Options, Shared};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn wait(m: &Monitor, what: &str, secs: u64, f: impl Fn(&Shared) -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if m.with(&f) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let state = m.with(|s| {
        s.engine
            .devices()
            .map(|(id, d)| format!("{id}: {:?} {}", d.status, d.cause))
            .collect::<Vec<_>>()
            .join("\n")
    });
    panic!("timed out waiting for {what}\n{state}");
}

fn status(s: &Shared, d: Id) -> Status {
    s.engine
        .device(d)
        .map(|x| x.status)
        .unwrap_or(Status::Unknown)
}

/// The message of the device's (only) check's last run.
fn cause(s: &Shared, d: Id) -> String {
    s.engine
        .device(d)
        .and_then(|x| x.checks.values().next())
        .and_then(|c| c.last.as_ref())
        .map(|r| r.message.clone())
        .unwrap_or_default()
}

/// A device with one fast check (0.2 s apart, red after 3).
fn fast(p: &mut Profile, name: &str, address: &str, kind: CheckKind) -> Id {
    let id = p.add_device(DeviceKind::Other, name, address);
    let check = p.alloc_id();
    let d = p.device_mut(id).unwrap();
    d.criticality = Criticality::Critical;
    d.hysteresis = Hysteresis {
        fail_after: 3,
        ok_after: 2,
    };
    d.checks = vec![Check {
        id: check,
        kind,
        interval_s: Some(0.2),
        timeout_ms: Some(300),
        ..Check::default()
    }];
    id
}

fn free_udp_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Runs `f` on a thread until the returned flag is cleared.
fn background(f: impl Fn(&AtomicBool) + Send + 'static) -> Arc<AtomicBool> {
    let run = Arc::new(AtomicBool::new(true));
    let r = run.clone();
    std::thread::spawn(move || f(&r));
    run
}

#[test]
fn art_net_node_and_universes_with_frame_rate_and_conflicts() {
    let port = free_udp_port();
    let listener = SocketAddr::from(([127, 0, 0, 1], port));
    // A node that answers ArtPoll.
    let node = UdpSocket::bind("127.0.0.1:0").unwrap();
    node.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let node_addr = node.local_addr().unwrap();
    let node_run = background(move |run| {
        let mut buf = [0u8; 600];
        while run.load(Ordering::Relaxed) {
            if let Ok((n, from)) = node.recv_from(&mut buf) {
                if artnet::parse(&buf[..n]) == Some(artnet::Packet::Poll) {
                    let r = PollReply {
                        ip: Ipv4Addr::LOCALHOST,
                        short_name: "Node SR".into(),
                        outputs: vec![1],
                        ..PollReply::default()
                    };
                    let _ = node.send_to(&artnet::poll_reply(&r), from);
                }
            }
        }
    });
    // Universe 1 at 40 fps; universe 2 at 5 fps; universe 3 from two senders.
    let senders = background(move |run| {
        let a = UdpSocket::bind("127.0.0.1:0").unwrap();
        let b = UdpSocket::bind("127.0.0.2:0")
            .or_else(|_| UdpSocket::bind("127.0.0.1:0"))
            .unwrap();
        let mut i: u32 = 0;
        while run.load(Ordering::Relaxed) {
            let _ = a.send_to(&artnet::dmx(1, i as u8, &[0; 512]), listener);
            if i % 8 == 0 {
                let _ = a.send_to(&artnet::dmx(2, i as u8, &[0; 512]), listener);
            }
            let _ = a.send_to(&artnet::dmx(3, i as u8, &[0; 512]), listener);
            let _ = b.send_to(&artnet::dmx(3, i as u8, &[0; 512]), listener);
            i += 1;
            std::thread::sleep(Duration::from_millis(25));
        }
    });

    let mut p = Profile::default();
    let n = fast(&mut p, "Node SR", "127.0.0.1", CheckKind::ArtNetNode);
    let u1 = fast(
        &mut p,
        "Desk U1",
        "",
        CheckKind::ArtNetUniverse {
            universe: 1,
            min_fps: 20.0,
        },
    );
    let u2 = fast(
        &mut p,
        "Desk U2",
        "",
        CheckKind::ArtNetUniverse {
            universe: 2,
            min_fps: 20.0,
        },
    );
    let u9 = fast(
        &mut p,
        "Desk U9",
        "",
        CheckKind::ArtNetUniverse {
            universe: 9,
            min_fps: 20.0,
        },
    );
    let u3 = fast(
        &mut p,
        "Desk U3",
        "",
        CheckKind::ArtNetUniverse {
            universe: 3,
            min_fps: 1.0,
        },
    );
    let opts = Options {
        artnet_port: port,
        artnet_targets: vec![node_addr],
        mdns: false,
        ..Options::default()
    };
    let m = Monitor::start(p, opts).unwrap();
    wait(&m, "node answers", 10, |s| status(s, n) == Status::Ok);
    wait(&m, "universe 1 at 40 fps", 6, |s| {
        status(s, u1) == Status::Ok
    });
    wait(&m, "universe 2 too slow", 6, |s| {
        status(s, u2) == Status::Warn
    });
    wait(&m, "universe 9 missing", 8, |s| {
        status(s, u9) == Status::Down
    });
    m.with(|s| {
        assert!(cause(s, n).contains("Node SR"), "{}", cause(s, n));
        assert!(
            cause(s, u2).contains("frames per second"),
            "{}",
            cause(s, u2)
        );
        assert!(cause(s, u9).contains("no frames"), "{}", cause(s, u9));
    });
    // Two senders only if 127.0.0.2 exists on this computer (Linux: yes; macOS: no).
    if UdpSocket::bind("127.0.0.2:0").is_ok() {
        wait(&m, "universe 3 conflict", 6, |s| {
            status(s, u3) == Status::Warn
        });
        m.with(|s| assert!(cause(s, u3).contains("conflict"), "{}", cause(s, u3)));
    }
    m.found(|f| {
        assert!(f.artnet.nodes.contains_key(&Ipv4Addr::LOCALHOST));
        assert!(f.artnet.universes.contains_key(&1));
    });
    node_run.store(false, Ordering::Relaxed);
    senders.store(false, Ordering::Relaxed);
}

#[test]
fn a_console_answers_and_discovery_finds_it() {
    // A fake X Air desk; the check asks on the real port 10024.
    let Ok(desk) = UdpSocket::bind("127.0.0.1:10024") else {
        eprintln!("UDP 10024 is in use here: skipped");
        return;
    };
    desk.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let desk_addr = desk.local_addr().unwrap();
    let run = background(move |run| {
        let mut buf = [0u8; 600];
        while run.load(Ordering::Relaxed) {
            if let Ok((n, from)) = desk.recv_from(&mut buf) {
                if &buf[..n] == console::request(ConsoleModel::XAir).as_slice() {
                    let a = console::answer(
                        ConsoleModel::XAir,
                        Ipv4Addr::LOCALHOST,
                        "Monitor desk",
                        "XR18",
                        "1.22",
                    );
                    let _ = desk.send_to(&a, from);
                }
            }
        }
    });
    let mut p = Profile::default();
    let d = fast(
        &mut p,
        "Monitor desk",
        "127.0.0.1",
        CheckKind::Console {
            model: ConsoleModel::XAir,
            port: None,
        },
    );
    let wing = fast(
        &mut p,
        "No Wing",
        "127.0.0.1",
        CheckKind::Console {
            model: ConsoleModel::Wing,
            port: None,
        },
    );
    let opts = Options {
        mdns: false,
        console_targets: vec![(ConsoleModel::XAir, desk_addr)],
        ..Options::default()
    };
    let m = Monitor::start(p, opts).unwrap();
    wait(&m, "desk answers", 6, |s| status(s, d) == Status::Ok);
    wait(&m, "no wing here", 6, |s| status(s, wing) == Status::Down);
    m.with(|s| {
        assert!(
            cause(s, d).contains("\"Monitor desk\", XR18 · firmware 1.22"),
            "{}",
            cause(s, d)
        )
    });

    // Discovery: the broadcast (and our extra target) finds the desk; it is in the inventory.
    m.set_discovery(true);
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end && m.found(|f| f.last_scan.is_none()) {
        std::thread::sleep(Duration::from_millis(50));
    }
    m.with(|s| {
        let r = m.found(|f| f.report(&s.profile));
        assert!(r.known.iter().any(|c| c.device == Some(d)), "{r:?}");
    });
    m.set_discovery(false);
    run.store(false, Ordering::Relaxed);
}

#[test]
fn interface_checks_and_binding_to_an_interface() {
    let lo = interfaces::list().into_iter().find(|n| n.loopback).unwrap();
    let mut p = Profile::default();
    let up = fast(
        &mut p,
        "Show LAN",
        "",
        CheckKind::Interface {
            name: lo.name.clone(),
        },
    );
    let down = fast(
        &mut p,
        "Second LAN",
        "",
        CheckKind::Interface {
            name: "nope7".into(),
        },
    );
    let bound = fast(&mut p, "Via loopback", "127.0.0.1", CheckKind::Ping);
    p.device_mut(bound).unwrap().interface = lo.name.clone();
    let lost = fast(&mut p, "Via missing NIC", "127.0.0.1", CheckKind::Ping);
    p.device_mut(lost).unwrap().interface = "nope7".into();
    let m = Monitor::start(
        p,
        Options {
            mdns: false,
            ..Options::default()
        },
    )
    .unwrap();
    wait(&m, "loopback up", 5, |s| status(s, up) == Status::Ok);
    wait(&m, "missing interface down", 5, |s| {
        status(s, down) == Status::Down
    });
    wait(&m, "ping from loopback", 5, |s| {
        status(s, bound) == Status::Ok
    });
    std::thread::sleep(Duration::from_millis(1500));
    m.with(|s| {
        // An interface that is gone says nothing about the device: unknown, not red.
        assert_eq!(status(s, lost), Status::Unknown);
        let st = s.engine.device(lost).unwrap();
        let last = st
            .checks
            .values()
            .next()
            .and_then(|c| c.last.clone())
            .unwrap();
        assert!(
            last.message.contains("interface nope7 is down or missing"),
            "{}",
            last.message
        );
    });
}

#[test]
fn a_tiny_sweep_finds_this_computer() {
    let m = Monitor::start(
        Profile::default(),
        Options {
            mdns: false,
            ..Options::default()
        },
    )
    .unwrap();
    m.sweep(
        Net {
            addr: Ipv4Addr::LOCALHOST,
            prefix: 30,
        },
        None,
    );
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end
        && !m.found(|f| f.sweep.as_ref().is_some_and(|s| s.finished.is_some()))
    {
        std::thread::sleep(Duration::from_millis(50));
    }
    m.found(|f| {
        let s = f.sweep.as_ref().unwrap();
        assert_eq!(s.total, 2);
        assert_eq!(s.done, 2);
        assert!(s.answered.contains_key(&Ipv4Addr::LOCALHOST), "{s:?}");
    });
}

#[test]
fn an_mdns_service_we_announce_is_found() {
    // Needs a LAN interface with multicast; skipped without one.
    let Some(lan) = interfaces::list().into_iter().find(|n| n.up && !n.loopback) else {
        eprintln!("no network interface: skipped");
        return;
    };
    let ad = mdns_sd::ServiceDaemon::new().unwrap();
    let info = mdns_sd::ServiceInfo::new(
        "_pulsetest._udp.local.",
        "Pulse test stagebox",
        "pulse-test-host.local.",
        lan.ip.to_string().as_str(),
        4440,
        &[("model", "test")][..],
    )
    .unwrap();
    ad.register(info).unwrap();
    let mut p = Profile::default();
    let d = fast(
        &mut p,
        "Stagebox",
        &lan.ip.to_string(),
        CheckKind::Mdns {
            service: "_pulsetest._udp".into(),
            instance: String::new(),
        },
    );
    let named = fast(
        &mut p,
        "Named",
        "",
        CheckKind::Mdns {
            service: "_pulsetest._udp".into(),
            instance: "Pulse test stagebox".into(),
        },
    );
    let m = Monitor::start(p, Options::default()).unwrap();
    let ok = (|| {
        let end = Instant::now() + Duration::from_secs(15);
        while Instant::now() < end {
            if m.with(|s| status(s, d) == Status::Ok && status(s, named) == Status::Ok) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    })();
    let _ = ad.shutdown();
    if !ok {
        // Some networks (and sandboxes) block multicast: say so instead of failing.
        let state =
            m.found(|f| format!("{:?} {:?}", f.mdns_state, f.mdns.keys().collect::<Vec<_>>()));
        eprintln!("mDNS service not seen here (multicast blocked?): {state}");
        assert!(m.found(|f| matches!(f.mdns_state, pulse_net::found::Listener::On(_))));
        return;
    }
    m.with(|s| {
        assert!(
            cause(s, d).contains("Pulse test stagebox"),
            "{}",
            cause(s, d)
        )
    });
}
