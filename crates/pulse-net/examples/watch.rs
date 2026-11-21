//! Watch a few addresses from the terminal: `cargo run -p pulse-net --example watch -- 127.0.0.1 10.0.0.1`.
//! Prints every status change and, every 5 s, the overall light.

use pulse_core::model::{Criticality, DeviceKind, Profile};
use pulse_net::{Monitor, Options};
use std::time::Duration;

fn main() -> std::io::Result<()> {
    let mut p = Profile::default();
    for a in std::env::args().skip(1) {
        let id = p.add_device(DeviceKind::Other, &a, &a);
        if let Some(d) = p.device_mut(id) {
            d.criticality = Criticality::Critical;
        }
    }
    let m = Monitor::start(p, Options::default())?;
    println!("Ping method: {}", m.with(|s| s.method.label()));
    let mut seen = 0;
    let mut i = 0u64;
    loop {
        i += 1;
        std::thread::sleep(Duration::from_secs(1));
        m.with(|s| {
            for e in s.log.iter().skip(seen) {
                println!(
                    "{:>8} {}  {}  {}",
                    e.at % 100_000_000,
                    e.name,
                    e.kind.label(),
                    e.cause
                );
            }
            seen = s.log.len();
            if i % 5 == 0 {
                let o = &s.overview;
                let why = o.reasons.first().map(|r| r.text("")).unwrap_or_default();
                println!(
                    "-- overall {} {why}  ({} packets, {:.1}/s)",
                    o.light.label(),
                    s.load.packets,
                    s.load.packet_rate(pulse_net::monitor::now_ms())
                );
            }
        });
    }
}
