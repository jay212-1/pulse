//! A minimal OSC 1.0 message codec: enough to ask a console who it is
//! (`/xinfo`, `/status`) and to read the answer. No bundles, no time tags.

#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Int(i32),
    Float(f32),
    Str(String),
    Blob(Vec<u8>),
}

impl Arg {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Arg::Str(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub address: String,
    pub args: Vec<Arg>,
}

fn pad(out: &mut Vec<u8>) {
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
    pad(out);
}

pub fn encode(address: &str, args: &[Arg]) -> Vec<u8> {
    let mut out = Vec::new();
    put_str(&mut out, address);
    let mut tags = String::from(",");
    for a in args {
        tags.push(match a {
            Arg::Int(_) => 'i',
            Arg::Float(_) => 'f',
            Arg::Str(_) => 's',
            Arg::Blob(_) => 'b',
        });
    }
    put_str(&mut out, &tags);
    for a in args {
        match a {
            Arg::Int(i) => out.extend_from_slice(&i.to_be_bytes()),
            Arg::Float(f) => out.extend_from_slice(&f.to_be_bytes()),
            Arg::Str(s) => put_str(&mut out, s),
            Arg::Blob(b) => {
                out.extend_from_slice(&(b.len() as u32).to_be_bytes());
                out.extend_from_slice(b);
                pad(&mut out);
            }
        }
    }
    out
}

/// A padded string at `*at`; moves `at` past it.
fn get_str(b: &[u8], at: &mut usize) -> Option<String> {
    let rest = b.get(*at..)?;
    let end = rest.iter().position(|&c| c == 0)?;
    let s = String::from_utf8_lossy(&rest[..end]).into_owned();
    *at += (end + 4) & !3;
    Some(s)
}

fn get4(b: &[u8], at: &mut usize) -> Option<[u8; 4]> {
    let v: [u8; 4] = b.get(*at..*at + 4)?.try_into().ok()?;
    *at += 4;
    Some(v)
}

pub fn decode(b: &[u8]) -> Option<Message> {
    let mut at = 0;
    let address = get_str(b, &mut at)?;
    if !address.starts_with('/') {
        return None;
    }
    // Old senders leave out the type tags.
    if at >= b.len() {
        return Some(Message {
            address,
            args: Vec::new(),
        });
    }
    let tags = get_str(b, &mut at)?;
    let tags = tags.strip_prefix(',')?;
    let mut args = Vec::new();
    for t in tags.chars() {
        args.push(match t {
            'i' => Arg::Int(i32::from_be_bytes(get4(b, &mut at)?)),
            'f' => Arg::Float(f32::from_be_bytes(get4(b, &mut at)?)),
            's' | 'S' => Arg::Str(get_str(b, &mut at)?),
            'b' => {
                let n = u32::from_be_bytes(get4(b, &mut at)?) as usize;
                let v = b.get(at..at + n)?.to_vec();
                at += (n + 3) & !3;
                Arg::Blob(v)
            }
            // Types without data.
            'T' | 'F' | 'N' | 'I' => continue,
            _ => return None,
        });
    }
    Some(Message { address, args })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xinfo_request_is_padded_like_the_x32_expects() {
        let b = encode("/xinfo", &[]);
        assert_eq!(b, b"/xinfo\0\0,\0\0\0".to_vec());
        assert_eq!(
            decode(&b),
            Some(Message {
                address: "/xinfo".into(),
                args: vec![]
            })
        );
    }

    #[test]
    fn round_trip_with_all_types() {
        let args = vec![
            Arg::Str("192.168.1.20".into()),
            Arg::Int(-3),
            Arg::Float(1.5),
            Arg::Blob(vec![1, 2, 3]),
            Arg::Str("X32".into()),
        ];
        let b = encode("/xinfo", &args);
        assert_eq!(b.len() % 4, 0);
        let m = decode(&b).unwrap();
        assert_eq!(m.args, args);
        assert_eq!(m.args[4].as_str(), Some("X32"));
    }

    #[test]
    fn rubbish_is_rejected() {
        assert_eq!(decode(b""), None);
        assert_eq!(decode(b"xinfo\0\0\0"), None);
        assert_eq!(decode(b"/a\0\0,s\0\0"), None, "string missing");
        assert_eq!(decode(b"/a\0\0,q\0\0"), None, "unknown type");
    }
}
