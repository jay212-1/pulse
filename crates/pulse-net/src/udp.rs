//! UDP sockets the way Pulse needs them: shared ports (Art-Net 6454 may be
//! used by lighting software on the same computer), broadcasts, a chosen
//! source address.

use socket2::{Domain, Protocol, Socket, Type};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

/// A tokio UDP socket on `ip:port`. `shared`: other programs may bind the
/// same port (SO_REUSEADDR, and SO_REUSEPORT on unix). Needs a tokio runtime.
pub fn bind(ip: Ipv4Addr, port: u16, shared: bool) -> std::io::Result<tokio::net::UdpSocket> {
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    if shared {
        s.set_reuse_address(true)?;
        #[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos"))))]
        s.set_reuse_port(true)?;
    }
    s.set_broadcast(true)?;
    s.set_nonblocking(true)?;
    s.bind(&SocketAddr::V4(SocketAddrV4::new(ip, port)).into())?;
    tokio::net::UdpSocket::from_std(s.into())
}

/// One question, the first answer `accept` takes (or `None` in time).
pub async fn ask<T>(
    source: Option<Ipv4Addr>,
    to: SocketAddr,
    packet: &[u8],
    timeout: std::time::Duration,
    accept: impl Fn(&[u8], SocketAddr) -> Option<T>,
) -> std::io::Result<Option<(T, f64)>> {
    let sock = bind(source.unwrap_or(Ipv4Addr::UNSPECIFIED), 0, false)?;
    let start = std::time::Instant::now();
    sock.send_to(packet, to).await?;
    let mut buf = vec![0u8; 2048];
    let wait = async {
        loop {
            let (n, from) = sock.recv_from(&mut buf).await?;
            if let Some(t) = accept(&buf[..n], from) {
                return Ok::<_, std::io::Error>((t, start.elapsed().as_secs_f64() * 1000.0));
            }
        }
    };
    match tokio::time::timeout(timeout, wait).await {
        Ok(Ok(r)) => Ok(Some(r)),
        Ok(Err(e)) => Err(e),
        Err(_) => Ok(None),
    }
}
