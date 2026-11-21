//! Art-Net, as far as Pulse needs it: ArtPoll (asking which nodes are
//! there), ArtPollReply (a node answers with its name and ports) and ArtDmx
//! (a frame of one universe). Plus the statistics per universe: frames per
//! second, the last frame, who sends it and whether two senders collide.
//!
//! Pulse only listens and polls; it never sends DMX.

use crate::status::Ms;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::net::Ipv4Addr;

pub const PORT: u16 = 6454;
pub const ID: &[u8; 8] = b"Art-Net\0";
pub const OP_POLL: u16 = 0x2000;
pub const OP_POLL_REPLY: u16 = 0x2100;
pub const OP_DMX: u16 = 0x5000;
pub const PROTOCOL: u16 = 14;

/// A node's answer to ArtPoll.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PollReply {
    pub ip: Ipv4Addr,
    pub short_name: String,
    pub long_name: String,
    pub report: String,
    /// The universes (15-bit port addresses) of its outputs.
    pub outputs: Vec<u16>,
    /// The universes of its inputs.
    pub inputs: Vec<u16>,
    pub mac: String,
    pub firmware: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Packet {
    Poll,
    PollReply(Box<PollReply>),
    Dmx {
        universe: u16,
        sequence: u8,
        /// Number of channels in the frame.
        length: u16,
    },
}

impl Default for PollReply {
    fn default() -> Self {
        PollReply {
            ip: Ipv4Addr::UNSPECIFIED,
            short_name: String::new(),
            long_name: String::new(),
            report: String::new(),
            outputs: Vec::new(),
            inputs: Vec::new(),
            mac: String::new(),
            firmware: 0,
        }
    }
}

/// The ArtPoll Pulse sends: "send me your ArtPollReply", nothing else.
pub fn poll() -> Vec<u8> {
    let mut p = ID.to_vec();
    p.extend_from_slice(&OP_POLL.to_le_bytes());
    p.extend_from_slice(&PROTOCOL.to_be_bytes());
    // Flags: no diagnostics, replies only when asked; priority: none.
    p.extend_from_slice(&[0, 0]);
    p
}

fn text(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_string()
}

/// Read an Art-Net packet; `None` for anything else (or kinds Pulse ignores).
pub fn parse(b: &[u8]) -> Option<Packet> {
    if b.len() < 12 || &b[..8] != ID {
        return None;
    }
    match u16::from_le_bytes([b[8], b[9]]) {
        OP_POLL => Some(Packet::Poll),
        OP_DMX if b.len() >= 18 => {
            let universe = u16::from(b[15] & 0x7f) << 8 | u16::from(b[14]);
            let length = u16::from_be_bytes([b[16], b[17]]);
            Some(Packet::Dmx {
                universe,
                sequence: b[12],
                length,
            })
        }
        // The fixed part up to the node report; newer fields are optional.
        OP_POLL_REPLY if b.len() >= 174 => {
            let ip = Ipv4Addr::new(b[10], b[11], b[12], b[13]);
            let firmware = u16::from_be_bytes([b[16], b[17]]);
            let net = u16::from(b[18] & 0x7f);
            let sub = u16::from(b[19] & 0x0f);
            let ports = usize::from(b[173]).min(4);
            let addr = |sw: u8| net << 8 | sub << 4 | u16::from(sw & 0x0f);
            let get = |i: usize| b.get(i).copied();
            let mut outputs = Vec::new();
            let mut inputs = Vec::new();
            for i in 0..ports {
                let kind = get(174 + i).unwrap_or(0);
                // Bit 7: can output DMX from Art-Net; bit 6: can input to Art-Net.
                if kind & 0x80 != 0 {
                    if let Some(sw) = get(190 + i) {
                        outputs.push(addr(sw));
                    }
                }
                if kind & 0x40 != 0 {
                    if let Some(sw) = get(186 + i) {
                        inputs.push(addr(sw));
                    }
                }
            }
            let mac = b
                .get(201..207)
                .filter(|m| m.iter().any(|&x| x != 0))
                .map(|m| {
                    m.iter()
                        .map(|x| format!("{x:02x}"))
                        .collect::<Vec<_>>()
                        .join(":")
                })
                .unwrap_or_default();
            Some(Packet::PollReply(Box::new(PollReply {
                ip,
                short_name: text(&b[26..44]),
                long_name: text(&b[44..108]),
                report: text(&b[108..172]),
                outputs,
                inputs,
                mac,
                firmware,
            })))
        }
        _ => None,
    }
}

/// An ArtPollReply as a node sends it (for the simulator and tests).
pub fn poll_reply(r: &PollReply) -> Vec<u8> {
    let mut p = vec![0u8; 239];
    p[..8].copy_from_slice(ID);
    p[8..10].copy_from_slice(&OP_POLL_REPLY.to_le_bytes());
    p[10..14].copy_from_slice(&r.ip.octets());
    p[14..16].copy_from_slice(&PORT.to_le_bytes());
    p[16..18].copy_from_slice(&r.firmware.to_be_bytes());
    let first = r.outputs.first().or(r.inputs.first()).copied().unwrap_or(0);
    p[18] = ((first >> 8) & 0x7f) as u8;
    p[19] = ((first >> 4) & 0x0f) as u8;
    let put = |p: &mut [u8], at: usize, len: usize, s: &str| {
        let b = s.as_bytes();
        let n = b.len().min(len - 1);
        p[at..at + n].copy_from_slice(&b[..n]);
    };
    put(&mut p, 26, 18, &r.short_name);
    put(&mut p, 44, 64, &r.long_name);
    put(&mut p, 108, 64, &r.report);
    let n = r.outputs.len().max(r.inputs.len()).min(4);
    p[173] = n as u8;
    for i in 0..n {
        let mut kind = 0u8;
        if let Some(u) = r.outputs.get(i) {
            kind |= 0x80;
            p[190 + i] = (u & 0x0f) as u8;
        }
        if let Some(u) = r.inputs.get(i) {
            kind |= 0x40;
            p[186 + i] = (u & 0x0f) as u8;
        }
        p[174 + i] = kind;
    }
    p
}

/// An ArtDmx frame (for the simulator and tests).
pub fn dmx(universe: u16, sequence: u8, channels: &[u8]) -> Vec<u8> {
    let mut p = ID.to_vec();
    p.extend_from_slice(&OP_DMX.to_le_bytes());
    p.extend_from_slice(&PROTOCOL.to_be_bytes());
    p.push(sequence);
    p.push(0);
    p.push((universe & 0xff) as u8);
    p.push(((universe >> 8) & 0x7f) as u8);
    let len = channels.len().clamp(2, 512) as u16;
    p.extend_from_slice(&len.to_be_bytes());
    p.extend_from_slice(&channels[..channels.len().min(512)]);
    p.resize(18 + usize::from(len), 0);
    p
}

/// "Net:Sub:Uni" of a 15-bit port address, as consoles show it.
pub fn universe_label(u: u16) -> String {
    format!("{u} ({}:{}:{})", u >> 8, (u >> 4) & 0x0f, u & 0x0f)
}

/// How long frames are counted for the frame rate.
pub const FPS_WINDOW_MS: Ms = 2000;
/// A sender is "current" this long after its last frame.
pub const SOURCE_TTL_MS: Ms = 3000;

/// Frames of one universe.
#[derive(Clone, Debug, Default)]
pub struct Universe {
    frames: VecDeque<Ms>,
    pub last: Option<Ms>,
    pub channels: u16,
    /// Sender -> last frame from it.
    pub sources: BTreeMap<Ipv4Addr, Ms>,
}

impl Universe {
    pub fn record(&mut self, from: Ipv4Addr, channels: u16, now: Ms) {
        self.frames.push_back(now);
        while self
            .frames
            .front()
            .is_some_and(|&t| t + FPS_WINDOW_MS < now)
        {
            self.frames.pop_front();
        }
        self.last = Some(now);
        self.channels = channels;
        self.sources.insert(from, now);
    }
    /// Frames per second over the last two seconds.
    pub fn fps(&self, now: Ms) -> f64 {
        let n = self
            .frames
            .iter()
            .filter(|&&t| t + FPS_WINDOW_MS >= now)
            .count();
        n as f64 * 1000.0 / FPS_WINDOW_MS as f64
    }
    /// Who sent in the last three seconds.
    pub fn senders(&self, now: Ms) -> Vec<Ipv4Addr> {
        self.sources
            .iter()
            .filter(|(_, &t)| t + SOURCE_TTL_MS >= now)
            .map(|(ip, _)| *ip)
            .collect()
    }
}

/// All universes seen, and the nodes that answered ArtPoll.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    pub universes: BTreeMap<u16, Universe>,
    /// Node address -> (reply, when).
    pub nodes: BTreeMap<Ipv4Addr, (PollReply, Ms)>,
}

impl Seen {
    pub fn packet(&mut self, from: Ipv4Addr, p: Packet, now: Ms) {
        match p {
            Packet::Dmx {
                universe, length, ..
            } => self
                .universes
                .entry(universe)
                .or_default()
                .record(from, length, now),
            Packet::PollReply(r) => {
                // A node may name another address than it sends from (bound
                // ports); the address it names is the one to match.
                let ip = if r.ip.is_unspecified() { from } else { r.ip };
                self.nodes.insert(ip, (*r, now));
            }
            Packet::Poll => {}
        }
    }
    /// Forget what has not been seen for a minute.
    pub fn prune(&mut self, now: Ms) {
        self.nodes.retain(|_, (_, t)| *t + 60_000 >= now);
        self.universes
            .retain(|_, u| u.last.is_some_and(|t| t + 60_000 >= now));
        for u in self.universes.values_mut() {
            u.sources.retain(|_, t| *t + 60_000 >= now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_is_a_valid_art_net_packet() {
        let p = poll();
        assert_eq!(p.len(), 14);
        assert_eq!(parse(&p), Some(Packet::Poll));
        assert_eq!(parse(b"hello"), None);
        assert_eq!(parse(&[0u8; 20]), None);
    }

    #[test]
    fn dmx_round_trip_with_net_and_sub_net() {
        let u = 0x0123; // net 1, sub 2, universe 3
        let p = dmx(u, 7, &[255; 512]);
        assert_eq!(p.len(), 18 + 512);
        assert_eq!(
            parse(&p),
            Some(Packet::Dmx {
                universe: u,
                sequence: 7,
                length: 512
            })
        );
        assert_eq!(universe_label(u), "291 (1:2:3)");
    }

    #[test]
    fn poll_reply_round_trip() {
        let r = PollReply {
            ip: Ipv4Addr::new(10, 0, 0, 40),
            short_name: "Node SR".into(),
            long_name: "Truss node stage right".into(),
            report: "#0001 [0042] OK".into(),
            outputs: vec![0x10, 0x11],
            inputs: vec![],
            mac: "00:11:22:33:44:55".into(),
            firmware: 3,
        };
        let mut b = poll_reply(&r);
        b[201..207].copy_from_slice(&[0, 0x11, 0x22, 0x33, 0x44, 0x55]);
        let Some(Packet::PollReply(got)) = parse(&b) else {
            panic!("no reply")
        };
        assert_eq!(*got, r);
        // Too short to be a reply.
        assert_eq!(parse(&b[..100]), None);
    }

    #[test]
    fn frame_rate_senders_and_conflicts() {
        let mut u = Universe::default();
        let a = Ipv4Addr::new(10, 0, 0, 5);
        let b = Ipv4Addr::new(10, 0, 0, 6);
        for i in 0..80 {
            u.record(a, 512, 10_000 + i * 25); // 40 fps for 2 s
        }
        let now = 10_000 + 79 * 25;
        assert!((u.fps(now) - 40.0).abs() < 1.0, "{}", u.fps(now));
        assert_eq!(u.senders(now), vec![a]);
        u.record(b, 512, now);
        assert_eq!(u.senders(now), vec![a, b]);
        // Five seconds later nothing arrives and nobody is current.
        assert_eq!(u.fps(now + 5000), 0.0);
        assert!(u.senders(now + 5000).is_empty());
    }

    #[test]
    fn seen_keeps_nodes_by_the_address_they_name() {
        let mut s = Seen::default();
        let r = PollReply {
            ip: Ipv4Addr::new(2, 0, 0, 9),
            ..PollReply::default()
        };
        s.packet(
            Ipv4Addr::new(10, 0, 0, 9),
            Packet::PollReply(Box::new(r)),
            1000,
        );
        assert!(s.nodes.contains_key(&Ipv4Addr::new(2, 0, 0, 9)));
        s.prune(100_000);
        assert!(s.nodes.is_empty());
    }
}
