//! # pulse-core
//!
//! Everything in Pulse that needs neither the network nor a window: the
//! inventory and its profile file, the result format of the probes, ping
//! statistics, the status engine (hysteresis, flapping, acknowledgements,
//! area and overall lights with reasons), the event log and the history for
//! the graphs. Time comes in from outside, so all of it is plain, testable
//! logic.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod agent;
pub mod artnet;
pub mod demo;
pub mod discovery;
pub mod events;
pub mod exchange;
pub mod hints;
pub mod history;
pub mod model;
pub mod osc;
pub mod ping;
pub mod presets;
pub mod probe;
pub mod profile;
pub mod report;
pub mod sacn;
pub mod snmp;
pub mod status;

pub use events::{Event, EventKind, EventLog};
pub use model::{Check, CheckKind, ConsoleModel, Criticality, Device, DeviceKind, Id, Profile};
pub use presets::Preset;
pub use probe::{Depth, Metric, Outcome, ProbeResult};
pub use status::{Engine, Ms, Overview, Status};
