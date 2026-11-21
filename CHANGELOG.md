# Changelog

## 0.1.0

- The core of Pulse (phase 2): inventory with devices, checks, thresholds and hysteresis per
  device; venue profiles as `.pulse` files (JSON with schema version, saved atomically).
- Status engine: red only after N failures in a row, green only after M passes, flapping turns
  yellow, partial failures are yellow, critical ones red. Area and overall lights by criticality
  (critical red -> red, important red or critical yellow -> yellow, optional at most yellow or
  ignored), always with a reason ("Stagebox SL: not reachable since 00:42"). Acknowledged
  problems stay visible but no longer colour the light and end by themselves once the device is OK.
- Every result says how deep the check looked (reachable, port open, present, answers, data).
- Probes: ping with latency, jitter and loss over a rolling window, without administrator rights
  (ICMP datagram sockets on macOS and Linux, `IcmpSendEcho` on Windows, TCP fallback otherwise,
  the method is shown); TCP port check (connect and close, nothing sent).
- Monitor: every check on its own interval, spread out in time, under a packet limit (50/s by
  default), each run isolated with a hard timeout; down checks back off; network load is counted.
- Event log with filter and CSV export; history of the measurements for the graphs.
