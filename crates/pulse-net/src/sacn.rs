//! The sACN listener: one socket on UDP 5568 (shared with other programs).
//! It joins the multicast group of every universe a check wants, on every
//! network of this computer, and leaves the ones no longer wanted. Unicast
//! sACN to this computer arrives as well. It only listens.

use crate::found::{lock, Found, Listener};
use crate::interfaces;
use crate::monitor::now_ms;
use pulse_core::sacn;
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Listen until the task is aborted. `universes`: the groups to be in
/// (read every second). `lan`: join groups at all (tests: unicast only).
pub async fn listen(
    found: Arc<Mutex<Found>>,
    port: u16,
    universes: Arc<Mutex<BTreeSet<u16>>>,
    lan: bool,
) {
    let sock = match crate::udp::bind(Ipv4Addr::UNSPECIFIED, port, true) {
        Ok(s) => s,
        Err(e) => {
            lock(&found).sacn_state = Listener::Failed(format!("cannot listen on UDP {port}: {e}"));
            return;
        }
    };
    {
        let mut f = lock(&found);
        f.sacn_state = Listener::On(now_ms());
        f.sacn_port = sock.local_addr().ok().map(|a| a.port());
    }
    // (group, interface address) joined.
    let mut joined: BTreeSet<(Ipv4Addr, Ipv4Addr)> = BTreeSet::new();
    let mut buf = vec![0u8; 1500];
    let mut every = tokio::time::interval(Duration::from_secs(1));
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            r = sock.recv_from(&mut buf) => {
                let Ok((n, from)) = r else { continue };
                let IpAddr::V4(ip) = from.ip() else { continue };
                if let Some(d) = sacn::parse(&buf[..n]) {
                    lock(&found).sacn.packet(ip, d, now_ms());
                }
            }
            _ = every.tick() => {
                if !lan {
                    continue;
                }
                let want: BTreeSet<(Ipv4Addr, Ipv4Addr)> = {
                    let us = universes.lock().map(|g| g.clone()).unwrap_or_default();
                    let f = lock(&found);
                    interfaces::lan(&f.nics)
                        .flat_map(|n| us.iter().map(move |u| (sacn::group(*u), n.ip)))
                        .collect()
                };
                for g in joined.difference(&want).copied().collect::<Vec<_>>() {
                    let _ = sock.leave_multicast_v4(g.0, g.1);
                    joined.remove(&g);
                }
                for g in want.difference(&joined).copied().collect::<Vec<_>>() {
                    match sock.join_multicast_v4(g.0, g.1) {
                        Ok(()) => {
                            joined.insert(g);
                        }
                        Err(e) => tracing::debug!("sACN: cannot join {} on {}: {e}", g.0, g.1),
                    }
                }
            }
        }
    }
}
