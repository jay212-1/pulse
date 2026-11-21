//! Active discovery, only while the discovery page is open or after an
//! explicit "scan": asking consoles by broadcast who is there, and the
//! gentle ping sweep of one subnet (20 addresses per second at most, only
//! after the user confirmed it).

use crate::console;
use crate::found::{lock, ConsoleAnswer, Found, Sweep};
use crate::icmp::Method;
use crate::interfaces;
use crate::monitor::now_ms;
use pulse_core::discovery::Net;
use pulse_core::model::ConsoleModel;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long console answers are collected after a broadcast.
pub const LISTEN: Duration = Duration::from_millis(1500);
/// Addresses per second in a sweep.
pub const SWEEP_RATE: f64 = 20.0;

/// Ask every console on every interface (broadcast); `extra` = more
/// targets (tests, the simulator). Answers go to [`Found::consoles`].
pub async fn scan_consoles(
    found: Arc<Mutex<Found>>,
    extra: Vec<(ConsoleModel, SocketAddr)>,
    lan: bool,
    sent: Arc<AtomicU64>,
) {
    let Ok(sock) = crate::udp::bind(Ipv4Addr::UNSPECIFIED, 0, false) else {
        return;
    };
    let mut targets: Vec<(ConsoleModel, SocketAddr)> = {
        let f = lock(&found);
        interfaces::lan(&f.nics)
            .filter(|_| lan)
            .flat_map(|n| {
                let b = n.broadcast_addr();
                ConsoleModel::ALL
                    .into_iter()
                    .map(move |m| (m, SocketAddr::V4(SocketAddrV4::new(b, m.port()))))
            })
            .collect()
    };
    targets.extend(extra);
    for (m, to) in &targets {
        if sock.send_to(&console::request(*m), *to).await.is_ok() {
            sent.fetch_add(1, Ordering::Relaxed);
        }
    }
    let mut buf = vec![0u8; 2048];
    let collect = async {
        loop {
            let Ok((n, from)) = sock.recv_from(&mut buf).await else {
                continue;
            };
            let IpAddr::V4(ip) = from.ip() else { continue };
            // Which question this answers: by the port it comes from, else by its content.
            let by_port = targets
                .iter()
                .find(|(_, t)| t.port() == from.port())
                .map(|(m, _)| *m);
            let models = by_port
                .map(|m| vec![m])
                .unwrap_or_else(|| ConsoleModel::ALL.to_vec());
            for m in models {
                if let Some(info) = console::parse(m, &buf[..n]) {
                    let detail = info.detail();
                    let now = now_ms();
                    lock(&found).consoles.insert(
                        (ip, m),
                        ConsoleAnswer {
                            model: m,
                            ip,
                            name: info.name,
                            detail,
                            at: now,
                        },
                    );
                    break;
                }
            }
        }
    };
    let _ = tokio::time::timeout(LISTEN, collect).await;
    lock(&found).last_scan = Some(now_ms());
}

/// Ping every address of `net` once, `SWEEP_RATE` per second; progress and
/// answers go to [`Found::sweep`]. Stops when the sweep is cancelled.
pub async fn sweep(
    found: Arc<Mutex<Found>>,
    net: Net,
    source: Option<Ipv4Addr>,
    method: Method,
    sent: Arc<AtomicU64>,
) {
    let hosts = net.hosts();
    lock(&found).sweep = Some(Sweep {
        net,
        total: hosts.len(),
        done: 0,
        answered: Default::default(),
        started: now_ms(),
        finished: None,
        cancelled: false,
    });
    let mut tasks = Vec::new();
    let gap = Duration::from_secs_f64(1.0 / SWEEP_RATE);
    for (i, ip) in hosts.into_iter().enumerate() {
        if lock(&found).sweep.as_ref().is_none_or(|s| s.cancelled) {
            break;
        }
        let f = found.clone();
        let m = method.clone();
        sent.fetch_add(1, Ordering::Relaxed);
        tasks.push(tokio::spawn(async move {
            let r = crate::probe::ping_once(
                IpAddr::V4(ip),
                &m,
                source,
                i as u16,
                Duration::from_secs(1),
            )
            .await;
            let mut g = lock(&f);
            if let Some(s) = g.sweep.as_mut() {
                s.done += 1;
                if let Ok(Some(ms)) = r {
                    s.answered.insert(ip, ms);
                }
            }
        }));
        tokio::time::sleep(gap).await;
    }
    for t in tasks {
        let _ = t.await;
    }
    if let Some(s) = lock(&found).sweep.as_mut() {
        s.finished = Some(now_ms());
    }
}
