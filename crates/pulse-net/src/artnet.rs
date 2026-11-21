//! The Art-Net listener: one socket on UDP 6454 (shared with other
//! programs), counts the frames of every universe and keeps the nodes that
//! answer ArtPoll. Polls go out every 3 s, only while something needs them
//! (an Art-Net node check, or the discovery page), to the directed
//! broadcast of every interface.

use crate::found::{Found, Listener};
use crate::interfaces;
use crate::monitor::now_ms;
use pulse_core::artnet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const POLL_EVERY: Duration = Duration::from_secs(3);

/// Listen until the task is aborted. `poll`: send ArtPoll now and then.
/// `extra`: more poll targets (tests, the simulator).
pub async fn listen(
    found: Arc<Mutex<Found>>,
    port: u16,
    poll: Arc<AtomicBool>,
    extra: Vec<SocketAddr>,
    lan: bool,
    sent: Arc<std::sync::atomic::AtomicU64>,
) {
    let sock = match crate::udp::bind(Ipv4Addr::UNSPECIFIED, port, true) {
        Ok(s) => s,
        Err(e) => {
            crate::found::lock(&found).artnet_state =
                Listener::Failed(format!("cannot listen on UDP {port}: {e}"));
            return;
        }
    };
    crate::found::lock(&found).artnet_state = Listener::On(now_ms());
    let mut buf = vec![0u8; 1500];
    let mut every = tokio::time::interval(POLL_EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let packet = artnet::poll();
    loop {
        tokio::select! {
            r = sock.recv_from(&mut buf) => {
                let Ok((n, from)) = r else { continue };
                let IpAddr::V4(ip) = from.ip() else { continue };
                if let Some(p) = artnet::parse(&buf[..n]) {
                    crate::found::lock(&found).artnet.packet(ip, p, now_ms());
                }
            }
            _ = every.tick() => {
                if !poll.load(Ordering::Relaxed) {
                    continue;
                }
                let targets: Vec<SocketAddr> = {
                    let f = crate::found::lock(&found);
                    interfaces::lan(&f.nics)
                        .filter(|_| lan)
                        .map(|n| SocketAddr::V4(SocketAddrV4::new(n.broadcast_addr(), artnet::PORT)))
                        .chain(extra.iter().copied())
                        .collect()
                };
                for t in targets {
                    if sock.send_to(&packet, t).await.is_ok() {
                        sent.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }
}
