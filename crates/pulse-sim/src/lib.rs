//! # pulse-sim
//!
//! Invented show devices on this computer (127.0.0.1), for the demo, the
//! integration tests and the network load measurement:
//!
//! * consoles that answer their query (X32/M32, X Air, Wing) on a port of their own,
//! * an Art-Net node that answers ArtPoll, a lighting desk that sends a universe,
//! * TCP services (Show Master, anything with a port),
//! * announcements (Dante, Lantern, NDI) written straight into Pulse's findings:
//!   the simulator never announces anything on the real network.
//!
//! Every device can be switched off, its port closed, its answers slowed,
//! its frame rate lowered; a scenario does that now and then for the demo.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod demo;

use pulse_core::artnet::{self, PollReply};
use pulse_core::model::{service_short, ConsoleModel};
use pulse_net::console;
use pulse_net::found::{lock, Found, Listener, Service};
use pulse_net::monitor::now_ms;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What a simulated device does.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Answers the console query of `model`.
    Console {
        model: ConsoleModel,
        name: String,
        firmware: String,
    },
    /// Answers ArtPoll (and reports itself every 3 s).
    ArtNetNode {
        short_name: String,
        outputs: Vec<u16>,
    },
    /// Sends a universe at `fps` frames per second to Pulse's Art-Net listener.
    ArtNetSender { universe: u16, fps: f64 },
    /// A TCP port that accepts connections.
    Tcp,
    /// An mDNS announcement (`_netaudio-arc._udp`, `_lantern._udp` ...).
    Announce { service: String, instance: String },
    /// An NDI source: announced via `_ndi._tcp`, with a TCP port.
    Ndi { source: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub name: String,
    pub kind: Kind,
}

impl Spec {
    pub fn new(name: &str, kind: Kind) -> Self {
        Spec {
            name: name.into(),
            kind,
        }
    }
}

/// What is wrong with a device right now.
#[derive(Debug, Default)]
pub struct Fault {
    /// Gone: no answers, no announcements, no frames, port closed.
    pub down: AtomicBool,
    /// Only the TCP port is closed (announced, but takes no connections).
    pub closed: AtomicBool,
    /// Answers this much later (UDP devices).
    pub slow_ms: AtomicU32,
    /// Frame rate instead of the configured one (0 = configured), in 1/100 fps.
    pub fps_centi: AtomicU32,
}

pub struct Device {
    pub name: String,
    pub kind: Kind,
    /// The UDP or TCP port it answers on (0 = none).
    pub port: u16,
    pub fault: Arc<Fault>,
}

impl Device {
    pub fn addr(&self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, self.port))
    }
}

/// Where the simulator delivers: Pulse's findings (announcements, the Art-Net port).
type Target = Arc<Mutex<Option<Arc<Mutex<Found>>>>>;

pub struct Sim {
    pub devices: Vec<Device>,
    handle: tokio::runtime::Handle,
    target: Target,
    scenario: Arc<AtomicBool>,
    /// Packets the simulated devices sent.
    pub sent: Arc<AtomicU64>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

fn target_found(t: &Target) -> Option<Arc<Mutex<Found>>> {
    t.lock().ok().and_then(|g| g.clone())
}

/// Pulse's Art-Net listener, if one runs.
fn artnet_dest(t: &Target) -> Option<SocketAddr> {
    let f = target_found(t)?;
    let port = lock(&f).artnet_port?;
    Some(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
}

impl Sim {
    /// Start the devices (on a runtime of their own).
    pub fn start(specs: Vec<Spec>) -> std::io::Result<Sim> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("pulse-sim")
            .enable_all()
            .build()?;
        let handle = rt.handle().clone();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        std::thread::Builder::new()
            .name("pulse-sim".into())
            .spawn(move || {
                rt.block_on(async {
                    let _ = stopped.await;
                });
                rt.shutdown_timeout(Duration::from_millis(200));
            })?;
        let target: Target = Arc::default();
        let sent = Arc::new(AtomicU64::new(0));
        let mut devices = Vec::new();
        for s in specs {
            let fault = Arc::new(Fault::default());
            let port = {
                let _g = handle.enter();
                spawn_device(
                    &handle,
                    &s.kind,
                    fault.clone(),
                    target.clone(),
                    sent.clone(),
                )?
            };
            devices.push(Device {
                name: s.name,
                kind: s.kind,
                port,
                fault,
            });
        }
        let sim = Sim {
            devices,
            handle,
            target,
            scenario: Arc::new(AtomicBool::new(false)),
            sent,
            stop: Some(stop),
        };
        sim.spawn_announcer();
        Ok(sim)
    }

    pub fn device(&self, name: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.name == name)
    }

    fn fault(&self, name: &str) -> Option<&Fault> {
        self.device(name).map(|d| d.fault.as_ref())
    }

    /// Switch a device off (or on again).
    pub fn set_down(&self, name: &str, down: bool) {
        if let Some(f) = self.fault(name) {
            f.down.store(down, Ordering::Relaxed);
        }
    }
    /// Close (or open) its TCP port while it stays announced.
    pub fn set_closed(&self, name: &str, closed: bool) {
        if let Some(f) = self.fault(name) {
            f.closed.store(closed, Ordering::Relaxed);
        }
    }
    pub fn set_slow(&self, name: &str, ms: u32) {
        if let Some(f) = self.fault(name) {
            f.slow_ms.store(ms, Ordering::Relaxed);
        }
    }
    /// Another frame rate (`None`: the configured one again).
    pub fn set_fps(&self, name: &str, fps: Option<f64>) {
        if let Some(f) = self.fault(name) {
            let v = fps.map(|x| (x.max(0.01) * 100.0) as u32).unwrap_or(0);
            f.fps_centi.store(v, Ordering::Relaxed);
        }
    }

    /// Deliver to a monitor's findings: announcements, frames, node reports.
    /// Where Pulse runs without mDNS, the simulator stands in for it.
    pub fn attach(&self, found: Arc<Mutex<Found>>) {
        {
            let mut f = lock(&found);
            if f.mdns_state == Listener::Off {
                f.mdns_state = Listener::On(now_ms().saturating_sub(60_000));
            }
        }
        if let Ok(mut t) = self.target.lock() {
            *t = Some(found);
        }
    }

    /// The demo's scenario: now and then something goes wrong for a while.
    pub fn scenario(&self, steps: Vec<Step>, period: Duration) {
        self.scenario.store(true, Ordering::Relaxed);
        let on = self.scenario.clone();
        let faults: Vec<(String, Arc<Fault>)> = self
            .devices
            .iter()
            .map(|d| (d.name.clone(), d.fault.clone()))
            .collect();
        self.handle.spawn(async move {
            let find = |n: &str| faults.iter().find(|(x, _)| x == n).map(|(_, f)| f.clone());
            loop {
                let start = tokio::time::Instant::now();
                for s in &steps {
                    tokio::time::sleep_until(start + s.at).await;
                    if !on.load(Ordering::Relaxed) {
                        return;
                    }
                    if let Some(f) = find(&s.device) {
                        s.action.apply(&f);
                    }
                }
                tokio::time::sleep_until(start + period).await;
            }
        });
    }

    /// Writes the announcements into the findings every second.
    fn spawn_announcer(&self) {
        let list: Vec<(String, String, u16, Arc<Fault>)> = self
            .devices
            .iter()
            .filter_map(|d| match &d.kind {
                Kind::Announce { service, instance } => Some((
                    service_short(service).to_string(),
                    instance.clone(),
                    d.port,
                    d.fault.clone(),
                )),
                Kind::Ndi { source } => Some((
                    pulse_core::model::NDI_SERVICE.to_string(),
                    source.clone(),
                    d.port,
                    d.fault.clone(),
                )),
                _ => None,
            })
            .collect();
        if list.is_empty() {
            return;
        }
        let target = self.target.clone();
        self.handle.spawn(async move {
            let mut every = tokio::time::interval(Duration::from_secs(1));
            loop {
                every.tick().await;
                let Some(found) = target_found(&target) else {
                    continue;
                };
                let now = now_ms();
                let mut f = lock(&found);
                for (ty, instance, port, fault) in &list {
                    let full = format!("{instance}.{ty}.local.");
                    if fault.down.load(Ordering::Relaxed) {
                        f.mdns.remove(&full);
                    } else {
                        f.mdns.insert(
                            full,
                            Service {
                                ty: ty.clone(),
                                instance: instance.clone(),
                                host: "pulse-sim.local".into(),
                                port: *port,
                                addrs: vec![Ipv4Addr::LOCALHOST],
                                txt: vec![("simulated".into(), "yes".into())],
                                at: now,
                            },
                        );
                    }
                }
            }
        });
    }
}

impl Drop for Sim {
    fn drop(&mut self) {
        self.scenario.store(false, Ordering::Relaxed);
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
    }
}

/// A step of a scenario: at `at` after the period's start, do `action` to `device`.
#[derive(Clone, Debug)]
pub struct Step {
    pub at: Duration,
    pub device: String,
    pub action: Action,
}

#[derive(Clone, Copy, Debug)]
pub enum Action {
    Down(bool),
    Closed(bool),
    Fps(Option<f64>),
    Slow(u32),
}

impl Action {
    fn apply(&self, f: &Fault) {
        match *self {
            Action::Down(b) => f.down.store(b, Ordering::Relaxed),
            Action::Closed(b) => f.closed.store(b, Ordering::Relaxed),
            Action::Fps(x) => f.fps_centi.store(
                x.map(|v| (v.max(0.01) * 100.0) as u32).unwrap_or(0),
                Ordering::Relaxed,
            ),
            Action::Slow(ms) => f.slow_ms.store(ms, Ordering::Relaxed),
        }
    }
}

/// Start one device; returns its port.
fn spawn_device(
    h: &tokio::runtime::Handle,
    kind: &Kind,
    fault: Arc<Fault>,
    target: Target,
    sent: Arc<AtomicU64>,
) -> std::io::Result<u16> {
    match kind.clone() {
        Kind::Console {
            model,
            name,
            firmware,
        } => {
            let sock = pulse_net::udp::bind(Ipv4Addr::LOCALHOST, 0, false)?;
            let port = sock.local_addr()?.port();
            let kind_name = match model {
                ConsoleModel::X32 => "X32",
                ConsoleModel::XAir => "XR18",
                ConsoleModel::Wing => "ngc-full",
            };
            h.spawn(async move {
                let mut buf = vec![0u8; 1500];
                let question = console::request(model);
                loop {
                    let Ok((n, from)) = sock.recv_from(&mut buf).await else {
                        continue;
                    };
                    if fault.down.load(Ordering::Relaxed) || buf[..n] != question[..] {
                        continue;
                    }
                    let slow = fault.slow_ms.load(Ordering::Relaxed);
                    if slow > 0 {
                        tokio::time::sleep(Duration::from_millis(u64::from(slow))).await;
                    }
                    let a =
                        console::answer(model, Ipv4Addr::LOCALHOST, &name, kind_name, &firmware);
                    if sock.send_to(&a, from).await.is_ok() {
                        sent.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
            Ok(port)
        }
        Kind::ArtNetNode {
            short_name,
            outputs,
        } => {
            let sock = pulse_net::udp::bind(Ipv4Addr::LOCALHOST, 0, false)?;
            let port = sock.local_addr()?.port();
            let reply = artnet::poll_reply(&PollReply {
                ip: Ipv4Addr::LOCALHOST,
                short_name: short_name.clone(),
                long_name: format!("{short_name} (simulated)"),
                report: "#0001 [0001] simulated node OK".into(),
                outputs,
                ..PollReply::default()
            });
            h.spawn(async move {
                let mut buf = vec![0u8; 1500];
                let mut every = tokio::time::interval(Duration::from_secs(3));
                loop {
                    tokio::select! {
                        r = sock.recv_from(&mut buf) => {
                            let Ok((n, from)) = r else { continue };
                            if !fault.down.load(Ordering::Relaxed)
                                && artnet::parse(&buf[..n]) == Some(artnet::Packet::Poll)
                                && sock.send_to(&reply, from).await.is_ok()
                            {
                                sent.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        _ = every.tick() => {
                            // Nodes on this computer report to Pulse directly (it may not poll here).
                            if fault.down.load(Ordering::Relaxed) {
                                continue;
                            }
                            if let Some(to) = artnet_dest(&target) {
                                if sock.send_to(&reply, to).await.is_ok() {
                                    sent.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                    }
                }
            });
            Ok(port)
        }
        Kind::ArtNetSender { universe, fps } => {
            let sock = pulse_net::udp::bind(Ipv4Addr::LOCALHOST, 0, false)?;
            h.spawn(async move {
                let mut seq: u8 = 0;
                let mut level: u8 = 0;
                loop {
                    let o = fault.fps_centi.load(Ordering::Relaxed);
                    let rate = if o > 0 { f64::from(o) / 100.0 } else { fps }.clamp(0.1, 60.0);
                    tokio::time::sleep(Duration::from_secs_f64(1.0 / rate)).await;
                    if fault.down.load(Ordering::Relaxed) {
                        continue;
                    }
                    let Some(to) = artnet_dest(&target) else {
                        continue;
                    };
                    seq = seq.wrapping_add(1).max(1);
                    level = level.wrapping_add(3);
                    let mut ch = [0u8; 512];
                    ch[0] = level;
                    if sock
                        .send_to(&artnet::dmx(universe, seq, &ch), to)
                        .await
                        .is_ok()
                    {
                        sent.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
            Ok(0)
        }
        Kind::Tcp | Kind::Ndi { .. } => {
            let l = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
            let port = l.local_addr()?.port();
            l.set_nonblocking(true)?;
            let first = tokio::net::TcpListener::from_std(l)?;
            h.spawn(tcp_service(first, port, fault));
            Ok(port)
        }
        Kind::Announce { .. } => Ok(0),
    }
}

/// Accept and close at once; closed or down = no listener at all (refused).
async fn tcp_service(first: tokio::net::TcpListener, port: u16, fault: Arc<Fault>) {
    let mut l = Some(first);
    loop {
        let off = fault.down.load(Ordering::Relaxed) || fault.closed.load(Ordering::Relaxed);
        match (&l, off) {
            (Some(_), true) => l = None,
            (None, false) => {
                l = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))
                    .await
                    .ok();
            }
            _ => {}
        }
        match &l {
            Some(listener) => {
                if let Ok(Ok((s, _))) =
                    tokio::time::timeout(Duration::from_millis(200), listener.accept()).await
                {
                    drop(s);
                }
            }
            None => tokio::time::sleep(Duration::from_millis(200)).await,
        }
    }
}
