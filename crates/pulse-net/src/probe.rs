//! The probe interface and the probes for each kind of check.
//!
//! A probe belongs to one check of one device and may keep state between
//! runs (the ping window, a resolved address). The scheduler moves it into a
//! task for each run, with a hard timeout around it, and gets it back with
//! the result. Probes only ask: they never write to a device.

use crate::console;
use crate::found::{lock, Found, Listener};
use crate::icmp::{self, Method};
use crate::interfaces;
use crate::monitor::now_ms;
use pulse_core::artnet::{universe_label, SOURCE_TTL_MS};
use pulse_core::model::{
    service_short, Check, CheckKind, ConsoleModel, Device, Thresholds, NDI_SERVICE,
};
use pulse_core::ping::{self, PingWindow};
use pulse_core::probe::{Depth, Metric, Outcome, ProbeResult};
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What a run may use.
#[derive(Clone, Debug)]
pub struct Ctx {
    /// Per request (ping) or connection (TCP).
    pub timeout: Duration,
    /// Echo requests per ping run (more in the pre-show check).
    pub ping_count: u32,
    pub method: Method,
    /// Send from this address (the device's interface); `None` = the system's choice.
    pub source: Option<Ipv4Addr>,
    /// What the listeners found.
    pub found: Arc<Mutex<Found>>,
}

impl Ctx {
    /// A context for a probe run outside the monitor (tests, diagnosis).
    pub fn simple(timeout: Duration) -> Self {
        Ctx {
            timeout,
            ping_count: 1,
            method: icmp::method().clone(),
            source: None,
            found: Arc::default(),
        }
    }
}

pub trait Probe: Send {
    /// Packets one run sends (for the rate limit and the network load).
    fn cost(&self, ctx: &Ctx) -> u32;
    fn run<'a>(&'a mut self, ctx: &'a Ctx) -> BoxFut<'a, ProbeResult>;
}

/// The probe for `check` of `device`.
pub fn make(device: &Device, check: &Check) -> Box<dyn Probe> {
    let target = Target::new(&device.address);
    match &check.kind {
        CheckKind::Ping => Box::new(PingProbe {
            target,
            thresholds: check.thresholds,
            window: PingWindow::default(),
            seq: 0,
        }),
        CheckKind::Tcp { port } => Box::new(TcpProbe {
            target,
            port: *port,
            thresholds: check.thresholds,
        }),
        CheckKind::Console { model, port } => Box::new(ConsoleProbe {
            target,
            model: *model,
            port: port.unwrap_or_else(|| model.port()),
        }),
        CheckKind::ArtNetNode => Box::new(ArtNetNodeProbe { target }),
        CheckKind::ArtNetUniverse { universe, min_fps } => Box::new(UniverseProbe {
            universe: *universe,
            min_fps: *min_fps,
            sacn: false,
        }),
        CheckKind::SacnUniverse { universe, min_fps } => Box::new(UniverseProbe {
            universe: *universe,
            min_fps: *min_fps,
            sacn: true,
        }),
        CheckKind::Snmp {
            community,
            if_index,
            port,
        } => Box::new(crate::snmp::SnmpProbe::new(
            target,
            community,
            *if_index,
            port.unwrap_or(pulse_core::snmp::PORT),
        )),
        CheckKind::Agent {
            processes,
            min_free_gb,
            port,
        } => Box::new(crate::agent::AgentProbe::new(
            target,
            processes.clone(),
            *min_free_gb,
            port.unwrap_or(pulse_core::agent::PORT),
        )),
        CheckKind::Mdns { service, instance } => Box::new(MdnsProbe {
            target,
            service: service_short(service).to_string(),
            instance: instance.trim().to_string(),
        }),
        CheckKind::Ndi { source } => Box::new(NdiProbe {
            target,
            source: source.trim().to_string(),
        }),
        CheckKind::Interface { name } => Box::new(InterfaceProbe {
            name: if name.trim().is_empty() {
                device.interface.trim().to_string()
            } else {
                name.trim().to_string()
            },
        }),
    }
}

/// An address or host name, resolved now and then (names may move).
pub(crate) struct Target {
    host: String,
    resolved: Option<(IpAddr, Instant)>,
}

/// How long a resolved name is trusted.
const RESOLVE_TTL: Duration = Duration::from_secs(60);

impl Target {
    pub(crate) fn new(host: &str) -> Self {
        let host = host.trim().to_string();
        let resolved = host.parse::<IpAddr>().ok().map(|ip| (ip, Instant::now()));
        Target { host, resolved }
    }

    pub(crate) async fn ip(&mut self, timeout: Duration) -> Result<IpAddr, String> {
        if let Ok(ip) = self.host.parse::<IpAddr>() {
            return Ok(ip);
        }
        if self.host.is_empty() {
            return Err("no address set".into());
        }
        if let Some((ip, at)) = self.resolved {
            if at.elapsed() < RESOLVE_TTL {
                return Ok(ip);
            }
        }
        let lookup = tokio::net::lookup_host((self.host.as_str(), 0));
        let addrs: Vec<SocketAddr> =
            match tokio::time::timeout(timeout.max(Duration::from_secs(2)), lookup).await {
                Ok(Ok(a)) => a.collect(),
                Ok(Err(e)) => return Err(format!("cannot resolve {}: {e}", self.host)),
                Err(_) => return Err(format!("cannot resolve {} (timed out)", self.host)),
            };
        // IPv4 first: that is what show networks use.
        let ip = addrs
            .iter()
            .map(|a| a.ip())
            .find(IpAddr::is_ipv4)
            .or_else(|| addrs.first().map(|a| a.ip()))
            .ok_or_else(|| format!("cannot resolve {}", self.host))?;
        self.resolved = Some((ip, Instant::now()));
        Ok(ip)
    }
}

/// Opening a TCP connection: the time it took, and Ok or Err((refused, message)).
pub(crate) async fn connect(
    ip: IpAddr,
    port: u16,
    source: Option<Ipv4Addr>,
    timeout: Duration,
) -> (f64, Result<(), (bool, String)>) {
    let start = Instant::now();
    let to = SocketAddr::new(ip, port);
    let open = async move {
        match (ip, source) {
            (IpAddr::V4(_), Some(src)) => {
                let s = tokio::net::TcpSocket::new_v4()?;
                s.bind(SocketAddr::new(IpAddr::V4(src), 0))?;
                s.connect(to).await
            }
            _ => tokio::net::TcpStream::connect(to).await,
        }
    };
    let r = match tokio::time::timeout(timeout, open).await {
        // Closed again at once; nothing is sent.
        Ok(Ok(stream)) => {
            drop(stream);
            Ok(())
        }
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            Err((true, "connection refused".into()))
        }
        Ok(Err(e)) => Err((false, e.to_string())),
        Err(_) => Err((
            false,
            format!("no answer within {} ms", timeout.as_millis()),
        )),
    };
    (start.elapsed().as_secs_f64() * 1000.0, r)
}

/// One echo (or the TCP fallback): `Ok(Some(ms))`, `Ok(None)` = no answer,
/// `Err` = could not be sent.
pub(crate) async fn ping_once(
    ip: IpAddr,
    method: &Method,
    source: Option<Ipv4Addr>,
    seq: u16,
    timeout: Duration,
) -> Result<Option<f64>, String> {
    match (method, ip) {
        (Method::Icmp, IpAddr::V4(v4)) => {
            match tokio::task::spawn_blocking(move || icmp::echo(v4, source, seq, timeout)).await {
                Ok(Ok(s)) => Ok(s),
                Ok(Err(e)) => Err(e.to_string()),
                Err(e) => Err(e.to_string()),
            }
        }
        // Refused means a device answered: it is there.
        _ => match connect(ip, icmp::TCP_FALLBACK_PORT, source, timeout).await {
            (ms, Ok(()) | Err((true, _))) => Ok(Some(ms)),
            (_, Err((false, e))) if e.starts_with("no answer") => Ok(None),
            (_, Err((false, e))) => Err(e),
        },
    }
}

struct PingProbe {
    target: Target,
    thresholds: Thresholds,
    window: PingWindow,
    seq: u16,
}

impl PingProbe {
    async fn round(&mut self, ip: IpAddr, ctx: &Ctx) -> (Vec<Option<f64>>, String, Vec<String>) {
        let count = ctx.ping_count.clamp(1, 20);
        let mut round = Vec::new();
        let mut errors = Vec::new();
        let (method, via) = match (&ctx.method, ip) {
            (Method::Icmp, IpAddr::V4(_)) => (Method::Icmp, Method::Icmp.label()),
            (Method::Icmp, IpAddr::V6(_)) => {
                let m = Method::Tcp {
                    reason: "IPv6 address".into(),
                };
                let l = m.label();
                (m, l)
            }
            (m, _) => (m.clone(), m.label()),
        };
        for i in 0..count {
            if i > 0 {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            self.seq = self.seq.wrapping_add(1);
            match ping_once(ip, &method, ctx.source, self.seq, ctx.timeout).await {
                Ok(s) => round.push(s),
                Err(e) => {
                    errors.push(e);
                    round.push(None);
                }
            }
        }
        (round, via, errors)
    }
}

impl Probe for PingProbe {
    fn cost(&self, ctx: &Ctx) -> u32 {
        ctx.ping_count.clamp(1, 20)
    }

    fn run<'a>(&'a mut self, ctx: &'a Ctx) -> BoxFut<'a, ProbeResult> {
        Box::pin(async move {
            let ip = match self.target.ip(ctx.timeout).await {
                Ok(ip) => ip,
                Err(e) => {
                    self.window.push(None);
                    let mut r = ProbeResult::new(Outcome::Fail, Depth::Reachable, e);
                    r.via = ctx.method.label();
                    return r;
                }
            };
            let (round, via, errors) = self.round(ip, ctx).await;
            for s in &round {
                self.window.push(*s);
            }
            let mut r = ping::evaluate(&self.window, &round, &self.thresholds, &via);
            if let Some(e) = errors.first() {
                if r.outcome == Outcome::Fail {
                    r.message = format!("no answer: {e}");
                }
                r.raw.push_str(&format!("; errors: {}", errors.join("; ")));
            }
            r.raw = format!("{ip}: {}", r.raw);
            r
        })
    }
}

struct TcpProbe {
    target: Target,
    port: u16,
    thresholds: Thresholds,
}

impl Probe for TcpProbe {
    fn cost(&self, _ctx: &Ctx) -> u32 {
        // SYN, ACK and the closing FIN.
        3
    }

    fn run<'a>(&'a mut self, ctx: &'a Ctx) -> BoxFut<'a, ProbeResult> {
        Box::pin(async move {
            let via = format!("TCP {}", self.port);
            let ip = match self.target.ip(ctx.timeout).await {
                Ok(ip) => ip,
                Err(e) => {
                    let mut r = ProbeResult::new(Outcome::Fail, Depth::Port, e);
                    r.via = via;
                    return r;
                }
            };
            let mut r = match connect(ip, self.port, ctx.source, ctx.timeout).await {
                (ms, Ok(())) => {
                    let mut r = if ms > self.thresholds.latency_ms {
                        ProbeResult::new(
                            Outcome::Degraded,
                            Depth::Port,
                            format!(
                                "open, slow to connect: {} (limit {})",
                                Metric::ConnectMs.format(ms),
                                Metric::ConnectMs.format(self.thresholds.latency_ms)
                            ),
                        )
                    } else {
                        ProbeResult::new(
                            Outcome::Ok,
                            Depth::Port,
                            format!("open ({})", Metric::ConnectMs.format(ms)),
                        )
                    };
                    r.metrics.push((Metric::ConnectMs, ms));
                    r
                }
                (_, Err((refused, e))) => {
                    let mut r = ProbeResult::new(Outcome::Fail, Depth::Port, e);
                    if refused {
                        r.message =
                            "connection refused (the device is there, the port is closed)".into();
                    }
                    r
                }
            };
            r.via = via;
            r.raw = format!("{ip}:{}", self.port);
            r
        })
    }
}

/// A result that says nothing about the device (the check could not look).
pub(crate) fn unknown(depth: Depth, msg: impl Into<String>, via: &str) -> ProbeResult {
    let mut r = ProbeResult::new(Outcome::Unknown, depth, msg);
    r.via = via.into();
    r
}

/// The device's IPv4 address, or a result saying why there is none.
pub(crate) async fn ipv4(
    target: &mut Target,
    ctx: &Ctx,
    depth: Depth,
    via: &str,
) -> Result<Ipv4Addr, ProbeResult> {
    match target.ip(ctx.timeout).await {
        Ok(IpAddr::V4(ip)) => Ok(ip),
        Ok(IpAddr::V6(ip)) => Err(unknown(
            depth,
            format!("{ip} is IPv6: this check needs IPv4"),
            via,
        )),
        Err(e) => {
            let mut r = ProbeResult::new(Outcome::Fail, depth, e);
            r.via = via.into();
            Err(r)
        }
    }
}

/// Why a listener cannot answer yet, if it cannot.
fn listener_gap(state: &Listener, what: &str, now: u64, grace: u64) -> Option<String> {
    match state {
        Listener::Off => Some(format!("{what} is not listening")),
        Listener::Failed(e) => Some(e.clone()),
        s if s.fresh(now, grace) => Some(format!("{what}: listening, nothing seen yet")),
        Listener::On(_) => None,
    }
}

fn seconds(ms: u64) -> String {
    format!("{:.0} s", ms as f64 / 1000.0)
}

struct ConsoleProbe {
    target: Target,
    model: ConsoleModel,
    port: u16,
}

impl Probe for ConsoleProbe {
    fn cost(&self, _ctx: &Ctx) -> u32 {
        1
    }

    fn run<'a>(&'a mut self, ctx: &'a Ctx) -> BoxFut<'a, ProbeResult> {
        Box::pin(async move {
            let port = self.port;
            let via = format!("UDP {port}");
            let ip = match ipv4(&mut self.target, ctx, Depth::Answers, &via).await {
                Ok(ip) => ip,
                Err(r) => return r,
            };
            let model = self.model;
            let to = SocketAddr::new(IpAddr::V4(ip), port);
            let asked = crate::udp::ask(
                ctx.source,
                to,
                &console::request(model),
                ctx.timeout,
                |b, from| {
                    if from.ip() == IpAddr::V4(ip) {
                        console::parse(model, b)
                    } else {
                        None
                    }
                },
            )
            .await;
            let mut r = match asked {
                Ok(Some((info, ms))) => {
                    let name = if info.name.is_empty() {
                        model.label().to_string()
                    } else {
                        format!("\"{}\"", info.name)
                    };
                    let detail = info.detail();
                    let mut r = ProbeResult::new(
                        Outcome::Ok,
                        Depth::Answers,
                        if detail.is_empty() {
                            format!("answers: {name}")
                        } else {
                            format!("answers: {name}, {detail}")
                        },
                    );
                    r.metrics.push((Metric::AnswerMs, ms));
                    r.raw = format!(
                        "{ip}:{port} name={} model={} firmware={}",
                        info.name, info.model, info.firmware
                    );
                    let now = now_ms();
                    lock(&ctx.found).consoles.insert(
                        (ip, model),
                        crate::found::ConsoleAnswer {
                            model,
                            ip,
                            name: info.name,
                            detail,
                            at: now,
                        },
                    );
                    r
                }
                Ok(None) => {
                    let mut r = ProbeResult::new(
                        Outcome::Fail,
                        Depth::Answers,
                        format!(
                            "no answer to the {} query within {} ms",
                            model.label(),
                            ctx.timeout.as_millis()
                        ),
                    );
                    r.raw = format!("{ip}:{port}");
                    r
                }
                Err(e) => unknown(Depth::Answers, format!("cannot ask: {e}"), &via),
            };
            r.via = via;
            r
        })
    }
}

/// How long an ArtPollReply counts (polls go out every 3 s).
const NODE_TTL_MS: u64 = 10_000;

struct ArtNetNodeProbe {
    target: Target,
}

impl Probe for ArtNetNodeProbe {
    fn cost(&self, _ctx: &Ctx) -> u32 {
        0
    }

    fn run<'a>(&'a mut self, ctx: &'a Ctx) -> BoxFut<'a, ProbeResult> {
        Box::pin(async move {
            let via = "ArtPoll";
            let ip = match ipv4(&mut self.target, ctx, Depth::Answers, via).await {
                Ok(ip) => ip,
                Err(r) => return r,
            };
            let now = now_ms();
            let f = lock(&ctx.found);
            let seen = f.artnet.nodes.get(&ip).cloned();
            let gap = listener_gap(&f.artnet_state, "Art-Net", now, 4000);
            drop(f);
            let mut r = match (seen, gap) {
                (Some((reply, at)), _) if at + NODE_TTL_MS >= now => {
                    let name = if reply.short_name.is_empty() {
                        "a node".to_string()
                    } else {
                        format!("\"{}\"", reply.short_name)
                    };
                    let outs: Vec<String> = reply.outputs.iter().map(|u| u.to_string()).collect();
                    let mut r = ProbeResult::new(
                        Outcome::Ok,
                        Depth::Answers,
                        if outs.is_empty() {
                            format!("answers ArtPoll as {name}")
                        } else {
                            format!(
                                "answers ArtPoll as {name}, outputs universe {}",
                                outs.join(", ")
                            )
                        },
                    );
                    r.raw = format!(
                        "{ip} short=\"{}\" long=\"{}\" report=\"{}\" outputs={:?} inputs={:?} mac={} firmware={}",
                        reply.short_name, reply.long_name, reply.report, reply.outputs, reply.inputs, reply.mac, reply.firmware
                    );
                    r
                }
                (_, Some(gap)) => unknown(Depth::Answers, gap, via),
                (Some((_, at)), None) => ProbeResult::new(
                    Outcome::Fail,
                    Depth::Answers,
                    format!("no ArtPollReply for {}", seconds(now.saturating_sub(at))),
                ),
                (None, None) => {
                    ProbeResult::new(Outcome::Fail, Depth::Answers, "no answer to ArtPoll")
                }
            };
            r.via = via.into();
            r
        })
    }
}

struct UniverseProbe {
    universe: u16,
    min_fps: f64,
    /// sACN instead of Art-Net.
    sacn: bool,
}

impl Probe for UniverseProbe {
    fn cost(&self, _ctx: &Ctx) -> u32 {
        0
    }

    fn run<'a>(&'a mut self, ctx: &'a Ctx) -> BoxFut<'a, ProbeResult> {
        Box::pin(async move {
            let via = if self.sacn { "sACN" } else { "Art-Net" };
            let now = now_ms();
            let f = lock(&ctx.found);
            let (u, gap, top) = if self.sacn {
                let (prio, top) = f.sacn.top(self.universe, now);
                (
                    f.sacn.universes.get(&self.universe).cloned(),
                    listener_gap(&f.sacn_state, via, now, 2500),
                    Some((prio, top)),
                )
            } else {
                (
                    f.artnet.universes.get(&self.universe).cloned(),
                    listener_gap(&f.artnet_state, via, now, 2500),
                    None,
                )
            };
            drop(f);
            let label = if self.sacn {
                self.universe.to_string()
            } else {
                universe_label(self.universe)
            };
            let mut r = match (u, gap) {
                (Some(u), _) if u.last.is_some_and(|t| t + SOURCE_TTL_MS >= now) => {
                    let fps = u.fps(now);
                    // sACN: only the sources at the highest priority count (a
                    // backup at a lower priority is no conflict).
                    let from: Vec<String> = match &top {
                        Some((_, top)) => top
                            .iter()
                            .map(|(ip, s)| {
                                if s.name.is_empty() {
                                    ip.to_string()
                                } else {
                                    format!("\"{}\" ({ip})", s.name)
                                }
                            })
                            .collect(),
                        None => u.senders(now).iter().map(|s| s.to_string()).collect(),
                    };
                    let backups = match &top {
                        Some((prio, _)) => {
                            let all = u.senders(now).len();
                            let n = all.saturating_sub(from.len());
                            if n > 0 {
                                format!(" at priority {prio}, {n} backup(s) at a lower one")
                            } else {
                                format!(" at priority {prio}")
                            }
                        }
                        None => String::new(),
                    };
                    let mut r = if from.len() > 1 {
                        ProbeResult::new(
                            Outcome::Degraded,
                            Depth::Data,
                            format!(
                                "conflict: universe {label} is sent by {}",
                                from.join(" and ")
                            ),
                        )
                    } else if fps < self.min_fps && u.measuring(now) {
                        ProbeResult::new(
                            Outcome::Ok,
                            Depth::Data,
                            format!(
                                "frames arrive from {}{backups} (measuring the rate)",
                                from.join(", ")
                            ),
                        )
                    } else if fps < self.min_fps {
                        ProbeResult::new(
                            Outcome::Degraded,
                            Depth::Data,
                            format!(
                                "only {fps:.0} frames per second (at least {:.0})",
                                self.min_fps
                            ),
                        )
                    } else {
                        ProbeResult::new(
                            Outcome::Ok,
                            Depth::Data,
                            format!(
                                "{fps:.0} frames per second from {}{backups}",
                                from.join(", ")
                            ),
                        )
                    };
                    r.metrics.push((Metric::Fps, fps));
                    r.raw = format!(
                        "universe {label}: {fps:.1} fps, {} channels, last frame {} ms ago, senders {:?}",
                        u.channels,
                        now.saturating_sub(u.last.unwrap_or(now)),
                        u.sources
                    );
                    r
                }
                (_, Some(gap)) => unknown(Depth::Data, gap, via),
                (Some(u), None) => {
                    let mut r = ProbeResult::new(
                        Outcome::Fail,
                        Depth::Data,
                        format!(
                            "no frames for {}",
                            seconds(now.saturating_sub(u.last.unwrap_or(0)))
                        ),
                    );
                    r.metrics.push((Metric::Fps, 0.0));
                    r
                }
                (None, None) => {
                    let mut r = ProbeResult::new(
                        Outcome::Fail,
                        Depth::Data,
                        format!("no frames of universe {label}"),
                    );
                    r.metrics.push((Metric::Fps, 0.0));
                    r
                }
            };
            r.via = via.into();
            r
        })
    }
}

/// Does the service belong to the device at `ip` / `host`?
fn at_device(s: &crate::found::Service, ip: Option<Ipv4Addr>, host: &str) -> bool {
    ip.is_some_and(|ip| s.addrs.contains(&ip))
        || (!host.is_empty() && s.host.eq_ignore_ascii_case(host.trim_end_matches('.')))
}

struct MdnsProbe {
    target: Target,
    service: String,
    instance: String,
}

impl Probe for MdnsProbe {
    fn cost(&self, _ctx: &Ctx) -> u32 {
        0
    }

    fn run<'a>(&'a mut self, ctx: &'a Ctx) -> BoxFut<'a, ProbeResult> {
        Box::pin(async move {
            let via = "mDNS";
            let ip = match self.target.ip(ctx.timeout).await {
                Ok(IpAddr::V4(ip)) => Some(ip),
                _ => None,
            };
            let host = self.target.host.clone();
            let now = now_ms();
            let f = lock(&ctx.found);
            let hit = f
                .services(&self.service)
                .find(|s| {
                    if self.instance.is_empty() {
                        at_device(s, ip, &host)
                    } else {
                        s.instance.eq_ignore_ascii_case(&self.instance)
                    }
                })
                .cloned();
            let gap = listener_gap(&f.mdns_state, "mDNS", now, 6000);
            drop(f);
            let mut r = match (hit, gap) {
                (Some(s), _) => {
                    let mut r = ProbeResult::new(
                        Outcome::Ok,
                        Depth::Present,
                        format!("announced as \"{}\" on port {}", s.instance, s.port),
                    );
                    let txt: Vec<String> = s.txt.iter().map(|(k, v)| format!("{k}={v}")).collect();
                    r.raw = format!(
                        "{} host={} addresses={:?} txt: {}",
                        s.ty,
                        s.host,
                        s.addrs,
                        txt.join(" ")
                    );
                    r
                }
                (None, Some(gap)) => unknown(Depth::Present, gap, via),
                (None, None) => ProbeResult::new(
                    Outcome::Fail,
                    Depth::Present,
                    if self.instance.is_empty() {
                        format!("no {} announcement from this address", self.service)
                    } else {
                        format!("\"{}\" is not announced ({})", self.instance, self.service)
                    },
                ),
            };
            r.via = via.into();
            r
        })
    }
}

struct NdiProbe {
    target: Target,
    source: String,
}

impl Probe for NdiProbe {
    fn cost(&self, _ctx: &Ctx) -> u32 {
        3
    }

    fn run<'a>(&'a mut self, ctx: &'a Ctx) -> BoxFut<'a, ProbeResult> {
        Box::pin(async move {
            let via = "NDI";
            let ip = match self.target.ip(ctx.timeout).await {
                Ok(IpAddr::V4(ip)) => Some(ip),
                _ => None,
            };
            let host = self.target.host.clone();
            let now = now_ms();
            let (hit, gap) = {
                let f = lock(&ctx.found);
                let hit = f
                    .services(NDI_SERVICE)
                    .find(|s| {
                        if self.source.is_empty() {
                            at_device(s, ip, &host)
                        } else {
                            s.instance.eq_ignore_ascii_case(&self.source)
                        }
                    })
                    .cloned();
                (hit, listener_gap(&f.mdns_state, "mDNS", now, 6000))
            };
            let s = match (hit, gap) {
                (Some(s), _) => s,
                (None, Some(gap)) => return unknown(Depth::Present, gap, via),
                (None, None) => {
                    let mut r = ProbeResult::new(
                        Outcome::Fail,
                        Depth::Present,
                        if self.source.is_empty() {
                            "no NDI source announced from this address".to_string()
                        } else {
                            format!("NDI source \"{}\" is not announced", self.source)
                        },
                    );
                    r.via = via.into();
                    return r;
                }
            };
            let addr = ip
                .filter(|ip| s.addrs.contains(ip))
                .or_else(|| s.addrs.first().copied());
            let Some(addr) = addr else {
                return unknown(
                    Depth::Present,
                    format!("\"{}\" is announced without an IPv4 address", s.instance),
                    via,
                );
            };
            let mut r = match connect(IpAddr::V4(addr), s.port, ctx.source, ctx.timeout).await {
                (ms, Ok(())) => {
                    let mut r = ProbeResult::new(
                        Outcome::Ok,
                        Depth::Port,
                        format!("\"{}\" announced, port {} open", s.instance, s.port),
                    );
                    r.metrics.push((Metric::ConnectMs, ms));
                    r
                }
                (_, Err((_, e))) => ProbeResult::new(
                    Outcome::Degraded,
                    Depth::Present,
                    format!(
                        "\"{}\" announced, but port {} does not accept connections: {e}",
                        s.instance, s.port
                    ),
                ),
            };
            r.raw = format!("{} at {addr}:{} host={}", s.instance, s.port, s.host);
            r.via = via.into();
            r
        })
    }
}

struct InterfaceProbe {
    name: String,
}

impl Probe for InterfaceProbe {
    fn cost(&self, _ctx: &Ctx) -> u32 {
        0
    }

    fn run<'a>(&'a mut self, ctx: &'a Ctx) -> BoxFut<'a, ProbeResult> {
        Box::pin(async move {
            let via = "this computer";
            if self.name.is_empty() {
                return unknown(Depth::Link, "no interface chosen", via);
            }
            let f = lock(&ctx.found);
            let nic = interfaces::find(&f.nics, &self.name).cloned();
            let all: Vec<String> = f.nics.iter().map(|n| n.label()).collect();
            drop(f);
            let mut r = match nic {
                Some(n) => {
                    ProbeResult::new(Outcome::Ok, Depth::Link, format!("{} is up", n.label()))
                }
                None => ProbeResult::new(
                    Outcome::Fail,
                    Depth::Link,
                    format!("{} is down or has no address", self.name),
                ),
            };
            r.raw = format!("interfaces: {}", all.join(", "));
            r.via = via.into();
            r
        })
    }
}
