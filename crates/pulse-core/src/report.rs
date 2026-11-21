//! The pre-show report: what the thorough check found, who released the
//! technology and when. One self-contained HTML file (prints well, opens in
//! any browser) and a CSV with one line per check.

use crate::events::csv_field;
use crate::model::{Criticality, Profile};
use crate::probe::Depth;
use crate::status::{Engine, Overview, Status};

/// One check in the report.
#[derive(Clone, Debug, PartialEq)]
pub struct CheckRow {
    pub label: String,
    pub status: Status,
    pub depth: Option<Depth>,
    pub message: String,
    /// "Latency 0.4 ms · Loss 0 %".
    pub values: String,
    pub via: String,
}

/// One device in the report.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceRow {
    pub name: String,
    pub area: String,
    pub kind: String,
    pub address: String,
    pub criticality: Criticality,
    pub status: Status,
    pub cause: String,
    /// Acknowledged as known, with the note.
    pub acked: Option<String>,
    pub checks: Vec<CheckRow>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreShowReport {
    pub profile: String,
    pub venue: String,
    /// Local date and time the check started and was signed off ("2026-11-21 18:30").
    pub started: String,
    pub signed_at: String,
    pub signed_by: String,
    pub light: Status,
    /// The overall reason lines ("IEM rack: not reachable").
    pub reasons: Vec<String>,
    pub devices: Vec<DeviceRow>,
    /// "Pulse 0.4.0".
    pub generator: String,
}

impl PreShowReport {
    /// Collect the report from the inventory and the current state.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        p: &Profile,
        e: &Engine,
        o: &Overview,
        started: &str,
        signed_at: &str,
        signed_by: &str,
        generator: &str,
    ) -> Self {
        let mut devices: Vec<DeviceRow> = p
            .devices
            .iter()
            .filter(|d| d.enabled)
            .map(|d| {
                let st = e.device(d.id);
                let checks = d
                    .checks
                    .iter()
                    .filter(|c| c.enabled)
                    .map(|c| {
                        let cs = st.and_then(|s| s.checks.get(&c.id));
                        let last = cs.and_then(|s| s.last.as_ref());
                        CheckRow {
                            label: c.kind.label(),
                            status: cs.map(|s| s.status).unwrap_or_default(),
                            depth: last.map(|r| r.depth),
                            message: last
                                .map(|r| r.message.clone())
                                .unwrap_or_else(|| "not run".into()),
                            values: last
                                .map(|r| {
                                    r.metrics
                                        .iter()
                                        .map(|(k, v)| format!("{} {}", k.label(), k.format(*v)))
                                        .collect::<Vec<_>>()
                                        .join(" · ")
                                })
                                .unwrap_or_default(),
                            via: last.map(|r| r.via.clone()).unwrap_or_default(),
                        }
                    })
                    .collect();
                DeviceRow {
                    name: d.name.clone(),
                    area: d.area.clone(),
                    kind: d.kind.label().into(),
                    address: d.address.clone(),
                    criticality: d.criticality,
                    status: st.map(|s| s.status).unwrap_or_default(),
                    cause: st.map(|s| s.cause.clone()).unwrap_or_default(),
                    acked: st.and_then(|s| s.ack.as_ref()).map(|a| a.note.clone()),
                    checks,
                }
            })
            .collect();
        // Problems first, then by area and name.
        devices.sort_by(|a, b| {
            b.status
                .rank()
                .cmp(&a.status.rank())
                .then_with(|| a.area.cmp(&b.area))
                .then_with(|| a.name.cmp(&b.name))
        });
        PreShowReport {
            profile: p.name.clone(),
            venue: p.venue.clone(),
            started: started.into(),
            signed_at: signed_at.into(),
            signed_by: signed_by.into(),
            light: o.light,
            reasons: o.reasons.iter().map(|r| r.text("")).collect(),
            devices,
            generator: generator.into(),
        }
    }

    pub fn problems(&self) -> usize {
        self.devices
            .iter()
            .filter(|d| matches!(d.status, Status::Warn | Status::Down))
            .count()
    }

    /// "Released: everything OK" / "Released with 2 known problems".
    pub fn verdict(&self) -> String {
        match self.problems() {
            0 => "Released: every device OK".into(),
            1 => "Released with 1 known problem".into(),
            n => format!("Released with {n} known problems"),
        }
    }

    /// One line per check (devices without checks get one line too).
    pub fn csv(&self) -> String {
        let mut out = String::from(
            "Device,Area,Kind,Address,Criticality,Device status,Cause,Acknowledged,Check,Check status,Checks,Message,Values,Via\n",
        );
        for d in &self.devices {
            let head = [
                d.name.as_str(),
                &d.area,
                &d.kind,
                &d.address,
                d.criticality.label(),
                d.status.label(),
                &d.cause,
                d.acked
                    .as_deref()
                    .map(|n| if n.is_empty() { "yes" } else { n })
                    .unwrap_or(""),
            ]
            .map(csv_field)
            .join(",");
            if d.checks.is_empty() {
                out.push_str(&format!("{head},,,,,,\n"));
            }
            for c in &d.checks {
                let tail = [
                    c.label.as_str(),
                    c.status.label(),
                    c.depth.map(|x| x.label()).unwrap_or(""),
                    &c.message,
                    &c.values,
                    &c.via,
                ]
                .map(csv_field)
                .join(",");
                out.push_str(&format!("{head},{tail}\n"));
            }
        }
        out
    }

    /// A self-contained HTML page (no scripts, no external files).
    pub fn html(&self) -> String {
        let title = format!("Pre-show check: {}", esc(&self.profile));
        let mut rows = String::new();
        for d in &self.devices {
            let checks: Vec<String> = d
                .checks
                .iter()
                .map(|c| {
                    format!(
                        "<div class=\"chk\"><span class=\"st {}\">{}</span> <b>{}</b> {}{}{}</div>",
                        class(c.status),
                        symbol(c.status),
                        esc(&c.label),
                        esc(&c.message),
                        if c.values.is_empty() {
                            String::new()
                        } else {
                            format!(" <span class=\"m\">{}</span>", esc(&c.values))
                        },
                        c.depth
                            .map(|x| format!(
                                " <span class=\"m\">(checks: {})</span>",
                                esc(x.label())
                            ))
                            .unwrap_or_default(),
                    )
                })
                .collect();
            let note = match &d.acked {
                Some(n) if n.is_empty() => {
                    "<div class=\"ack\">Acknowledged as known</div>".to_string()
                }
                Some(n) => format!("<div class=\"ack\">Acknowledged as known: {}</div>", esc(n)),
                None => String::new(),
            };
            rows.push_str(&format!(
                "<tr><td class=\"st {}\">{} {}</td><td><b>{}</b><div class=\"m\">{} · {}</div></td><td>{}</td><td class=\"mono\">{}</td><td>{}</td><td>{}{}{}</td></tr>\n",
                class(d.status),
                symbol(d.status),
                d.status.label(),
                esc(&d.name),
                esc(&d.kind),
                esc(d.criticality.label()),
                esc(&d.area),
                esc(&d.address),
                esc(d.criticality.label()),
                if d.cause.is_empty() { String::new() } else { format!("<div>{}</div>", esc(&d.cause)) },
                checks.join(""),
                note,
            ));
        }
        let reasons = if self.reasons.is_empty() {
            String::new()
        } else {
            format!(
                "<ul>{}</ul>",
                self.reasons
                    .iter()
                    .map(|r| format!("<li>{}</li>", esc(r)))
                    .collect::<String>()
            )
        };
        let venue = if self.venue.is_empty() {
            String::new()
        } else {
            format!(" · {}", esc(&self.venue))
        };
        format!(
            r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>{title}</title>
<style>
body{{font-family:-apple-system,"Segoe UI",Roboto,Helvetica,Arial,sans-serif;color:#111;margin:32px;}}
h1{{font-size:22px;margin:0 0 4px}} .sub{{color:#555;margin-bottom:18px}}
.box{{border:2px solid #999;border-radius:8px;padding:12px 16px;margin-bottom:18px}}
.box.ok{{border-color:#16a34a}} .box.warn{{border-color:#ca8a04}} .box.down{{border-color:#dc2626}}
.verdict{{font-size:18px;font-weight:600}} .sig{{margin-top:6px}}
table{{border-collapse:collapse;width:100%;font-size:13px}} th,td{{text-align:left;vertical-align:top;padding:6px 8px;border-bottom:1px solid #ddd}}
th{{font-size:11px;text-transform:uppercase;color:#666}}
.st{{font-weight:600;white-space:nowrap}} .ok{{color:#15803d}} .warn{{color:#a16207}} .down{{color:#b91c1c}} .unknown{{color:#6b7280}}
.m{{color:#666;font-size:12px}} .mono{{font-family:Menlo,Consolas,monospace}} .chk{{margin-top:2px}} .ack{{color:#6b7280;font-style:italic}}
footer{{margin-top:24px;color:#888;font-size:11px}}
@media print{{body{{margin:12mm}} tr{{page-break-inside:avoid}}}}
</style></head><body>
<h1>{title}</h1>
<div class="sub">{profile}{venue} · started {started}</div>
<div class="box {lclass}">
<div class="verdict">{verdict}</div>
<div>Overall: <span class="st {lclass}">{lsym} {lword}</span> · {n} devices, {probs} with problems</div>
{reasons}
<div class="sig">Technology released by <b>{by}</b> at {at}</div>
</div>
<table><thead><tr><th>Status</th><th>Device</th><th>Area</th><th>Address</th><th>Criticality</th><th>Result</th></tr></thead>
<tbody>
{rows}</tbody></table>
<footer>{gen}. A pass means only what its check proves (reachable, port open, present, answers, data).</footer>
</body></html>
"#,
            profile = esc(&self.profile),
            started = esc(&self.started),
            lclass = class(self.light),
            verdict = esc(&self.verdict()),
            lsym = symbol(self.light),
            lword = self.light.label(),
            n = self.devices.len(),
            probs = self.problems(),
            by = esc(&self.signed_by),
            at = esc(&self.signed_at),
            gen = esc(&self.generator),
        )
    }
}

fn class(s: Status) -> &'static str {
    match s {
        Status::Ok => "ok",
        Status::Warn => "warn",
        Status::Down => "down",
        Status::Unknown => "unknown",
    }
}

fn symbol(s: Status) -> &'static str {
    match s {
        Status::Ok => "\u{2714}",
        Status::Warn => "\u{25B2}",
        Status::Down => "\u{2716}",
        Status::Unknown => "\u{2013}",
    }
}

/// Escape text for HTML.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CheckKind, DeviceKind};
    use crate::probe::{Metric, Outcome, ProbeResult};

    #[test]
    fn report_lists_problems_first_and_escapes_everything() {
        let mut p = Profile {
            name: "Riverside <Hall>".into(),
            ..Profile::default()
        };
        let ok = p.add_device(DeviceKind::Console, "FOH desk", "10.0.0.1");
        let bad = p.add_device(DeviceKind::Other, "IEM \"rack\"", "10.0.0.20");
        p.add_check(ok, CheckKind::Tcp { port: 10023 });
        let mut e = Engine::new(&p, 0);
        let d = p.device(ok).cloned().unwrap_or_default();
        let mut r = ProbeResult::new(Outcome::Ok, Depth::Reachable, "answers");
        r.metrics.push((Metric::LatencyMs, 0.4));
        e.apply(&p, ok, d.checks[0].id, &r, 1000);
        let b = p.device(bad).cloned().unwrap_or_default();
        for t in 0..3 {
            e.apply(
                &p,
                bad,
                b.checks[0].id,
                &ProbeResult::new(Outcome::Fail, Depth::Reachable, "no answer"),
                1000 + t,
            );
        }
        e.ack(&p, bad, "rack <off>", 2000);
        let o = e.overview(&p);
        let rep = PreShowReport::build(
            &p,
            &e,
            &o,
            "2026-11-21 18:00",
            "2026-11-21 18:10",
            "Jan & Co",
            "Pulse test",
        );
        assert_eq!(rep.devices[0].name, "IEM \"rack\"");
        assert_eq!(rep.problems(), 1);
        assert_eq!(rep.verdict(), "Released with 1 known problem");
        let html = rep.html();
        assert!(html.contains("Pre-show check: Riverside &lt;Hall&gt;"));
        assert!(html.contains("Technology released by <b>Jan &amp; Co</b> at 2026-11-21 18:10"));
        assert!(html.contains("IEM &quot;rack&quot;"));
        assert!(html.contains("Acknowledged as known: rack &lt;off&gt;"));
        assert!(html.contains("Latency 0.4 ms"));
        assert!(!html.contains("<script"));
        let csv = rep.csv();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 1 + 3, "{csv}");
        assert!(
            lines[1].starts_with("\"IEM \"\"rack\"\"\",Stage,Other,10.0.0.20,Important,Down,"),
            "{}",
            lines[1]
        );
        assert!(csv.contains("TCP 10023,Unknown,,not run"));
    }
}
