# Fortschritt

| Phase | Stand |
|---|---|
| 1 Analyse und Plan | fertig (2026-11-21): `ARCHITEKTUR-ANALYSE.md`, `PLAN.md` |
| 2 Kern | fertig (2026-11-21, 0.1.0): Modell, Profile, Status-Engine, Event-Log, Verlauf, Probe-Trait, Ping/TCP, Monitor. 33 Unit-Tests, 5 Integrationstests gegen echte Sockets |
| 3 UI-Grundgerüst | fertig (2026-11-21, 0.2.0): Startseite, Monitor mit Ampel/Begründung/Kacheln, Detailpanel, Inventar-Editor, Event-Log mit CSV, Icon, `install-mac.sh`. 5 UI-Tests mit Screenshots (`PULSE_SHOTS`). Launcher-Kachel vorbereitet |
| 4 Protokoll-Checks | offen |
| 5 Modi | offen |
| 6 Simulator | offen |
| 7 Erweiterungen | offen |
| 8 Build/Release | offen |

## Offen aus Phase 2

- Windows: `icmp_windows.rs` ist geschrieben, aber noch nicht kompiliert (auf diesem Mac fehlt das Windows-Target). Prüfen mit `cargo check --target x86_64-pc-windows-msvc` bzw. auf dem Arch-PC.
- Linux: ICMP ohne Root hängt von `net.ipv4.ping_group_range` ab; sonst greift der TCP-Fallback. Auf dem Arch-PC ausprobieren.
- Bindung an ein bestimmtes Netzwerk-Interface (`Device.interface`) kommt mit der Interface-Überwachung in Phase 4.

## Offen aus Phase 3

- Launcher-Kachel: Diff liegt in `showtime-suite` bereit, wird nach Freigabe committet. Das Icon kommt mit dem ersten Release ins Launcher-Release-Repo (`icons/pulse.png`), wie bei Blueprint.
- `install.sh` (Linux), Windows-Installer und `release.sh` kommen in Phase 8.
- Doppelklick auf eine `.pulse`-Datei im Finder öffnet sie noch nicht (Dateizuordnung später). Öffnen per Menü und Drag & Drop geht.
