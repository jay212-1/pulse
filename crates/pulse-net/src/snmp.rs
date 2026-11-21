//! The SNMP probe: one v2c GET (read-only) to a switch per run, for its
//! name and uptime and, if a port is set, that port's state and errors.

use crate::probe::{ipv4, unknown, BoxFut, Ctx, Probe, Target};
use pulse_core::probe::{Depth, Metric, Outcome, ProbeResult};
use pulse_core::snmp::{judge, Message, Reading, RESPONSE};
use std::net::{IpAddr, SocketAddr};

pub struct SnmpProbe {
    target: Target,
    community: String,
    if_index: u32,
    port: u16,
    request: i32,
    last: Option<Reading>,
}

impl SnmpProbe {
    pub(crate) fn new(target: Target, community: &str, if_index: u32, port: u16) -> Self {
        SnmpProbe {
            target,
            community: community.trim().to_string(),
            if_index,
            port,
            request: 1,
            last: None,
        }
    }
}

impl Probe for SnmpProbe {
    fn cost(&self, _ctx: &Ctx) -> u32 {
        1
    }

    fn run<'a>(&'a mut self, ctx: &'a Ctx) -> BoxFut<'a, ProbeResult> {
        Box::pin(async move {
            let via = format!("SNMP (UDP {})", self.port);
            if self.community.is_empty() {
                return unknown(Depth::Answers, "no SNMP community set", &via);
            }
            let ip = match ipv4(&mut self.target, ctx, Depth::Answers, &via).await {
                Ok(ip) => ip,
                Err(r) => return r,
            };
            self.request = self.request.wrapping_add(1).max(1);
            let id = self.request;
            let req = Message::get(&self.community, id, Reading::oids(self.if_index)).encode();
            let to = SocketAddr::new(IpAddr::V4(ip), self.port);
            let asked = crate::udp::ask(ctx.source, to, &req, ctx.timeout, |b, from| {
                if from.ip() != IpAddr::V4(ip) {
                    return None;
                }
                Message::decode(b).filter(|m| m.pdu == RESPONSE && m.request_id == id)
            })
            .await;
            let mut r = match asked {
                Ok(Some((m, ms))) if m.error_status != 0 => {
                    let mut r = ProbeResult::new(
                        Outcome::Fail,
                        Depth::Answers,
                        format!("the switch answers with SNMP error {}", m.error_status),
                    );
                    r.metrics.push((Metric::AnswerMs, ms));
                    r
                }
                Ok(Some((m, ms))) => {
                    let reading = Reading::from(&m, self.if_index);
                    let (o, msg) = judge(self.last.as_ref(), &reading);
                    let mut r = ProbeResult::new(o, Depth::Answers, msg);
                    r.metrics.push((Metric::AnswerMs, ms));
                    r.raw = format!("{ip}:{} {:?}", self.port, reading);
                    self.last = Some(reading);
                    r
                }
                Ok(None) => ProbeResult::new(
                    Outcome::Fail,
                    Depth::Answers,
                    "no SNMP answer (SNMP off, wrong community, or not allowed from here?)",
                ),
                Err(e) => {
                    ProbeResult::new(Outcome::Fail, Depth::Answers, format!("cannot ask: {e}"))
                }
            };
            r.via = via;
            r
        })
    }
}
