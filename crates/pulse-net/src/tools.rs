//! Diagnosis tools, run on a click only: a test of one TCP port, and
//! traceroute (the system's own tool: `traceroute`/`tracepath` on macOS and
//! Linux, `tracert` on Windows; they need no administrator rights there).

use std::io::{BufRead, BufReader, Read};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Host names and addresses only: nothing a command line could misread.
pub fn valid_host(h: &str) -> bool {
    let h = h.trim();
    !h.is_empty()
        && h.len() <= 253
        && !h.starts_with('-')
        && h.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_'))
}

/// Open one TCP connection and close it again; blocks up to `timeout`.
/// The answer is a sentence for the diagnosis view.
pub fn port_test(host: &str, port: u16, timeout: Duration) -> String {
    let host = host.trim();
    if !valid_host(host) {
        return "No valid address".into();
    }
    let addrs: Vec<SocketAddr> = match (host, port).to_socket_addrs() {
        Ok(a) => a.collect(),
        Err(e) => return format!("Cannot resolve {host}: {e}"),
    };
    let Some(addr) = addrs
        .iter()
        .find(|a| a.is_ipv4())
        .or(addrs.first())
        .copied()
    else {
        return format!("Cannot resolve {host}");
    };
    let start = Instant::now();
    match TcpStream::connect_timeout(&addr, timeout) {
        Ok(s) => {
            drop(s);
            format!(
                "Port {port} is open ({:.1} ms)",
                start.elapsed().as_secs_f64() * 1000.0
            )
        }
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::ConnectionReset
            ) =>
        {
            format!("Port {port} is closed: the device answered, nothing listens there")
        }
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) =>
        {
            format!(
                "No answer on port {port} within {} ms (filtered, or the device is off)",
                timeout.as_millis()
            )
        }
        Err(e) => format!("Port {port}: {e}"),
    }
}

/// A traceroute running in the background.
#[derive(Clone, Default)]
pub struct Trace {
    pub lines: Arc<Mutex<Vec<String>>>,
    pub done: Arc<AtomicBool>,
}

impl Trace {
    pub fn text(&self) -> String {
        self.lines.lock().map(|l| l.join("\n")).unwrap_or_default()
    }
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }
}

/// The command for this system: program and arguments before the host.
fn command() -> Vec<(&'static str, Vec<&'static str>)> {
    if cfg!(windows) {
        vec![("tracert", vec!["-d", "-h", "20", "-w", "1000"])]
    } else {
        vec![
            ("traceroute", vec!["-n", "-q", "1", "-w", "1", "-m", "20"]),
            (
                "/usr/sbin/traceroute",
                vec!["-n", "-q", "1", "-w", "1", "-m", "20"],
            ),
            ("tracepath", vec!["-n", "-m", "20"]),
        ]
    }
}

/// Start a traceroute to `host`; `on_line` is called for every new line.
pub fn traceroute(host: &str, on_line: Arc<dyn Fn() + Send + Sync>) -> Trace {
    let t = Trace::default();
    let host = host.trim().to_string();
    let out = t.clone();
    let push = move |s: String| {
        if let Ok(mut l) = out.lines.lock() {
            l.push(s);
        }
        on_line();
    };
    let done = t.done.clone();
    let spawned = std::thread::Builder::new()
        .name("pulse-trace".into())
        .spawn(move || {
            if !valid_host(&host) {
                push("No valid address".into());
                done.store(true, Ordering::Relaxed);
                return;
            }
            let mut started = None;
            for (prog, args) in command() {
                if let Ok(c) = Command::new(prog)
                    .args(&args)
                    .arg(&host)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                {
                    started = Some(c);
                    break;
                }
            }
            let Some(mut child) = started else {
                push(
                    "No traceroute tool on this computer (install traceroute or tracepath).".into(),
                );
                done.store(true, Ordering::Relaxed);
                return;
            };
            if let Some(o) = child.stdout.take() {
                for line in BufReader::new(o).lines().map_while(Result::ok) {
                    push(line);
                }
            }
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                let _ = e.read_to_string(&mut err);
            }
            let _ = child.wait();
            for line in err.lines().filter(|l| !l.trim().is_empty()) {
                push(line.to_string());
            }
            done.store(true, Ordering::Relaxed);
        });
    if spawned.is_err() {
        t.done.store(true, Ordering::Relaxed);
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_checked() {
        assert!(valid_host("10.0.0.1"));
        assert!(valid_host("stagebox-1.local"));
        assert!(valid_host("fe80::1"));
        assert!(!valid_host("-n"));
        assert!(!valid_host("a b"));
        assert!(!valid_host("x;rm"));
        assert!(!valid_host(""));
    }

    #[test]
    fn open_and_closed_ports() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(port_test("127.0.0.1", port, Duration::from_secs(1))
            .starts_with(&format!("Port {port} is open")));
        // A freed port; a test running in parallel may grab it in between, so
        // a few tries with fresh ones.
        let mut last = String::new();
        let closed = (0..5).any(|_| {
            let p = std::net::TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            last = port_test("127.0.0.1", p, Duration::from_secs(1));
            last.contains("is closed")
        });
        assert!(closed, "{last}");
    }

    #[test]
    fn traceroute_to_localhost_ends() {
        let t = traceroute("127.0.0.1", Arc::new(|| {}));
        let end = Instant::now() + Duration::from_secs(20);
        while !t.is_done() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(t.is_done());
        assert!(!t.text().is_empty());
    }
}
