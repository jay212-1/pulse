//! # pulse-net
//!
//! The network side of Pulse, read-only: the probes ([`probe::Probe`]: ping
//! without administrator rights, TCP port, console queries, Art-Net, mDNS,
//! NDI, interfaces), the listeners (Art-Net, mDNS), discovery, and the
//! [`monitor::Monitor`]
//! that schedules them gently (spread out, under a packet limit, every run
//! isolated with a hard timeout) and feeds the status engine of `pulse-core`.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod agent;
pub mod artnet;
pub mod console;
pub mod discovery;
pub mod found;
pub mod http;
pub mod icmp;
#[cfg(windows)]
mod icmp_windows;
pub mod interfaces;
pub mod mdns;
pub mod monitor;
pub mod probe;
pub mod sacn;
pub mod snmp;
pub mod status_page;
pub mod tools;
pub mod udp;

pub use found::Found;
pub use icmp::Method;
pub use interfaces::Nic;
pub use monitor::{Mode, Monitor, Options, Shared};
