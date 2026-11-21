//! The demo venue "Riverside Hall" (invented) on the simulator: a desk that
//! answers its query, Dante stageboxes and an NDI camera that announce
//! themselves, a lighting desk sending a universe to an Art-Net node,
//! Show Master and Lantern; two devices that never answer (TEST-NET). Its
//! scenario lets something go wrong now and then.

use crate::{Action, Kind, Sim, Spec, Step};
use pulse_core::model::{
    CheckKind, ConsoleModel, Criticality, Profile, DANTE_SERVICE, LANTERN_SERVICE,
};
use pulse_core::Preset;
use std::time::Duration;

/// The simulated devices of the demo.
pub fn specs() -> Vec<Spec> {
    vec![
        Spec::new(
            "FOH desk",
            Kind::Console {
                model: ConsoleModel::X32,
                name: "FOH X32".into(),
                firmware: "4.06".into(),
            },
        ),
        Spec::new("Show Master Mac", Kind::Tcp),
        Spec::new(
            "Stagebox SL",
            Kind::Announce {
                service: DANTE_SERVICE.into(),
                instance: "SB-SL".into(),
            },
        ),
        Spec::new(
            "Stagebox SR",
            Kind::Announce {
                service: DANTE_SERVICE.into(),
                instance: "SB-SR".into(),
            },
        ),
        Spec::new(
            "Camera 1 (NDI)",
            Kind::Ndi {
                source: "CAM1 (Camera 1)".into(),
            },
        ),
        Spec::new(
            "Light desk",
            Kind::ArtNetSender {
                universe: 1,
                fps: 40.0,
            },
        ),
        Spec::new(
            "Art-Net node DS",
            Kind::ArtNetNode {
                short_name: "Node DS".into(),
                outputs: vec![1],
            },
        ),
        Spec::new(
            "Backup playback",
            Kind::Announce {
                service: LANTERN_SERVICE.into(),
                instance: "Lantern backup".into(),
            },
        ),
    ]
}

/// What goes wrong in the demo, every three minutes.
pub fn scenario() -> (Vec<Step>, Duration) {
    let s = |at: u64, device: &str, action: Action| Step {
        at: Duration::from_secs(at),
        device: device.into(),
        action,
    };
    (
        vec![
            s(40, "Stagebox SR", Action::Down(true)),
            s(65, "Stagebox SR", Action::Down(false)),
            s(85, "Light desk", Action::Fps(Some(8.0))),
            s(105, "Light desk", Action::Fps(None)),
            s(125, "Show Master Mac", Action::Closed(true)),
            s(150, "Show Master Mac", Action::Closed(false)),
        ],
        Duration::from_secs(180),
    )
}

/// The demo profile for a running simulator (its ports).
pub fn profile(sim: &Sim) -> Profile {
    let mut p = Profile {
        name: "Demo: Riverside Hall".into(),
        venue: "Riverside Hall (invented)".into(),
        notes: "Simulated devices on this computer. The IEM rack and the projector never answer; now and then a stagebox, the light desk or Show Master has a problem for a while.".into(),
        ..Profile::default()
    };
    let port = |n: &str| sim.device(n).map(|d| d.port).unwrap_or(0);
    let lo = "127.0.0.1";
    let add = |p: &mut Profile,
               preset: Preset,
               name: &str,
               address: &str,
               area: &str,
               crit: Criticality| {
        let id = p.add_preset(preset, name, address);
        if let Some(d) = p.device_mut(id) {
            d.area = area.into();
            d.criticality = crit;
        }
        id
    };
    use Criticality::*;
    let foh = add(&mut p, Preset::X32, "FOH desk", lo, "FOH", Critical);
    if let Some(d) = p.device_mut(foh) {
        for c in &mut d.checks {
            if let CheckKind::Console { port: pt, .. } = &mut c.kind {
                *pt = Some(port("FOH desk"));
            }
        }
    }
    let sm = add(
        &mut p,
        Preset::ShowMaster,
        "Show Master Mac",
        lo,
        "FOH",
        Critical,
    );
    if let Some(d) = p.device_mut(sm) {
        for c in &mut d.checks {
            if let CheckKind::Tcp { port: pt } = &mut c.kind {
                *pt = port("Show Master Mac");
                // Show Master not answering is as bad as the computer being gone.
                c.critical = true;
            }
        }
    }
    add(&mut p, Preset::TabletWifi, "iPad FOH", lo, "FOH", Important);
    for (name, instance) in [("Stagebox SL", "SB-SL"), ("Stagebox SR", "SB-SR")] {
        let id = add(&mut p, Preset::DanteDevice, name, lo, "Stage", Critical);
        if let Some(d) = p.device_mut(id) {
            for c in &mut d.checks {
                if let CheckKind::Mdns { instance: i, .. } = &mut c.kind {
                    *i = instance.into();
                }
            }
        }
    }
    add(
        &mut p,
        Preset::Generic,
        "IEM rack",
        "192.0.2.20",
        "Stage",
        Important,
    );
    let cam = add(
        &mut p,
        Preset::NdiSource,
        "Camera 1 (NDI)",
        lo,
        "Video",
        Important,
    );
    if let Some(d) = p.device_mut(cam) {
        for c in &mut d.checks {
            if let CheckKind::Ndi { source } = &mut c.kind {
                *source = "CAM1 (Camera 1)".into();
            }
        }
    }
    add(
        &mut p,
        Preset::Generic,
        "Projector",
        "192.0.2.30",
        "Video",
        Optional,
    );
    let desk = add(
        &mut p,
        Preset::ArtNetSender,
        "Light desk",
        lo,
        "Light",
        Critical,
    );
    if let Some(d) = p.device_mut(desk) {
        for c in &mut d.checks {
            if let CheckKind::ArtNetUniverse { universe, .. } = &mut c.kind {
                *universe = 1;
            }
        }
    }
    add(
        &mut p,
        Preset::ArtNetNode,
        "Art-Net node DS",
        lo,
        "Light",
        Important,
    );
    add(
        &mut p,
        Preset::Switch,
        "Core switch",
        lo,
        "Network",
        Critical,
    );
    add(
        &mut p,
        Preset::AccessPoint,
        "Stage Wi-Fi",
        lo,
        "Network",
        Important,
    );
    add(
        &mut p,
        Preset::Lantern,
        "Backup playback",
        lo,
        "Backup",
        Important,
    );
    p
}

/// Start the demo's simulator and its profile.
pub fn start() -> std::io::Result<(Profile, Sim)> {
    let sim = Sim::start(specs())?;
    let p = profile(&sim);
    Ok((p, sim))
}
