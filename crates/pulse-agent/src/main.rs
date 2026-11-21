//! Pulse Agent: runs on a show computer (playback, backup, FOH Mac) and
//! tells Pulse, when asked, that the computer is alive and how it is doing:
//! CPU, memory, free disk space, and whether named programs run.
//!
//! ```text
//! pulse-agent [--port 7471] [--bind 0.0.0.0]
//! ```
//!
//! It only answers `GET /status?process=...` (and `GET /` with a short
//! note). It sends nothing by itself, changes nothing on the computer, and
//! has no settings to change from the network.

use pulse_core::agent::{processes_of, Process, Status};
use pulse_net::http::{serve, Answer};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A program name the way it is compared: no case, no spaces, no ".exe"/".app".
fn norm(s: &str) -> String {
    let s = s.trim().to_lowercase();
    let s = s
        .strip_suffix(".exe")
        .or_else(|| s.strip_suffix(".app"))
        .unwrap_or(&s)
        .to_string();
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .collect()
}

/// What the background refresh knows.
#[derive(Default, Clone)]
struct Snapshot {
    status: Status,
    /// Normalized names of the running programs.
    running: Vec<String>,
}

fn refresh(sys: &mut sysinfo::System, disks: &mut sysinfo::Disks) -> Snapshot {
    use sysinfo::{ProcessesToUpdate, System};
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    disks.refresh(true);
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/"));
    // The disk with the home folder (the longest mount point that contains it).
    let disk = disks
        .list()
        .iter()
        .filter(|d| home.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len());
    let gb = |b: u64| b as f64 / 1_000_000_000.0;
    let mut running: Vec<String> = Vec::new();
    for p in sys.processes().values() {
        running.push(norm(&p.name().to_string_lossy()));
        if let Some(stem) = p.exe().and_then(|e| e.file_stem()) {
            running.push(norm(&stem.to_string_lossy()));
        }
        // macOS: ".../Show Master.app/Contents/MacOS/showmaster" -> "Show Master".
        if let Some(app) = p.exe().and_then(|e| {
            e.ancestors()
                .find(|a| a.extension().is_some_and(|x| x == "app"))
                .and_then(|a| a.file_stem())
        }) {
            running.push(norm(&app.to_string_lossy()));
        }
    }
    running.sort();
    running.dedup();
    Snapshot {
        status: Status {
            agent: "pulse-agent".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            host: System::host_name().unwrap_or_else(|| "unknown".into()),
            os: System::long_os_version().unwrap_or_else(|| std::env::consts::OS.into()),
            uptime_s: System::uptime(),
            cpu_percent: f64::from(sys.global_cpu_usage()),
            mem_used_mb: sys.used_memory() / 1_048_576,
            mem_total_mb: sys.total_memory() / 1_048_576,
            disk_free_gb: disk.map(|d| gb(d.available_space())).unwrap_or(0.0),
            disk_total_gb: disk.map(|d| gb(d.total_space())).unwrap_or(0.0),
            processes: Vec::new(),
        },
        running,
    }
}

fn answer(snap: &Snapshot, path: &str) -> Answer {
    let route = path.split('?').next().unwrap_or("");
    match route {
        "/status" => {
            let mut st = snap.status.clone();
            st.processes = processes_of(path)
                .into_iter()
                .map(|name| Process {
                    running: snap.running.contains(&norm(&name)),
                    name,
                })
                .collect();
            Answer::json(serde_json::to_string(&st).unwrap_or_default())
        }
        "/" => Answer::html(
            "<!doctype html><meta charset=utf-8><title>Pulse Agent</title><p>Pulse Agent is running. Pulse asks <code>/status</code>; nothing here can be changed.",
        ),
        _ => Answer::not_found(),
    }
}

fn main() -> ExitCode {
    let mut port = pulse_core::agent::PORT;
    let mut bind = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => match args.next().and_then(|v| v.parse().ok()) {
                Some(p) => port = p,
                None => {
                    eprintln!("--port needs a number");
                    return ExitCode::FAILURE;
                }
            },
            "--bind" => match args.next().and_then(|v| v.parse().ok()) {
                Some(ip) => bind = ip,
                None => {
                    eprintln!("--bind needs an IP address");
                    return ExitCode::FAILURE;
                }
            },
            "-h" | "--help" => {
                println!("pulse-agent [--port 7471] [--bind 0.0.0.0]");
                println!("Answers Pulse's questions about this computer (read-only): CPU, memory, disk, running programs.");
                return ExitCode::SUCCESS;
            }
            _ => {
                eprintln!("unknown option {a} (see --help)");
                return ExitCode::FAILURE;
            }
        }
    }
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("cannot start: {e}");
            return ExitCode::FAILURE;
        }
    };
    let snap = Arc::new(Mutex::new(Snapshot::default()));
    {
        // System values are read every two seconds, not per request.
        let snap = snap.clone();
        let started = std::thread::Builder::new()
            .name("refresh".into())
            .spawn(move || {
                let mut sys = sysinfo::System::new();
                let mut disks = sysinfo::Disks::new_with_refreshed_list();
                loop {
                    let s = refresh(&mut sys, &mut disks);
                    if let Ok(mut g) = snap.lock() {
                        *g = s;
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }
            });
        if let Err(e) = started {
            eprintln!("cannot start: {e}");
            return ExitCode::FAILURE;
        }
    }
    let addr = SocketAddr::new(bind, port);
    rt.block_on(async move {
        let l = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("cannot listen on {addr}: {e}");
                return ExitCode::FAILURE;
            }
        };
        println!(
            "Pulse Agent {} listening on {addr} (read-only)",
            env!("CARGO_PKG_VERSION")
        );
        serve(l, move |path| {
            let s = snap.lock().map(|g| g.clone()).unwrap_or_default();
            answer(&s, path)
        })
        .await;
        ExitCode::SUCCESS
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn names_compare_loosely() {
        assert_eq!(norm("Show Master"), "showmaster");
        assert_eq!(norm("show-master.exe"), "showmaster");
        assert_eq!(norm("QLab 5.app"), "qlab5");
    }

    #[test]
    fn answers_status_with_the_programs_asked_for() {
        let snap = Snapshot {
            status: Status {
                agent: "pulse-agent".into(),
                host: "pb".into(),
                ..Status::default()
            },
            running: vec!["showmaster".into(), "finder".into()],
        };
        let a = answer(&snap, "/status?process=Show%20Master&process=Lantern");
        assert_eq!(a.status, 200);
        let st: Status = serde_json::from_slice(&a.body).unwrap();
        assert_eq!(
            st.processes,
            vec![
                Process {
                    name: "Show Master".into(),
                    running: true
                },
                Process {
                    name: "Lantern".into(),
                    running: false
                },
            ]
        );
        assert_eq!(answer(&snap, "/x").status, 404);
    }

    #[test]
    fn reads_this_computer() {
        let mut sys = sysinfo::System::new();
        let mut disks = sysinfo::Disks::new_with_refreshed_list();
        let s = refresh(&mut sys, &mut disks);
        assert!(s.status.mem_total_mb > 0);
        assert!(!s.running.is_empty());
    }
}
