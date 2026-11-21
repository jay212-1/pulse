//! What the listeners found: Art-Net nodes and universes, mDNS services,
//! console answers, the ping sweep and this computer's interfaces. Shared
//! by the monitor, the probes that read it and the discovery page.

use crate::interfaces::Nic;
use pulse_core::artnet::Seen;
use pulse_core::discovery::{self, Net, Report, Sighting, Via};
use pulse_core::model::{service_short, ConsoleModel, Profile, NDI_SERVICE};
use pulse_core::status::Ms;
use std::collections::BTreeMap;
use std::net::Ipv4Addr;

/// State of a listener.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Listener {
    #[default]
    Off,
    /// Listening since.
    On(Ms),
    Failed(String),
}

impl Listener {
    /// Listening for less than `ms` (nothing found yet may mean nothing).
    pub fn fresh(&self, now: Ms, ms: Ms) -> bool {
        matches!(self, Listener::On(t) if now < t + ms)
    }
}

/// A resolved mDNS service.
#[derive(Clone, Debug, PartialEq)]
pub struct Service {
    /// Short type: `_ndi._tcp`.
    pub ty: String,
    pub instance: String,
    pub host: String,
    pub port: u16,
    pub addrs: Vec<Ipv4Addr>,
    pub txt: Vec<(String, String)>,
    pub at: Ms,
}

/// A console that answered its query.
#[derive(Clone, Debug, PartialEq)]
pub struct ConsoleAnswer {
    pub model: ConsoleModel,
    pub ip: Ipv4Addr,
    pub name: String,
    /// Model and firmware as the desk says them.
    pub detail: String,
    pub at: Ms,
}

/// A ping sweep of one net.
#[derive(Clone, Debug, PartialEq)]
pub struct Sweep {
    pub net: Net,
    pub total: usize,
    pub done: usize,
    pub answered: BTreeMap<Ipv4Addr, f64>,
    pub started: Ms,
    pub finished: Option<Ms>,
    pub cancelled: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Found {
    pub artnet: Seen,
    pub artnet_state: Listener,
    /// Full name -> service.
    pub mdns: BTreeMap<String, Service>,
    pub mdns_state: Listener,
    pub consoles: BTreeMap<(Ipv4Addr, ConsoleModel), ConsoleAnswer>,
    pub sweep: Option<Sweep>,
    pub nics: Vec<Nic>,
    /// Discovery is open (polls and console queries go out).
    pub discovering: bool,
    pub last_scan: Option<Ms>,
}

impl Found {
    /// Services of a type (short or full).
    pub fn services<'a>(&'a self, ty: &'a str) -> impl Iterator<Item = &'a Service> + 'a {
        let ty = service_short(ty);
        self.mdns.values().filter(move |s| s.ty == ty)
    }

    /// Everything seen, for [`discovery::report`].
    pub fn sightings(&self) -> Vec<Sighting> {
        let mut out = Vec::new();
        for s in self.mdns.values() {
            let via = if s.ty == NDI_SERVICE {
                Via::Ndi {
                    source: s.instance.clone(),
                }
            } else {
                Via::Mdns {
                    service: s.ty.clone(),
                    instance: s.instance.clone(),
                }
            };
            let name = match &via {
                // "MACHINE (Source)": the machine is the device.
                Via::Ndi { source } => source
                    .split(" (")
                    .next()
                    .unwrap_or(source)
                    .trim()
                    .to_string(),
                _ => s.instance.clone(),
            };
            for ip in &s.addrs {
                out.push(Sighting {
                    address: *ip,
                    name: name.clone(),
                    host: s.host.clone(),
                    via: via.clone(),
                    detail: format!("{} port {}", s.instance, s.port),
                    at: s.at,
                });
            }
        }
        for (ip, (r, at)) in &self.artnet.nodes {
            let outs: Vec<String> = r.outputs.iter().map(|u| u.to_string()).collect();
            out.push(Sighting {
                address: *ip,
                name: if r.long_name.is_empty() {
                    r.short_name.clone()
                } else {
                    r.long_name.clone()
                },
                host: String::new(),
                via: Via::ArtNet,
                detail: if outs.is_empty() {
                    "Art-Net node".into()
                } else {
                    format!("Art-Net outputs {}", outs.join(", "))
                },
                at: *at,
            });
        }
        for c in self.consoles.values() {
            out.push(Sighting {
                address: c.ip,
                name: c.name.clone(),
                host: String::new(),
                via: Via::Console(c.model),
                detail: c.detail.clone(),
                at: c.at,
            });
        }
        if let Some(sw) = &self.sweep {
            for (ip, ms) in &sw.answered {
                // This computer answers its own sweep: leave it out.
                if self.nics.iter().any(|n| n.ip == *ip) {
                    continue;
                }
                out.push(Sighting {
                    address: *ip,
                    name: String::new(),
                    host: String::new(),
                    via: Via::Sweep,
                    detail: format!("ping {ms:.1} ms"),
                    at: sw.started,
                });
            }
        }
        out
    }

    pub fn report(&self, p: &Profile) -> Report {
        let swept = self
            .sweep
            .as_ref()
            .filter(|s| s.finished.is_some() && !s.cancelled)
            .map(|s| Net {
                addr: s.net.addr,
                prefix: s.net.prefix.clamp(24, 30),
            });
        discovery::report(p, &self.sightings(), swept)
    }

    /// Forget what was not seen for a minute.
    pub fn prune(&mut self, now: Ms) {
        self.artnet.prune(now);
        self.consoles.retain(|_, c| c.at + 60_000 >= now);
    }
}

pub fn lock(m: &std::sync::Mutex<Found>) -> std::sync::MutexGuard<'_, Found> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
