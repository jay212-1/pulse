# Fortschritt

| Phase | Stand |
|---|---|
| 1 Analyse und Plan | fertig (2026-11-21): `ARCHITEKTUR-ANALYSE.md`, `PLAN.md` |
| 2 Kern | fertig (2026-11-21, 0.1.0): Modell, Profile, Status-Engine, Event-Log, Verlauf, Probe-Trait, Ping/TCP, Monitor. 33 Unit-Tests, 5 Integrationstests gegen echte Sockets |
| 3 UI-Grundgerüst | fertig (2026-11-21, 0.2.0): Startseite, Monitor mit Ampel/Begründung/Kacheln, Detailpanel, Inventar-Editor, Event-Log mit CSV, Icon, `install-mac.sh`. 5 UI-Tests mit Screenshots (`PULSE_SHOTS`). Launcher-Kachel vorbereitet |
| 4 Protokoll-Checks | fertig (2026-11-21, 0.3.0): Art-Net (Nodes, Universen mit fps/Konflikten), OSC-Konsolenabfrage (X32/M32, X Air, Wing), mDNS/Dante/NDI, Interfaces und Interface-Bindung, Presets, Auto-Discovery mit Ping-Sweep. Tests: 41 Kern, 13 Netz (davon 5 gegen Fake-Geräte), 7 UI |
| 5 Modi | fertig (2026-11-21, 0.4.0): CHECK/SHOW/DIAGNOSE, Pre-Show-Check mit Freigabe und HTML/CSV-Bericht, Show-Modus (ruhig, Blinken, Ton optional), Ampelfenster, Diagnose mit Graphen, Hinweisen, Port-Test und Traceroute. Tests: 44 Kern, 21 Netz, 10 UI |
| 6 Simulator | offen |
| 7 Erweiterungen | offen |
| 8 Build/Release | offen |

## Offen aus Phase 2

- Windows: `icmp_windows.rs` ist geschrieben, aber noch nicht kompiliert (auf diesem Mac fehlt das Windows-Target). Prüfen mit `cargo check --target x86_64-pc-windows-msvc` bzw. auf dem Arch-PC.
- Linux: ICMP ohne Root hängt von `net.ipv4.ping_group_range` ab; sonst greift der TCP-Fallback. Auf dem Arch-PC ausprobieren.

## Offen aus Phase 3

- Launcher-Kachel: Diff liegt in `showtime-suite` bereit, wird nach Freigabe committet. Das Icon kommt mit dem ersten Release ins Launcher-Release-Repo (`icons/pulse.png`), wie bei Blueprint.
- `install.sh` (Linux), Windows-Installer und `release.sh` kommen in Phase 8.
- Doppelklick auf eine `.pulse`-Datei im Finder öffnet sie noch nicht (Dateizuordnung später). Öffnen per Menü und Drag & Drop geht.

## Offen aus Phase 4

- Gegen echte Geräte (X32, Wing, Dante, NDI, Art-Net-Nodes) noch nicht getestet; nur gegen nachgebaute Antworten. Der Simulator (Phase 6) liefert dieselben Pakete.
- Zwei Art-Net-Sender im Test nur unter Linux (macOS hat kein 127.0.0.2); die Konflikt-Logik ist im Kern getestet.
- Windows: Ping-Bindung an ein Interface fehlt (IcmpSendEcho wählt selbst); TCP und UDP werden gebunden.

## Offen aus Phase 5

- Das Ampelfenster ist nur eingebettet (im Test) geprüft; als echtes zweites Fenster bitte einmal von Hand ansehen.
- Der Ton ist nicht automatisch testbar (Systemtöne); bitte im Show-Modus einmal einschalten und ein Gerät abziehen.
