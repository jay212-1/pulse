//! The inventory: devices, their checks and thresholds, and the profile
//! (a venue or show) that holds them. Everything refers to stable `u64` ids.

use serde::{Deserialize, Serialize};

pub type Id = u64;

/// Version of the profile file; raise it with a migration in `profile.rs`.
pub const SCHEMA_VERSION: u32 = 1;

/// The usual areas; a device may also name an area of its own.
pub const AREAS: [&str; 6] = ["FOH", "Stage", "Video", "Light", "Network", "Backup"];

#[derive(
    Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
)]
pub enum Criticality {
    /// Down -> the overall light turns red.
    Critical,
    /// Down -> yellow.
    #[default]
    Important,
    /// At most yellow, or not at all (see [`OptionalEffect`]).
    Optional,
}

impl Criticality {
    pub const ALL: [Criticality; 3] = [
        Criticality::Critical,
        Criticality::Important,
        Criticality::Optional,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Criticality::Critical => "Critical",
            Criticality::Important => "Important",
            Criticality::Optional => "Optional",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum DeviceKind {
    Console,
    Stagebox,
    NdiSource,
    ArtNetNode,
    Tablet,
    Computer,
    Switch,
    Router,
    AccessPoint,
    #[default]
    Other,
}

impl DeviceKind {
    pub const ALL: [DeviceKind; 10] = [
        DeviceKind::Console,
        DeviceKind::Stagebox,
        DeviceKind::NdiSource,
        DeviceKind::ArtNetNode,
        DeviceKind::Tablet,
        DeviceKind::Computer,
        DeviceKind::Switch,
        DeviceKind::Router,
        DeviceKind::AccessPoint,
        DeviceKind::Other,
    ];
    pub fn label(self) -> &'static str {
        match self {
            DeviceKind::Console => "Console",
            DeviceKind::Stagebox => "Stagebox",
            DeviceKind::NdiSource => "NDI source",
            DeviceKind::ArtNetNode => "Art-Net node",
            DeviceKind::Tablet => "Tablet",
            DeviceKind::Computer => "Computer",
            DeviceKind::Switch => "Switch",
            DeviceKind::Router => "Router",
            DeviceKind::AccessPoint => "Access point",
            DeviceKind::Other => "Other",
        }
    }
    /// Area a new device of this kind goes to.
    pub fn default_area(self) -> &'static str {
        match self {
            DeviceKind::Console => "FOH",
            DeviceKind::Stagebox => "Stage",
            DeviceKind::NdiSource => "Video",
            DeviceKind::ArtNetNode => "Light",
            DeviceKind::Tablet => "FOH",
            DeviceKind::Computer => "Backup",
            DeviceKind::Switch | DeviceKind::Router | DeviceKind::AccessPoint => "Network",
            DeviceKind::Other => "Stage",
        }
    }
    /// Thresholds that suit the device's link (tablets are on Wi-Fi).
    pub fn default_thresholds(self) -> Thresholds {
        match self {
            DeviceKind::Tablet => Thresholds::wifi(),
            _ => Thresholds::wired(),
        }
    }
}

/// What a check does. New probes are new variants (older files stay valid).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type")]
pub enum CheckKind {
    /// ICMP echo (or the TCP fallback): latency, jitter, loss.
    Ping,
    /// Can a TCP connection to the port be opened (and is closed again at once)?
    Tcp { port: u16 },
    /// Ask a mixing console who it is (OSC `/xinfo`, Wing discovery).
    /// `port`: another UDP port than the model's (simulator, port forwarding).
    Console {
        model: ConsoleModel,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<u16>,
    },
    /// The device answers ArtPoll with an ArtPollReply.
    ArtNetNode,
    /// DMX frames of a universe arrive, at least `min_fps` per second.
    ArtNetUniverse { universe: u16, min_fps: f64 },
    /// The device announces an mDNS service (`_netaudio-arc._udp` for Dante).
    /// An empty instance matches any instance at the device's address.
    Mdns { service: String, instance: String },
    /// An NDI source is announced and its port accepts connections. An empty
    /// source matches any source at the device's address.
    Ndi { source: String },
    /// A network interface of this computer is up and has an address.
    Interface { name: String },
    /// sACN (E1.31) frames of a universe arrive, at least `min_fps` per second.
    SacnUniverse { universe: u16, min_fps: f64 },
    /// A managed switch answers SNMP v2c (read-only); with `if_index` > 0
    /// also that port is up and gets no new errors.
    Snmp {
        community: String,
        #[serde(default)]
        if_index: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<u16>,
    },
    /// Pulse Agent on a show computer answers: CPU, memory, disk, and the
    /// named programs run.
    Agent {
        #[serde(default)]
        processes: Vec<String>,
        #[serde(default = "default_min_free_gb")]
        min_free_gb: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<u16>,
    },
}

fn default_min_free_gb() -> f64 {
    5.0
}

impl CheckKind {
    pub fn label(&self) -> String {
        match self {
            CheckKind::Ping => "Ping".into(),
            CheckKind::Tcp { port } => format!("TCP {port}"),
            CheckKind::Console { model, .. } => format!("{} query", model.label()),
            CheckKind::ArtNetNode => "Art-Net node".into(),
            CheckKind::ArtNetUniverse { universe, .. } => format!("Universe {universe}"),
            CheckKind::Mdns { service, .. } => format!("mDNS {}", service_short(service)),
            CheckKind::Ndi { source } if source.is_empty() => "NDI source".into(),
            CheckKind::Ndi { source } => format!("NDI {source}"),
            CheckKind::Interface { name } => format!("Interface {name}"),
            CheckKind::SacnUniverse { universe, .. } => format!("sACN {universe}"),
            CheckKind::Snmp { if_index: 0, .. } => "SNMP".into(),
            CheckKind::Snmp { if_index, .. } => format!("SNMP port {if_index}"),
            CheckKind::Agent { .. } => "Agent".into(),
        }
    }
    /// Seconds between two runs in monitoring.
    pub fn default_interval(&self) -> f64 {
        match self {
            CheckKind::Ping => 2.0,
            CheckKind::ArtNetUniverse { .. } | CheckKind::SacnUniverse { .. } => 1.0,
            CheckKind::Interface { .. } => 2.0,
            _ => 5.0,
        }
    }
    pub fn default_timeout_ms(&self) -> u32 {
        match self {
            CheckKind::Ping => 1000,
            _ => 1500,
        }
    }
    /// The check reads what the listeners found (it sends nothing itself).
    pub fn passive(&self) -> bool {
        matches!(
            self,
            CheckKind::ArtNetNode
                | CheckKind::ArtNetUniverse { .. }
                | CheckKind::SacnUniverse { .. }
                | CheckKind::Mdns { .. }
                | CheckKind::Interface { .. }
        )
    }
    /// Other kinds a new check of a device can be (for the inventory).
    pub fn templates() -> Vec<CheckKind> {
        vec![
            CheckKind::Ping,
            CheckKind::Tcp { port: 80 },
            CheckKind::Console {
                model: ConsoleModel::X32,
                port: None,
            },
            CheckKind::ArtNetNode,
            CheckKind::ArtNetUniverse {
                universe: 0,
                min_fps: 20.0,
            },
            CheckKind::Mdns {
                service: DANTE_SERVICE.into(),
                instance: String::new(),
            },
            CheckKind::Ndi {
                source: String::new(),
            },
            CheckKind::Interface {
                name: String::new(),
            },
            CheckKind::SacnUniverse {
                universe: 1,
                min_fps: 20.0,
            },
            CheckKind::Snmp {
                community: "public".into(),
                if_index: 0,
                port: None,
            },
            CheckKind::Agent {
                processes: Vec::new(),
                min_free_gb: 5.0,
                port: None,
            },
        ]
    }
    /// A short name of the kind, without its settings ("TCP port").
    pub fn kind_label(&self) -> &'static str {
        match self {
            CheckKind::Ping => "Ping",
            CheckKind::Tcp { .. } => "TCP port",
            CheckKind::Console { .. } => "Console query",
            CheckKind::ArtNetNode => "Art-Net node",
            CheckKind::ArtNetUniverse { .. } => "Art-Net universe",
            CheckKind::Mdns { .. } => "mDNS service",
            CheckKind::Ndi { .. } => "NDI source",
            CheckKind::Interface { .. } => "Interface",
            CheckKind::SacnUniverse { .. } => "sACN universe",
            CheckKind::Snmp { .. } => "SNMP switch",
            CheckKind::Agent { .. } => "Pulse Agent",
        }
    }
}

/// mDNS service of Dante devices.
pub const DANTE_SERVICE: &str = "_netaudio-arc._udp";
/// mDNS service of NDI sources.
pub const NDI_SERVICE: &str = "_ndi._tcp";
/// mDNS service of Lantern.
pub const LANTERN_SERVICE: &str = "_lantern._udp";
/// mDNS service of web servers (Show Master announces itself so).
pub const HTTP_SERVICE: &str = "_http._tcp";

/// "_netaudio-arc._udp.local." -> "_netaudio-arc._udp".
pub fn service_short(s: &str) -> &str {
    let s = s.trim().trim_end_matches('.');
    s.strip_suffix(".local").unwrap_or(s)
}

/// Consoles Pulse can ask who they are. Others get the basic checks.
#[derive(
    Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
)]
pub enum ConsoleModel {
    /// Behringer X32 and Midas M32: OSC `/xinfo` on UDP 10023.
    #[default]
    X32,
    /// Behringer X Air and Midas MR: OSC `/xinfo` on UDP 10024.
    XAir,
    /// Behringer Wing: "WING?" on UDP 2222.
    Wing,
}

impl ConsoleModel {
    pub const ALL: [ConsoleModel; 3] = [ConsoleModel::X32, ConsoleModel::XAir, ConsoleModel::Wing];
    pub fn label(self) -> &'static str {
        match self {
            ConsoleModel::X32 => "X32/M32",
            ConsoleModel::XAir => "X Air",
            ConsoleModel::Wing => "Wing",
        }
    }
    pub fn port(self) -> u16 {
        match self {
            ConsoleModel::X32 => 10023,
            ConsoleModel::XAir => 10024,
            ConsoleModel::Wing => 2222,
        }
    }
}

/// When a result counts as degraded (yellow).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default)]
pub struct Thresholds {
    pub latency_ms: f64,
    pub jitter_ms: f64,
    /// Loss over the rolling window, percent.
    pub loss_pct: f64,
}

impl Thresholds {
    pub fn wired() -> Self {
        Thresholds {
            latency_ms: 20.0,
            jitter_ms: 10.0,
            loss_pct: 5.0,
        }
    }
    /// Wi-Fi: more latency is normal, loss and jitter matter.
    pub fn wifi() -> Self {
        Thresholds {
            latency_ms: 150.0,
            jitter_ms: 50.0,
            loss_pct: 15.0,
        }
    }
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds::wired()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Check {
    pub id: Id,
    pub kind: CheckKind,
    pub enabled: bool,
    /// Down here means the device is down (otherwise only degraded, as long
    /// as other checks still pass).
    pub critical: bool,
    /// Seconds between runs; `None` = the kind's default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u32>,
    pub thresholds: Thresholds,
}

impl Default for Check {
    fn default() -> Self {
        Check {
            id: 0,
            kind: CheckKind::Ping,
            enabled: true,
            critical: true,
            interval_s: None,
            timeout_ms: None,
            thresholds: Thresholds::wired(),
        }
    }
}

impl Check {
    pub fn interval(&self) -> f64 {
        self.interval_s
            .unwrap_or_else(|| self.kind.default_interval())
            .clamp(0.2, 3600.0)
    }
    pub fn timeout_ms(&self) -> u32 {
        self.timeout_ms
            .unwrap_or_else(|| self.kind.default_timeout_ms())
            .clamp(50, 30_000)
    }
}

/// Debounce: red only after `fail_after` failures in a row, green again only
/// after `ok_after` passes in a row.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct Hysteresis {
    pub fail_after: u32,
    pub ok_after: u32,
}

impl Default for Hysteresis {
    fn default() -> Self {
        Hysteresis {
            fail_after: 3,
            ok_after: 2,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Device {
    pub id: Id,
    pub name: String,
    pub kind: DeviceKind,
    pub area: String,
    /// IP address or host name.
    pub address: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mac: String,
    /// Network interface to use (name); empty = the system's choice.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub interface: String,
    pub criticality: Criticality,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub notes: String,
    pub enabled: bool,
    pub hysteresis: Hysteresis,
    pub checks: Vec<Check>,
}

impl Default for Device {
    fn default() -> Self {
        Device {
            id: 0,
            name: String::new(),
            kind: DeviceKind::Other,
            area: "Stage".into(),
            address: String::new(),
            mac: String::new(),
            interface: String::new(),
            criticality: Criticality::Important,
            notes: String::new(),
            enabled: true,
            hysteresis: Hysteresis::default(),
            checks: Vec::new(),
        }
    }
}

impl Device {
    pub fn check(&self, id: Id) -> Option<&Check> {
        self.checks.iter().find(|c| c.id == id)
    }
}

/// How optional devices colour the area and overall lights.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OptionalEffect {
    /// Down or degraded -> at most yellow.
    #[default]
    UpToWarn,
    /// They show on their tile only.
    Ignore,
}

/// Flapping: this many changes between good and bad within the window make
/// the device yellow ("flapping") until it settles.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct FlapRule {
    pub changes: u32,
    pub window_s: u32,
}

impl Default for FlapRule {
    fn default() -> Self {
        FlapRule {
            changes: 4,
            window_s: 300,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Rules {
    pub optional: OptionalEffect,
    pub flapping: FlapRule,
}

/// A venue or show: the devices Pulse watches there.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Profile {
    pub schema_version: u32,
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub venue: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub notes: String,
    pub rules: Rules,
    pub devices: Vec<Device>,
    pub next_id: Id,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            schema_version: SCHEMA_VERSION,
            name: String::new(),
            venue: String::new(),
            notes: String::new(),
            rules: Rules::default(),
            devices: Vec::new(),
            next_id: 1,
        }
    }
}

impl Profile {
    pub fn alloc_id(&mut self) -> Id {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    pub fn device(&self, id: Id) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn device_mut(&mut self, id: Id) -> Option<&mut Device> {
        self.devices.iter_mut().find(|d| d.id == id)
    }

    /// A new device of `kind` with the checks that suit it, added at the end.
    pub fn add_device(&mut self, kind: DeviceKind, name: &str, address: &str) -> Id {
        let id = self.alloc_id();
        let mut d = Device {
            id,
            name: name.to_string(),
            kind,
            area: kind.default_area().to_string(),
            address: address.to_string(),
            ..Device::default()
        };
        let check = self.alloc_id();
        d.checks.push(Check {
            id: check,
            kind: CheckKind::Ping,
            thresholds: kind.default_thresholds(),
            ..Check::default()
        });
        self.devices.push(d);
        id
    }

    /// Add a check to a device; returns its id.
    pub fn add_check(&mut self, device: Id, kind: CheckKind) -> Option<Id> {
        let id = self.alloc_id();
        let d = self.device_mut(device)?;
        let critical = d.checks.is_empty();
        let thresholds = d.kind.default_thresholds();
        d.checks.push(Check {
            id,
            kind,
            critical,
            thresholds,
            ..Check::default()
        });
        Some(id)
    }

    /// A copy of a device (new ids, " copy" appended), right after it.
    pub fn duplicate(&mut self, device: Id) -> Option<Id> {
        let pos = self.devices.iter().position(|d| d.id == device)?;
        let mut copy = self.devices[pos].clone();
        copy.id = self.alloc_id();
        copy.name = format!("{} copy", copy.name);
        for c in &mut copy.checks {
            c.id = self.alloc_id();
        }
        let id = copy.id;
        self.devices.insert(pos + 1, copy);
        Some(id)
    }

    /// Areas in use: the usual ones first, then the profile's own, in order of appearance.
    pub fn areas(&self) -> Vec<String> {
        let mut out: Vec<String> = AREAS
            .iter()
            .filter(|a| self.devices.iter().any(|d| d.area == **a))
            .map(|a| a.to_string())
            .collect();
        for d in &self.devices {
            if !out.contains(&d.area) {
                out.push(d.area.clone());
            }
        }
        out
    }

    /// Highest id in use (to repair `next_id` after hand edits).
    pub fn max_id(&self) -> Id {
        self.devices
            .iter()
            .flat_map(|d| std::iter::once(d.id).chain(d.checks.iter().map(|c| c.id)))
            .max()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_devices_get_a_ping_check_that_suits_them() {
        let mut p = Profile::default();
        let tab = p.add_device(DeviceKind::Tablet, "iPad FOH", "10.0.0.50");
        let d = p.device(tab).cloned().unwrap_or_default();
        assert_eq!(d.area, "FOH");
        assert_eq!(d.checks.len(), 1);
        assert_eq!(d.checks[0].kind, CheckKind::Ping);
        assert!(d.checks[0].critical);
        assert_eq!(d.checks[0].thresholds, Thresholds::wifi());
        assert_ne!(d.checks[0].id, d.id);
        let tcp = p.add_check(tab, CheckKind::Tcp { port: 7400 });
        let d = p.device(tab).cloned().unwrap_or_default();
        assert!(!d
            .check(tcp.unwrap_or(0))
            .map(|c| c.critical)
            .unwrap_or(true));
    }

    #[test]
    fn duplicate_gets_new_ids_and_sits_after_the_original() {
        let mut p = Profile::default();
        let a = p.add_device(DeviceKind::Stagebox, "SB1", "10.0.0.11");
        p.add_device(DeviceKind::Console, "FOH", "10.0.0.1");
        let b = p.duplicate(a).unwrap_or(0);
        assert_eq!(p.devices[1].id, b);
        assert_eq!(p.devices[1].name, "SB1 copy");
        assert_ne!(p.devices[1].checks[0].id, p.devices[0].checks[0].id);
        assert!(p.next_id > p.max_id());
    }

    #[test]
    fn areas_keep_the_usual_order_and_own_ones_after() {
        let mut p = Profile::default();
        let a = p.add_device(DeviceKind::Switch, "Core", "10.0.0.254");
        p.add_device(DeviceKind::Console, "FOH", "10.0.0.1");
        if let Some(d) = p.device_mut(a) {
            d.area = "Balcony".into();
        }
        p.add_device(DeviceKind::Stagebox, "SB", "10.0.0.2");
        assert_eq!(p.areas(), vec!["FOH", "Stage", "Balcony"]);
    }

    #[test]
    fn intervals_and_timeouts_stay_sane() {
        let c = Check {
            interval_s: Some(0.0),
            timeout_ms: Some(1),
            ..Check::default()
        };
        assert_eq!(c.interval(), 0.2);
        assert_eq!(c.timeout_ms(), 50);
    }
}
