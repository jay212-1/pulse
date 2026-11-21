//! Import from the Showtime exchange file (`<name>.showtime.json`) that
//! Blueprint writes: its devices become Pulse devices, with a preset from
//! their kind and model, the location as the area, and their address.
//! Only what Pulse needs is read; everything else in the file is ignored.

use crate::model::{Criticality, Profile};
use crate::presets::Preset;
use serde::Deserialize;

pub const FORMAT: &str = "showtime-exchange";
/// The newest version Pulse knows.
pub const VERSION: u32 = 1;
pub const EXTENSION: &str = "showtime.json";

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Exchange {
    pub format: String,
    pub version: u32,
    pub generator: String,
    pub production: Production,
    pub devices: Vec<ExDevice>,
}

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Production {
    pub name: String,
    pub venue: String,
}

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct ExDevice {
    pub name: String,
    pub kind: String,
    pub location: String,
    pub model: String,
    pub address: String,
    pub notes: String,
}

/// Read an exchange file, refusing other files and newer versions.
pub fn read(json: &str) -> Result<Exchange, String> {
    let x: Exchange =
        serde_json::from_str(json).map_err(|e| format!("not a Showtime exchange file: {e}"))?;
    if x.format != FORMAT {
        return Err("not a Showtime exchange file".into());
    }
    if x.version > VERSION {
        return Err(format!(
            "written by a newer program (exchange version {}); update Pulse",
            x.version
        ));
    }
    Ok(x)
}

/// The preset for a device of `kind` and `model`.
pub fn preset_for(kind: &str, model: &str) -> Preset {
    let m = model.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| m.contains(w));
    if has(&["x32", "m32"]) {
        return Preset::X32;
    }
    if has(&["x air", "xair", "xr12", "xr16", "xr18", "mr12", "mr18"]) {
        return Preset::XAir;
    }
    if has(&["wing"]) {
        return Preset::Wing;
    }
    if has(&["si impact", "si expression", "si performer", "si compact"]) {
        return Preset::SoundcraftSi;
    }
    if has(&["grandma3", "grand ma3", "ma3"]) {
        return Preset::GrandMa3;
    }
    if has(&["dante", "rio", "tio", "ri8", "ro8", "ri16"]) {
        return Preset::DanteDevice;
    }
    match kind {
        "console" => Preset::OtherConsole,
        _ => Preset::Generic,
    }
}

/// What an import did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Imported {
    pub added: usize,
    /// Already there by name: address, area or model filled in where empty.
    pub updated: usize,
    /// Added without an address: switched off until one is typed in.
    pub without_address: usize,
}

impl Imported {
    pub fn summary(&self) -> String {
        let mut parts = vec![format!(
            "{} device{} added",
            self.added,
            if self.added == 1 { "" } else { "s" }
        )];
        if self.updated > 0 {
            parts.push(format!("{} updated", self.updated));
        }
        if self.without_address > 0 {
            parts.push(format!(
                "{} without an address (switched off)",
                self.without_address
            ));
        }
        parts.join(", ")
    }
}

impl Profile {
    /// Take over the devices of an exchange file. Devices already in the
    /// profile (same name) only get what they lack; nothing is removed.
    pub fn import_exchange(&mut self, x: &Exchange) -> Imported {
        let mut out = Imported::default();
        if self.venue.is_empty() {
            self.venue = x.production.venue.trim().to_string();
        }
        for d in &x.devices {
            let name = d.name.trim();
            if name.is_empty() {
                continue;
            }
            let address = d.address.trim();
            if let Some(have) = self
                .devices
                .iter_mut()
                .find(|h| h.name.trim().eq_ignore_ascii_case(name))
            {
                let mut changed = false;
                if have.address.trim().is_empty() && !address.is_empty() {
                    have.address = address.into();
                    have.enabled = true;
                    changed = true;
                }
                if have.area.trim().is_empty() && !d.location.trim().is_empty() {
                    have.area = d.location.trim().into();
                    changed = true;
                }
                if changed {
                    out.updated += 1;
                }
                continue;
            }
            let preset = preset_for(d.kind.trim(), &d.model);
            let id = self.add_preset(preset, name, address);
            if let Some(dev) = self.device_mut(id) {
                dev.area = d.location.trim().into();
                let mut notes = Vec::new();
                if !d.model.trim().is_empty() {
                    notes.push(d.model.trim().to_string());
                }
                if !d.notes.trim().is_empty() {
                    notes.push(d.notes.trim().to_string());
                }
                notes.push("from Blueprint".into());
                dev.notes = notes.join(" · ");
                // An amp or a recorder going away is rarely a show stopper.
                if matches!(d.kind.as_str(), "amp" | "recorder" | "other") {
                    dev.criticality = Criticality::Important;
                }
                if address.is_empty() {
                    dev.enabled = false;
                    out.without_address += 1;
                }
            }
            out.added += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CheckKind;

    const FILE: &str = r#"{
      "format": "showtime-exchange", "version": 1, "generator": "Blueprint 0.2.4",
      "production": { "name": "Harbour", "venue": "Riverside Hall" },
      "inputs": [{ "channel": 1, "name": "Kick" }],
      "devices": [
        { "name": "FOH", "kind": "console", "location": "FOH", "model": "Soundcraft Si Impact", "address": "192.168.1.10" },
        { "name": "Light", "kind": "console", "location": "FOH", "model": "grandMA3 light", "address": "2.0.0.10" },
        { "name": "SB1", "kind": "stagebox", "location": "Stage left", "model": "Yamaha Rio1608", "address": "" },
        { "name": "Amp rack", "kind": "amp", "location": "Stage", "model": "", "address": "amps.local" }
      ]
    }"#;

    #[test]
    fn reads_and_refuses() {
        let x = read(FILE).unwrap();
        assert_eq!(x.devices.len(), 4);
        assert!(read(r#"{"format":"other"}"#).is_err());
        assert!(read(r#"{"format":"showtime-exchange","version":2}"#)
            .unwrap_err()
            .contains("newer"));
        assert!(read("nope").is_err());
    }

    #[test]
    fn presets_from_kind_and_model() {
        assert_eq!(preset_for("console", "Behringer X32 Compact"), Preset::X32);
        assert_eq!(preset_for("console", "Midas MR18"), Preset::XAir);
        assert_eq!(
            preset_for("console", "Soundcraft Si Impact"),
            Preset::SoundcraftSi
        );
        assert_eq!(
            preset_for("console", "grandMA3 full-size"),
            Preset::GrandMa3
        );
        assert_eq!(preset_for("console", "Yamaha CL5"), Preset::OtherConsole);
        assert_eq!(preset_for("stagebox", "Rio3224-D2"), Preset::DanteDevice);
        assert_eq!(preset_for("stagebox", "DL32"), Preset::Generic);
    }

    #[test]
    fn imports_devices_and_fills_in_known_ones() {
        let x = read(FILE).unwrap();
        let mut p = Profile::default();
        let have = p.add_device(crate::model::DeviceKind::Console, "light", "");
        let r = p.import_exchange(&x);
        assert_eq!(
            r,
            Imported {
                added: 3,
                updated: 1,
                without_address: 1
            }
        );
        assert_eq!(
            r.summary(),
            "3 devices added, 1 updated, 1 without an address (switched off)"
        );
        assert_eq!(p.venue, "Riverside Hall");
        let foh = p.devices.iter().find(|d| d.name == "FOH").unwrap();
        assert_eq!(foh.address, "192.168.1.10");
        assert_eq!(foh.area, "FOH");
        assert!(foh
            .checks
            .iter()
            .any(|c| c.kind == CheckKind::Tcp { port: 3804 }));
        assert!(foh.notes.contains("from Blueprint"));
        let sb = p.devices.iter().find(|d| d.name == "SB1").unwrap();
        assert!(!sb.enabled);
        let light = p.device(have).unwrap();
        assert_eq!(
            (light.address.as_str(), light.area.as_str()),
            ("2.0.0.10", "FOH")
        );
        // Importing again changes nothing.
        assert_eq!(p.import_exchange(&x), Imported::default());
        assert_eq!(p.devices.len(), 4);
    }
}
