//! Ping statistics over a rolling window and how they are judged, apart from
//! the network so the rules can be tested.

use crate::model::Thresholds;
use crate::probe::{Depth, Metric, Outcome, ProbeResult};
use std::collections::VecDeque;

/// Echo requests the window remembers.
pub const WINDOW: usize = 20;

/// The last [`WINDOW`] echo requests: `Some(round trip ms)` or `None` (lost).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PingWindow {
    samples: VecDeque<Option<f64>>,
}

impl PingWindow {
    pub fn push(&mut self, rtt: Option<f64>) {
        self.samples.push_back(rtt);
        while self.samples.len() > WINDOW {
            self.samples.pop_front();
        }
    }
    pub fn len(&self) -> usize {
        self.samples.len()
    }
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
    pub fn loss_pct(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let lost = self.samples.iter().filter(|s| s.is_none()).count();
        100.0 * lost as f64 / self.samples.len() as f64
    }
    pub fn latency_ms(&self) -> Option<f64> {
        let ok: Vec<f64> = self.samples.iter().flatten().copied().collect();
        (!ok.is_empty()).then(|| ok.iter().sum::<f64>() / ok.len() as f64)
    }
    /// Mean absolute difference of consecutive answered round trips.
    pub fn jitter_ms(&self) -> Option<f64> {
        let ok: Vec<f64> = self.samples.iter().flatten().copied().collect();
        if ok.len() < 2 {
            return None;
        }
        let sum: f64 = ok.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
        Some(sum / (ok.len() - 1) as f64)
    }
}

/// Judge one ping round (`round` = this run's samples, already pushed into
/// `window`): no answer in the round fails; otherwise the window's latency,
/// jitter and loss against the thresholds.
pub fn evaluate(
    window: &PingWindow,
    round: &[Option<f64>],
    t: &Thresholds,
    via: &str,
) -> ProbeResult {
    let answered = round.iter().flatten().count();
    let mut metrics = Vec::new();
    if let Some(l) = window.latency_ms() {
        metrics.push((Metric::LatencyMs, l));
    }
    if let Some(j) = window.jitter_ms() {
        metrics.push((Metric::JitterMs, j));
    }
    let loss = window.loss_pct();
    metrics.push((Metric::LossPct, loss));
    let raw = round
        .iter()
        .map(|s| match s {
            Some(ms) => format!("{ms:.2} ms"),
            None => "lost".to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let (outcome, message) = if round.is_empty() {
        (Outcome::Unknown, "nothing sent".to_string())
    } else if answered == 0 {
        (
            Outcome::Fail,
            format!("no answer ({} of {} lost)", round.len(), round.len()),
        )
    } else {
        let mut bad = Vec::new();
        if let Some(l) = window.latency_ms().filter(|l| *l > t.latency_ms) {
            bad.push(format!(
                "latency {} (limit {})",
                Metric::LatencyMs.format(l),
                Metric::LatencyMs.format(t.latency_ms)
            ));
        }
        if let Some(j) = window.jitter_ms().filter(|j| *j > t.jitter_ms) {
            bad.push(format!(
                "jitter {} (limit {})",
                Metric::JitterMs.format(j),
                Metric::JitterMs.format(t.jitter_ms)
            ));
        }
        if loss > t.loss_pct {
            bad.push(format!(
                "loss {} (limit {})",
                Metric::LossPct.format(loss),
                Metric::LossPct.format(t.loss_pct)
            ));
        }
        if bad.is_empty() {
            let l = window.latency_ms().unwrap_or(0.0);
            (
                Outcome::Ok,
                format!("answers in {}", Metric::LatencyMs.format(l)),
            )
        } else {
            (Outcome::Degraded, bad.join(", "))
        }
    };
    ProbeResult {
        outcome,
        depth: Depth::Reachable,
        metrics,
        message,
        via: via.to_string(),
        raw: format!(
            "round: {raw}; window: {} samples, {:.0} % lost",
            window.len(),
            loss
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(w: &mut PingWindow, round: &[Option<f64>]) -> ProbeResult {
        for s in round {
            w.push(*s);
        }
        evaluate(w, round, &Thresholds::wired(), "ICMP")
    }

    #[test]
    fn statistics_over_the_window() {
        let mut w = PingWindow::default();
        for s in [Some(1.0), Some(3.0), None, Some(2.0)] {
            w.push(s);
        }
        assert_eq!(w.loss_pct(), 25.0);
        assert_eq!(w.latency_ms(), Some(2.0));
        // |3-1| + |2-3| over 2 steps.
        assert_eq!(w.jitter_ms(), Some(1.5));
        for _ in 0..WINDOW {
            w.push(Some(1.0));
        }
        assert_eq!(w.len(), WINDOW);
        assert_eq!(w.loss_pct(), 0.0);
    }

    #[test]
    fn good_slow_lossy_and_silent() {
        let mut w = PingWindow::default();
        let r = run(&mut w, &[Some(0.8)]);
        assert_eq!(r.outcome, Outcome::Ok);
        assert_eq!(r.depth, Depth::Reachable);
        assert!(r.message.starts_with("answers in"), "{}", r.message);

        let mut w = PingWindow::default();
        let r = run(&mut w, &[Some(45.0)]);
        assert_eq!(r.outcome, Outcome::Degraded);
        assert!(r.message.contains("latency 45 ms"), "{}", r.message);

        // One lost of four: 25 % loss over the window, but this round answered.
        let mut w = PingWindow::default();
        for s in [Some(1.0), None, Some(1.0)] {
            w.push(s);
        }
        let r = run(&mut w, &[Some(1.0)]);
        assert_eq!(r.outcome, Outcome::Degraded);
        assert!(r.message.contains("loss 25 %"), "{}", r.message);

        let r = run(&mut w, &[None, None, None]);
        assert_eq!(r.outcome, Outcome::Fail);
        assert_eq!(r.message, "no answer (3 of 3 lost)");
        assert_eq!(r.via, "ICMP");
    }

    #[test]
    fn wifi_thresholds_tolerate_more_latency() {
        let mut w = PingWindow::default();
        w.push(Some(60.0));
        let r = evaluate(&w, &[Some(60.0)], &Thresholds::wifi(), "ICMP");
        assert_eq!(r.outcome, Outcome::Ok);
    }
}
