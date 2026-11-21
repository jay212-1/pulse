//! # pulse-net
//!
//! The network side of Pulse, read-only: the probes ([`probe::Probe`]: ping
//! without administrator rights, TCP port), and the [`monitor::Monitor`]
//! that schedules them gently (spread out, under a packet limit, every run
//! isolated with a hard timeout) and feeds the status engine of `pulse-core`.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod icmp;
#[cfg(windows)]
mod icmp_windows;
pub mod monitor;
pub mod probe;

pub use icmp::Method;
pub use monitor::{Mode, Monitor, Options, Shared};
