//! The status engine: debounces check results, works out each device's
//! status with a reason, spots flapping, keeps acknowledgements and combines
//! everything into area and overall lights by criticality.
//!
//! Pure logic: no clock, no network. Time comes in as unix milliseconds.

use crate::events::{Event, EventKind};
use crate::model::{CheckKind, Criticality, Device, Id, OptionalEffect, Profile, Rules};
use crate::probe::{Outcome, ProbeResult};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

/// Unix time in milliseconds.
pub type Ms = u64;

/// Status of a check, a device, an area or the whole show (the traffic light).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Status {
    /// Green.
    Ok,
    /// Yellow: degraded, partly failing, flapping.
    Warn,
    /// Red: not reachable or a critical check failed.
    Down,
    /// Grey: disabled, not checked yet, or the check cannot run.
    #[default]
    Unknown,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Ok => "OK",
            Status::Warn => "Warning",
            Status::Down => "Down",
            Status::Unknown => "Unknown",
        }
    }
    /// Ok < Warn < Down; Unknown ranks below Ok (it says nothing).
    pub fn rank(self) -> u8 {
        match self {
            Status::Unknown => 0,
            Status::Ok => 1,
            Status::Warn => 2,
            Status::Down => 3,
        }
    }
    pub fn worst(a: Status, b: Status) -> Status {
        if b.rank() > a.rank() {
            b
        } else {
            a
        }
    }
    fn is_bad(self) -> bool {
        matches!(self, Status::Warn | Status::Down)
    }
}

/// One check after debouncing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CheckState {
    pub status: Status,
    pub since: Ms,
    pub last: Option<ProbeResult>,
    pub last_at: Option<Ms>,
    /// Results in a row: passing, not passing (degraded or failed), failed, unknown.
    pub ok_run: u32,
    pub bad_run: u32,
    pub fail_run: u32,
    pub unknown_run: u32,
}

impl CheckState {
    /// Feed one result through the hysteresis; true if the status changed.
    fn feed(&mut self, r: &ProbeResult, fail_after: u32, ok_after: u32, now: Ms) -> bool {
        let fail_after = fail_after.max(1);
        let ok_after = ok_after.max(1);
        let first = self.last.is_none();
        match r.outcome {
            Outcome::Ok => {
                self.ok_run += 1;
                self.bad_run = 0;
                self.fail_run = 0;
                self.unknown_run = 0;
            }
            Outcome::Degraded => {
                self.ok_run = 0;
                self.bad_run += 1;
                self.fail_run = 0;
                self.unknown_run = 0;
            }
            Outcome::Fail => {
                self.ok_run = 0;
                self.bad_run += 1;
                self.fail_run += 1;
                self.unknown_run = 0;
            }
            Outcome::Unknown => {
                self.ok_run = 0;
                self.unknown_run += 1;
            }
        }
        let old = self.status;
        let new = if self.fail_run >= fail_after {
            Status::Down
        } else if self.bad_run >= fail_after {
            // Degraded long enough (or failing, not yet long enough for red).
            Status::Warn
        } else if self.unknown_run >= fail_after {
            Status::Unknown
        } else if r.outcome == Outcome::Ok
            && (first || old == Status::Unknown || self.ok_run >= ok_after)
        {
            // The first answer counts at once; recovering needs `ok_after`.
            Status::Ok
        } else {
            old
        };
        self.last = Some(r.clone());
        self.last_at = Some(now);
        if new != old {
            self.status = new;
            self.since = now;
            true
        } else {
            false
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Ack {
    pub at: Ms,
    pub note: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceState {
    /// What the tile shows (flapping included).
    pub status: Status,
    /// Since when `status` holds.
    pub since: Ms,
    /// Why it is not OK ("not reachable", "TCP 10023: connection refused").
    pub cause: String,
    pub last_seen: Option<Ms>,
    pub ack: Option<Ack>,
    pub flapping: bool,
    pub checks: BTreeMap<Id, CheckState>,
    /// Status without flapping, and when it last changed between good and bad.
    base: Status,
    flips: VecDeque<Ms>,
    /// A status other than "not checked yet" has been seen.
    started: bool,
}

/// Why an area or the overall light is not green: one device.
#[derive(Clone, Debug, PartialEq)]
pub struct Reason {
    pub device: Id,
    pub name: String,
    pub area: String,
    pub criticality: Criticality,
    /// The device's own status.
    pub status: Status,
    /// What it does to the light (by criticality).
    pub light: Status,
    pub cause: String,
    pub since: Ms,
}

impl Reason {
    /// "Stagebox SL: not reachable since 00:42" (`since` formatted by the caller).
    pub fn text(&self, since: &str) -> String {
        if since.is_empty() {
            format!("{}: {}", self.name, self.cause)
        } else {
            format!("{}: {} since {since}", self.name, self.cause)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub ok: usize,
    pub warn: usize,
    pub down: usize,
    pub unknown: usize,
    pub acked: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AreaLight {
    pub area: String,
    pub light: Status,
    pub counts: Counts,
    pub reasons: Vec<Reason>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Overview {
    pub light: Status,
    pub counts: Counts,
    /// Worst first: light, then criticality, then the oldest problem.
    pub reasons: Vec<Reason>,
    pub areas: Vec<AreaLight>,
    /// Acknowledged problems: still shown, not counted.
    pub acked: Vec<Reason>,
}

/// What a device of `criticality` in `status` does to the light; `None` =
/// nothing (unknown says nothing about the show).
pub fn contribution(criticality: Criticality, status: Status, rules: &Rules) -> Option<Status> {
    match status {
        Status::Unknown => None,
        Status::Ok => Some(Status::Ok),
        Status::Warn | Status::Down => Some(match criticality {
            Criticality::Critical => status,
            Criticality::Important => Status::Warn,
            Criticality::Optional => match rules.optional {
                OptionalEffect::UpToWarn => Status::Warn,
                OptionalEffect::Ignore => Status::Ok,
            },
        }),
    }
}

#[derive(Clone, Debug, Default)]
pub struct Engine {
    devices: BTreeMap<Id, DeviceState>,
}

impl Engine {
    pub fn new(p: &Profile, now: Ms) -> Self {
        let mut e = Engine::default();
        e.sync(p, now);
        e
    }

    pub fn device(&self, id: Id) -> Option<&DeviceState> {
        self.devices.get(&id)
    }

    /// Follow the inventory: new devices and checks start unknown, removed
    /// ones go, disabled ones turn grey.
    pub fn sync(&mut self, p: &Profile, now: Ms) -> Vec<Event> {
        self.devices.retain(|id, _| p.device(*id).is_some());
        for d in &p.devices {
            let st = self.devices.entry(d.id).or_insert_with(|| DeviceState {
                since: now,
                cause: "not checked yet".into(),
                ..DeviceState::default()
            });
            st.checks
                .retain(|id, _| d.check(*id).is_some_and(|c| c.enabled));
            for c in d.checks.iter().filter(|c| c.enabled) {
                st.checks.entry(c.id).or_insert_with(|| CheckState {
                    since: now,
                    ..CheckState::default()
                });
            }
        }
        let ids: Vec<Id> = p.devices.iter().map(|d| d.id).collect();
        let mut out = Vec::new();
        for id in ids {
            out.extend(self.update(p, id, now));
        }
        out
    }

    /// One probe result for `check` of `device`.
    pub fn apply(
        &mut self,
        p: &Profile,
        device: Id,
        check: Id,
        r: &ProbeResult,
        now: Ms,
    ) -> Vec<Event> {
        let Some(d) = p.device(device) else {
            return Vec::new();
        };
        let Some(st) = self.devices.get_mut(&device) else {
            return Vec::new();
        };
        let Some(cs) = st.checks.get_mut(&check) else {
            return Vec::new();
        };
        cs.feed(r, d.hysteresis.fail_after, d.hysteresis.ok_after, now);
        if r.answered() {
            st.last_seen = Some(now);
        }
        self.update(p, device, now)
    }

    /// Let time pass: flapping ends once the window holds fewer changes.
    pub fn tick(&mut self, p: &Profile, now: Ms) -> Vec<Event> {
        let ids: Vec<Id> = self.devices.keys().copied().collect();
        let mut out = Vec::new();
        for id in ids {
            out.extend(self.update(p, id, now));
        }
        out
    }

    /// Acknowledge a known problem ("Tablet 2 is off on purpose").
    pub fn ack(&mut self, p: &Profile, device: Id, note: &str, now: Ms) -> Option<Event> {
        let d = p.device(device)?;
        let st = self.devices.get_mut(&device)?;
        if st.status == Status::Ok || st.ack.is_some() {
            return None;
        }
        st.ack = Some(Ack {
            at: now,
            note: note.to_string(),
        });
        Some(Event::new(now, d, EventKind::Acked, note))
    }

    pub fn unack(&mut self, p: &Profile, device: Id, now: Ms) -> Option<Event> {
        let d = p.device(device)?;
        let st = self.devices.get_mut(&device)?;
        st.ack.take()?;
        Some(Event::new(now, d, EventKind::Unacked, ""))
    }

    fn update(&mut self, p: &Profile, device: Id, now: Ms) -> Vec<Event> {
        let mut out = Vec::new();
        let (Some(d), Some(st)) = (p.device(device), self.devices.get_mut(&device)) else {
            return out;
        };
        let (base, cause) = device_status(d, st);
        // Flapping: changes between good and bad within the window.
        let rule = p.rules.flapping;
        if st.started
            && base != Status::Unknown
            && st.base != Status::Unknown
            && base.is_bad() != st.base.is_bad()
        {
            st.flips.push_back(now);
        }
        if base != Status::Unknown {
            st.base = base;
        }
        let window = u64::from(rule.window_s) * 1000;
        while st
            .flips
            .front()
            .is_some_and(|t| now.saturating_sub(*t) > window)
        {
            st.flips.pop_front();
        }
        st.flapping = rule.changes > 0 && st.flips.len() as u32 >= rule.changes;
        let (status, cause) = if st.flapping && base == Status::Ok {
            (
                Status::Warn,
                format!(
                    "flapping ({} changes in {} min)",
                    st.flips.len(),
                    rule.window_s.div_ceil(60)
                ),
            )
        } else {
            (base, cause)
        };
        let old = st.status;
        st.cause = cause.clone();
        if status != old {
            st.status = status;
            st.since = now;
            // The very first result is no change worth logging.
            if st.started || status != Status::Ok {
                out.push(Event::new(
                    now,
                    d,
                    EventKind::Status {
                        from: old,
                        to: status,
                    },
                    &cause,
                ));
            }
            if status != Status::Unknown {
                st.started = true;
            }
            if status == Status::Ok && st.ack.take().is_some() {
                out.push(Event::new(now, d, EventKind::AckCleared, "OK again"));
            }
        }
        out
    }

    /// Area and overall lights with reasons.
    pub fn overview(&self, p: &Profile) -> Overview {
        let mut all = Vec::new();
        let mut acked = Vec::new();
        let mut areas: Vec<AreaLight> = p
            .areas()
            .into_iter()
            .map(|area| AreaLight {
                area,
                light: Status::Unknown,
                counts: Counts::default(),
                reasons: Vec::new(),
            })
            .collect();
        let mut overall = Status::Unknown;
        let mut counts = Counts::default();
        for d in &p.devices {
            let Some(st) = self.devices.get(&d.id) else {
                continue;
            };
            let Some(area) = areas.iter_mut().find(|a| a.area == d.area) else {
                continue;
            };
            let reason = |light: Status| Reason {
                device: d.id,
                name: d.name.clone(),
                area: d.area.clone(),
                criticality: d.criticality,
                status: st.status,
                light,
                cause: st.cause.clone(),
                since: st.since,
            };
            for c in [&mut counts, &mut area.counts] {
                match st.status {
                    Status::Ok => c.ok += 1,
                    Status::Warn => c.warn += 1,
                    Status::Down => c.down += 1,
                    Status::Unknown => c.unknown += 1,
                }
            }
            if st.ack.is_some() {
                counts.acked += 1;
                area.counts.acked += 1;
                acked.push(reason(Status::Ok));
                // Known, but no longer colouring the light.
                area.light = Status::worst(area.light, Status::Ok);
                overall = Status::worst(overall, Status::Ok);
                continue;
            }
            let Some(light) = contribution(d.criticality, st.status, &p.rules) else {
                continue;
            };
            area.light = Status::worst(area.light, light);
            overall = Status::worst(overall, light);
            if light.is_bad() {
                area.reasons.push(reason(light));
                all.push(reason(light));
            }
        }
        let order = |a: &Reason, b: &Reason| {
            b.light
                .rank()
                .cmp(&a.light.rank())
                .then(a.criticality.cmp(&b.criticality))
                .then(b.status.rank().cmp(&a.status.rank()))
                .then(a.since.cmp(&b.since))
        };
        all.sort_by(order);
        for a in &mut areas {
            a.reasons.sort_by(order);
        }
        Overview {
            light: overall,
            counts,
            reasons: all,
            areas,
            acked,
        }
    }
}

/// A device's status from its checks: down if a critical check (or every
/// check) is down, warning if any check is not OK, unknown if nothing is known.
fn device_status(d: &Device, st: &DeviceState) -> (Status, String) {
    if !d.enabled {
        return (Status::Unknown, "disabled".into());
    }
    let checks: Vec<_> = d
        .checks
        .iter()
        .filter(|c| c.enabled)
        .filter_map(|c| st.checks.get(&c.id).map(|s| (c, s)))
        .collect();
    if checks.is_empty() {
        return (Status::Unknown, "no checks".into());
    }
    let known: Vec<_> = checks
        .iter()
        .filter(|(_, s)| s.status != Status::Unknown)
        .collect();
    if known.is_empty() {
        let cause = checks
            .iter()
            .find_map(|(c, s)| {
                s.last
                    .as_ref()
                    .map(|r| format!("{} cannot run: {}", c.kind.label(), r.message))
            })
            .unwrap_or_else(|| "not checked yet".into());
        return (Status::Unknown, cause);
    }
    let cause_of = |c: &crate::model::Check, s: &CheckState| -> String {
        let msg = s.last.as_ref().map(|r| r.message.as_str()).unwrap_or("");
        match (&c.kind, s.status) {
            (CheckKind::Ping, Status::Down) => "not reachable".into(),
            _ if msg.is_empty() => c.kind.label(),
            _ => format!("{}: {msg}", c.kind.label()),
        }
    };
    let down: Vec<_> = known
        .iter()
        .filter(|(_, s)| s.status == Status::Down)
        .collect();
    if let Some((c, s)) = down.iter().find(|(c, _)| c.critical) {
        return (Status::Down, cause_of(c, s));
    }
    if !down.is_empty() && down.len() == known.len() {
        let (c, s) = down[0];
        return (Status::Down, cause_of(c, s));
    }
    if let Some((c, s)) = down.first() {
        return (Status::Warn, cause_of(c, s));
    }
    if let Some((c, s)) = known.iter().find(|(_, s)| s.status == Status::Warn) {
        return (Status::Warn, cause_of(c, s));
    }
    (Status::Ok, String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DeviceKind, FlapRule, Hysteresis};
    use crate::probe::Depth;

    fn res(o: Outcome) -> ProbeResult {
        let msg = match o {
            Outcome::Ok => "answers in 0.5 ms",
            Outcome::Degraded => "latency 45 ms (limit 20 ms)",
            Outcome::Fail => "no answer (1 of 1 lost)",
            Outcome::Unknown => "interface en1 is down",
        };
        ProbeResult::new(o, Depth::Reachable, msg)
    }

    struct T {
        p: Profile,
        e: Engine,
        now: Ms,
        log: Vec<Event>,
    }

    impl T {
        fn new() -> Self {
            let mut p = Profile::default();
            p.add_device(DeviceKind::Stagebox, "SB left", "10.0.0.11");
            let e = Engine::new(&p, 0);
            T {
                p,
                e,
                now: 0,
                log: Vec::new(),
            }
        }
        fn add(&mut self, kind: DeviceKind, name: &str, crit: Criticality) -> Id {
            let id = self.p.add_device(kind, name, "10.0.0.99");
            if let Some(d) = self.p.device_mut(id) {
                d.criticality = crit;
            }
            self.log.extend(self.e.sync(&self.p, self.now));
            id
        }
        fn first(&self) -> Id {
            self.p.devices[0].id
        }
        fn check(&self, dev: Id, i: usize) -> Id {
            self.p.device(dev).map(|d| d.checks[i].id).unwrap_or(0)
        }
        /// One result per second.
        fn feed(&mut self, dev: Id, i: usize, o: Outcome) -> Status {
            self.now += 1000;
            let c = self.check(dev, i);
            let ev = self.e.apply(&self.p, dev, c, &res(o), self.now);
            self.log.extend(ev);
            self.status(dev)
        }
        fn status(&self, dev: Id) -> Status {
            self.e
                .device(dev)
                .map(|s| s.status)
                .unwrap_or(Status::Unknown)
        }
    }

    #[test]
    fn starts_grey_then_green_on_the_first_answer() {
        let mut t = T::new();
        let d = t.first();
        assert_eq!(t.status(d), Status::Unknown);
        assert_eq!(
            t.e.device(d).map(|s| s.cause.as_str()),
            Some("not checked yet")
        );
        assert_eq!(t.feed(d, 0, Outcome::Ok), Status::Ok);
        assert!(t.log.is_empty(), "first green is not logged: {:?}", t.log);
        assert_eq!(t.e.device(d).and_then(|s| s.last_seen), Some(1000));
    }

    #[test]
    fn red_only_after_n_failures_green_only_after_m_passes() {
        let mut t = T::new();
        let d = t.first();
        t.feed(d, 0, Outcome::Ok);
        // Default: red after 3 failures in a row, green after 2 passes.
        assert_eq!(t.feed(d, 0, Outcome::Fail), Status::Ok);
        assert_eq!(t.feed(d, 0, Outcome::Fail), Status::Ok);
        assert_eq!(t.feed(d, 0, Outcome::Fail), Status::Down);
        assert_eq!(
            t.e.device(d).map(|s| s.cause.as_str()),
            Some("not reachable")
        );
        assert_eq!(t.e.device(d).map(|s| s.since), Some(4000));
        assert_eq!(t.feed(d, 0, Outcome::Ok), Status::Down);
        assert_eq!(t.feed(d, 0, Outcome::Ok), Status::Ok);
        let changes: Vec<_> = t
            .log
            .iter()
            .filter_map(|e| match e.kind {
                EventKind::Status { from, to } => Some((from, to)),
                _ => None,
            })
            .collect();
        assert_eq!(
            changes,
            vec![(Status::Ok, Status::Down), (Status::Down, Status::Ok)]
        );
    }

    #[test]
    fn a_single_pass_between_failures_resets_the_count() {
        let mut t = T::new();
        let d = t.first();
        t.feed(d, 0, Outcome::Ok);
        for _ in 0..10 {
            t.feed(d, 0, Outcome::Fail);
            t.feed(d, 0, Outcome::Fail);
            assert_eq!(t.feed(d, 0, Outcome::Ok), Status::Ok);
        }
        assert!(t.log.is_empty());
    }

    #[test]
    fn hysteresis_is_per_device() {
        let mut t = T::new();
        let d = t.first();
        if let Some(dev) = t.p.device_mut(d) {
            dev.hysteresis = Hysteresis {
                fail_after: 1,
                ok_after: 4,
            };
        }
        t.feed(d, 0, Outcome::Ok);
        assert_eq!(t.feed(d, 0, Outcome::Fail), Status::Down);
        for _ in 0..3 {
            assert_eq!(t.feed(d, 0, Outcome::Ok), Status::Down);
        }
        assert_eq!(t.feed(d, 0, Outcome::Ok), Status::Ok);
    }

    #[test]
    fn degraded_long_enough_is_yellow_with_the_reason() {
        let mut t = T::new();
        let d = t.first();
        t.feed(d, 0, Outcome::Ok);
        t.feed(d, 0, Outcome::Degraded);
        t.feed(d, 0, Outcome::Degraded);
        assert_eq!(t.feed(d, 0, Outcome::Degraded), Status::Warn);
        assert_eq!(
            t.e.device(d).map(|s| s.cause.clone()).unwrap_or_default(),
            "Ping: latency 45 ms (limit 20 ms)"
        );
        // Failing after degraded: red once failures alone reach the count.
        t.feed(d, 0, Outcome::Fail);
        t.feed(d, 0, Outcome::Fail);
        assert_eq!(t.feed(d, 0, Outcome::Fail), Status::Down);
    }

    #[test]
    fn unknown_results_turn_grey_but_do_not_count_as_failures() {
        let mut t = T::new();
        let d = t.first();
        t.feed(d, 0, Outcome::Ok);
        t.feed(d, 0, Outcome::Unknown);
        t.feed(d, 0, Outcome::Unknown);
        assert_eq!(t.feed(d, 0, Outcome::Unknown), Status::Unknown);
        let cause = t.e.device(d).map(|s| s.cause.clone()).unwrap_or_default();
        assert_eq!(cause, "Ping cannot run: interface en1 is down");
        // Back: the first answer counts at once.
        assert_eq!(t.feed(d, 0, Outcome::Ok), Status::Ok);
    }

    #[test]
    fn partial_failures_are_yellow_critical_ones_red() {
        let mut t = T::new();
        let d = t.first();
        t.p.add_check(d, CheckKind::Tcp { port: 4440 });
        t.e.sync(&t.p, t.now);
        t.feed(d, 0, Outcome::Ok);
        t.feed(d, 1, Outcome::Ok);
        for _ in 0..3 {
            t.feed(d, 1, Outcome::Fail);
        }
        // Ping fine, TCP down (not critical): yellow.
        assert_eq!(t.status(d), Status::Warn);
        assert_eq!(
            t.e.device(d).map(|s| s.cause.clone()).unwrap_or_default(),
            "TCP 4440: no answer (1 of 1 lost)"
        );
        // Ping (critical) down too: red.
        for _ in 0..3 {
            t.feed(d, 0, Outcome::Fail);
        }
        assert_eq!(t.status(d), Status::Down);
        assert_eq!(
            t.e.device(d).map(|s| s.cause.clone()).unwrap_or_default(),
            "not reachable"
        );
    }

    #[test]
    fn all_checks_down_is_red_even_without_a_critical_one() {
        let mut t = T::new();
        let d = t.first();
        if let Some(dev) = t.p.device_mut(d) {
            dev.checks[0].critical = false;
        }
        t.feed(d, 0, Outcome::Ok);
        for _ in 0..3 {
            t.feed(d, 0, Outcome::Fail);
        }
        assert_eq!(t.status(d), Status::Down);
    }

    #[test]
    fn disabled_devices_and_checks_are_grey_and_do_not_count() {
        let mut t = T::new();
        let d = t.first();
        t.feed(d, 0, Outcome::Ok);
        if let Some(dev) = t.p.device_mut(d) {
            dev.enabled = false;
        }
        t.e.sync(&t.p, t.now);
        assert_eq!(t.status(d), Status::Unknown);
        assert_eq!(t.e.device(d).map(|s| s.cause.as_str()), Some("disabled"));
        assert_eq!(t.e.overview(&t.p).light, Status::Unknown);
        if let Some(dev) = t.p.device_mut(d) {
            dev.enabled = true;
            dev.checks[0].enabled = false;
        }
        t.e.sync(&t.p, t.now);
        assert_eq!(t.e.device(d).map(|s| s.cause.as_str()), Some("no checks"));
    }

    #[test]
    fn flapping_turns_yellow_and_settles() {
        let mut t = T::new();
        let d = t.first();
        t.p.rules.flapping = FlapRule {
            changes: 4,
            window_s: 60,
        };
        if let Some(dev) = t.p.device_mut(d) {
            dev.hysteresis = Hysteresis {
                fail_after: 1,
                ok_after: 1,
            };
        }
        t.feed(d, 0, Outcome::Ok);
        for _ in 0..2 {
            t.feed(d, 0, Outcome::Fail);
            t.feed(d, 0, Outcome::Ok);
        }
        // Four changes within a minute: yellow although it answers right now.
        assert_eq!(t.status(d), Status::Warn);
        let st = t.e.device(d).cloned().unwrap_or_default();
        assert!(st.flapping);
        assert_eq!(st.cause, "flapping (4 changes in 1 min)");
        // Calm for longer than the window: green again.
        t.now += 61_000;
        t.e.tick(&t.p, t.now);
        assert_eq!(t.status(d), Status::Ok);
    }

    #[test]
    fn criticality_rules_for_the_overall_light() {
        let rules = Rules::default();
        use Criticality::*;
        assert_eq!(
            contribution(Critical, Status::Down, &rules),
            Some(Status::Down)
        );
        assert_eq!(
            contribution(Critical, Status::Warn, &rules),
            Some(Status::Warn)
        );
        assert_eq!(
            contribution(Important, Status::Down, &rules),
            Some(Status::Warn)
        );
        assert_eq!(
            contribution(Optional, Status::Down, &rules),
            Some(Status::Warn)
        );
        assert_eq!(contribution(Critical, Status::Unknown, &rules), None);
        let ignore = Rules {
            optional: OptionalEffect::Ignore,
            ..Rules::default()
        };
        assert_eq!(
            contribution(Optional, Status::Down, &ignore),
            Some(Status::Ok)
        );
    }

    #[test]
    fn overview_names_the_worst_device_first_per_area_and_overall() {
        let mut t = T::new();
        let sb = t.first();
        if let Some(d) = t.p.device_mut(sb) {
            d.criticality = Criticality::Critical;
        }
        let desk = t.add(DeviceKind::Console, "FOH desk", Criticality::Important);
        let tab = t.add(DeviceKind::Tablet, "iPad 2", Criticality::Optional);
        assert_eq!(
            t.e.overview(&t.p).light,
            Status::Unknown,
            "nothing checked yet"
        );
        for d in [sb, desk, tab] {
            t.feed(d, 0, Outcome::Ok);
        }
        let o = t.e.overview(&t.p);
        assert_eq!(o.light, Status::Ok);
        assert!(o.reasons.is_empty());
        assert_eq!(o.counts.ok, 3);

        for _ in 0..3 {
            t.feed(tab, 0, Outcome::Fail);
        }
        let o = t.e.overview(&t.p);
        assert_eq!(
            o.light,
            Status::Warn,
            "an optional device makes it yellow at most"
        );
        for _ in 0..3 {
            t.feed(desk, 0, Outcome::Fail);
        }
        assert_eq!(
            t.e.overview(&t.p).light,
            Status::Warn,
            "important down = yellow"
        );
        for _ in 0..3 {
            t.feed(sb, 0, Outcome::Fail);
        }
        let o = t.e.overview(&t.p);
        assert_eq!(o.light, Status::Down, "critical down = red");
        let names: Vec<_> = o.reasons.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["SB left", "FOH desk", "iPad 2"]);
        assert_eq!(
            o.reasons[0].text("00:42"),
            "SB left: not reachable since 00:42"
        );
        let stage = o.areas.iter().find(|a| a.area == "Stage").cloned();
        assert_eq!(stage.map(|a| a.light), Some(Status::Down));
        let foh = o.areas.iter().find(|a| a.area == "FOH").cloned();
        assert_eq!(foh.as_ref().map(|a| a.light), Some(Status::Warn));
        assert_eq!(foh.map(|a| a.reasons.len()), Some(2));
    }

    #[test]
    fn acknowledged_problems_stay_visible_and_clear_themselves() {
        let mut t = T::new();
        let d = t.first();
        if let Some(dev) = t.p.device_mut(d) {
            dev.criticality = Criticality::Critical;
        }
        t.feed(d, 0, Outcome::Ok);
        assert!(
            t.e.ack(&t.p, d, "nothing wrong", t.now).is_none(),
            "nothing to acknowledge"
        );
        for _ in 0..3 {
            t.feed(d, 0, Outcome::Fail);
        }
        assert_eq!(t.e.overview(&t.p).light, Status::Down);
        let ev = t.e.ack(&t.p, d, "Stagebox off on purpose", t.now);
        assert!(matches!(ev.map(|e| e.kind), Some(EventKind::Acked)));
        let o = t.e.overview(&t.p);
        assert_eq!(
            o.light,
            Status::Ok,
            "acknowledged: no longer colours the light"
        );
        assert_eq!(o.acked.len(), 1);
        assert_eq!(o.counts.acked, 1);
        assert_eq!(t.status(d), Status::Down, "the tile still shows it");
        // Green again: the acknowledgement goes by itself.
        t.feed(d, 0, Outcome::Ok);
        t.feed(d, 0, Outcome::Ok);
        assert!(t.e.device(d).is_some_and(|s| s.ack.is_none()));
        assert!(t.log.iter().any(|e| e.kind == EventKind::AckCleared));
        // And a new failure counts again.
        for _ in 0..3 {
            t.feed(d, 0, Outcome::Fail);
        }
        assert_eq!(t.e.overview(&t.p).light, Status::Down);
        assert!(t.e.ack(&t.p, d, "", t.now).is_some());
        assert!(t.e.unack(&t.p, d, t.now).is_some());
        assert_eq!(t.e.overview(&t.p).light, Status::Down);
    }

    #[test]
    fn removed_devices_leave_the_engine() {
        let mut t = T::new();
        let d = t.first();
        t.p.devices.clear();
        t.e.sync(&t.p, t.now);
        assert!(t.e.device(d).is_none());
        // Results for it are ignored.
        assert!(t.e.apply(&t.p, d, 2, &res(Outcome::Fail), t.now).is_empty());
    }
}
