# Changelog

## 1.0.0

- First stable release: the development phase is complete. From here on there are fixes and smaller
  improvements.

## 0.7.0

- Packaging (phase 8): Windows installer (`Pulse-Setup-<v>.exe`, Inno Setup, no admin rights, into
  `Showtime Suite\Pulse`), Linux archive (`pulse-<v>-linux-x86_64.tar.gz`, with menu entry and
  icon), `install.sh` for Linux from the source, and `packaging/build-on-linux.sh`, which builds
  Linux and Windows on one Linux PC.
- Pulse Agent as a download: `pulse-agent-<v>-macos.zip`, `pulse-agent-<v>-windows-x86_64.zip` and
  `pulse-agent-<v>-x86_64-linux.tar.gz`, each with a readme (start, firewall, start at login).
  `pulse-agent --version`.
- `release.sh` as in the other suite apps: builds what this computer can build and uploads it to
  `jay212-1/pulse-releases`; `status` shows what is online, `dry-run` builds and uploads nothing.
- `.pulse` files open with a double-click: on the Mac through the "open documents" event (as in
  Show Master and Blueprint), on Windows through the installer's file association.
- README in English, user guide in German (`docs/ANLEITUNG.md`): profiles, presets including
  Soundcraft Si and grandMA3, the agent, the modes, the status page, what to do when something is
  red.

## 0.6.0

- Status page (phase 7): Pulse's traffic light for phones and other computers of the crew, read-only,
  on TCP 7470, with problems first, areas and all devices; it updates every 2 seconds and says when
  the connection to Pulse is lost. Off by default; "Status page" in the status bar switches it on
  and shows its address. While on, it is announced as `_pulse._tcp`.
- Pulse Agent: a small program for show computers (`pulse-agent`, TCP 7471). Asked by Pulse, it
  tells CPU, memory, free disk space and whether named programs run (e.g. Show Master, Lantern,
  QLab). It only answers; it sends nothing by itself and changes nothing. New check "Pulse Agent"
  and preset "Show computer with Pulse Agent".
- sACN (E1.31): new check "sACN universe": frames per second and who sends at which priority. A
  backup at a lower priority is fine, two sources at the same priority are a conflict. Pulse joins
  the universe's multicast group only while a check needs it. Preset "Lighting desk (sACN)".
- SNMP: new check "SNMP switch" (v2c, read-only): the switch answers, and a chosen port is up and
  gets no new errors; a restart shows. Preset "Managed switch (SNMP)".
- Import from Blueprint: "Import from Blueprint…" in the inventory takes over the devices of a
  Showtime exchange file (Blueprint 0.2.4 and later) with a preset from their kind and model, the
  location as the area and their address. Devices without an address come in switched off;
  devices already there only get what they lack.
- Presets for the Soundcraft Si Impact / Expression / Performer (ping and the HiQnet control port
  TCP 3804) and grandMA3 (ping, ArtPoll and the universe's frames).
- Frame rates are right from the first frame: no false "only 2 frames per second" when a universe
  starts or Pulse has just started.
- The demo has a media server sending sACN, the core switch over SNMP and Pulse Agent on the backup
  computer; now and then Lantern stops there.
- Simulator: sACN sender, SNMP switch and Pulse Agent.

## 0.5.0

- Simulator (phase 6): `pulse-sim`, invented show devices on this computer. A desk that answers
  its X32/M32, X Air or Wing query, an Art-Net node that answers ArtPoll, a lighting desk sending
  a universe, TCP services, Dante, Lantern and NDI announcements. Each device can be switched
  off, its port closed, its answers slowed or its frame rate lowered. It never announces
  anything on the real network.
- The demo profile now runs on the simulator. All its devices really answer, and now and then a
  stagebox, the light desk or Show Master has a problem for a while. The IEM rack and the
  projector still never answer.
- `pulse-sim` as a program: runs the demo devices and writes a profile for them
  (`pulse-sim [PROFILE] [--calm]`).
- Console checks can use another port than the model's standard one.
- Integration tests against the simulator. Every kind of check passes, faults turn the right
  devices yellow or red with the right cause, and they recover. A scenario runs, and 100 devices
  stay within the packet limit.
- Network load measured with 100 devices: 2.9 kB/s sent, 51 packets/s, 2 % CPU. See
  `docs/netzlast.md`.
- The load counters count a run postponed by the packet limit once, and record how late it
  started.
- Discovery: a known device counts as seen when anything answers at its address or host name.

## 0.4.0

- Modes (phase 5): CHECK, SHOW and DIAGNOSE in the top bar.
- Pre-show check: every check runs at once and more thoroughly (five echo requests per ping),
  a checklist with progress, problems first; known problems can be acknowledged on the spot.
  "Technology released" with name and time writes a report: a self-contained HTML page (prints
  well) and a CSV, next to the profile in "Reports".
- Show mode: a calm view with the light large, only devices with a problem as big tiles (the
  unacknowledged ones blink slowly), every area as one quiet line, no dialogs. An alert sound
  when the light gets worse or a device gets a problem, off by default.
- Light window: the traffic light with its reasons in a window of its own, for a second monitor
  (F or F11: fullscreen).
- Diagnosis: one device at a time with graphs of latency, jitter, loss, connect time, answer
  time and frame rate (with their limits, over 5, 15 or 60 minutes), raw data, check now,
  plain-language hints (address outside this computer's networks, firewall blocks ping, Wi-Fi
  or cable losses, Art-Net only visible as broadcast, multicast blocked by IGMP snooping ...),
  a test of one TCP port and traceroute.

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
