//! The monitor: runs every enabled check on its own interval, spread out in
//! time and under a global packet limit, each run in its own task with a
//! hard timeout. Results go through the status engine; the window reads a
//! shared snapshot and never waits for the network.
//!
//! The monitor owns a tokio runtime on a thread of its own, so the egui app
//! (which has no runtime) can simply start it and read [`Monitor::with`].

use crate::found::{self, Found};
use crate::icmp::{self, Method};
use crate::interfaces;
use crate::mdns::Registry;
use crate::probe::{self, Ctx, Probe};
use pulse_core::discovery::{Net, DISCOVERY_SERVICES};
use pulse_core::events::{Event, EventLog};
use pulse_core::history::History;
use pulse_core::model::{Check, Device, Id, Profile};
use pulse_core::model::{CheckKind, ConsoleModel, NDI_SERVICE};
use pulse_core::probe::{Depth, Outcome, ProbeResult};
use pulse_core::status::{Engine, Ms, Overview, Status};
use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

/// Unix time now, in milliseconds.
pub fn now_ms() -> Ms {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as Ms)
        .unwrap_or(0)
}

/// Monitoring, or the more thorough pre-show check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Monitor,
    /// More echo requests per run.
    Thorough,
}

#[derive(Clone)]
pub struct Options {
    /// Packets per second at most, for all checks together.
    pub max_rate: f64,
    pub mode: Mode,
    /// Called after the snapshot changed (e.g. to repaint the window).
    pub on_change: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Ping method; `None` = what this computer allows ([`icmp::method`]).
    pub method: Option<Method>,
    /// The Art-Net port to listen on (6454; tests use another).
    pub artnet_port: u16,
    /// More ArtPoll targets than the broadcasts (tests, the simulator).
    pub artnet_targets: Vec<SocketAddr>,
    /// More console query targets than the broadcasts (tests, the simulator).
    pub console_targets: Vec<(ConsoleModel, SocketAddr)>,
    /// Use mDNS (tests without a network may turn it off).
    pub mdns: bool,
    /// Broadcast ArtPoll and console queries on this computer's networks
    /// (off: only to the extra targets; tests).
    pub lan: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            max_rate: 50.0,
            mode: Mode::Monitor,
            on_change: None,
            method: None,
            artnet_port: pulse_core::artnet::PORT,
            artnet_targets: Vec::new(),
            console_targets: Vec::new(),
            mdns: true,
            lan: true,
        }
    }
}

/// Counters for the network load.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Load {
    pub runs: u64,
    pub packets: u64,
    /// Estimated bytes put on the wire (requests only).
    pub bytes: u64,
    /// Runs that hit the hard timeout.
    pub timeouts: u64,
    /// Runs that crashed (their check is set up again).
    pub crashes: u64,
    /// Runs postponed by the packet limit.
    pub throttled: u64,
    pub since: Ms,
}

impl Load {
    /// Packets per second since start.
    pub fn packet_rate(&self, now: Ms) -> f64 {
        let s = now.saturating_sub(self.since) as f64 / 1000.0;
        if s > 0.0 {
            self.packets as f64 / s
        } else {
            0.0
        }
    }
}

/// What the window reads.
pub struct Shared {
    pub profile: Profile,
    pub engine: Engine,
    pub overview: Overview,
    pub log: EventLog,
    pub history: History,
    pub method: Method,
    pub mode: Mode,
    pub load: Load,
    /// Checks running right now.
    pub running: usize,
}

enum Cmd {
    Profile(Box<Profile>),
    CheckNow(Option<Id>),
    Ack(Id, String),
    Unack(Id),
    Mode(Mode),
    Discovery(bool),
    Scan,
    Sweep(Net, Option<Ipv4Addr>),
    CancelSweep,
    Stop,
}

pub struct Monitor {
    shared: Arc<Mutex<Shared>>,
    found: Arc<Mutex<Found>>,
    tx: mpsc::UnboundedSender<Cmd>,
}

fn lock(m: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Monitor {
    /// Start watching `profile` on a thread of its own.
    pub fn start(profile: Profile, opts: Options) -> std::io::Result<Monitor> {
        let now = now_ms();
        let method = opts
            .method
            .clone()
            .unwrap_or_else(|| icmp::method().clone());
        let engine = Engine::new(&profile, now);
        let shared = Arc::new(Mutex::new(Shared {
            overview: engine.overview(&profile),
            profile,
            engine,
            log: EventLog::default(),
            history: History::default(),
            method,
            mode: opts.mode,
            load: Load {
                since: now,
                ..Load::default()
            },
            running: 0,
        }));
        let found = Arc::new(Mutex::new(Found {
            nics: interfaces::list(),
            ..Found::default()
        }));
        let (tx, rx) = mpsc::unbounded_channel();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(256)
            .thread_name("pulse-probe")
            .enable_all()
            .build()?;
        let s = shared.clone();
        let f = found.clone();
        std::thread::Builder::new()
            .name("pulse-monitor".into())
            .spawn(move || {
                rt.block_on(run(s, f, rx, opts));
                // Probes still running are dropped with the runtime.
                rt.shutdown_timeout(Duration::from_millis(200));
            })?;
        Ok(Monitor { shared, found, tx })
    }

    /// Read the current state (briefly locked).
    pub fn with<R>(&self, f: impl FnOnce(&Shared) -> R) -> R {
        f(&lock(&self.shared))
    }

    /// Watch a changed inventory (checks that changed start over).
    pub fn set_profile(&self, p: Profile) {
        let _ = self.tx.send(Cmd::Profile(Box::new(p)));
    }
    /// Run the checks of one device (or all) now.
    pub fn check_now(&self, device: Option<Id>) {
        let _ = self.tx.send(Cmd::CheckNow(device));
    }
    pub fn ack(&self, device: Id, note: &str) {
        let _ = self.tx.send(Cmd::Ack(device, note.to_string()));
    }
    pub fn unack(&self, device: Id) {
        let _ = self.tx.send(Cmd::Unack(device));
    }
    pub fn set_mode(&self, m: Mode) {
        let _ = self.tx.send(Cmd::Mode(m));
    }

    /// What the listeners found (briefly locked).
    pub fn found<R>(&self, f: impl FnOnce(&Found) -> R) -> R {
        f(&found::lock(&self.found))
    }
    /// Change what was found (the simulator and tests put devices there).
    pub fn inject(&self, f: impl FnOnce(&mut Found)) {
        f(&mut found::lock(&self.found))
    }
    /// The discovery page is open: listen to everything, poll, ask consoles.
    pub fn set_discovery(&self, on: bool) {
        let _ = self.tx.send(Cmd::Discovery(on));
    }
    /// Ask the consoles and Art-Net nodes again now.
    pub fn scan(&self) {
        let _ = self.tx.send(Cmd::Scan);
    }
    /// Ping every address of `net` once (gently); only after the user confirmed it.
    pub fn sweep(&self, net: Net, source: Option<Ipv4Addr>) {
        let _ = self.tx.send(Cmd::Sweep(net, source));
    }
    pub fn cancel_sweep(&self) {
        let _ = self.tx.send(Cmd::CancelSweep);
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

/// A token bucket: `rate` packets per second, bursts up to one second's worth.
struct Bucket {
    rate: f64,
    tokens: f64,
    at: Instant,
}

impl Bucket {
    fn new(rate: f64) -> Self {
        let rate = rate.max(1.0);
        Bucket {
            rate,
            tokens: rate,
            at: Instant::now(),
        }
    }
    fn take(&mut self, n: u32) -> bool {
        let now = Instant::now();
        self.tokens =
            (self.tokens + now.duration_since(self.at).as_secs_f64() * self.rate).min(self.rate);
        self.at = now;
        let n = f64::from(n).min(self.rate);
        if self.tokens >= n {
            self.tokens -= n;
            true
        } else {
            false
        }
    }
}

/// One check's place in the schedule.
struct Slot {
    device: Id,
    /// The check as configured when the probe was made (a change makes a new probe).
    check: Check,
    address: String,
    interface: String,
    /// `None` while it runs.
    probe: Option<Box<dyn Probe>>,
    next: Instant,
}

struct Done {
    check: Id,
    probe: Option<Box<dyn Probe>>,
    result: ProbeResult,
    timed_out: bool,
}

/// Where in its interval a check starts: spread by id, the same every time.
fn offset(id: Id, interval: Duration) -> Duration {
    let ms = interval.as_millis().max(1) as u64;
    Duration::from_millis(id.wrapping_mul(2_654_435_761) % ms)
}

fn slots_for(p: &Profile, old: &mut BTreeMap<Id, Slot>, now: Instant) -> BTreeMap<Id, Slot> {
    let mut out = BTreeMap::new();
    for d in p.devices.iter().filter(|d| d.enabled) {
        for c in d.checks.iter().filter(|c| c.enabled) {
            let interval = Duration::from_secs_f64(c.interval());
            match old.remove(&c.id) {
                Some(s)
                    if s.check == *c
                        && s.address == d.address
                        && s.interface == d.interface
                        && s.device == d.id =>
                {
                    out.insert(c.id, s);
                }
                // In flight or changed: a new probe (a late result of the old one is dropped).
                _ => {
                    out.insert(c.id, new_slot(d, c, now + offset(c.id, interval)));
                }
            }
        }
    }
    out
}

fn new_slot(d: &Device, c: &Check, next: Instant) -> Slot {
    Slot {
        device: d.id,
        check: c.clone(),
        address: d.address.clone(),
        interface: d.interface.trim().to_string(),
        probe: Some(probe::make(d, c)),
        next,
    }
}

/// What the listeners must do for this inventory (and the discovery page).
#[derive(Default, PartialEq, Debug)]
struct Wants {
    artnet: bool,
    poll: bool,
    mdns: BTreeSet<String>,
}

fn wants(p: &Profile, discovering: bool) -> Wants {
    let mut w = Wants {
        artnet: discovering,
        poll: discovering,
        mdns: BTreeSet::new(),
    };
    if discovering {
        w.mdns
            .extend(DISCOVERY_SERVICES.iter().map(|s| s.to_string()));
    }
    for d in p.devices.iter().filter(|d| d.enabled) {
        for c in d.checks.iter().filter(|c| c.enabled) {
            match &c.kind {
                CheckKind::ArtNetNode => {
                    w.artnet = true;
                    w.poll = true;
                }
                CheckKind::ArtNetUniverse { .. } => w.artnet = true,
                CheckKind::Mdns { service, .. } if !service.trim().is_empty() => {
                    w.mdns
                        .insert(pulse_core::model::service_short(service).to_string());
                }
                CheckKind::Ndi { .. } => {
                    w.mdns.insert(NDI_SERVICE.to_string());
                }
                _ => {}
            }
        }
    }
    w
}

/// The listeners and discovery tasks.
struct Listeners {
    registry: Registry,
    artnet: Option<tokio::task::JoinHandle<()>>,
    poll: Arc<AtomicBool>,
    sweep: Option<tokio::task::JoinHandle<()>>,
    discovering: bool,
    /// Packets sent by listeners and discovery (added to the load).
    sent: Arc<AtomicU64>,
    counted: u64,
}

impl Listeners {
    fn apply(&mut self, p: &Profile, found: &Arc<Mutex<Found>>, opts: &Options) {
        let w = wants(p, self.discovering);
        self.poll.store(w.poll, Ordering::Relaxed);
        if w.artnet && self.artnet.is_none() {
            self.artnet = Some(tokio::spawn(crate::artnet::listen(
                found.clone(),
                opts.artnet_port,
                self.poll.clone(),
                opts.artnet_targets.clone(),
                opts.lan,
                self.sent.clone(),
            )));
        } else if !w.artnet {
            if let Some(h) = self.artnet.take() {
                h.abort();
                found::lock(found).artnet_state = found::Listener::Off;
            }
        }
        if opts.mdns {
            self.registry.set_types(found, &w.mdns);
        }
    }

    fn scan(&self, found: &Arc<Mutex<Found>>, opts: &Options) {
        tokio::spawn(crate::discovery::scan_consoles(
            found.clone(),
            opts.console_targets.clone(),
            opts.lan,
            self.sent.clone(),
        ));
    }
}

impl Drop for Listeners {
    fn drop(&mut self) {
        if let Some(h) = self.artnet.take() {
            h.abort();
        }
        if let Some(h) = self.sweep.take() {
            h.abort();
        }
    }
}

/// Seconds between console broadcasts while discovery is open.
const SCAN_EVERY: Duration = Duration::from_secs(10);

async fn run(
    shared: Arc<Mutex<Shared>>,
    found: Arc<Mutex<Found>>,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
    opts: Options,
) {
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<Done>();
    let mut ls = Listeners {
        registry: Registry::new(),
        artnet: None,
        poll: Arc::new(AtomicBool::new(false)),
        sweep: None,
        discovering: false,
        sent: Arc::new(AtomicU64::new(0)),
        counted: 0,
    };
    {
        let s = lock(&shared);
        ls.apply(&s.profile, &found, &opts);
    }
    let mut last_nics = Instant::now();
    let mut last_drain = Instant::now();
    let mut last_scan: Option<Instant> = None;
    let mut bucket = Bucket::new(opts.max_rate);
    let mut slots = {
        let s = lock(&shared);
        slots_for(&s.profile, &mut BTreeMap::new(), Instant::now())
    };
    let notify = || {
        if let Some(f) = &opts.on_change {
            f();
        }
    };
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_engine_tick = Instant::now();
    loop {
        tokio::select! {
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { return };
                let now = now_ms();
                let mut s = lock(&shared);
                match cmd {
                    Cmd::Stop => return,
                    Cmd::Profile(p) => {
                        s.profile = *p;
                        let Shared { profile, engine, log, history, .. } = &mut *s;
                        log.extend(engine.sync(profile, now));
                        let ids: std::collections::HashSet<Id> =
                            profile.devices.iter().flat_map(|d| d.checks.iter().map(|c| c.id)).collect();
                        history.retain(|c| ids.contains(&c));
                        slots = slots_for(profile, &mut slots, Instant::now());
                    }
                    Cmd::CheckNow(device) => {
                        let at = Instant::now();
                        for sl in slots.values_mut().filter(|sl| device.is_none_or(|d| d == sl.device)) {
                            sl.next = at;
                        }
                    }
                    Cmd::Ack(d, note) => {
                        let Shared { profile, engine, log, .. } = &mut *s;
                        if let Some(e) = engine.ack(profile, d, &note, now) {
                            log.push(e);
                        }
                    }
                    Cmd::Unack(d) => {
                        let Shared { profile, engine, log, .. } = &mut *s;
                        if let Some(e) = engine.unack(profile, d, now) {
                            log.push(e);
                        }
                    }
                    Cmd::Mode(m) => s.mode = m,
                    Cmd::Discovery(on) => {
                        ls.discovering = on;
                        found::lock(&found).discovering = on;
                        if on {
                            ls.scan(&found, &opts);
                            last_scan = Some(Instant::now());
                        }
                    }
                    Cmd::Scan => {
                        ls.scan(&found, &opts);
                        last_scan = Some(Instant::now());
                    }
                    Cmd::Sweep(net, source) => {
                        if let Some(h) = ls.sweep.take() {
                            h.abort();
                        }
                        ls.sweep = Some(tokio::spawn(crate::discovery::sweep(
                            found.clone(),
                            net,
                            source,
                            s.method.clone(),
                            ls.sent.clone(),
                        )));
                    }
                    Cmd::CancelSweep => {
                        if let Some(sw) = found::lock(&found).sweep.as_mut() {
                            sw.cancelled = true;
                            sw.finished.get_or_insert(now);
                        }
                        if let Some(h) = ls.sweep.take() {
                            h.abort();
                        }
                    }
                }
                ls.apply(&s.profile, &found, &opts);
                s.overview = s.engine.overview(&s.profile);
                drop(s);
                notify();
            }
            Some(done) = done_rx.recv() => {
                let now = now_ms();
                let mut s = lock(&shared);
                s.running = s.running.saturating_sub(1);
                if done.timed_out {
                    s.load.timeouts += 1;
                }
                if done.probe.is_none() {
                    s.load.crashes += 1;
                }
                let Some(slot) = slots.get_mut(&done.check) else { continue };
                if slot.probe.is_some() {
                    // A result of a probe replaced in the meantime.
                    continue;
                }
                slot.probe = match done.probe {
                    Some(p) => Some(p),
                    None => {
                        let d = s.profile.device(slot.device).cloned().unwrap_or_default();
                        Some(probe::make(&d, &slot.check))
                    }
                };
                let device = slot.device;
                let Shared { profile, engine, log, history, overview, .. } = &mut *s;
                history.record(done.check, &done.result, now);
                let events: Vec<Event> = engine.apply(profile, device, done.check, &done.result, now);
                log.extend(events);
                *overview = engine.overview(profile);
                drop(s);
                notify();
            }
            _ = tick.tick() => {
                let at = Instant::now();
                if at.duration_since(last_drain) >= Duration::from_millis(200) {
                    last_drain = at;
                    ls.registry.drain(&found);
                }
                if at.duration_since(last_nics) >= Duration::from_secs(2) {
                    last_nics = at;
                    let nics = interfaces::list();
                    let mut f = found::lock(&found);
                    f.nics = nics;
                    f.prune(now_ms());
                }
                if ls.discovering && last_scan.is_none_or(|t| at.duration_since(t) >= SCAN_EVERY) {
                    last_scan = Some(at);
                    ls.scan(&found, &opts);
                }
                let nics = found::lock(&found).nics.clone();
                let (method, mode, backoff) = {
                    let s = lock(&shared);
                    let down: std::collections::HashSet<Id> = slots
                        .iter()
                        .filter(|(id, sl)| {
                            s.engine
                                .device(sl.device)
                                .and_then(|d| d.checks.get(id))
                                .is_some_and(|c| c.status == Status::Down)
                        })
                        .map(|(id, _)| *id)
                        .collect();
                    (s.method.clone(), s.mode, down)
                };
                let mut started = 0u64;
                let mut packets = 0u64;
                let mut bytes = 0u64;
                let mut throttled = 0u64;
                for (id, slot) in slots.iter_mut() {
                    if slot.next > at || slot.probe.is_none() {
                        continue;
                    }
                    // Bound to the device's interface: from its address, or not at all.
                    let bound = !slot.interface.is_empty()
                        && !slot.check.kind.passive();
                    let (source, blocked) = if bound {
                        match interfaces::find(&nics, &slot.interface) {
                            Some(n) => (Some(n.ip), None),
                            None => (None, Some(format!("interface {} is down or missing", slot.interface))),
                        }
                    } else {
                        (None, None)
                    };
                    let ctx = Ctx {
                        timeout: Duration::from_millis(u64::from(slot.check.timeout_ms())),
                        ping_count: if mode == Mode::Thorough { 5 } else { 1 },
                        method: method.clone(),
                        source,
                        found: found.clone(),
                    };
                    let Some(mut p) = slot.probe.take() else { continue };
                    let cost = p.cost(&ctx);
                    if !bucket.take(cost) {
                        slot.probe = Some(p);
                        slot.next = at + Duration::from_millis(100);
                        throttled += 1;
                        continue;
                    }
                    // Down checks back off (twice the interval, up to 10 s more).
                    let interval = Duration::from_secs_f64(slot.check.interval());
                    let wait = if backoff.contains(id) {
                        interval + interval.min(Duration::from_secs(10))
                    } else {
                        interval
                    };
                    slot.next = at + wait;
                    started += 1;
                    packets += u64::from(cost);
                    bytes += u64::from(cost) * icmp::WIRE_BYTES;
                    // Hard limit: the probe's own timeouts, all requests, plus a margin.
                    let hard = ctx.timeout * ctx.ping_count + Duration::from_millis(200 * u64::from(ctx.ping_count) + 2000);
                    let tx = done_tx.clone();
                    let check = *id;
                    let task = tokio::spawn(async move {
                        if let Some(why) = blocked {
                            let mut r = ProbeResult::new(Outcome::Unknown, Depth::Reachable, why);
                            r.via = "this computer".into();
                            return (p, Ok(r));
                        }
                        let r = tokio::time::timeout(hard, p.run(&ctx)).await;
                        (p, r)
                    });
                    tokio::spawn(async move {
                        let done = match task.await {
                            Ok((p, Ok(result))) => Done { check, probe: Some(p), result, timed_out: false },
                            Ok((p, Err(_))) => Done {
                                check,
                                probe: Some(p),
                                result: ProbeResult::new(Outcome::Fail, Depth::Reachable, "no result in time (probe timed out)"),
                                timed_out: true,
                            },
                            Err(e) => Done {
                                check,
                                probe: None,
                                result: ProbeResult::new(Outcome::Unknown, Depth::Reachable, format!("check crashed: {e}")),
                                timed_out: false,
                            },
                        };
                        let _ = tx.send(done);
                    });
                }
                let mut s = lock(&shared);
                let sent = ls.sent.load(Ordering::Relaxed);
                if sent > ls.counted {
                    s.load.packets += sent - ls.counted;
                    s.load.bytes += (sent - ls.counted) * 60;
                    ls.counted = sent;
                }
                if started > 0 || throttled > 0 {
                    s.running += started as usize;
                    s.load.runs += started;
                    s.load.packets += packets;
                    s.load.bytes += bytes;
                    s.load.throttled += throttled;
                }
                if last_engine_tick.elapsed() >= Duration::from_secs(1) {
                    last_engine_tick = at;
                    let now = now_ms();
                    let Shared { profile, engine, log, overview, .. } = &mut *s;
                    let events = engine.tick(profile, now);
                    if !events.is_empty() {
                        log.extend(events);
                        *overview = engine.overview(profile);
                        drop(s);
                        notify();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_spread_checks_over_the_interval() {
        let i = Duration::from_secs(2);
        let offs: std::collections::BTreeSet<_> = (1..50u64)
            .map(|id| offset(id, i).as_millis() / 100)
            .collect();
        assert!(
            offs.len() > 12,
            "checks should not all start together: {offs:?}"
        );
        assert!((1..50u64).all(|id| offset(id, i) < i));
    }

    #[test]
    fn bucket_limits_packets_per_second() {
        let mut b = Bucket::new(10.0);
        let taken = (0..30).filter(|_| b.take(1)).count();
        assert_eq!(taken, 10);
        assert!(!b.take(1));
    }
}
