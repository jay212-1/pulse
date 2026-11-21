# Pulse

Network and device monitor for live shows, part of the Showtime Suite. Pulse checks before and during
a show that the desk, stageboxes, NDI sources, Art-Net and sACN universes, tablets, backup computers
and the network are reachable and healthy, and turns all of it into one traffic light with the
reasons. Written in Rust (egui), for macOS, Windows and Linux.

**Status: 0.7.** Everything planned works; the first release follows (see `docs/FORTSCHRITT.md`).
How to use it: [Anleitung](docs/ANLEITUNG.md) (German).

- **One profile file** (`.pulse`) per venue or show: devices with their area, address, criticality
  and checks. 19 presets fill in the checks (X32 / M32, X Air, Wing, Soundcraft Si, grandMA3,
  Dante, NDI, Art-Net, sACN, switches, show computers and more).
- **Checks that only ask:** ping (no admin rights needed), TCP ports, console queries over OSC,
  Art-Net nodes and universes with frame rate and conflicts, sACN universes with priorities,
  mDNS / Dante / NDI announcements, switch ports over SNMP v2c, and show computers through the
  Pulse Agent. Pulse never writes to a device.
- **One light:** green, yellow, red, with the reason in plain words, per area and overall.
  Hysteresis and flapping detection keep it calm; known problems can be acknowledged.
- **Three modes:** CHECK to set up (monitor, pre-show check, inventory, discovery, event log),
  SHOW for the show (calm, only problems large, optional sound, a separate light window) and
  DIAGNOSE for one device (graphs, raw data, hints, port test, traceroute).
- **Pre-show check:** every check once, sign-off with your name, report as HTML and CSV.
- **Discovery:** finds consoles, Art-Net nodes, Dante and NDI devices and, after you confirm, the
  rest of the subnet with a ping sweep.
- **Status page:** read-only view for phones in the same network (off by default, TCP 7470).
- **Blueprint import:** devices from Blueprint's exchange file, with presets from their model.
- **Simulator and demo:** a whole demo venue on 127.0.0.1, with faults that come and go.

## Pulse Agent

A small read-only program for show computers (`pulse-agent`, TCP 7471): CPU, memory, free disk
space and whether the show programs run. Download it from the releases
(`pulse-agent-<version>-macos.zip`, `-windows-x86_64.zip`, `-x86_64-linux.tar.gz`); the readme
inside says how to start it automatically.

## Install

The easiest way is Showtime Suite. From the source:

```bash
bash install-mac.sh   # macOS
./install.sh          # Linux
```

## Develop

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo run -p pulse
cargo run --release -p pulse-sim -- sim.pulse   # simulated devices and their profile
```

Releases: `./release.sh dry-run` builds and shows what would be uploaded; `./release.sh status`
shows what is online.

License: MIT OR Apache-2.0.
