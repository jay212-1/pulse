//! `pulse-sim`: the demo's invented devices on this computer, for trying
//! Pulse by hand. Writes a profile for them and runs until Ctrl-C.
//!
//! ```text
//! pulse-sim [PROFILE] [--calm]
//! ```
//!
//! PROFILE defaults to `pulse-sim.pulse` in the current folder. `--calm`
//! leaves out the scenario (nothing goes wrong by itself). The light desk
//! sends its universe to 127.0.0.1:6454 and the media server sACN to
//! 127.0.0.1:5568, where Pulse listens.
//!
//! The simulator never announces anything on the real network. Without a
//! Pulse in the same process the Dante and Lantern announcements cannot
//! reach it, so their checks are left out of the profile, and the NDI
//! camera is checked by its TCP port. The built-in demo (start screen)
//! has all of them.

use pulse_core::model::CheckKind;
use pulse_net::found::Found;
use pulse_sim::{demo, Kind};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

fn main() -> ExitCode {
    let mut path = PathBuf::from("pulse-sim.pulse");
    let mut calm = false;
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--calm" => calm = true,
            "-h" | "--help" => {
                println!("pulse-sim [PROFILE] [--calm]");
                println!("Runs the demo's simulated devices on 127.0.0.1 and writes their profile (default pulse-sim.pulse).");
                return ExitCode::SUCCESS;
            }
            _ if a.starts_with('-') => {
                eprintln!("unknown option {a} (see --help)");
                return ExitCode::FAILURE;
            }
            _ => path = PathBuf::from(a),
        }
    }
    let (mut profile, sim) = match demo::start() {
        Ok(x) => x,
        Err(e) => {
            eprintln!("could not start the simulated devices: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Art-Net to where a Pulse on this computer listens.
    let found = Found {
        artnet_port: Some(pulse_core::artnet::PORT),
        sacn_port: Some(pulse_core::sacn::PORT),
        ..Found::default()
    };
    sim.attach(Arc::new(Mutex::new(found)));
    if !calm {
        let (steps, period) = demo::scenario();
        sim.scenario(steps, period);
    }

    profile.name = "Simulator: Riverside Hall".into();
    profile.notes = "Devices simulated by pulse-sim on this computer. Dante and Lantern announcements only show in the built-in demo.".into();
    let mut ndi = Vec::new();
    for d in &mut profile.devices {
        if let Some(s) = sim.device(&d.name) {
            if matches!(s.kind, Kind::Ndi { .. }) {
                ndi.push((d.id, s.port));
            }
        }
        d.checks
            .retain(|c| !matches!(c.kind, CheckKind::Mdns { .. } | CheckKind::Ndi { .. }));
    }
    for (id, port) in ndi {
        profile.add_check(id, CheckKind::Tcp { port });
    }
    if let Err(e) = pulse_core::profile::save(&profile, &path) {
        eprintln!("could not write {}: {e}", path.display());
        return ExitCode::FAILURE;
    }

    println!("Simulated devices on 127.0.0.1:");
    for d in &sim.devices {
        println!("  {:<18} port {}", d.name, d.port);
    }
    println!("Profile: {}", path.display());
    println!(
        "{}Press Ctrl-C to stop.",
        if calm {
            ""
        } else {
            "Now and then a stagebox, the light desk, Show Master or the backup's Lantern has a problem. "
        }
    );
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
