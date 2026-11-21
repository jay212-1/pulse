//! The mDNS registry: browses the service types the checks and the
//! discovery page need (and stops browsing the others), and keeps every
//! resolved service in [`Found`]. mDNS only asks; Pulse announces nothing.

use crate::found::{lock, Found, Listener, Service};
use crate::monitor::now_ms;
use mdns_sd::{Receiver, ServiceDaemon, ServiceEvent};
use pulse_core::model::service_short;
use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;
use std::sync::Mutex;

/// "_ndi._tcp" -> "_ndi._tcp.local.".
pub fn full_type(ty: &str) -> String {
    format!("{}.local.", service_short(ty))
}

/// The instance part of a full name.
fn instance(fullname: &str, ty_domain: &str) -> String {
    fullname
        .strip_suffix(ty_domain)
        .unwrap_or(fullname)
        .trim_end_matches('.')
        .replace("\\032", " ")
}

pub struct Registry {
    daemon: Option<ServiceDaemon>,
    browsing: BTreeMap<String, Receiver<ServiceEvent>>,
}

impl Registry {
    pub fn new() -> Self {
        Registry {
            daemon: None,
            browsing: BTreeMap::new(),
        }
    }

    /// Browse exactly these types (short or full names).
    pub fn set_types(&mut self, found: &Mutex<Found>, types: &BTreeSet<String>) {
        let want: BTreeSet<String> = types.iter().map(|t| full_type(t)).collect();
        if want.is_empty() && self.daemon.is_none() {
            return;
        }
        if self.daemon.is_none() {
            match ServiceDaemon::new() {
                Ok(d) => {
                    self.daemon = Some(d);
                    lock(found).mdns_state = Listener::On(now_ms());
                }
                Err(e) => {
                    lock(found).mdns_state = Listener::Failed(format!("mDNS not available: {e}"));
                    return;
                }
            }
        }
        let Some(d) = &self.daemon else { return };
        let gone: Vec<String> = self
            .browsing
            .keys()
            .filter(|t| !want.contains(*t))
            .cloned()
            .collect();
        for t in gone {
            let _ = d.stop_browse(&t);
            self.browsing.remove(&t);
            let short = service_short(&t).to_string();
            lock(found).mdns.retain(|_, s| s.ty != short);
        }
        for t in want {
            if self.browsing.contains_key(&t) {
                continue;
            }
            match d.browse(&t) {
                Ok(rx) => {
                    self.browsing.insert(t, rx);
                }
                Err(e) => {
                    lock(found).mdns_state = Listener::Failed(format!("cannot browse {t}: {e}"));
                }
            }
        }
    }

    /// Take what the daemon found since the last call.
    pub fn drain(&mut self, found: &Mutex<Found>) {
        let now = now_ms();
        let mut events = Vec::new();
        for rx in self.browsing.values() {
            while let Ok(e) = rx.try_recv() {
                events.push(e);
            }
        }
        if events.is_empty() {
            return;
        }
        let mut f = lock(found);
        for e in events {
            match e {
                ServiceEvent::ServiceResolved(r) => {
                    let mut addrs: Vec<Ipv4Addr> = r.get_addresses_v4().into_iter().collect();
                    addrs.sort();
                    let txt = r
                        .get_properties()
                        .iter()
                        .map(|p| (p.key().to_string(), p.val_str().to_string()))
                        .collect();
                    f.mdns.insert(
                        r.fullname.clone(),
                        Service {
                            ty: service_short(&r.ty_domain).to_string(),
                            instance: instance(&r.fullname, &r.ty_domain),
                            host: r.host.trim_end_matches('.').to_string(),
                            port: r.port,
                            addrs,
                            txt,
                            at: now,
                        },
                    );
                }
                ServiceEvent::ServiceRemoved(_, fullname) => {
                    f.mdns.remove(&fullname);
                }
                _ => {}
            }
        }
    }

    pub fn active(&self) -> bool {
        self.daemon.is_some()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Registry::new()
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        if let Some(d) = self.daemon.take() {
            let _ = d.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(full_type("_ndi._tcp"), "_ndi._tcp.local.");
        assert_eq!(full_type("_ndi._tcp.local."), "_ndi._tcp.local.");
        assert_eq!(
            instance("CAM1 (Camera 1)._ndi._tcp.local.", "_ndi._tcp.local."),
            "CAM1 (Camera 1)"
        );
    }
}
