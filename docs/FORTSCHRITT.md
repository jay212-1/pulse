# Fortschritt

| Phase | Stand |
|---|---|
| 1 Analyse und Plan | fertig (2026-11-21): `ARCHITEKTUR-ANALYSE.md`, `PLAN.md` |
| 2 Kern | fertig (2026-11-21, 0.1.0): Modell, Profile, Status-Engine, Event-Log, Verlauf, Probe-Trait, Ping/TCP, Monitor. 33 Unit-Tests, 5 Integrationstests gegen echte Sockets |
| 3 UI-Grundgerüst | fertig (2026-11-21, 0.2.0): Startseite, Monitor mit Ampel/Begründung/Kacheln, Detailpanel, Inventar-Editor, Event-Log mit CSV, Icon, `install-mac.sh`. 5 UI-Tests mit Screenshots (`PULSE_SHOTS`). Launcher-Kachel vorbereitet |
| 4 Protokoll-Checks | fertig (2026-11-21, 0.3.0): Art-Net (Nodes, Universen mit fps/Konflikten), OSC-Konsolenabfrage (X32/M32, X Air, Wing), mDNS/Dante/NDI, Interfaces und Interface-Bindung, Presets, Auto-Discovery mit Ping-Sweep. Tests: 41 Kern, 13 Netz (davon 5 gegen Fake-Geräte), 7 UI |
| 5 Modi | fertig (2026-11-21, 0.4.0): CHECK/SHOW/DIAGNOSE, Pre-Show-Check mit Freigabe und HTML/CSV-Bericht, Show-Modus (ruhig, Blinken, Ton optional), Ampelfenster, Diagnose mit Graphen, Hinweisen, Port-Test und Traceroute. Tests: 44 Kern, 21 Netz, 10 UI |
| 6 Simulator | fertig (2026-11-21, 0.5.0): `pulse-sim` als Bibliothek und Programm, Demo läuft auf dem Simulator mit Szenario, 4 Integrationstests gegen den Simulator, Netzlast mit 100 Geräten gemessen (`netzlast.md`: 2,9 kB/s, Limit greift ab etwa 85 Geräten). Tests: 44 Kern, 21 Netz, 4 Simulator, 10 UI |
| 7 Erweiterungen | fertig (2026-11-21, 0.6.0): Statusseite (TCP 7470, LAN, standardmäßig aus), Pulse Agent (`pulse-agent`, TCP 7471), sACN-Universen, SNMP-Switch-Ports, Blueprint-Import über die Austauschdatei (Blueprint 0.2.4), Presets für Soundcraft Si und grandMA3. Tests: 59 Kern, 24 Netz, 6 Simulator, 3 Agent, 12 UI |
| 8 Build/Release | fertig (2026-11-21, 0.7.0, Mac-Release 0.7.0; 1.0.0 als erste stabile Version): Windows-Installer, Linux-Archiv, Agent-Pakete für alle drei Systeme, `release.sh` (Probelauf auf dem Mac), `install.sh`, `.pulse`-Doppelklick, README, Anleitung. Tests: 59 Kern, 24 Netz, 6 Simulator, 3 Agent, 14 App (davon 12 UI). |

## Offen aus Phase 2

- Windows: `icmp_windows.rs` ist geschrieben, aber noch nicht kompiliert (auf diesem Mac fehlt das Windows-Target). Prüfen mit `cargo check --target x86_64-pc-windows-msvc` bzw. auf dem Arch-PC.
- Linux: ICMP ohne Root hängt von `net.ipv4.ping_group_range` ab; sonst greift der TCP-Fallback. Auf dem Arch-PC ausprobieren.

## Offen aus Phase 3

- Launcher-Kachel: in `showtime-suite` 0.3.3 committet. Das Icon kommt mit dem ersten Release ins Launcher-Release-Repo (`icons/pulse.png`), wie bei Blueprint.

## Offen aus Phase 4

- Gegen echte Geräte (X32, Wing, Dante, NDI, Art-Net-Nodes) noch nicht getestet; nur gegen nachgebaute Antworten. Der Simulator (Phase 6) liefert dieselben Pakete.
- Zwei Art-Net-Sender im Test nur unter Linux (macOS hat kein 127.0.0.2); die Konflikt-Logik ist im Kern getestet.
- Windows: Ping-Bindung an ein Interface fehlt (IcmpSendEcho wählt selbst); TCP und UDP werden gebunden.

## Offen aus Phase 5

- Das Ampelfenster ist nur eingebettet (im Test) geprüft; als echtes zweites Fenster bitte einmal von Hand ansehen.
- Der Ton ist nicht automatisch testbar (Systemtöne); bitte im Show-Modus einmal einschalten und ein Gerät abziehen.

## Offen aus Phase 6

- Das Programm `pulse-sim` mit der App zusammen einmal von Hand ausprobieren (Profil öffnen, Lichtpult und Node sollten grün werden).
- Die Messung lief auf Loopback. Im echten Netz bleibt die Paketzahl gleich; Antwortzeiten und Verluste kommen dazu.

## Offen aus Phase 7

- Soundcraft Si Impact und grandMA3 nur mit Presets, nicht gegen echte Geräte getestet. Beim Si bitte prüfen, ob TCP 3804 antwortet (Diagnose › Port-Test); bei der MA3 das Universum im Inventar auf das stellen, was die Konsole sendet. Art-Net, das die MA3 per Unicast an Nodes schickt, sieht Pulse nicht (nur Broadcast oder an diesen Rechner).
- SNMP und sACN nur gegen den Simulator getestet. Echte Switches: Community und erlaubte Absender prüfen.
- Die Statusseite einmal vom Handy im echten Netz öffnen (Firewall von macOS fragt beim ersten Mal).

## Offen aus Phase 8

- Windows, Linux und die beiden Agent-Pakete reicht der Arch-PC mit `./release.sh` nach (oder "Release all" in Showtime Suite).
- Windows ist noch nie gebaut worden (`icmp_windows.rs`, Installer, Agent): das passiert beim ersten `./release.sh` auf dem Arch-PC. Danach den Installer einmal auf einem Windows-Rechner ausprobieren.
- Mit dem Release: `icons/pulse.png` ins Launcher-Release-Repo und den Katalog dort aktualisieren (die Kachel ist in `showtime-suite` 0.3.3 schon drin).
- Doppelklick auf eine `.pulse`-Datei einmal im Finder ausprobieren (nach `bash install-mac.sh`).
