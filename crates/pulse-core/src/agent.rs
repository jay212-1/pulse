//! Pulse Agent: a small program on show computers (playback, backup, FOH
//! Mac) that tells Pulse, when asked, that the computer is alive and how it
//! is doing: CPU, memory, free disk space and whether named programs run.
//! Pulse asks with `GET /status?process=...` on TCP 7471; the agent sends
//! nothing by itself and changes nothing. This is the shared format and how
//! an answer is judged.

use crate::probe::Outcome;
use serde::{Deserialize, Serialize};

pub const PORT: u16 = 7471;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Status {
    /// "pulse-agent"
    pub agent: String,
    pub version: String,
    pub host: String,
    pub os: String,
    pub uptime_s: u64,
    /// All cores together, 0 ..= 100.
    pub cpu_percent: f64,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    /// The disk with the system (or the home folder).
    pub disk_free_gb: f64,
    pub disk_total_gb: f64,
    /// The programs asked for, and whether one with that name runs.
    #[serde(default)]
    pub processes: Vec<Process>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Process {
    pub name: String,
    pub running: bool,
}

/// Limits for an answer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub min_free_gb: f64,
    pub max_cpu: f64,
    pub max_mem_percent: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            min_free_gb: 5.0,
            max_cpu: 90.0,
            max_mem_percent: 95.0,
        }
    }
}

/// The request path for these process names.
pub fn path(processes: &[String]) -> String {
    let q: Vec<String> = processes
        .iter()
        .filter(|p| !p.trim().is_empty())
        .map(|p| format!("process={}", encode(p.trim())))
        .collect();
    if q.is_empty() {
        "/status".into()
    } else {
        format!("/status?{}", q.join("&"))
    }
}

/// The process names in a request path (the agent's side).
pub fn processes_of(path: &str) -> Vec<String> {
    let Some((_, q)) = path.split_once('?') else {
        return Vec::new();
    };
    q.split('&')
        .filter_map(|kv| kv.split_once('='))
        .filter(|(k, _)| *k == "process")
        .filter_map(|(_, v)| decode(v))
        .filter(|v| !v.is_empty())
        .take(32)
        .collect()
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(h, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// How an answer is judged: a program that should run and does not is a
/// failure; a full disk, a busy CPU or full memory a warning.
pub fn judge(s: &Status, wanted: &[String], l: &Limits) -> (Outcome, String) {
    let missing: Vec<&str> = wanted
        .iter()
        .map(|w| w.trim())
        .filter(|w| !w.is_empty())
        .filter(|w| {
            !s.processes
                .iter()
                .any(|p| p.name.eq_ignore_ascii_case(w) && p.running)
        })
        .collect();
    if !missing.is_empty() {
        return (
            Outcome::Fail,
            format!(
                "{} {} not running on {}",
                missing.join(", "),
                if missing.len() == 1 { "is" } else { "are" },
                s.host
            ),
        );
    }
    let mut warn = Vec::new();
    if s.disk_total_gb > 0.0 && s.disk_free_gb < l.min_free_gb {
        warn.push(format!("only {:.1} GB free on the disk", s.disk_free_gb));
    }
    if s.cpu_percent > l.max_cpu {
        warn.push(format!("CPU at {:.0} %", s.cpu_percent));
    }
    let mem = if s.mem_total_mb > 0 {
        s.mem_used_mb as f64 * 100.0 / s.mem_total_mb as f64
    } else {
        0.0
    };
    if mem > l.max_mem_percent {
        warn.push(format!("memory {mem:.0} % used"));
    }
    if !warn.is_empty() {
        return (
            Outcome::Degraded,
            format!("{}: {}", s.host, warn.join(", ")),
        );
    }
    let running = if wanted.iter().any(|w| !w.trim().is_empty()) {
        format!(
            ", {} running",
            wanted
                .iter()
                .map(|w| w.trim())
                .filter(|w| !w.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        String::new()
    };
    (
        Outcome::Ok,
        format!(
            "{} answers: CPU {:.0} %, {:.0} GB free{running}",
            s.host, s.cpu_percent, s.disk_free_gb
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st() -> Status {
        Status {
            agent: "pulse-agent".into(),
            version: "0.6.0".into(),
            host: "playback-1".into(),
            os: "macOS".into(),
            uptime_s: 3600,
            cpu_percent: 12.0,
            mem_used_mb: 4000,
            mem_total_mb: 16000,
            disk_free_gb: 120.0,
            disk_total_gb: 500.0,
            processes: vec![Process {
                name: "Show Master".into(),
                running: true,
            }],
        }
    }

    #[test]
    fn query_round_trip() {
        let p = path(&["Show Master".into(), "".into(), "QLab 5&x".into()]);
        assert_eq!(p, "/status?process=Show%20Master&process=QLab%205%26x");
        assert_eq!(processes_of(&p), vec!["Show Master", "QLab 5&x"]);
        assert_eq!(path(&[]), "/status");
        assert!(processes_of("/status").is_empty());
        assert!(processes_of("/status?process=%zz").is_empty());
    }

    #[test]
    fn judging() {
        let l = Limits::default();
        let (o, m) = judge(&st(), &["show master".into()], &l);
        assert_eq!(o, Outcome::Ok);
        assert!(m.contains("show master running"), "{m}");
        let (o, m) = judge(&st(), &["Lantern".into()], &l);
        assert_eq!(o, Outcome::Fail);
        assert_eq!(m, "Lantern is not running on playback-1");
        let full = Status {
            disk_free_gb: 2.0,
            cpu_percent: 97.0,
            ..st()
        };
        let (o, m) = judge(&full, &[], &l);
        assert_eq!(o, Outcome::Degraded);
        assert_eq!(m, "playback-1: only 2.0 GB free on the disk, CPU at 97 %");
    }

    #[test]
    fn json_is_stable() {
        let j = serde_json::to_string(&st()).unwrap();
        let back: Status = serde_json::from_str(&j).unwrap();
        assert_eq!(back, st());
        // Older agents without processes still parse.
        let mut v: serde_json::Value = serde_json::from_str(&j).unwrap();
        v.as_object_mut().unwrap().remove("processes");
        assert!(serde_json::from_value::<Status>(v).is_ok());
    }
}
