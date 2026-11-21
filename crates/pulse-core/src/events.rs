//! The event log: every status change with time, device, old -> new and
//! cause, plus acknowledgements. Kept in memory (bounded), filterable,
//! exported as CSV.

use crate::model::{Device, Id};
use crate::status::{Ms, Status};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Events kept; the oldest go first.
pub const MAX_EVENTS: usize = 20_000;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Status {
        from: Status,
        to: Status,
    },
    Acked,
    Unacked,
    /// The device is OK again, so its acknowledgement ended by itself.
    AckCleared,
}

impl EventKind {
    pub fn label(&self) -> String {
        match self {
            EventKind::Status { from, to } => format!("{} -> {}", from.label(), to.label()),
            EventKind::Acked => "Acknowledged".into(),
            EventKind::Unacked => "Acknowledgement removed".into(),
            EventKind::AckCleared => "Acknowledgement ended".into(),
        }
    }
    /// The status it changed to, if it is a status change.
    pub fn to(&self) -> Option<Status> {
        match self {
            EventKind::Status { to, .. } => Some(*to),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Event {
    pub at: Ms,
    pub device: Id,
    pub name: String,
    pub area: String,
    pub kind: EventKind,
    pub cause: String,
}

impl Event {
    pub fn new(at: Ms, d: &Device, kind: EventKind, cause: &str) -> Self {
        Event {
            at,
            device: d.id,
            name: d.name.clone(),
            area: d.area.clone(),
            kind,
            cause: cause.to_string(),
        }
    }
}

/// What the log view shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    /// Matches name, area and cause, ignoring case.
    pub text: String,
    pub device: Option<Id>,
    pub area: Option<String>,
    /// Only changes to these statuses (empty = all, acknowledgements included).
    pub to: Vec<Status>,
}

impl Filter {
    pub fn matches(&self, e: &Event) -> bool {
        if self.device.is_some_and(|d| d != e.device) {
            return false;
        }
        if self.area.as_ref().is_some_and(|a| *a != e.area) {
            return false;
        }
        if !self.to.is_empty() && !e.kind.to().is_some_and(|s| self.to.contains(&s)) {
            return false;
        }
        let q = self.text.trim().to_lowercase();
        q.is_empty()
            || e.name.to_lowercase().contains(&q)
            || e.area.to_lowercase().contains(&q)
            || e.cause.to_lowercase().contains(&q)
    }
}

#[derive(Clone, Debug, Default)]
pub struct EventLog {
    events: VecDeque<Event>,
}

impl EventLog {
    pub fn push(&mut self, e: Event) {
        self.events.push_back(e);
        while self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
    }
    pub fn extend(&mut self, es: impl IntoIterator<Item = Event>) {
        for e in es {
            self.push(e);
        }
    }
    pub fn len(&self) -> usize {
        self.events.len()
    }
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
    /// Oldest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Event> {
        self.events.iter()
    }
    pub fn filtered<'a>(&'a self, f: &'a Filter) -> impl DoubleEndedIterator<Item = &'a Event> {
        self.events.iter().filter(move |e| f.matches(e))
    }
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// CSV (comma separated, quoted where needed). `time` formats a timestamp
    /// (local time is the caller's business).
    pub fn to_csv<'a>(
        events: impl IntoIterator<Item = &'a Event>,
        time: impl Fn(Ms) -> String,
    ) -> String {
        let mut out = String::from("Time,Device,Area,Event,From,To,Cause\n");
        for e in events {
            let (from, to) = match e.kind {
                EventKind::Status { from, to } => (from.label(), to.label()),
                _ => ("", ""),
            };
            let row = [
                time(e.at),
                e.name.clone(),
                e.area.clone(),
                e.kind.label(),
                from.into(),
                to.into(),
                e.cause.clone(),
            ];
            out.push_str(
                &row.iter()
                    .map(|f| csv_field(f))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            out.push('\n');
        }
        out
    }
}

fn csv_field(s: &str) -> String {
    // A leading = + - @ would run as a formula in a spreadsheet.
    let s = if s.starts_with(['=', '+', '-', '@']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(at: Ms, name: &str, area: &str, to: Status, cause: &str) -> Event {
        Event {
            at,
            device: at,
            name: name.into(),
            area: area.into(),
            kind: EventKind::Status {
                from: Status::Ok,
                to,
            },
            cause: cause.into(),
        }
    }

    #[test]
    fn filter_by_text_area_device_and_status() {
        let mut log = EventLog::default();
        log.push(ev(1, "SB left", "Stage", Status::Down, "not reachable"));
        log.push(ev(
            2,
            "FOH desk",
            "FOH",
            Status::Warn,
            "Ping: latency 45 ms",
        ));
        log.push(ev(3, "iPad", "FOH", Status::Ok, ""));
        let names = |f: &Filter| log.filtered(f).map(|e| e.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&Filter::default()).len(), 3);
        let f = Filter {
            text: "LATENCY".into(),
            ..Filter::default()
        };
        assert_eq!(names(&f), vec!["FOH desk"]);
        let f = Filter {
            area: Some("FOH".into()),
            to: vec![Status::Ok],
            ..Filter::default()
        };
        assert_eq!(names(&f), vec!["iPad"]);
        let f = Filter {
            device: Some(1),
            ..Filter::default()
        };
        assert_eq!(names(&f), vec!["SB left"]);
    }

    #[test]
    fn bounded() {
        let mut log = EventLog::default();
        for i in 0..(MAX_EVENTS as u64 + 5) {
            log.push(ev(i, "x", "y", Status::Ok, ""));
        }
        assert_eq!(log.len(), MAX_EVENTS);
        assert_eq!(log.iter().next().map(|e| e.at), Some(5));
    }

    #[test]
    fn csv_quotes_and_defuses_formulas() {
        let es = [
            ev(1, "Desk, FOH", "FOH", Status::Down, "said \"no\""),
            ev(2, "=HYPERLINK()", "FOH", Status::Ok, ""),
        ];
        let csv = EventLog::to_csv(es.iter(), |t| format!("t{t}"));
        let lines: Vec<_> = csv.lines().collect();
        assert_eq!(lines[0], "Time,Device,Area,Event,From,To,Cause");
        assert_eq!(
            lines[1],
            "t1,\"Desk, FOH\",FOH,OK -> Down,OK,Down,\"said \"\"no\"\"\""
        );
        assert!(lines[2].starts_with("t2,'=HYPERLINK(),"));
    }
}
