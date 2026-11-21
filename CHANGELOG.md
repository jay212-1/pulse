# Changelog

## 0.3.0

- Protocol checks (phase 4): console queries (Behringer X32/M32 and X Air via OSC `/xinfo`, Wing
  via its discovery answer; name, model and firmware, nothing is changed on the desk),
  Art-Net nodes (ArtPoll every 3 s, only while needed, answered by ArtPollReply), Art-Net
  universes (frames per second, last frame, who sends, a warning when two senders collide or
  the frame rate drops), mDNS services (Dante, Lantern or any type), NDI sources (announced via
  `_ndi._tcp` and their port open; no NDI SDK needed), and network interfaces of this computer.
- Devices can name the interface to check from; if it is down, their checks say so (unknown,
  not red) instead of blaming the device.
- Presets: X32/M32, X Air, Wing, other consoles (basic checks), Dante device, NDI source,
  Art-Net node, lighting desk, tablet on Wi-Fi, Show Master, Lantern, switch, access point.
- Auto discovery: a new page lists what announces itself or answers (mDNS, NDI, Dante,
  Art-Net nodes, consoles) and, after you confirm it, a gentle ping sweep of one subnet
  (20 addresses per second). New devices are added with one click (or all at once) with the
  preset that suits them; devices already in the inventory and expected devices nobody saw are
  listed too. The navigation shows how many new devices were found.
- Inventory: "Add from preset", "Add check" with every kind of check and their settings, an
  interface chooser.

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
