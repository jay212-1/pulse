//! The probe interface and the probes for each kind of check.
//!
//! A probe belongs to one check of one device and may keep state between
//! runs (the ping window, a resolved address). The scheduler moves it into a
//! task for each run, with a hard timeout around it, and gets it back with
//! the result. Probes only ask: they never write to a device.

use crate::icmp::{self, Method};
use pulse_core::model::{Check, CheckKind, Device, Thresholds};
use pulse_core::ping::{self, PingWindow};
use pulse_core::probe::{Depth, Metric, Outcome, ProbeResult};
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
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
    }
}

/// An address or host name, resolved now and then (names may move).
struct Target {
    host: String,
    resolved: Option<(IpAddr, Instant)>,
}

/// How long a resolved name is trusted.
const RESOLVE_TTL: Duration = Duration::from_secs(60);

impl Target {
    fn new(host: &str) -> Self {
        let host = host.trim().to_string();
        let resolved = host.parse::<IpAddr>().ok().map(|ip| (ip, Instant::now()));
        Target { host, resolved }
    }

    async fn ip(&mut self, timeout: Duration) -> Result<IpAddr, String> {
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
async fn connect(ip: IpAddr, port: u16, timeout: Duration) -> (f64, Result<(), (bool, String)>) {
    let start = Instant::now();
    let r = match tokio::time::timeout(timeout, tokio::net::TcpStream::connect((ip, port))).await {
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
            let sample = match (&method, ip) {
                (Method::Icmp, IpAddr::V4(v4)) => {
                    let (seq, timeout) = (self.seq, ctx.timeout);
                    match tokio::task::spawn_blocking(move || icmp::echo(v4, seq, timeout)).await {
                        Ok(Ok(s)) => s,
                        Ok(Err(e)) => {
                            errors.push(e.to_string());
                            None
                        }
                        Err(e) => {
                            errors.push(e.to_string());
                            None
                        }
                    }
                }
                // Refused means a device answered: it is there.
                _ => match connect(ip, icmp::TCP_FALLBACK_PORT, ctx.timeout).await {
                    (ms, Ok(()) | Err((true, _))) => Some(ms),
                    (_, Err((false, e))) => {
                        errors.push(e);
                        None
                    }
                },
            };
            round.push(sample);
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
            let mut r = match connect(ip, self.port, ctx.timeout).await {
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
