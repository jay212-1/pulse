//! Tiny HTTP/1.1 for what Pulse needs: one GET to Pulse Agent, and the
//! read-only status page. No keep-alive, no bodies in requests, small limits.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Largest answer read (the agent's is about 1 kB).
const MAX_ANSWER: usize = 256 * 1024;

/// `GET path` from `ip:port`: (status code, body).
pub async fn get(
    ip: IpAddr,
    port: u16,
    path: &str,
    source: Option<Ipv4Addr>,
    timeout: Duration,
) -> Result<(u16, String), String> {
    let to = SocketAddr::new(ip, port);
    let work = async {
        let mut s = match (ip, source) {
            (IpAddr::V4(_), Some(src)) => {
                let s = tokio::net::TcpSocket::new_v4().map_err(|e| e.to_string())?;
                s.bind(SocketAddr::new(IpAddr::V4(src), 0))
                    .map_err(|e| e.to_string())?;
                s.connect(to).await
            }
            _ => tokio::net::TcpStream::connect(to).await,
        }
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::ConnectionRefused => format!("TCP {port}: connection refused"),
            _ => format!("TCP {port}: {e}"),
        })?;
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {ip}:{port}\r\nUser-Agent: Pulse\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
        );
        s.write_all(req.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = s.read(&mut chunk).await.map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.len() > MAX_ANSWER {
                return Err("answer too large".to_string());
            }
        }
        parse_response(&buf).ok_or_else(|| "not an HTTP answer".to_string())
    };
    match tokio::time::timeout(timeout, work).await {
        Ok(r) => r,
        Err(_) => Err(format!("TCP {port}: no answer in time")),
    }
}

fn parse_response(b: &[u8]) -> Option<(u16, String)> {
    let end = b.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&b[..end]).ok()?;
    let status = head
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    let chunked = head.lines().any(|l| {
        l.to_ascii_lowercase()
            .replace(' ', "")
            .starts_with("transfer-encoding:chunked")
    });
    let body = &b[end + 4..];
    let body = if chunked {
        dechunk(body)?
    } else {
        body.to_vec()
    };
    Some((status, String::from_utf8_lossy(&body).to_string()))
}

fn dechunk(mut b: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let nl = b.windows(2).position(|w| w == b"\r\n")?;
        let size_s = std::str::from_utf8(&b[..nl]).ok()?;
        let size = usize::from_str_radix(size_s.split(';').next()?.trim(), 16).ok()?;
        b = &b[nl + 2..];
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(b.get(..size)?);
        b = b.get(size + 2..)?;
    }
}

/// A request line read by a server: method and path. Reads the head only
/// (up to 8 kB, a few seconds).
pub async fn read_request(s: &mut tokio::net::TcpStream) -> Option<(String, String)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let read = async {
        loop {
            let n = s.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                return Some(());
            }
            if buf.len() > 8192 {
                return None;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), read)
        .await
        .ok()??;
    let head = String::from_utf8_lossy(&buf);
    let mut first = head.lines().next()?.split_whitespace();
    Some((first.next()?.to_string(), first.next()?.to_string()))
}

/// Write an answer and close.
pub async fn respond(s: &mut tokio::net::TcpStream, status: u16, ty: &str, body: &[u8]) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {ty}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(head.as_bytes()).await;
    let _ = s.write_all(body).await;
    let _ = s.shutdown().await;
}

/// An answer of a [`serve`] handler.
pub struct Answer {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Answer {
    pub fn json(body: String) -> Self {
        Answer {
            status: 200,
            content_type: "application/json",
            body: body.into_bytes(),
        }
    }
    pub fn html(body: &str) -> Self {
        Answer {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: body.as_bytes().to_vec(),
        }
    }
    pub fn not_found() -> Self {
        Answer {
            status: 404,
            content_type: "text/plain; charset=utf-8",
            body: b"not found".to_vec(),
        }
    }
}

/// Answer GET requests on `l` with `handler(path)` until the task is
/// aborted. Other methods get 405. At most 64 connections at once.
pub async fn serve<F>(l: tokio::net::TcpListener, handler: F)
where
    F: Fn(&str) -> Answer + Send + Sync + 'static,
{
    let handler = std::sync::Arc::new(handler);
    let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(64));
    loop {
        let Ok((mut s, _)) = l.accept().await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            continue;
        };
        let h = handler.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let Some((method, path)) = read_request(&mut s).await else {
                return;
            };
            if method != "GET" {
                respond(&mut s, 405, "text/plain", b"read-only").await;
                return;
            }
            let a = h(&path);
            respond(&mut s, a.status, a.content_type, &a.body).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses() {
        let r = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").unwrap();
        assert_eq!(r, (200, "{}".into()));
        let c = parse_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n",
        )
        .unwrap();
        assert_eq!(c.1, "abcde");
        assert_eq!(parse_response(b"garbage"), None);
    }

    #[tokio::test]
    async fn get_and_serve() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let (m, p) = read_request(&mut s).await.unwrap();
            respond(&mut s, 200, "text/plain", format!("{m} {p}").as_bytes()).await;
        });
        let r = get(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            port,
            "/status?x=1",
            None,
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(r, (200, "GET /status?x=1".into()));
    }
}
