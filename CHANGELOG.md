# Changelog

## 0.2.0

- The window (phase 3), in the suite's look: a start screen with new profile, open, recent
  profiles and a demo venue; Save, Save as / export, profiles saved shortly after each change.
- Monitor page: the overall light with symbol and word, the reasons ("IEM rack: not reachable
  since 15:54:35"), the counts and the acknowledged problems; a tile per device grouped by area
  (or problems first, or by name) with status, address, latency and "seen … ago"; search,
  "problems only", areas in the navigation with their own lights. Colour is never the only sign:
  every status has its own drawn symbol and word.
- Detail panel: status, cause and since when, acknowledge with a note, check now, every check
  with what it proves (check depth), how it was measured, values and raw data, latest events.
- Inventory: all devices in a list, several selected for bulk changes (area, criticality, on/off,
  Wi-Fi or wired thresholds), duplicate, delete with confirmation; a form per device with its
  checks (ping, TCP port; interval, timeout, thresholds, critical) and its debounce.
- Event log: newest first, search, filter by status and area, CSV export.
- Status bar with the ping method, the counts and the network load Pulse causes.
- App icon (a teal pulse), `install-mac.sh` and the DMG build, so Pulse can be installed on a Mac.

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
