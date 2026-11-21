//! sACN (ANSI E1.31) data packets: enough to count the frames of a universe,
//! who sends them and at which priority. Pulse only listens; `data` builds
//! packets for the simulator and the tests.

use crate::artnet::Universe;
use crate::status::Ms;
use std::collections::BTreeMap;
use std::net::Ipv4Addr;

pub const PORT: u16 = 5568;
const ACN_ID: &[u8; 12] = b"ASC-E1.17\0\0\0";
const VECTOR_ROOT_DATA: u32 = 0x0000_0004;
const VECTOR_FRAME_DATA: u32 = 0x0000_0002;
const VECTOR_DMP_SET: u8 = 0x02;
/// Universes 1 ..= 63999 are valid.
pub const MAX_UNIVERSE: u16 = 63999;

/// The multicast group of a universe: 239.255.hi.lo.
pub fn group(universe: u16) -> Ipv4Addr {
    Ipv4Addr::new(239, 255, (universe >> 8) as u8, (universe & 0xff) as u8)
}

/// A data packet.
#[derive(Clone, Debug, PartialEq)]
pub struct Data {
    pub cid: [u8; 16],
    pub source_name: String,
    pub priority: u8,
    pub sequence: u8,
    pub universe: u16,
    /// The source stops sending this universe.
    pub terminated: bool,
    pub preview: bool,
    pub start_code: u8,
    /// Channels (without the start code).
    pub channels: u16,
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]))
}
fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *b.get(i)?,
        *b.get(i + 1)?,
        *b.get(i + 2)?,
        *b.get(i + 3)?,
    ]))
}

/// Parse an E1.31 data packet (anything else: `None`).
pub fn parse(b: &[u8]) -> Option<Data> {
    if b.len() < 126 || u16_at(b, 0)? != 0x0010 || &b[4..16] != ACN_ID {
        return None;
    }
    if u32_at(b, 18)? != VECTOR_ROOT_DATA || u32_at(b, 40)? != VECTOR_FRAME_DATA {
        return None;
    }
    if b[117] != VECTOR_DMP_SET || b[118] != 0xa1 {
        return None;
    }
    let mut cid = [0u8; 16];
    cid.copy_from_slice(&b[22..38]);
    let name = &b[44..108];
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    let universe = u16_at(b, 113)?;
    if universe == 0 || universe > MAX_UNIVERSE {
        return None;
    }
    let count = u16_at(b, 123)?;
    if count == 0 {
        return None;
    }
    Some(Data {
        cid,
        source_name: String::from_utf8_lossy(&name[..end]).trim().to_string(),
        priority: b[108],
        sequence: b[111],
        universe,
        terminated: b[112] & 0x40 != 0,
        preview: b[112] & 0x80 != 0,
        start_code: b[125],
        channels: (count - 1).min(512),
    })
}

/// A data packet with `channels` (start code 0).
pub fn data(d: &Data, channels: &[u8]) -> Vec<u8> {
    let n = channels.len().min(512);
    let total = 126 + n;
    let fl = |len: usize| (0x7000u16 | (len as u16 & 0x0fff)).to_be_bytes();
    let mut b = Vec::with_capacity(total);
    b.extend_from_slice(&0x0010u16.to_be_bytes());
    b.extend_from_slice(&0u16.to_be_bytes());
    b.extend_from_slice(ACN_ID);
    b.extend_from_slice(&fl(total - 16));
    b.extend_from_slice(&VECTOR_ROOT_DATA.to_be_bytes());
    b.extend_from_slice(&d.cid);
    b.extend_from_slice(&fl(total - 38));
    b.extend_from_slice(&VECTOR_FRAME_DATA.to_be_bytes());
    let mut name = [0u8; 64];
    let src = d.source_name.as_bytes();
    let k = src.len().min(63);
    name[..k].copy_from_slice(&src[..k]);
    b.extend_from_slice(&name);
    b.push(d.priority);
    b.extend_from_slice(&0u16.to_be_bytes());
    b.push(d.sequence);
    b.push(if d.terminated { 0x40 } else { 0 } | if d.preview { 0x80 } else { 0 });
    b.extend_from_slice(&d.universe.to_be_bytes());
    b.extend_from_slice(&fl(total - 115));
    b.push(VECTOR_DMP_SET);
    b.push(0xa1);
    b.extend_from_slice(&0u16.to_be_bytes());
    b.extend_from_slice(&1u16.to_be_bytes());
    b.extend_from_slice(&((n + 1) as u16).to_be_bytes());
    b.push(0);
    b.extend_from_slice(&channels[..n]);
    b
}

/// A source of a universe.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub name: String,
    pub priority: u8,
    pub at: Ms,
}

/// The sACN universes seen.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    pub universes: BTreeMap<u16, Universe>,
    /// Universe -> sender address -> what it says about itself.
    pub sources: BTreeMap<u16, BTreeMap<Ipv4Addr, Source>>,
}

impl Seen {
    pub fn packet(&mut self, from: Ipv4Addr, d: Data, now: Ms) {
        // Preview data and other start codes (e.g. per-channel priority 0xdd) are no frames.
        if d.preview || d.start_code != 0 {
            return;
        }
        let srcs = self.sources.entry(d.universe).or_default();
        if d.terminated {
            srcs.remove(&from);
            if let Some(u) = self.universes.get_mut(&d.universe) {
                u.sources.remove(&from);
            }
            return;
        }
        srcs.insert(
            from,
            Source {
                name: d.source_name,
                priority: d.priority,
                at: now,
            },
        );
        self.universes
            .entry(d.universe)
            .or_default()
            .record(from, d.channels, now);
    }

    /// Current senders with the highest priority; more than one is a conflict
    /// (a lower-priority backup is not).
    pub fn top(&self, universe: u16, now: Ms) -> (u8, Vec<(Ipv4Addr, Source)>) {
        let current: Vec<(Ipv4Addr, Source)> = self
            .sources
            .get(&universe)
            .map(|m| {
                m.iter()
                    .filter(|(_, s)| s.at + crate::artnet::SOURCE_TTL_MS >= now)
                    .map(|(ip, s)| (*ip, s.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let best = current.iter().map(|(_, s)| s.priority).max().unwrap_or(0);
        (
            best,
            current
                .into_iter()
                .filter(|(_, s)| s.priority == best)
                .collect(),
        )
    }

    pub fn prune(&mut self, now: Ms) {
        self.universes
            .retain(|_, u| u.last.is_some_and(|t| t + 60_000 >= now));
        for m in self.sources.values_mut() {
            m.retain(|_, s| s.at + 60_000 >= now);
        }
        self.sources.retain(|_, m| !m.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(universe: u16, priority: u8) -> Data {
        Data {
            cid: [7; 16],
            source_name: "Light desk".into(),
            priority,
            sequence: 3,
            universe,
            terminated: false,
            preview: false,
            start_code: 0,
            channels: 0,
        }
    }

    #[test]
    fn round_trip() {
        let p = data(&d(300, 100), &[1, 2, 3, 4]);
        assert_eq!(p.len(), 130);
        let got = parse(&p).unwrap();
        assert_eq!(got.universe, 300);
        assert_eq!(got.priority, 100);
        assert_eq!(got.channels, 4);
        assert_eq!(got.source_name, "Light desk");
        assert_eq!(got.sequence, 3);
        let full = data(&d(1, 100), &[0; 512]);
        assert_eq!(full.len(), 638);
        assert_eq!(parse(&full).unwrap().channels, 512);
    }

    #[test]
    fn rejects_other_packets() {
        assert_eq!(parse(b"hello"), None);
        assert_eq!(parse(&crate::artnet::dmx(1, 0, &[0; 512])), None);
        let mut p = data(&d(1, 100), &[0; 10]);
        p[113] = 0;
        p[114] = 0;
        assert_eq!(parse(&p), None, "universe 0 is invalid");
    }

    #[test]
    fn multicast_group() {
        assert_eq!(group(1), Ipv4Addr::new(239, 255, 0, 1));
        assert_eq!(group(0x0102), Ipv4Addr::new(239, 255, 1, 2));
    }

    #[test]
    fn priorities_backups_and_conflicts() {
        let a = Ipv4Addr::new(10, 0, 0, 1);
        let b = Ipv4Addr::new(10, 0, 0, 2);
        let mut s = Seen::default();
        s.packet(a, d(1, 100), 1000);
        s.packet(b, d(1, 50), 1000);
        let (best, top) = s.top(1, 1000);
        assert_eq!((best, top.len()), (100, 1), "a backup at lower priority");
        s.packet(b, d(1, 100), 1100);
        assert_eq!(s.top(1, 1100).1.len(), 2, "same priority: conflict");
        let mut t = d(1, 100);
        t.terminated = true;
        s.packet(b, t, 1200);
        assert_eq!(s.top(1, 1200).1.len(), 1);
        // Old sources fall out.
        assert!(s.top(1, 10_000).1.is_empty());
        let mut pv = d(2, 100);
        pv.preview = true;
        s.packet(a, pv, 1000);
        assert!(!s.universes.contains_key(&2));
    }
}
