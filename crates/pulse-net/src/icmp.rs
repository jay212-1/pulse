//! ICMP echo without administrator rights.
//!
//! * macOS and Linux: an unprivileged ICMP datagram socket (`SOCK_DGRAM`,
//!   `IPPROTO_ICMP`). Linux allows it only for groups in
//!   `net.ipv4.ping_group_range` (most desktop distributions allow everyone).
//! * Windows: `IcmpSendEcho` of the IP helper API (`icmp_windows.rs`).
//!
//! If neither works, [`Method::Tcp`] is used: a TCP connection attempt to
//! port 80, where "connection refused" also proves the device is there.
//! The method is chosen once and shown in the window.

use std::net::Ipv4Addr;
use std::sync::OnceLock;
use std::time::Duration;

/// How Pulse pings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Method {
    Icmp,
    /// No ICMP: TCP connection attempts; the reason says why.
    Tcp {
        reason: String,
    },
}

impl Method {
    /// "ICMP" or "TCP 80 (no ICMP: …)".
    pub fn label(&self) -> String {
        match self {
            Method::Icmp => "ICMP".into(),
            Method::Tcp { reason } => format!("TCP {TCP_FALLBACK_PORT} (no ICMP: {reason})"),
        }
    }
}

/// Port of the TCP fallback.
pub const TCP_FALLBACK_PORT: u16 = 80;
/// Bytes of payload in an echo request (a token to recognise the answer).
pub const PAYLOAD: usize = 16;
/// Bytes one echo request puts on the wire (Ethernet + IPv4 + ICMP + payload).
pub const WIRE_BYTES: u64 = 14 + 20 + 8 + PAYLOAD as u64;

/// The method this computer allows (checked once).
pub fn method() -> &'static Method {
    static M: OnceLock<Method> = OnceLock::new();
    M.get_or_init(|| match probe_permission() {
        Ok(()) => Method::Icmp,
        Err(reason) => Method::Tcp { reason },
    })
}

#[cfg(unix)]
fn probe_permission() -> Result<(), String> {
    unix::open().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn probe_permission() -> Result<(), String> {
    crate::icmp_windows::available()
}

#[cfg(not(any(unix, windows)))]
fn probe_permission() -> Result<(), String> {
    Err("not supported on this system".into())
}

/// One echo request; blocks up to `timeout`. `Ok(Some(ms))` = answer,
/// `Ok(None)` = no answer in time, `Err` = could not be sent.
pub fn echo(addr: Ipv4Addr, seq: u16, timeout: Duration) -> std::io::Result<Option<f64>> {
    #[cfg(unix)]
    {
        unix::echo(addr, seq, timeout)
    }
    #[cfg(windows)]
    {
        crate::icmp_windows::echo(addr, seq, timeout)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (addr, seq, timeout);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "no ICMP",
        ))
    }
}

/// A token nobody else sends: process id, a counter and the time.
pub fn token() -> [u8; PAYLOAD] {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut out = [0u8; PAYLOAD];
    out[..4].copy_from_slice(&std::process::id().to_be_bytes());
    out[4..8].copy_from_slice(&(n as u32).to_be_bytes());
    out[8..].copy_from_slice(&t.to_be_bytes());
    out
}

/// Internet checksum (RFC 1071).
pub fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    for chunk in data.chunks(2) {
        let word = match chunk {
            [a, b] => u16::from_be_bytes([*a, *b]),
            [a] => u16::from_be_bytes([*a, 0]),
            _ => 0,
        };
        sum += u32::from(word);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// An echo request: type 8, code 0, checksum, identifier, sequence, payload.
pub fn request(id: u16, seq: u16, payload: &[u8]) -> Vec<u8> {
    let mut p = vec![8u8, 0, 0, 0];
    p.extend_from_slice(&id.to_be_bytes());
    p.extend_from_slice(&seq.to_be_bytes());
    p.extend_from_slice(payload);
    let c = checksum(&p);
    p[2..4].copy_from_slice(&c.to_be_bytes());
    p
}

/// Is `packet` the echo reply to our request? macOS hands over the IP header,
/// Linux does not, so it is skipped when present. The identifier is not
/// compared (Linux replaces it with the socket's).
pub fn is_reply(packet: &[u8], seq: u16, payload: &[u8]) -> bool {
    let icmp = match packet.first() {
        Some(b) if b >> 4 == 4 => {
            let ihl = usize::from(b & 0x0f) * 4;
            packet.get(ihl..).unwrap_or(&[])
        }
        _ => packet,
    };
    icmp.len() >= 8 + payload.len()
        && icmp[0] == 0
        && icmp[1] == 0
        && icmp[6..8] == seq.to_be_bytes()
        && &icmp[8..8 + payload.len()] == payload
}

#[cfg(unix)]
mod unix {
    use super::*;
    use socket2::{Domain, Protocol, SockAddr, Socket, Type};
    use std::io::Read;
    use std::net::SocketAddrV4;
    use std::time::Instant;

    pub fn open() -> std::io::Result<Socket> {
        Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4))
    }

    pub fn echo(addr: Ipv4Addr, seq: u16, timeout: Duration) -> std::io::Result<Option<f64>> {
        let mut sock = open()?;
        let payload = token();
        let packet = request(std::process::id() as u16, seq, &payload);
        let to = SockAddr::from(SocketAddrV4::new(addr, 0));
        let start = Instant::now();
        sock.send_to(&packet, &to)?;
        let mut buf = [0u8; 1500];
        loop {
            let left = timeout.saturating_sub(start.elapsed());
            if left.is_zero() {
                return Ok(None);
            }
            sock.set_read_timeout(Some(left.max(Duration::from_millis(1))))?;
            // The token tells our answer apart from anything else this socket sees.
            match sock.read(&mut buf) {
                Ok(n) => {
                    if is_reply(&buf[..n], seq, &payload) {
                        return Ok(Some(start.elapsed().as_secs_f64() * 1000.0));
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None)
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_has_a_valid_checksum_and_matches_its_reply() {
        let payload = token();
        let req = request(0x1234, 7, &payload);
        assert_eq!(req.len(), 8 + PAYLOAD);
        assert_eq!(checksum(&req), 0, "checksum over a valid packet is zero");
        // A reply: type 0, same sequence and payload (Linux: no IP header).
        let mut reply = req.clone();
        reply[0] = 0;
        assert!(is_reply(&reply, 7, &payload));
        assert!(!is_reply(&reply, 8, &payload));
        assert!(!is_reply(&req, 7, &payload), "our own request is no reply");
        // macOS: with a 20-byte IP header in front.
        let mut with_ip = vec![0x45u8];
        with_ip.extend_from_slice(&[0; 19]);
        with_ip.extend_from_slice(&reply);
        assert!(is_reply(&with_ip, 7, &payload));
        assert!(
            !is_reply(&with_ip[..30], 7, &payload),
            "short packets are ignored"
        );
        assert!(!is_reply(&[], 7, &payload));
    }

    #[test]
    fn tokens_differ() {
        assert_ne!(token(), token());
    }
}
