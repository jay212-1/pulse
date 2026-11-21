//! Auto discovery: what the listeners saw (mDNS, NDI, ArtPollReply, console
//! answers, an optional ping sweep) grouped by address and held against the
//! inventory. The result: new devices (with a suggested preset, added with
//! one click), devices already in the inventory, and devices the inventory
//! expects that nobody saw.

use crate::model::{
    service_short, CheckKind, ConsoleModel, Device, Id, Profile, DANTE_SERVICE, HTTP_SERVICE,
    LANTERN_SERVICE, NDI_SERVICE,
};
use crate::presets::Preset;
use crate::status::Ms;
use std::collections::BTreeMap;
use std::net::Ipv4Addr;

/// How something was found.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Via {
    /// An mDNS service (short type, e.g. `_netaudio-arc._udp`) and its instance name.
    Mdns { service: String, instance: String },
    /// An NDI source (its full name).
    Ndi { source: String },
    /// An ArtPollReply.
    ArtNet,
    /// The console answered its query.
    Console(ConsoleModel),
    /// It answered the ping sweep.
    Sweep,
}

impl Via {
    pub fn label(&self) -> String {
        match self {
            Via::Mdns { service, .. } if service == DANTE_SERVICE => "Dante (mDNS)".into(),
            Via::Mdns { service, .. } if service == LANTERN_SERVICE => "Lantern (mDNS)".into(),
            Via::Mdns { service, .. } => format!("mDNS {service}"),
            Via::Ndi { .. } => "NDI".into(),
            Via::ArtNet => "Art-Net".into(),
            Via::Console(m) => m.label().into(),
            Via::Sweep => "Ping sweep".into(),
        }
    }
}

/// One thing seen at one address.
#[derive(Clone, Debug, PartialEq)]
pub struct Sighting {
    pub address: Ipv4Addr,
    /// The name it gives itself (may be empty).
    pub name: String,
    /// mDNS host name, if any ("stagebox-1.local").
    pub host: String,
    pub via: Via,
    /// Model, firmware, port ... for the list.
    pub detail: String,
    pub at: Ms,
}

/// Everything seen at one address.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub address: Ipv4Addr,
    pub name: String,
    pub host: String,
    pub via: Vec<Via>,
    pub details: Vec<String>,
    pub preset: Preset,
    /// The inventory device at this address.
    pub device: Option<Id>,
    pub last: Ms,
}

impl Candidate {
    pub fn via_label(&self) -> String {
        let mut v: Vec<String> = self.via.iter().map(Via::label).collect();
        v.dedup();
        v.join(", ")
    }
}

/// An inventory device nobody saw.
#[derive(Clone, Debug, PartialEq)]
pub struct Missing {
    pub device: Id,
    pub name: String,
    pub address: String,
    /// What should have seen it.
    pub expected: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub new: Vec<Candidate>,
    pub known: Vec<Candidate>,
    pub missing: Vec<Missing>,
}

/// The network a sweep covered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Net {
    pub addr: Ipv4Addr,
    pub prefix: u8,
}

impl Net {
    pub fn contains(&self, ip: Ipv4Addr) -> bool {
        let mask = u32::MAX
            .checked_shl(32 - u32::from(self.prefix.min(32)))
            .unwrap_or(0);
        u32::from(ip) & mask == u32::from(self.addr) & mask
    }
    /// The hosts of the net (without network and broadcast address); for
    /// nets larger than a /24 the /24 around `addr`.
    pub fn hosts(&self) -> Vec<Ipv4Addr> {
        let prefix = self.prefix.clamp(24, 30);
        let mask = u32::MAX << (32 - u32::from(prefix));
        let base = u32::from(self.addr) & mask;
        let size = 1u32 << (32 - u32::from(prefix));
        (1..size - 1).map(|i| Ipv4Addr::from(base + i)).collect()
    }
    pub fn label(&self) -> String {
        let prefix = self.prefix.clamp(24, 30);
        let mask = u32::MAX << (32 - u32::from(prefix));
        format!("{}/{prefix}", Ipv4Addr::from(u32::from(self.addr) & mask))
    }
}

/// The device a sighting belongs to: same IPv4 address, or the mDNS host name.
pub fn device_for<'a>(p: &'a Profile, address: Ipv4Addr, host: &str) -> Option<&'a Device> {
    let host = host.trim_end_matches('.');
    p.devices.iter().find(|d| {
        let a = d.address.trim();
        a.parse::<Ipv4Addr>().ok() == Some(address)
            || (!host.is_empty() && a.trim_end_matches('.').eq_ignore_ascii_case(host))
    })
}

/// The preset that suits what was seen.
fn suggest(via: &[Via]) -> Preset {
    for v in via {
        match v {
            Via::Console(ConsoleModel::X32) => return Preset::X32,
            Via::Console(ConsoleModel::XAir) => return Preset::XAir,
            Via::Console(ConsoleModel::Wing) => return Preset::Wing,
            _ => {}
        }
    }
    for v in via {
        match v {
            Via::Ndi { .. } => return Preset::NdiSource,
            Via::Mdns { service, .. } if service == DANTE_SERVICE => return Preset::DanteDevice,
            Via::Mdns { service, .. } if service == LANTERN_SERVICE => return Preset::Lantern,
            Via::Mdns { service, instance }
                if service == HTTP_SERVICE && instance.starts_with("Show Master") =>
            {
                return Preset::ShowMaster
            }
            Via::ArtNet => return Preset::ArtNetNode,
            _ => {}
        }
    }
    Preset::Generic
}

/// Group the sightings by address and hold them against the inventory.
/// `swept`: the net a finished ping sweep covered (devices there that did
/// not answer count as missing).
pub fn report(p: &Profile, sightings: &[Sighting], swept: Option<Net>) -> Report {
    let mut by: BTreeMap<Ipv4Addr, Candidate> = BTreeMap::new();
    for s in sightings {
        let c = by.entry(s.address).or_insert_with(|| Candidate {
            address: s.address,
            name: String::new(),
            host: String::new(),
            via: Vec::new(),
            details: Vec::new(),
            preset: Preset::Generic,
            device: None,
            last: 0,
        });
        if !c.via.contains(&s.via) {
            c.via.push(s.via.clone());
        }
        if !s.detail.is_empty() && !c.details.contains(&s.detail) {
            c.details.push(s.detail.clone());
        }
        if c.host.is_empty() {
            c.host = s.host.trim_end_matches('.').to_string();
        }
        // Names from a protocol answer beat mDNS names beat nothing.
        let better = match &s.via {
            Via::Console(_) | Via::ArtNet => true,
            Via::Mdns { .. } | Via::Ndi { .. } => c.name.is_empty(),
            Via::Sweep => false,
        };
        if better && !s.name.trim().is_empty() {
            c.name = s.name.trim().to_string();
        }
        c.last = c.last.max(s.at);
    }
    let mut out = Report::default();
    for (_, mut c) in by {
        c.via.sort();
        c.preset = suggest(&c.via);
        if c.name.is_empty() {
            c.name = if c.host.is_empty() {
                format!("Device at {}", c.address)
            } else {
                c.host.trim_end_matches(".local").to_string()
            };
        }
        c.device = device_for(p, c.address, &c.host).map(|d| d.id);
        if c.device.is_some() {
            out.known.push(c);
        } else {
            out.new.push(c);
        }
    }
    for d in p.devices.iter().filter(|d| d.enabled) {
        // Seen if anything was seen at its address (several devices may share one).
        let seen = out.known.iter().chain(out.new.iter()).any(|c| {
            c.device == Some(d.id)
                || d.address.trim().parse::<Ipv4Addr>().ok() == Some(c.address)
                || (!c.host.is_empty()
                    && d.address
                        .trim()
                        .trim_end_matches('.')
                        .eq_ignore_ascii_case(&c.host))
        });
        if seen {
            continue;
        }
        let expected: Vec<&str> = d
            .checks
            .iter()
            .filter(|c| c.enabled)
            .filter_map(|c| match &c.kind {
                CheckKind::Console { .. } => Some("console query"),
                CheckKind::ArtNetNode => Some("Art-Net"),
                CheckKind::Mdns { .. } => Some("mDNS"),
                CheckKind::Ndi { .. } => Some("NDI"),
                _ => None,
            })
            .collect();
        let in_sweep = swept.is_some_and(|n| {
            d.address
                .trim()
                .parse::<Ipv4Addr>()
                .is_ok_and(|ip| n.contains(ip))
        });
        if expected.is_empty() && !in_sweep {
            continue;
        }
        let mut what: Vec<&str> = expected;
        what.dedup();
        if in_sweep {
            what.push("ping sweep");
        }
        out.missing.push(Missing {
            device: d.id,
            name: d.name.clone(),
            address: d.address.clone(),
            expected: what.join(", "),
        });
    }
    out
}

impl Profile {
    /// Add a found device with its preset; the checks are pointed at what
    /// was seen (the NDI source, the Dante name). Returns the new id.
    pub fn add_candidate(&mut self, c: &Candidate) -> Id {
        let id = self.add_preset(c.preset, &c.name, &c.address.to_string());
        let ndi = c.via.iter().find_map(|v| match v {
            Via::Ndi { source } => Some(source.clone()),
            _ => None,
        });
        if let Some(d) = self.device_mut(id) {
            for check in &mut d.checks {
                match &mut check.kind {
                    CheckKind::Ndi { source } => {
                        if let Some(s) = &ndi {
                            *source = s.clone();
                        }
                    }
                    CheckKind::Mdns { service, instance } => {
                        let want = service_short(service).to_string();
                        if let Some(i) = c.via.iter().find_map(|v| match v {
                            Via::Mdns {
                                service: s,
                                instance: i,
                            } if *s == want => Some(i.clone()),
                            _ => None,
                        }) {
                            *instance = i;
                        }
                    }
                    _ => {}
                }
            }
        }
        id
    }
}

/// Every mDNS type discovery listens to (and [`NDI_SERVICE`] for NDI).
pub const DISCOVERY_SERVICES: [&str; 4] =
    [DANTE_SERVICE, NDI_SERVICE, LANTERN_SERVICE, HTTP_SERVICE];

#[cfg(test)]
mod tests {
    use super::*;

    fn s(ip: [u8; 4], name: &str, via: Via) -> Sighting {
        Sighting {
            address: Ipv4Addr::from(ip),
            name: name.into(),
            host: String::new(),
            via,
            detail: String::new(),
            at: 1000,
        }
    }

    #[test]
    fn sightings_become_new_known_and_missing_devices() {
        let mut p = Profile::default();
        let known = p.add_preset(Preset::DanteDevice, "Stagebox", "10.0.0.11");
        let gone = p.add_preset(Preset::ArtNetNode, "Node SR", "10.0.0.40");
        p.add_preset(Preset::Switch, "Core", "10.0.0.254");
        let seen = vec![
            s(
                [10, 0, 0, 11],
                "SB-1",
                Via::Mdns {
                    service: DANTE_SERVICE.into(),
                    instance: "SB-1".into(),
                },
            ),
            s([10, 0, 0, 1], "", Via::Sweep),
            s([10, 0, 0, 1], "FOH X32", Via::Console(ConsoleModel::X32)),
            s(
                [10, 0, 0, 30],
                "CAM1",
                Via::Ndi {
                    source: "CAM1 (Camera 1)".into(),
                },
            ),
        ];
        let r = report(&p, &seen, None);
        assert_eq!(r.known.len(), 1);
        assert_eq!(r.known[0].device, Some(known));
        assert_eq!(r.new.len(), 2);
        assert_eq!(r.new[0].name, "FOH X32");
        assert_eq!(r.new[0].preset, Preset::X32);
        assert_eq!(r.new[0].via_label(), "X32/M32, Ping sweep");
        assert_eq!(r.new[1].preset, Preset::NdiSource);
        // The node is expected via Art-Net; the switch only pings (no sweep ran).
        assert_eq!(r.missing.len(), 1);
        assert_eq!(r.missing[0].device, gone);
        assert_eq!(r.missing[0].expected, "Art-Net");

        // After a sweep of 10.0.0.0/24 the switch counts as missing too.
        let net = Net {
            addr: Ipv4Addr::new(10, 0, 0, 7),
            prefix: 24,
        };
        let r = report(&p, &seen, Some(net));
        assert_eq!(r.missing.len(), 2);
        assert_eq!(r.missing[1].expected, "ping sweep");

        // Adding takes the preset and points the NDI check at the source.
        let id = p.add_candidate(&r.new[1]);
        let d = p.device(id).cloned().unwrap_or_default();
        assert_eq!(d.address, "10.0.0.30");
        assert_eq!(
            d.checks[1].kind,
            CheckKind::Ndi {
                source: "CAM1 (Camera 1)".into()
            }
        );
        assert_eq!(report(&p, &seen, None).new.len(), 1);
    }

    #[test]
    fn host_names_match_and_unnamed_devices_get_one() {
        let mut p = Profile::default();
        p.add_device(crate::DeviceKind::Computer, "Backup", "backup-mac.local");
        let mut a = s(
            [10, 0, 0, 9],
            "",
            Via::Mdns {
                service: LANTERN_SERVICE.into(),
                instance: "Lantern".into(),
            },
        );
        a.host = "backup-mac.local.".into();
        let b = s([10, 0, 0, 77], "", Via::Sweep);
        let r = report(&p, &[a, b], None);
        assert_eq!(r.known.len(), 1);
        assert_eq!(r.new[0].name, "Device at 10.0.0.77");
    }

    #[test]
    fn nets_and_their_hosts() {
        let n = Net {
            addr: Ipv4Addr::new(192, 168, 1, 77),
            prefix: 24,
        };
        assert!(n.contains(Ipv4Addr::new(192, 168, 1, 3)));
        assert!(!n.contains(Ipv4Addr::new(192, 168, 2, 3)));
        assert_eq!(n.hosts().len(), 254);
        assert_eq!(n.label(), "192.168.1.0/24");
        // A /16 is swept as the /24 around the address only.
        let big = Net { prefix: 16, ..n };
        assert_eq!(big.hosts().len(), 254);
        assert_eq!(big.hosts()[0], Ipv4Addr::new(192, 168, 1, 1));
        let small = Net { prefix: 30, ..n };
        assert_eq!(small.hosts().len(), 2);
    }
}
