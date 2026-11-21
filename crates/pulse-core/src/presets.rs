//! Device presets: a kind of device with the checks that suit it, so a
//! console, a Dante stagebox or an NDI camera is set up with one click.
//! Consoles Pulse does not know get the basic checks (ping).

use crate::model::{
    CheckKind, ConsoleModel, Criticality, DeviceKind, Id, Profile, DANTE_SERVICE, LANTERN_SERVICE,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Preset {
    X32,
    XAir,
    Wing,
    /// Soundcraft Si Impact / Expression / Performer: the control port
    /// (HiQnet, TCP 3804) that ViSi Remote uses.
    SoundcraftSi,
    /// A console Pulse cannot ask: ping only.
    OtherConsole,
    DanteDevice,
    NdiSource,
    ArtNetNode,
    /// A lighting desk or media server that sends Art-Net.
    ArtNetSender,
    /// grandMA3 console or onPC sending Art-Net (it also answers ArtPoll).
    GrandMa3,
    /// A lighting desk or media server that sends sACN (E1.31).
    SacnSender,
    TabletWifi,
    ShowMaster,
    Lantern,
    Switch,
    /// A managed switch that answers SNMP.
    ManagedSwitch,
    AccessPoint,
    /// A show computer with Pulse Agent.
    AgentComputer,
    /// Anything else: ping.
    Generic,
}

impl Preset {
    pub const ALL: [Preset; 19] = [
        Preset::X32,
        Preset::XAir,
        Preset::Wing,
        Preset::SoundcraftSi,
        Preset::OtherConsole,
        Preset::DanteDevice,
        Preset::NdiSource,
        Preset::ArtNetNode,
        Preset::ArtNetSender,
        Preset::GrandMa3,
        Preset::SacnSender,
        Preset::TabletWifi,
        Preset::ShowMaster,
        Preset::Lantern,
        Preset::Switch,
        Preset::ManagedSwitch,
        Preset::AccessPoint,
        Preset::AgentComputer,
        Preset::Generic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Preset::X32 => "Behringer X32 / Midas M32",
            Preset::XAir => "Behringer X Air / Midas MR",
            Preset::Wing => "Behringer Wing",
            Preset::SoundcraftSi => "Soundcraft Si Impact / Expression / Performer",
            Preset::OtherConsole => "Other console (basic checks)",
            Preset::DanteDevice => "Dante device",
            Preset::NdiSource => "NDI source",
            Preset::ArtNetNode => "Art-Net node",
            Preset::ArtNetSender => "Lighting desk (Art-Net)",
            Preset::SacnSender => "Lighting desk (sACN)",
            Preset::GrandMa3 => "grandMA3 (Art-Net)",
            Preset::TabletWifi => "Tablet on Wi-Fi",
            Preset::ShowMaster => "Show Master computer",
            Preset::Lantern => "Lantern computer",
            Preset::Switch => "Network switch",
            Preset::ManagedSwitch => "Managed switch (SNMP)",
            Preset::AgentComputer => "Show computer with Pulse Agent",
            Preset::AccessPoint => "Wi-Fi access point",
            Preset::Generic => "Other device (ping)",
        }
    }

    /// What the preset checks, said plainly.
    pub fn hint(self) -> &'static str {
        match self {
            Preset::X32 | Preset::XAir => "Ping, and asks the desk who it is (OSC /xinfo).",
            Preset::Wing => "Ping, and asks the desk who it is (Wing discovery).",
            Preset::OtherConsole => "Ping only: Pulse cannot ask this desk anything.",
            Preset::SoundcraftSi => "Ping, and the control port (HiQnet, TCP 3804) that ViSi Remote connects to. Audio is not checked.",
            Preset::GrandMa3 => "Ping, the answer to ArtPoll, and the frames of universe 0 (at least 20 per second). Set the universe to the one the desk sends.",
            Preset::DanteDevice => {
                "Ping, and the Dante announcement (mDNS). Clock and audio are not checked."
            }
            Preset::NdiSource => "Ping, the NDI announcement and its port.",
            Preset::ArtNetNode => "Ping, and the answer to ArtPoll.",
            Preset::ArtNetSender => "Ping, and the frames of universe 0 (at least 20 per second).",
            Preset::SacnSender => "Ping, and the sACN frames of universe 1 (at least 20 per second).",
            Preset::ManagedSwitch => "Ping, and the switch answers SNMP (read-only, community \"public\"). Add a port to watch it.",
            Preset::AgentComputer => "Ping, and Pulse Agent on the computer: CPU, memory, free disk, programs that must run.",
            Preset::TabletWifi => "Ping with Wi-Fi limits (more latency allowed, loss matters).",
            Preset::ShowMaster => "Ping, and the Show Master web port 7400.",
            Preset::Lantern => "Ping, and the Lantern announcement (mDNS).",
            Preset::Switch | Preset::AccessPoint | Preset::Generic => "Ping.",
        }
    }

    pub fn kind(self) -> DeviceKind {
        match self {
            Preset::X32 | Preset::XAir | Preset::Wing | Preset::OtherConsole => DeviceKind::Console,
            Preset::DanteDevice => DeviceKind::Stagebox,
            Preset::NdiSource => DeviceKind::NdiSource,
            Preset::ArtNetNode => DeviceKind::ArtNetNode,
            Preset::ArtNetSender | Preset::SacnSender | Preset::GrandMa3 => DeviceKind::Console,
            Preset::SoundcraftSi => DeviceKind::Console,
            Preset::ManagedSwitch => DeviceKind::Switch,
            Preset::AgentComputer => DeviceKind::Computer,
            Preset::TabletWifi => DeviceKind::Tablet,
            Preset::ShowMaster | Preset::Lantern => DeviceKind::Computer,
            Preset::Switch => DeviceKind::Switch,
            Preset::AccessPoint => DeviceKind::AccessPoint,
            Preset::Generic => DeviceKind::Other,
        }
    }

    /// Name of a new device of this preset.
    pub fn default_name(self) -> &'static str {
        match self {
            Preset::X32 | Preset::XAir | Preset::Wing | Preset::OtherConsole => "Console",
            Preset::DanteDevice => "Dante device",
            Preset::NdiSource => "NDI source",
            Preset::ArtNetNode => "Art-Net node",
            Preset::ArtNetSender | Preset::SacnSender => "Lighting desk",
            Preset::GrandMa3 => "grandMA3",
            Preset::SoundcraftSi => "Si Impact",
            Preset::ManagedSwitch => "Switch",
            Preset::AgentComputer => "Show computer",
            Preset::TabletWifi => "Tablet",
            Preset::ShowMaster => "Show Master",
            Preset::Lantern => "Lantern",
            Preset::Switch => "Switch",
            Preset::AccessPoint => "Access point",
            Preset::Generic => "Device",
        }
    }

    /// The checks after the ping (which every preset has first).
    pub fn checks(self) -> Vec<CheckKind> {
        match self {
            Preset::X32 => vec![CheckKind::Console {
                model: ConsoleModel::X32,
                port: None,
            }],
            Preset::XAir => vec![CheckKind::Console {
                model: ConsoleModel::XAir,
                port: None,
            }],
            Preset::Wing => vec![CheckKind::Console {
                model: ConsoleModel::Wing,
                port: None,
            }],
            Preset::DanteDevice => vec![CheckKind::Mdns {
                service: DANTE_SERVICE.into(),
                instance: String::new(),
            }],
            Preset::NdiSource => vec![CheckKind::Ndi {
                source: String::new(),
            }],
            Preset::ArtNetNode => vec![CheckKind::ArtNetNode],
            Preset::ArtNetSender => vec![CheckKind::ArtNetUniverse {
                universe: 0,
                min_fps: 20.0,
            }],
            Preset::SoundcraftSi => vec![CheckKind::Tcp { port: 3804 }],
            Preset::GrandMa3 => vec![
                CheckKind::ArtNetNode,
                CheckKind::ArtNetUniverse {
                    universe: 0,
                    min_fps: 20.0,
                },
            ],
            Preset::SacnSender => vec![CheckKind::SacnUniverse {
                universe: 1,
                min_fps: 20.0,
            }],
            Preset::ManagedSwitch => vec![CheckKind::Snmp {
                community: "public".into(),
                if_index: 0,
                port: None,
            }],
            Preset::AgentComputer => vec![CheckKind::Agent {
                processes: Vec::new(),
                min_free_gb: 5.0,
                port: None,
            }],
            Preset::ShowMaster => vec![CheckKind::Tcp { port: 7400 }],
            Preset::Lantern => vec![CheckKind::Mdns {
                service: LANTERN_SERVICE.into(),
                instance: String::new(),
            }],
            Preset::OtherConsole
            | Preset::TabletWifi
            | Preset::Switch
            | Preset::AccessPoint
            | Preset::Generic => Vec::new(),
        }
    }

    pub fn criticality(self) -> Criticality {
        match self {
            Preset::X32
            | Preset::XAir
            | Preset::Wing
            | Preset::SoundcraftSi
            | Preset::OtherConsole
            | Preset::GrandMa3 => Criticality::Critical,
            Preset::Switch | Preset::ManagedSwitch => Criticality::Critical,
            _ => Criticality::Important,
        }
    }
}

impl Profile {
    /// A new device from a preset (ping first and critical, the rest not).
    pub fn add_preset(&mut self, preset: Preset, name: &str, address: &str) -> Id {
        let id = self.add_device(preset.kind(), name, address);
        for k in preset.checks() {
            self.add_check(id, k);
        }
        if let Some(d) = self.device_mut(id) {
            d.criticality = preset.criticality();
        }
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soundcraft_si_and_grandma3() {
        let mut p = Profile::default();
        let si = p.add_preset(Preset::SoundcraftSi, "Si Impact", "192.168.1.20");
        let d = p.device(si).cloned().unwrap_or_default();
        assert_eq!(d.kind, DeviceKind::Console);
        assert_eq!(d.criticality, Criticality::Critical);
        assert_eq!(d.checks[1].kind, CheckKind::Tcp { port: 3804 });
        let ma = p.add_preset(Preset::GrandMa3, "grandMA3", "2.0.0.10");
        let kinds: Vec<CheckKind> = p
            .device(ma)
            .map(|d| d.checks.iter().map(|c| c.kind.clone()).collect())
            .unwrap_or_default();
        assert_eq!(
            kinds,
            vec![
                CheckKind::Ping,
                CheckKind::ArtNetNode,
                CheckKind::ArtNetUniverse {
                    universe: 0,
                    min_fps: 20.0
                }
            ]
        );
    }

    #[test]
    fn presets_bring_their_checks() {
        let mut p = Profile::default();
        let x = p.add_preset(Preset::X32, "FOH X32", "10.0.0.1");
        let d = p.device(x).cloned().unwrap_or_default();
        assert_eq!(d.kind, DeviceKind::Console);
        assert_eq!(d.criticality, Criticality::Critical);
        assert_eq!(d.checks.len(), 2);
        assert_eq!(d.checks[0].kind, CheckKind::Ping);
        assert!(d.checks[0].critical);
        assert!(!d.checks[1].critical);
        assert_eq!(
            d.checks[1].kind,
            CheckKind::Console {
                model: ConsoleModel::X32,
                port: None
            }
        );
        let o = p.add_preset(Preset::OtherConsole, "Old desk", "10.0.0.2");
        assert_eq!(p.device(o).map(|d| d.checks.len()), Some(1));
        let t = p.add_preset(Preset::TabletWifi, "iPad", "10.0.0.50");
        assert_eq!(
            p.device(t).map(|d| d.checks[0].thresholds),
            Some(crate::model::Thresholds::wifi())
        );
        for pr in Preset::ALL {
            assert!(!pr.label().is_empty() && !pr.hint().is_empty());
        }
    }
}
