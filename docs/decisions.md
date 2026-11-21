# Entscheidungen

- 2026-11-21: Aufbau als Workspace wie Show Master/Blueprint: `pulse-core` (rein, ohne I/O), `pulse-net` (Probes, Scheduler), später `pulse` (App), `pulse-agent`, `pulse-sim`.
- 2026-11-21: NDI zuerst ohne SDK (Präsenz über `_ndi._tcp` + TCP). Discovery Server/Stream-Prüfung später als Option mit einer *installierten* NDI-Laufzeit, nicht mitgeliefert.
- 2026-11-21: Ping unter Windows über `IcmpSendEcho`; das ist das einzige `unsafe` in Pulse (`icmp_windows.rs`, `#[allow(unsafe_code)]`). macOS/Linux: unprivilegierte ICMP-Datagram-Sockets. Sonst TCP-Fallback auf Port 80, bei dem auch „connection refused“ als erreichbar gilt.
- 2026-11-21: Blueprint-Import über die Austauschdatei. Vorschlag: `Device.address` in Blueprint und ein Abschnitt `devices` in `showtime-exchange` v1. Umsetzung erst in Phase 7, nach Freigabe.
- 2026-11-21: Pre-Show-Bericht als eigenständige HTML-Datei (druckbar) plus CSV, kein Typst.
- 2026-11-21: Kein gemeinsames Crate. `theme.rs` und `scripts/` werden kopiert (Suite-Muster), `mdns-sd` wird direkt in der Version von `sm-mdns` eingebunden.
- 2026-11-21: Statusseite auf TCP 7470, Agent auf TCP 7471 (Pulse fragt ab, der Agent sendet nichts von sich aus). Kachelfarbe Türkis `#2dd4bf`.
- 2026-11-21: tokio wie in Show Master. Der Monitor hat eine eigene Laufzeit in einem eigenen Thread, die egui-App liest nur einen Schnappschuss.
- 2026-11-21: Hysterese pro Prüfung: Rot nach `fail_after` Fehlschlägen in Folge, Gelb nach `fail_after` nicht bestandenen (degradiert oder fehlgeschlagen), Grün nach `ok_after` Erfolgen. Die erste Antwort zählt sofort. „Unknown“-Ergebnisse (Prüfung kann nicht laufen) zählen nicht als Fehlschlag.
- 2026-11-21: Gerätestatus: Rot, wenn eine kritische Prüfung (Standard: Ping) oder alle Prüfungen rot sind. Gelb, wenn irgendeine Prüfung nicht OK ist. Flapping (Standard: 4 Wechsel in 5 min) macht ein gerade grünes Gerät gelb.
- 2026-11-21: Ein quittiertes Gerät zählt für die Ampel als bekannt und grün. Die Quittierung endet, sobald das Gerät wieder grün ist.
- 2026-11-21: Das erste Grün nach dem Start wird nicht ins Log geschrieben (sonst 100 Zeilen „Unknown -> OK“ beim Start).
- 2026-11-21: Schonend: Prüfungen über ihr Intervall verteilt (fester Versatz pro Id), global standardmäßig höchstens 50 Pakete/s, rote Prüfungen mit verdoppeltem Intervall (+ höchstens 10 s), harter Timeout um jeden Lauf.
- 2026-11-21: Formatierung mit `cargo fmt` (Standard, wie Show Master), `cargo fmt --check` gehört zur Prüfung.
