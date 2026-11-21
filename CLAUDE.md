# Pulse – notes for Claude Code

Network and device monitor of the Showtime Suite: checks before and during a show whether the
desk, stageboxes, NDI sources, Art-Net universes, tablets, backup computers and the network are
reachable and healthy, and turns that into one traffic light with reasons. Rust workspace, built
like the other suite apps (Show Master, Blueprint).

## Git workflow

- **Work directly on `main`.** No feature branches, no pull requests.
- **Before you start and before every push:** `git pull --rebase`, then build and test, then push.
- Never force-push `main`, never rewrite pushed history, never delete or move tags.

## Version and changelog with every change

- Every change raises the version in the root `Cargo.toml` (`[workspace.package] version`), then
  `cargo update -w` so `Cargo.lock` follows. Small changes: patch step; a new phase: minor step.
- Add a `## x.y.z` section at the top of `CHANGELOG.md` (English).
- New decisions go into `docs/decisions.md` (German, one line each).
- After each phase: update `docs/FORTSCHRITT.md`. The plan is `docs/PLAN.md`.

## Releases

Released files go to **`jay212-1/pulse-releases`** (only downloads and a README, never develop
there). No release without Jan's go-ahead.

## The other projects

All live side by side in `~/Desktop/dev` (folder names in lower case):

| Folder | Repository | Release downloads |
|---|---|---|
| `show-master` | Cheter0410/Show-Master | Cheter0410/show-master-releases |
| `lantern` | Cheter0410/lantern | Cheter0410/lantern-releases |
| `showtime-suite` | Cheter0410/showtime-suite | Cheter0410/showtime-suite-releases |
| `beamweave` | Cheter0410/beamweave | Cheter0410/beamweave-releases |
| `blueprint` | jay212-1/blueprint | jay212-1/blueprint-releases |
| `pulse` | jay212-1/pulse | jay212-1/pulse-releases |

Showtime Suite installs, updates and starts the others; its catalog is `showtime-suite/apps.json`.

## Layout

- `crates/pulse-core` – pure logic, no network, no window, time comes in as unix ms:
  `model.rs` (devices, checks, thresholds, hysteresis, profile), `profile.rs` (the `.pulse` file:
  JSON, `schema_version`, migrations, atomic save), `probe.rs` (the result format every probe
  returns, with its check depth), `ping.rs` (rolling window and how it is judged), `status.rs`
  (status engine: hysteresis, flapping, acknowledgements, area and overall lights, reasons),
  `events.rs` (event log, filter, CSV), `history.rs` (series for the graphs), `artnet.rs`
  (ArtPoll/ArtPollReply/ArtDmx, frames per universe), `osc.rs` (minimal OSC codec),
  `presets.rs` (device presets), `discovery.rs` (sightings -> new / known / missing devices),
  `report.rs` (pre-show report, HTML and CSV), `hints.rs` (plain-language diagnosis hints).
- `crates/pulse-net` – the network side, read-only: `icmp.rs` (ping without admin rights:
  ICMP datagram sockets on macOS/Linux, `icmp_windows.rs` with `IcmpSendEcho` – the only
  `unsafe` code –, TCP fallback), `probe.rs` (the `Probe` trait and the probes), `monitor.rs`
  (scheduler on its own tokio runtime: spread-out intervals, packet limit, hard timeout per run,
  shared snapshot for the window, starts the listeners only when needed), `found.rs` (what the
  listeners found), `artnet.rs` (listener on UDP 6454), `mdns.rs` (mDNS registry, `mdns-sd`),
  `console.rs` (X32/X Air/Wing queries), `interfaces.rs`, `udp.rs`, `discovery.rs` (console
  broadcast, ping sweep), `tools.rs` (port test, traceroute via the system tool). `examples/watch.rs` watches addresses from the terminal.
- `crates/pulse` – the egui app: `app.rs` (window, profile file, pages, navigation, status bar),
  `app_start.rs` (start screen, new-profile form, backdrop), `app_monitor.rs` (overall light,
  tiles), `app_detail.rs` (detail panel), `app_inventory.rs` (inventory editor),
  `app_log.rs` (event log, CSV), `app_discovery.rs` (auto discovery), `app_preshow.rs`
  (pre-show check, sign-off, report), `app_show.rs` (show mode, light window, alert sound),
  `app_diagnose.rs` (graphs, hints, tools), `platform.rs` (open a file, alert sound), `theme.rs` (suite colours, drawn status symbols),
  `settings.rs` (`PULSE_CONFIG_DIR` redirects them; tests never touch the real settings).
- `install-mac.sh`, `packaging/macos/` – as in Blueprint.
- `scripts/setup.sh` and `scripts/ui.sh` are identical copies of the files in the other suite
  apps – change all of them together.

## Build and test

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
PULSE_SHOTS=/some/folder cargo test -p pulse --test ui   # UI tests also write screenshots
cargo run -p pulse-net --example watch -- 127.0.0.1 192.0.2.1
```

- Pulse never writes to devices: probes only ask (ping, TCP connect and close, protocol queries).
  Every probe has a timeout; nothing may block the window.
- No `unwrap()` / `expect()` / `panic!` outside tests.
- UI texts are English. Only glyphs of egui's default fonts in UI strings.
- Tests never touch the real settings or the real Documents folder. UI tests set `app.lan =
  false` (no mDNS, no broadcasts); network tests use fake devices on loopback.
- Broadcasts and the ping sweep only while the discovery page is open or after the user
  confirmed them.
