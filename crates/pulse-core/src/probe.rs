//! What a probe reports: one result format for every kind of check.

use serde::{Deserialize, Serialize};

/// What one run of a check found (before debouncing).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    Ok,
    /// Answers, but worse than the thresholds (latency, loss, low frame rate ...).
    Degraded,
    Fail,
    /// The check could not run (interface down, no permission ...): says
    /// nothing about the device.
    Unknown,
}

/// How deep a check looks: what a pass actually proves. Shown with every result.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Depth {
    /// The device answers on the network (ping).
    Reachable,
    /// A TCP port accepts connections.
    Port,
    /// The device announces a service (mDNS, NDI): presence, not that it works.
    Present,
    /// The device answers a protocol query (OSC info, ArtPollReply).
    Answers,
    /// Data is flowing (DMX frames arrive, the process runs).
    Data,
    /// A network interface of this computer is up.
    Link,
}

impl Depth {
    pub fn label(self) -> &'static str {
        match self {
            Depth::Reachable => "Reachable",
            Depth::Port => "Port open",
            Depth::Present => "Present",
            Depth::Answers => "Answers",
            Depth::Data => "Data",
            Depth::Link => "Link",
        }
    }
    /// What a pass at this depth proves, said plainly.
    pub fn meaning(self) -> &'static str {
        match self {
            Depth::Reachable => {
                "The device answers on the network. It says nothing about audio, video or light."
            }
            Depth::Port => {
                "The port accepts connections. The software behind it may still not work."
            }
            Depth::Present => {
                "The device announces the service. Clock, audio or stream state is not checked."
            }
            Depth::Answers => "The device answers a protocol query.",
            Depth::Data => "Data is arriving.",
            Depth::Link => "This computer's network interface is up and has an address.",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Metric {
    /// Average round trip over the window, ms.
    LatencyMs,
    /// Mean difference between consecutive round trips, ms.
    JitterMs,
    /// Lost over the window, percent.
    LossPct,
    /// Time to open a TCP connection, ms.
    ConnectMs,
    /// DMX frames per second.
    Fps,
    /// Time until a protocol answer arrived, ms.
    AnswerMs,
}

impl Metric {
    pub fn label(self) -> &'static str {
        match self {
            Metric::LatencyMs => "Latency",
            Metric::JitterMs => "Jitter",
            Metric::LossPct => "Loss",
            Metric::ConnectMs => "Connect",
            Metric::Fps => "Frame rate",
            Metric::AnswerMs => "Answer",
        }
    }
    pub fn format(self, v: f64) -> String {
        match self {
            Metric::LossPct => format!("{v:.0} %"),
            Metric::Fps => format!("{v:.0} fps"),
            _ if v < 10.0 => format!("{v:.1} ms"),
            _ => format!("{v:.0} ms"),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ProbeResult {
    pub outcome: Outcome,
    pub depth: Depth,
    pub metrics: Vec<(Metric, f64)>,
    /// Plain English, short: "no answer (3 of 3 lost)".
    pub message: String,
    /// How it was measured: "ICMP", "TCP 80 (no ICMP permission)".
    pub via: String,
    /// Raw data for the diagnosis view.
    pub raw: String,
}

impl ProbeResult {
    pub fn new(outcome: Outcome, depth: Depth, message: impl Into<String>) -> Self {
        ProbeResult {
            outcome,
            depth,
            metrics: Vec::new(),
            message: message.into(),
            via: String::new(),
            raw: String::new(),
        }
    }
    pub fn metric(&self, m: Metric) -> Option<f64> {
        self.metrics.iter().find(|(k, _)| *k == m).map(|(_, v)| *v)
    }
    /// The device answered (so it was "seen"), even if degraded.
    pub fn answered(&self) -> bool {
        matches!(self.outcome, Outcome::Ok | Outcome::Degraded)
    }
}
