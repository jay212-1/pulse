//! The status page: Pulse's traffic light for phones and other computers of
//! the crew, read-only, on TCP 7470. `GET /` is one self-contained page
//! (no scripts or fonts from elsewhere) that asks `GET /api/status` every
//! two seconds. Nothing can be changed through it. Off unless switched on;
//! while on, it is announced as `_pulse._tcp` (mDNS).

use crate::monitor::{now_ms, Shared};
use pulse_core::status::Status;
use serde_json::json;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

pub const PORT: u16 = 7470;
pub const SERVICE: &str = "_pulse._tcp.local.";
const PAGE: &str = include_str!("status_page.html");

/// What the page shows: the monitor of the open profile, if any.
pub type Source = Arc<Mutex<Option<Arc<Mutex<Shared>>>>>;

fn word(s: Status) -> &'static str {
    match s {
        Status::Ok => "ok",
        Status::Warn => "warn",
        Status::Down => "down",
        Status::Unknown => "unknown",
    }
}

/// The JSON the page reads.
pub fn snapshot(s: &Shared) -> serde_json::Value {
    let o = &s.overview;
    let reason = |r: &pulse_core::status::Reason| {
        json!({
            "name": r.name,
            "area": r.area,
            "status": word(r.status),
            "cause": r.cause,
            "since": r.since,
        })
    };
    let devices: Vec<serde_json::Value> = s
        .profile
        .devices
        .iter()
        .filter(|d| d.enabled)
        .filter_map(|d| {
            let st = s.engine.device(d.id)?;
            Some(json!({
                "name": d.name,
                "area": d.area,
                "status": word(st.status),
                "cause": st.cause,
                "since": st.since,
                "acked": st.ack.is_some(),
            }))
        })
        .collect();
    json!({
        "pulse": env!("CARGO_PKG_VERSION"),
        "profile": s.profile.name,
        "venue": s.profile.venue,
        "now": now_ms(),
        "light": word(o.light),
        "counts": {
            "ok": o.counts.ok,
            "warn": o.counts.warn,
            "down": o.counts.down,
            "unknown": o.counts.unknown,
            "acked": o.counts.acked,
        },
        "reasons": o.reasons.iter().map(reason).collect::<Vec<_>>(),
        "acked": o.acked.iter().map(reason).collect::<Vec<_>>(),
        "areas": o.areas.iter().map(|a| json!({"area": a.area, "light": word(a.light)})).collect::<Vec<_>>(),
        "devices": devices,
    })
}

fn answer(source: &Source, path: &str) -> crate::http::Answer {
    match path.split('?').next().unwrap_or("") {
        "/" | "/index.html" => crate::http::Answer::html(PAGE),
        "/api/status" => {
            let shared = source.lock().ok().and_then(|g| g.clone());
            let body = match shared {
                Some(s) => {
                    let g = s.lock().unwrap_or_else(|e| e.into_inner());
                    snapshot(&g)
                }
                None => {
                    json!({"pulse": env!("CARGO_PKG_VERSION"), "profile": null, "now": now_ms()})
                }
            };
            crate::http::Answer::json(body.to_string())
        }
        _ => crate::http::Answer::not_found(),
    }
}

/// A running status page; dropping it stops it (and its announcement).
pub struct StatusPage {
    pub port: u16,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl StatusPage {
    /// Serve on `bind:port` (port 0: any, for tests). `announce`: as
    /// `_pulse._tcp` on the networks of this computer.
    pub fn start(
        bind: IpAddr,
        port: u16,
        source: Source,
        announce: bool,
    ) -> std::io::Result<StatusPage> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("pulse-status-page")
            .enable_all()
            .build()?;
        let l = {
            let _g = rt.enter();
            let std_l = std::net::TcpListener::bind(SocketAddr::new(bind, port))?;
            std_l.set_nonblocking(true)?;
            tokio::net::TcpListener::from_std(std_l)?
        };
        let port = l.local_addr()?.port();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let mdns = if announce { announce_on(port) } else { None };
        std::thread::Builder::new()
            .name("pulse-status-page".into())
            .spawn(move || {
                rt.block_on(async move {
                    tokio::select! {
                        _ = crate::http::serve(l, move |p| answer(&source, p)) => {}
                        _ = stopped => {}
                    }
                });
                if let Some((d, name)) = mdns {
                    let _ = d.unregister(&name);
                    let _ = d.shutdown();
                }
                rt.shutdown_timeout(std::time::Duration::from_millis(200));
            })?;
        Ok(StatusPage {
            port,
            stop: Some(stop),
        })
    }

    /// Addresses the page is reachable at, on these networks of this computer.
    pub fn urls(&self, nics: &[crate::Nic]) -> Vec<String> {
        crate::interfaces::lan(nics)
            .map(|n| format!("http://{}:{}/", n.ip, self.port))
            .collect()
    }
}

impl Drop for StatusPage {
    fn drop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
    }
}

/// Announce the page; the daemon and the full name, to take it back.
fn announce_on(port: u16) -> Option<(mdns_sd::ServiceDaemon, String)> {
    let nics = crate::interfaces::list();
    let ip = crate::interfaces::lan(&nics).next()?.ip;
    let d = mdns_sd::ServiceDaemon::new().ok()?;
    let host = format!("pulse-{}.local.", ip.to_string().replace('.', "-"));
    let info = mdns_sd::ServiceInfo::new(
        SERVICE,
        &format!("Pulse ({ip})"),
        &host,
        "",
        port,
        &[("path", "/"), ("api", "/api/status")][..],
    )
    .ok()?
    .enable_addr_auto();
    let name = info.get_fullname().to_string();
    match d.register(info) {
        Ok(()) => Some((d, name)),
        Err(e) => {
            tracing::debug!("status page: cannot announce: {e}");
            let _ = d.shutdown();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::{Monitor, Options};
    use std::net::Ipv4Addr;
    use std::time::Duration;

    #[test]
    fn serves_the_page_and_the_status_read_only() {
        let mut p = pulse_core::Profile {
            name: "Test hall".into(),
            ..Default::default()
        };
        p.add_device(pulse_core::DeviceKind::Other, "Nowhere", "192.0.2.1");
        let m = Monitor::start(
            p,
            Options {
                mdns: false,
                lan: false,
                artnet_port: 0,
                sacn_port: 0,
                ..Options::default()
            },
        )
        .unwrap();
        let source: Source = Arc::new(Mutex::new(None));
        let page =
            StatusPage::start(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, source.clone(), false).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let get = |path: &str| {
            rt.block_on(crate::http::get(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                page.port,
                path,
                None,
                Duration::from_secs(2),
            ))
            .unwrap()
        };
        let (code, html) = get("/");
        assert_eq!(code, 200);
        assert!(
            html.contains("/api/status") && !html.contains("http://") && !html.contains("https://")
        );
        let (_, none) = get("/api/status");
        assert!(none.contains("\"profile\":null"), "{none}");
        *source.lock().unwrap() = Some(m.shared_handle());
        let (_, body) = get("/api/status");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["profile"], "Test hall");
        assert_eq!(v["devices"][0]["name"], "Nowhere");
        assert_eq!(get("/nope").0, 404);
        drop(page);
    }
}
