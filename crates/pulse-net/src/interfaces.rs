//! This computer's network interfaces (IPv4): which are up, their address,
//! net and broadcast address. Devices may name the interface to use.

use pulse_core::discovery::Net;
use std::net::Ipv4Addr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nic {
    pub name: String,
    pub ip: Ipv4Addr,
    pub prefix: u8,
    pub broadcast: Option<Ipv4Addr>,
    pub up: bool,
    pub loopback: bool,
}

impl Nic {
    pub fn net(&self) -> Net {
        Net {
            addr: self.ip,
            prefix: self.prefix,
        }
    }
    /// "en0 10.0.0.5/24".
    pub fn label(&self) -> String {
        format!("{} {}/{}", self.name, self.ip, self.prefix)
    }
    /// Where a broadcast on this interface goes (the directed broadcast).
    pub fn broadcast_addr(&self) -> Ipv4Addr {
        self.broadcast.unwrap_or_else(|| {
            let host = u32::MAX.checked_shr(u32::from(self.prefix)).unwrap_or(0);
            Ipv4Addr::from(u32::from(self.ip) | host)
        })
    }
}

/// The IPv4 interfaces with an address (loopback included, marked).
pub fn list() -> Vec<Nic> {
    let Ok(all) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    let mut out: Vec<Nic> = all
        .into_iter()
        .filter_map(|i| match &i.addr {
            if_addrs::IfAddr::V4(a) => Some(Nic {
                up: i.is_oper_up() || i.is_loopback(),
                loopback: i.is_loopback(),
                name: i.name.clone(),
                ip: a.ip,
                prefix: a.prefixlen,
                broadcast: a.broadcast,
            }),
            if_addrs::IfAddr::V6(_) => None,
        })
        .filter(|n| !n.ip.is_link_local() || n.loopback)
        .collect();
    out.sort_by(|a, b| (a.loopback, &a.name).cmp(&(b.loopback, &b.name)));
    out
}

/// The interface called `name` if it is up with an address.
pub fn find<'a>(nics: &'a [Nic], name: &str) -> Option<&'a Nic> {
    let name = name.trim();
    nics.iter().find(|n| n.name == name && n.up)
}

/// The interfaces broadcasts go out on (up, not loopback).
pub fn lan(nics: &[Nic]) -> impl Iterator<Item = &Nic> {
    nics.iter().filter(|n| n.up && !n.loopback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_is_listed_and_found() {
        let nics = list();
        assert!(nics.iter().any(|n| n.loopback), "{nics:?}");
        let lo = nics.iter().find(|n| n.loopback).unwrap();
        assert_eq!(find(&nics, &lo.name).map(|n| n.ip), Some(lo.ip));
        assert!(find(&nics, "no-such-interface").is_none());
    }

    #[test]
    fn directed_broadcast_without_a_given_one() {
        let n = Nic {
            name: "en9".into(),
            ip: Ipv4Addr::new(10, 1, 2, 3),
            prefix: 16,
            broadcast: None,
            up: true,
            loopback: false,
        };
        assert_eq!(n.broadcast_addr(), Ipv4Addr::new(10, 1, 255, 255));
        assert_eq!(n.label(), "en9 10.1.2.3/16");
    }
}
