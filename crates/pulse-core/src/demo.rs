//! The demo profile: an invented venue whose devices sit on this computer
//! (127.0.0.1, so they answer) and two that never answer (TEST-NET,
//! 192.0.2.x), so the demo shows green, yellow and their reasons.
//! The simulator (phase 6) will give a livelier one.

use crate::model::{Criticality, DeviceKind, Profile};

pub fn profile() -> Profile {
    let mut p = Profile {
        name: "Demo: Riverside Hall".into(),
        venue: "Riverside Hall (invented)".into(),
        notes: "Most devices are this computer (127.0.0.1); the IEM rack and the projector never answer.".into(),
        ..Profile::default()
    };
    use Criticality::*;
    use DeviceKind::*;
    let devices: [(&str, DeviceKind, &str, &str, Criticality); 13] = [
        ("FOH desk", Console, "FOH", "127.0.0.1", Critical),
        ("Show Master Mac", Computer, "FOH", "127.0.0.1", Critical),
        ("iPad FOH", Tablet, "FOH", "127.0.0.1", Important),
        ("Stagebox SL", Stagebox, "Stage", "127.0.0.1", Critical),
        ("Stagebox SR", Stagebox, "Stage", "127.0.0.1", Critical),
        ("IEM rack", Other, "Stage", "192.0.2.20", Important),
        ("Camera 1 (NDI)", NdiSource, "Video", "127.0.0.1", Important),
        ("Projector", Other, "Video", "192.0.2.30", Optional),
        ("Light desk", Console, "Light", "127.0.0.1", Critical),
        (
            "Art-Net node DS",
            ArtNetNode,
            "Light",
            "127.0.0.1",
            Important,
        ),
        ("Core switch", Switch, "Network", "127.0.0.1", Critical),
        (
            "Stage Wi-Fi",
            AccessPoint,
            "Network",
            "127.0.0.1",
            Important,
        ),
        (
            "Backup playback",
            Computer,
            "Backup",
            "127.0.0.1",
            Important,
        ),
    ];
    for (name, kind, area, address, crit) in devices {
        let id = p.add_device(kind, name, address);
        if let Some(d) = p.device_mut(id) {
            d.area = area.into();
            d.criticality = crit;
        }
    }
    p
}

#[cfg(test)]
mod tests {
    #[test]
    fn demo_has_every_area_and_unique_ids() {
        let p = super::profile();
        assert_eq!(
            p.areas(),
            vec!["FOH", "Stage", "Video", "Light", "Network", "Backup"]
        );
        let mut ids: Vec<_> = p
            .devices
            .iter()
            .flat_map(|d| std::iter::once(d.id).chain(d.checks.iter().map(|c| c.id)))
            .collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }
}
