//! Asking a mixing console who it is: OSC `/xinfo` (X32/M32 on UDP 10023,
//! X Air/MR on 10024) and the Wing discovery ("WING?" on UDP 2222). The
//! consoles answer with their address, name, model and firmware. Nothing
//! is changed on the desk.

use pulse_core::model::ConsoleModel;
use pulse_core::osc;
use std::net::Ipv4Addr;

/// The question.
pub fn request(model: ConsoleModel) -> Vec<u8> {
    match model {
        ConsoleModel::X32 | ConsoleModel::XAir => osc::encode("/xinfo", &[]),
        ConsoleModel::Wing => b"WING?".to_vec(),
    }
}

/// What a desk says about itself.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub ip: Option<Ipv4Addr>,
    pub name: String,
    pub model: String,
    pub firmware: String,
}

impl Info {
    /// "X32 · firmware 4.06".
    pub fn detail(&self) -> String {
        match (self.model.is_empty(), self.firmware.is_empty()) {
            (false, false) => format!("{} · firmware {}", self.model, self.firmware),
            (false, true) => self.model.clone(),
            (true, false) => format!("firmware {}", self.firmware),
            (true, true) => String::new(),
        }
    }
}

/// The answer, if `b` is one.
pub fn parse(model: ConsoleModel, b: &[u8]) -> Option<Info> {
    match model {
        ConsoleModel::X32 | ConsoleModel::XAir => {
            let m = osc::decode(b)?;
            // The question has no arguments (a broadcast may come back to us).
            if m.address != "/xinfo" || m.args.is_empty() {
                return None;
            }
            let s = |i: usize| {
                m.args
                    .get(i)
                    .and_then(osc::Arg::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string()
            };
            Some(Info {
                ip: s(0).parse().ok(),
                name: s(1),
                model: s(2),
                firmware: s(3),
            })
        }
        ConsoleModel::Wing => {
            // "WING,192.168.1.62,PGM,ngc-full,S123,1.07.2"
            let t = std::str::from_utf8(b).ok()?.trim_end_matches('\0');
            let mut f = t.split(',');
            if f.next()? != "WING" {
                return None;
            }
            let ip = f.next().and_then(|s| s.trim().parse().ok());
            let name = f.next().unwrap_or("").trim().to_string();
            let model = f.next().unwrap_or("").trim().to_string();
            let _serial = f.next();
            let firmware = f.next().unwrap_or("").trim().to_string();
            Some(Info {
                ip,
                name,
                model: if model.is_empty() {
                    "Wing".into()
                } else {
                    format!("Wing {model}")
                },
                firmware,
            })
        }
    }
}

/// An answer as a desk sends it (for the simulator and tests).
pub fn answer(
    model: ConsoleModel,
    ip: Ipv4Addr,
    name: &str,
    kind: &str,
    firmware: &str,
) -> Vec<u8> {
    match model {
        ConsoleModel::X32 | ConsoleModel::XAir => osc::encode(
            "/xinfo",
            &[
                osc::Arg::Str(ip.to_string()),
                osc::Arg::Str(name.into()),
                osc::Arg::Str(kind.into()),
                osc::Arg::Str(firmware.into()),
            ],
        ),
        ConsoleModel::Wing => format!("WING,{ip},{name},{kind},0000,{firmware}").into_bytes(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x32_and_wing_answers_are_read() {
        let ip = Ipv4Addr::new(192, 168, 1, 20);
        let a = answer(ConsoleModel::X32, ip, "FOH", "X32", "4.06");
        let i = parse(ConsoleModel::X32, &a).unwrap();
        assert_eq!(i.ip, Some(ip));
        assert_eq!(i.name, "FOH");
        assert_eq!(i.detail(), "X32 · firmware 4.06");
        // The question itself is no answer.
        assert!(parse(ConsoleModel::X32, &request(ConsoleModel::X32)).is_none());
        assert!(parse(ConsoleModel::X32, &osc::encode("/status", &[])).is_none());

        let w = answer(ConsoleModel::Wing, ip, "PGM", "ngc-full", "1.07.2");
        let i = parse(ConsoleModel::Wing, &w).unwrap();
        assert_eq!(i.name, "PGM");
        assert_eq!(i.model, "Wing ngc-full");
        assert_eq!(i.firmware, "1.07.2");
        assert!(parse(ConsoleModel::Wing, b"WING?").is_none());
        assert!(parse(ConsoleModel::Wing, b"HELLO,1").is_none());
    }
}
