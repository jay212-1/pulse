//! The Pulse Agent probe: one HTTP GET to the agent on the show computer.

use crate::probe::{BoxFut, Ctx, Probe, Target};
use pulse_core::agent::{judge, path, Limits, Status};
use pulse_core::probe::{Depth, Metric, Outcome, ProbeResult};

pub struct AgentProbe {
    target: Target,
    processes: Vec<String>,
    limits: Limits,
    port: u16,
}

impl AgentProbe {
    pub(crate) fn new(target: Target, processes: Vec<String>, min_free_gb: f64, port: u16) -> Self {
        AgentProbe {
            target,
            processes,
            limits: Limits {
                min_free_gb,
                ..Limits::default()
            },
            port,
        }
    }
}

impl Probe for AgentProbe {
    fn cost(&self, _ctx: &Ctx) -> u32 {
        // Connect, request, close.
        3
    }

    fn run<'a>(&'a mut self, ctx: &'a Ctx) -> BoxFut<'a, ProbeResult> {
        Box::pin(async move {
            let via = format!("Pulse Agent (TCP {})", self.port);
            let ip = match self.target.ip(ctx.timeout).await {
                Ok(ip) => ip,
                Err(e) => {
                    let mut r = ProbeResult::new(Outcome::Fail, Depth::Data, e);
                    r.via = via;
                    return r;
                }
            };
            let start = std::time::Instant::now();
            let got = crate::http::get(
                ip,
                self.port,
                &path(&self.processes),
                ctx.source,
                ctx.timeout,
            )
            .await;
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            let mut r = match got {
                Ok((200, body)) => match serde_json::from_str::<Status>(&body) {
                    Ok(s) if s.agent == "pulse-agent" => {
                        let (o, msg) = judge(&s, &self.processes, &self.limits);
                        let mut r = ProbeResult::new(o, Depth::Data, msg);
                        r.metrics.push((Metric::AnswerMs, ms));
                        r.raw = body;
                        r
                    }
                    _ => ProbeResult::new(
                        Outcome::Fail,
                        Depth::Port,
                        format!("port {} answers, but not as Pulse Agent", self.port),
                    ),
                },
                Ok((code, _)) => ProbeResult::new(
                    Outcome::Fail,
                    Depth::Port,
                    format!("port {} answers HTTP {code}, not Pulse Agent", self.port),
                ),
                Err(e) if e.contains("refused") => ProbeResult::new(
                    Outcome::Fail,
                    Depth::Reachable,
                    "Pulse Agent is not running (connection refused)",
                ),
                Err(e) => ProbeResult::new(Outcome::Fail, Depth::Port, format!("Pulse Agent: {e}")),
            };
            r.via = via;
            r
        })
    }
}
