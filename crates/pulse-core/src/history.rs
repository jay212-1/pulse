//! Measurements over time for the diagnosis graphs: one bounded series per
//! check and metric.

use crate::model::Id;
use crate::probe::{Metric, ProbeResult};
use crate::status::Ms;
use std::collections::{BTreeMap, VecDeque};

/// Points kept per series (an hour at one result every 2 s).
pub const MAX_POINTS: usize = 1800;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Series {
    points: VecDeque<(Ms, f64)>,
}

impl Series {
    pub fn push(&mut self, at: Ms, v: f64) {
        self.points.push_back((at, v));
        while self.points.len() > MAX_POINTS {
            self.points.pop_front();
        }
    }
    pub fn points(&self) -> impl Iterator<Item = &(Ms, f64)> {
        self.points.iter()
    }
    pub fn len(&self) -> usize {
        self.points.len()
    }
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
    pub fn last(&self) -> Option<(Ms, f64)> {
        self.points.back().copied()
    }
    /// Points at or after `since`.
    pub fn since(&self, since: Ms) -> impl Iterator<Item = &(Ms, f64)> {
        self.points.iter().filter(move |(t, _)| *t >= since)
    }
    /// Smallest and largest value at or after `since`.
    pub fn range(&self, since: Ms) -> Option<(f64, f64)> {
        self.since(since).fold(None, |acc, (_, v)| match acc {
            None => Some((*v, *v)),
            Some((lo, hi)) => Some((lo.min(*v), hi.max(*v))),
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct History {
    series: BTreeMap<(Id, Metric), Series>,
}

impl History {
    /// Record a result's metrics for `check`.
    pub fn record(&mut self, check: Id, r: &ProbeResult, at: Ms) {
        for (m, v) in &r.metrics {
            if v.is_finite() {
                self.series.entry((check, *m)).or_default().push(at, *v);
            }
        }
    }
    pub fn series(&self, check: Id, m: Metric) -> Option<&Series> {
        self.series.get(&(check, m))
    }
    /// Drop the series of checks that are gone.
    pub fn retain(&mut self, keep: impl Fn(Id) -> bool) {
        self.series.retain(|(c, _), _| keep(*c));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::{Depth, Outcome};

    #[test]
    fn records_metrics_bounded_and_by_time() {
        let mut h = History::default();
        let mut r = ProbeResult::new(Outcome::Ok, Depth::Reachable, "ok");
        for i in 0..(MAX_POINTS as u64 + 10) {
            r.metrics = vec![(Metric::LatencyMs, i as f64), (Metric::LossPct, f64::NAN)];
            h.record(7, &r, i * 1000);
        }
        let s = h.series(7, Metric::LatencyMs).cloned().unwrap_or_default();
        assert_eq!(s.len(), MAX_POINTS);
        assert!(h.series(7, Metric::LossPct).is_none(), "NaN is not stored");
        let from = (MAX_POINTS as u64) * 1000;
        assert_eq!(s.since(from).count(), 10);
        assert_eq!(
            s.range(from),
            Some((MAX_POINTS as f64, MAX_POINTS as f64 + 9.0))
        );
        h.retain(|c| c != 7);
        assert!(h.series(7, Metric::LatencyMs).is_none());
    }
}
