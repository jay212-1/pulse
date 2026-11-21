//! Plain-language hints for the diagnosis view: from a device's checks and
//! their last results, what to look at first. Rules of thumb, said as such.

use crate::discovery::Net;
use crate::model::{CheckKind, Device, DeviceKind};
use crate::probe::{Metric, Outcome, ProbeResult};
use std::net::Ipv4Addr;

/// Hints for `d`, given the last result of each of its checks (by position)
/// and the networks of this computer.
pub fn hints(d: &Device, last: &[Option<&ProbeResult>], nets: &[Net]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut say = |s: String| {
        if !out.contains(&s) {
            out.push(s);
        }
    };
    let results: Vec<(&CheckKind, &crate::model::Check, &ProbeResult)> = d
        .checks
        .iter()
        .zip(last.iter())
        .filter(|(c, _)| c.enabled)
        .filter_map(|(c, r)| r.map(|r| (&c.kind, c, r)))
        .collect();
    let failed = |r: &ProbeResult| r.outcome == Outcome::Fail;
    let ping_failed = results
        .iter()
        .any(|(k, _, r)| **k == CheckKind::Ping && failed(r));
    let other_ok = results
        .iter()
        .any(|(k, _, r)| **k != CheckKind::Ping && !k.passive() && r.answered());
    let ping_ok = results
        .iter()
        .any(|(k, _, r)| **k == CheckKind::Ping && r.answered());

    // The address is outside every network of this computer.
    if let Ok(ip) = d.address.trim().parse::<Ipv4Addr>() {
        if !ip.is_loopback() && !nets.is_empty() && !nets.iter().any(|n| n.contains(ip)) {
            let list: Vec<String> = nets.iter().map(|n| n.label()).collect();
            say(format!(
                "{ip} is not in a network of this computer ({}). Without a router it cannot be reached: check the address, or this computer's network settings.",
                list.join(", ")
            ));
        }
    }
    if ping_failed && other_ok {
        say("It answers other checks but not ping: its firewall probably blocks ICMP. Make another check the critical one.".into());
    } else if ping_failed {
        say("No answer at all: check the power, the cable and the switch port (link light), and that the address is right.".into());
    }
    for (k, c, r) in &results {
        let msg = r.message.to_lowercase();
        if msg.contains("connection refused") {
            say("The device is there, but nothing listens on that port: is the software or service running?".into());
        }
        if (msg.contains("interface") && msg.contains("down or missing"))
            || (matches!(k, CheckKind::Interface { .. }) && failed(r))
        {
            say("A network interface of this computer is down: check its cable or adapter.".into());
        }
        match k {
            CheckKind::Ping => {
                let th = c.thresholds;
                let loss = r.metric(Metric::LossPct).unwrap_or(0.0);
                if loss > 0.0 && loss < 100.0 {
                    if d.kind == DeviceKind::Tablet || th.latency_ms > 50.0 {
                        say("Losses on Wi-Fi: check the signal and the channel. Stage Wi-Fi gets crowded when the audience arrives.".into());
                    } else {
                        say("Losses on a cable: check the cable, the switch port and its error counters.".into());
                    }
                }
                if r.metric(Metric::JitterMs).is_some_and(|j| j > th.jitter_ms) {
                    say("Uneven answer times: the network or the device is busy (large transfers, a broadcast storm).".into());
                }
                if r.metric(Metric::LatencyMs).is_some_and(|l| l > th.latency_ms) && th.latency_ms <= 50.0 {
                    say("Slow answers on a wired network usually mean an overloaded switch or a busy device.".into());
                }
            }
            CheckKind::Console { model } if failed(r) && ping_ok => say(format!(
                "The desk answers ping but not the {} query: is it that model? Network remote control may be off on the desk.",
                model.label()
            )),
            CheckKind::ArtNetNode if failed(r) && ping_ok => say(
                "The node does not answer ArtPoll: it may use sACN only, or another Art-Net net.".into(),
            ),
            CheckKind::ArtNetUniverse { .. } => {
                if failed(r) {
                    say("No frames: is the desk sending this universe? Pulse sees only Art-Net that is broadcast or sent to this computer; frames sent straight to a node are invisible here.".into());
                } else if msg.contains("conflict") {
                    say("Two senders send the same universe: the lights flicker between them. Switch one off or move it to another universe.".into());
                } else if r.outcome == Outcome::Degraded {
                    say("Few frames per second: some desks send less when nothing changes. Lower the minimum if that is normal for this desk.".into());
                }
            }
            CheckKind::Mdns { .. } | CheckKind::Ndi { .. } if failed(r) => say(
                "Announcements use multicast: switches with IGMP snooping but no querier drop them after a few minutes. Check the switch's IGMP settings.".into(),
            ),
            CheckKind::Ndi { .. } if r.outcome == Outcome::Degraded => say(
                "The NDI source is announced but takes no connections: the sending software may hang, or a firewall blocks it.".into(),
            ),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Profile;
    use crate::probe::Depth;

    fn res(o: Outcome, msg: &str) -> ProbeResult {
        ProbeResult::new(o, Depth::Reachable, msg)
    }

    #[test]
    fn firewall_wrong_subnet_and_refused() {
        let mut p = Profile::default();
        let id = p.add_device(DeviceKind::Console, "Desk", "10.0.1.5");
        p.add_check(id, CheckKind::Tcp { port: 10023 });
        let d = p.device(id).cloned().unwrap_or_default();
        let net = Net {
            addr: Ipv4Addr::new(192, 168, 1, 10),
            prefix: 24,
        };
        let ping = res(Outcome::Fail, "no answer");
        let tcp = res(Outcome::Ok, "open");
        let h = hints(&d, &[Some(&ping), Some(&tcp)], &[net]);
        assert!(
            h[0].contains("not in a network of this computer (192.168.1.0/24)"),
            "{h:?}"
        );
        assert!(h[1].contains("firewall"), "{h:?}");

        let refused = res(
            Outcome::Fail,
            "connection refused (the device is there, the port is closed)",
        );
        let h = hints(&d, &[Some(&ping), Some(&refused)], &[]);
        assert!(h.iter().any(|s| s.contains("No answer at all")), "{h:?}");
        assert!(h.iter().any(|s| s.contains("nothing listens")), "{h:?}");
    }

    #[test]
    fn wifi_losses_and_art_net() {
        let mut p = Profile::default();
        let id = p.add_device(DeviceKind::Tablet, "iPad", "10.0.0.50");
        let d = p.device(id).cloned().unwrap_or_default();
        let mut r = res(Outcome::Degraded, "loss 20 %");
        r.metrics.push((Metric::LossPct, 20.0));
        let h = hints(&d, &[Some(&r)], &[]);
        assert!(h[0].starts_with("Losses on Wi-Fi"), "{h:?}");

        let u = p.add_device(DeviceKind::Console, "Light desk", "");
        p.add_check(
            u,
            CheckKind::ArtNetUniverse {
                universe: 1,
                min_fps: 20.0,
            },
        );
        let d = p.device(u).cloned().unwrap_or_default();
        let none = res(Outcome::Fail, "no frames of universe 1");
        let h = hints(&d, &[None, Some(&none)], &[]);
        assert!(h[0].contains("invisible here"), "{h:?}");
        assert!(hints(&d, &[None, None], &[]).is_empty());
    }
}
