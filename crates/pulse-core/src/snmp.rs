//! SNMP v2c, read-only and minimal: a GET for a few values of a switch
//! (name, uptime, one port's description, state and error counters), the
//! BER encoding both ways (the simulator answers), and how a reading is
//! judged. Pulse never sends SET.

use crate::probe::Outcome;

pub const PORT: u16 = 161;
pub const GET: u8 = 0xa0;
pub const RESPONSE: u8 = 0xa2;

pub type Oid = Vec<u32>;

pub fn sys_name() -> Oid {
    vec![1, 3, 6, 1, 2, 1, 1, 5, 0]
}
pub fn sys_uptime() -> Oid {
    vec![1, 3, 6, 1, 2, 1, 1, 3, 0]
}
fn if_col(col: u32, index: u32) -> Oid {
    vec![1, 3, 6, 1, 2, 1, 2, 2, 1, col, index]
}
pub fn if_descr(i: u32) -> Oid {
    if_col(2, i)
}
/// 1 up, 2 down, 3 testing, 5 dormant, 7 lower layer down ...
pub fn if_oper_status(i: u32) -> Oid {
    if_col(8, i)
}
pub fn if_in_errors(i: u32) -> Oid {
    if_col(14, i)
}
pub fn if_out_errors(i: u32) -> Oid {
    if_col(20, i)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Bytes(Vec<u8>),
    Oid(Oid),
    Counter(u64),
    Gauge(u64),
    /// Hundredths of a second.
    Ticks(u64),
    NoSuchObject,
    NoSuchInstance,
    EndOfMib,
}

impl Value {
    pub fn text(&self) -> Option<String> {
        match self {
            Value::Bytes(b) => Some(String::from_utf8_lossy(b).trim().to_string()),
            _ => None,
        }
    }
    pub fn number(&self) -> Option<u64> {
        match self {
            Value::Int(i) => u64::try_from(*i).ok(),
            Value::Counter(n) | Value::Gauge(n) | Value::Ticks(n) => Some(*n),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub community: String,
    pub pdu: u8,
    pub request_id: i32,
    pub error_status: i64,
    pub error_index: i64,
    pub vars: Vec<(Oid, Value)>,
}

impl Message {
    pub fn get(community: &str, request_id: i32, oids: Vec<Oid>) -> Self {
        Message {
            community: community.into(),
            pdu: GET,
            request_id,
            error_status: 0,
            error_index: 0,
            vars: oids.into_iter().map(|o| (o, Value::Null)).collect(),
        }
    }
    pub fn value(&self, oid: &[u32]) -> Option<&Value> {
        self.vars.iter().find(|(o, _)| o == oid).map(|(_, v)| v)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut vars = Vec::new();
        for (oid, v) in &self.vars {
            let mut vb = tlv(0x06, &enc_oid(oid));
            vb.extend(enc_value(v));
            vars.extend(tlv(0x30, &vb));
        }
        let mut pdu = enc_int(i64::from(self.request_id));
        pdu.extend(enc_int(self.error_status));
        pdu.extend(enc_int(self.error_index));
        pdu.extend(tlv(0x30, &vars));
        let mut msg = enc_int(1); // version 2c
        msg.extend(tlv(0x04, self.community.as_bytes()));
        msg.extend(tlv(self.pdu, &pdu));
        tlv(0x30, &msg)
    }

    pub fn decode(b: &[u8]) -> Option<Message> {
        let mut r = Reader { b, i: 0 };
        let mut msg = r.inner(0x30)?;
        if msg.int()? != 1 {
            return None;
        }
        let community = String::from_utf8_lossy(msg.bytes(0x04)?).to_string();
        let (pdu, body) = msg.any()?;
        let mut p = Reader { b: body, i: 0 };
        let request_id = i32::try_from(p.int()?).ok()?;
        let error_status = p.int()?;
        let error_index = p.int()?;
        let mut list = p.inner(0x30)?;
        let mut vars = Vec::new();
        while !list.done() {
            let mut vb = list.inner(0x30)?;
            let oid = dec_oid(vb.bytes(0x06)?)?;
            let (t, v) = vb.any()?;
            vars.push((oid, dec_value(t, v)?));
        }
        Some(Message {
            community,
            pdu,
            request_id,
            error_status,
            error_index,
            vars,
        })
    }
}

fn tlv(t: u8, v: &[u8]) -> Vec<u8> {
    let mut out = vec![t];
    let n = v.len();
    if n < 0x80 {
        out.push(n as u8);
    } else {
        let bytes: Vec<u8> = n
            .to_be_bytes()
            .iter()
            .copied()
            .skip_while(|&x| x == 0)
            .collect();
        out.push(0x80 | bytes.len() as u8);
        out.extend(bytes);
    }
    out.extend_from_slice(v);
    out
}

fn int_bytes(v: i64) -> Vec<u8> {
    let b = v.to_be_bytes();
    let mut i = 0;
    while i < 7 && ((b[i] == 0 && b[i + 1] & 0x80 == 0) || (b[i] == 0xff && b[i + 1] & 0x80 != 0)) {
        i += 1;
    }
    b[i..].to_vec()
}
fn uint_bytes(v: u64) -> Vec<u8> {
    let mut b: Vec<u8> = v
        .to_be_bytes()
        .iter()
        .copied()
        .skip_while(|&x| x == 0)
        .collect();
    if b.is_empty() || b[0] & 0x80 != 0 {
        b.insert(0, 0);
    }
    b
}
fn enc_int(v: i64) -> Vec<u8> {
    tlv(0x02, &int_bytes(v))
}
fn enc_oid(o: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    if o.len() >= 2 {
        out.push((o[0] * 40 + o[1]) as u8);
    }
    for &n in o.iter().skip(2) {
        let mut stack = vec![(n & 0x7f) as u8];
        let mut x = n >> 7;
        while x > 0 {
            stack.push(0x80 | (x & 0x7f) as u8);
            x >>= 7;
        }
        out.extend(stack.iter().rev());
    }
    out
}
fn dec_oid(b: &[u8]) -> Option<Oid> {
    let first = *b.first()?;
    let mut out = vec![u32::from(first / 40), u32::from(first % 40)];
    let mut x: u32 = 0;
    for &c in &b[1..] {
        x = x.checked_mul(128)? | u32::from(c & 0x7f);
        if c & 0x80 == 0 {
            out.push(x);
            x = 0;
        }
    }
    Some(out)
}
fn enc_value(v: &Value) -> Vec<u8> {
    match v {
        Value::Null => tlv(0x05, &[]),
        Value::Int(i) => enc_int(*i),
        Value::Bytes(b) => tlv(0x04, b),
        Value::Oid(o) => tlv(0x06, &enc_oid(o)),
        Value::Counter(n) => tlv(0x41, &uint_bytes(*n)),
        Value::Gauge(n) => tlv(0x42, &uint_bytes(*n)),
        Value::Ticks(n) => tlv(0x43, &uint_bytes(*n)),
        Value::NoSuchObject => tlv(0x80, &[]),
        Value::NoSuchInstance => tlv(0x81, &[]),
        Value::EndOfMib => tlv(0x82, &[]),
    }
}
fn dec_uint(b: &[u8]) -> Option<u64> {
    if b.len() > 9 {
        return None;
    }
    Some(b.iter().fold(0u64, |a, &c| (a << 8) | u64::from(c)))
}
fn dec_int(b: &[u8]) -> Option<i64> {
    if b.is_empty() || b.len() > 8 {
        return None;
    }
    let neg = b[0] & 0x80 != 0;
    let mut v: i64 = if neg { -1 } else { 0 };
    for &c in b {
        v = (v << 8) | i64::from(c);
    }
    Some(v)
}
fn dec_value(t: u8, b: &[u8]) -> Option<Value> {
    Some(match t {
        0x05 => Value::Null,
        0x02 => Value::Int(dec_int(b)?),
        0x04 => Value::Bytes(b.to_vec()),
        0x06 => Value::Oid(dec_oid(b)?),
        0x41 | 0x46 => Value::Counter(dec_uint(b)?),
        0x42 => Value::Gauge(dec_uint(b)?),
        0x43 => Value::Ticks(dec_uint(b)?),
        0x80 => Value::NoSuchObject,
        0x81 => Value::NoSuchInstance,
        0x82 => Value::EndOfMib,
        // IpAddress, Opaque and others: kept as bytes.
        _ => Value::Bytes(b.to_vec()),
    })
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn done(&self) -> bool {
        self.i >= self.b.len()
    }
    fn any(&mut self) -> Option<(u8, &'a [u8])> {
        let t = *self.b.get(self.i)?;
        let l0 = *self.b.get(self.i + 1)?;
        let mut j = self.i + 2;
        let len = if l0 & 0x80 == 0 {
            usize::from(l0)
        } else {
            let k = usize::from(l0 & 0x7f);
            if k == 0 || k > 4 {
                return None;
            }
            let mut n = 0usize;
            for _ in 0..k {
                n = (n << 8) | usize::from(*self.b.get(j)?);
                j += 1;
            }
            n
        };
        let v = self.b.get(j..j.checked_add(len)?)?;
        self.i = j + len;
        Some((t, v))
    }
    fn bytes(&mut self, t: u8) -> Option<&'a [u8]> {
        let (got, v) = self.any()?;
        (got == t).then_some(v)
    }
    fn inner(&mut self, t: u8) -> Option<Reader<'a>> {
        Some(Reader {
            b: self.bytes(t)?,
            i: 0,
        })
    }
    fn int(&mut self) -> Option<i64> {
        dec_int(self.bytes(0x02)?)
    }
}

/// What one GET told about a switch (and one of its ports).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Reading {
    pub name: String,
    pub uptime_s: u64,
    /// The port asked for (0: none).
    pub port: u32,
    pub port_name: String,
    /// `None`: the switch has no such port.
    pub oper_status: Option<i64>,
    pub errors: u64,
}

impl Reading {
    pub fn oids(port: u32) -> Vec<Oid> {
        let mut v = vec![sys_name(), sys_uptime()];
        if port > 0 {
            v.extend([
                if_descr(port),
                if_oper_status(port),
                if_in_errors(port),
                if_out_errors(port),
            ]);
        }
        v
    }
    pub fn from(m: &Message, port: u32) -> Reading {
        let num = |o: Oid| m.value(&o).and_then(Value::number);
        Reading {
            name: m
                .value(&sys_name())
                .and_then(Value::text)
                .unwrap_or_default(),
            uptime_s: num(sys_uptime()).unwrap_or(0) / 100,
            port,
            port_name: if port > 0 {
                m.value(&if_descr(port))
                    .and_then(Value::text)
                    .unwrap_or_default()
            } else {
                String::new()
            },
            oper_status: if port > 0 {
                m.value(&if_oper_status(port)).and_then(|v| match v {
                    Value::Int(i) => Some(*i),
                    _ => None,
                })
            } else {
                None
            },
            errors: if port > 0 {
                num(if_in_errors(port)).unwrap_or(0) + num(if_out_errors(port)).unwrap_or(0)
            } else {
                0
            },
        }
    }
}

fn oper_word(s: i64) -> &'static str {
    match s {
        1 => "up",
        2 => "down",
        3 => "testing",
        5 => "dormant",
        6 => "not present",
        7 => "lower layer down",
        _ => "unknown",
    }
}

/// How a reading is judged, against the one before (error counters, restarts).
pub fn judge(prev: Option<&Reading>, r: &Reading) -> (Outcome, String) {
    let who = if r.name.is_empty() {
        "switch".to_string()
    } else {
        format!("\"{}\"", r.name)
    };
    let restarted = prev.is_some_and(|p| r.uptime_s < p.uptime_s);
    if r.port > 0 {
        let pn = if r.port_name.is_empty() {
            format!("port {}", r.port)
        } else {
            format!("port {} ({})", r.port, r.port_name)
        };
        match r.oper_status {
            None => {
                return (
                    Outcome::Fail,
                    format!("{who} answers, but has no port {}", r.port),
                )
            }
            Some(1) => {}
            Some(s) => return (Outcome::Fail, format!("{who}: {pn} is {}", oper_word(s))),
        }
        let new = prev
            .filter(|p| p.port == r.port && !restarted)
            .map(|p| r.errors.saturating_sub(p.errors))
            .unwrap_or(0);
        if new > 0 {
            return (
                Outcome::Degraded,
                format!("{who}: {pn} has {new} new errors since the last check"),
            );
        }
        if restarted {
            return (
                Outcome::Degraded,
                format!("{who} restarted {} ago; {pn} is up", ago(r.uptime_s)),
            );
        }
        return (Outcome::Ok, format!("{who}: {pn} is up"));
    }
    if restarted {
        return (
            Outcome::Degraded,
            format!("{who} restarted {} ago", ago(r.uptime_s)),
        );
    }
    (
        Outcome::Ok,
        format!("{who} answers, up {}", ago(r.uptime_s)),
    )
}

fn ago(s: u64) -> String {
    match s {
        0..=119 => format!("{s} s"),
        120..=7199 => format!("{} min", s / 60),
        7200..=172_799 => format!("{} h", s / 3600),
        _ => format!("{} days", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_and_response_round_trip() {
        let m = Message::get("public", 4711, Reading::oids(3));
        let b = m.encode();
        assert_eq!(Message::decode(&b).unwrap(), m);
        let resp = Message {
            pdu: RESPONSE,
            vars: vec![
                (sys_name(), Value::Bytes(b"Core switch".to_vec())),
                (sys_uptime(), Value::Ticks(360_000)),
                (if_descr(3), Value::Bytes(b"Port 3".to_vec())),
                (if_oper_status(3), Value::Int(1)),
                (if_in_errors(3), Value::Counter(4_000_000_000)),
                (if_out_errors(3), Value::Counter(2)),
            ],
            ..m.clone()
        };
        let back = Message::decode(&resp.encode()).unwrap();
        assert_eq!(back, resp);
        let r = Reading::from(&back, 3);
        assert_eq!(r.name, "Core switch");
        assert_eq!(r.uptime_s, 3600);
        assert_eq!(r.oper_status, Some(1));
        assert_eq!(r.errors, 4_000_000_002);
    }

    #[test]
    fn known_encoding() {
        // sysUpTime.0 as BER: 06 08 2b 06 01 02 01 01 03 00
        assert_eq!(
            tlv(0x06, &enc_oid(&sys_uptime())),
            vec![0x06, 0x08, 0x2b, 6, 1, 2, 1, 1, 3, 0]
        );
        assert_eq!(
            enc_oid(&[1, 3, 6, 1, 4, 1, 300]),
            vec![0x2b, 6, 1, 4, 1, 0x82, 0x2c]
        );
        assert_eq!(int_bytes(128), vec![0, 128]);
        assert_eq!(int_bytes(-1), vec![0xff]);
        assert_eq!(dec_int(&[0xff, 0x7f]), Some(-129));
        // Long lengths.
        let big = tlv(0x04, &[0u8; 300]);
        assert_eq!(&big[..4], &[0x04, 0x82, 0x01, 0x2c]);
        assert_eq!(Message::decode(b"\x30\x03\x02\x01"), None);
        assert_eq!(Message::decode(&[]), None);
    }

    #[test]
    fn judging() {
        let up = Reading {
            name: "Core".into(),
            uptime_s: 1000,
            port: 3,
            port_name: "ge-3".into(),
            oper_status: Some(1),
            errors: 10,
        };
        assert_eq!(judge(None, &up).0, Outcome::Ok);
        let more = Reading {
            errors: 15,
            uptime_s: 1005,
            ..up.clone()
        };
        let (o, m) = judge(Some(&up), &more);
        assert_eq!(o, Outcome::Degraded);
        assert!(m.contains("5 new errors"), "{m}");
        let down = Reading {
            oper_status: Some(2),
            ..up.clone()
        };
        let (o, m) = judge(Some(&up), &down);
        assert_eq!(o, Outcome::Fail);
        assert!(m.contains("port 3 (ge-3) is down"), "{m}");
        let none = Reading {
            oper_status: None,
            ..up.clone()
        };
        assert!(judge(None, &none).1.contains("has no port 3"));
        let rebooted = Reading {
            uptime_s: 30,
            errors: 0,
            ..up.clone()
        };
        let (o, m) = judge(Some(&up), &rebooted);
        assert_eq!(o, Outcome::Degraded);
        assert!(m.contains("restarted 30 s ago"), "{m}");
        let plain = Reading {
            port: 0,
            ..up.clone()
        };
        assert_eq!(judge(None, &plain).1, "\"Core\" answers, up 16 min");
    }
}
