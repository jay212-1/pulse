# Pulse – Architektur- und Umsetzungsplan

Stand: 2026-11-21. Die Grundlage ist `docs/ARCHITEKTUR-ANALYSE.md`. Pulse wird wie Show Master und Blueprint
gebaut: als Workspace mit einem reinen Kern, einem Netz-Teil und einer egui-App.

## Aufbau

```
pulse/
  Cargo.toml          [workspace] crates/*, version 0.1.0, Lints und Profile wie Blueprint
  crates/pulse-core   rein, ohne I/O: Inventar, Profile (.pulse, JSON, schema_version, Migrationen, atomares
                      Speichern), Status-Engine (Hysterese, Flapping, Bereiche, Gesamtampel, Quittieren,
                      Begründungen), Event-Log, Messreihen (Ringpuffer), Art-Net/OSC/sACN-Parser
  crates/pulse-net    tokio: Probe-Trait und Probes, Scheduler mit Rate-Limit, Discovery (mDNS, ArtPoll),
                      Interface-Überwachung, Statusseite (axum + WebSocket, read-only)
  crates/pulse        egui-App (app.rs, app_*.rs pro Bereich, theme.rs aus Blueprint, settings.rs mit
                      PULSE_CONFIG_DIR), Binary `pulse`
  crates/pulse-agent  eigenes, schlankes Binary ohne egui: Heartbeat und Systemwerte, nur lesend
  crates/pulse-sim    Simulator als Binary und als Bibliothek für die Integrationstests
  assets/ packaging/ scripts/ (ui.sh, setup.sh als Kopien) release.sh install-mac.sh install.sh build.rs
  docs/               ARCHITEKTUR-ANALYSE.md, PLAN.md, decisions.md, FORTSCHRITT.md, Anleitung
```

- Binary `pulse`, Anzeigename „Pulse“, Bundle-ID `com.cheter.pulse` und eine neue Windows-AppId.
- Asset-Namen: `Pulse-<v>.dmg`, `Pulse-Setup-<v>.exe` und `pulse-<v>-linux-x86_64.tar.gz`.
- Der Agent kommt als eigene Assets dazu: `pulse-agent-<v>-<os>.zip` bzw. `.tar.gz`.

## Kern: Modell und Status-Engine

- **Gerät:** `Device { id, name, kind, area, address, mac?, interface?, criticality, notes, checks: Vec<Check>,
  hysteresis { fail_after, ok_after }, enabled }`.
  - `area`: FOH, Stage, Video, Light, Network, Backup oder ein frei benannter Bereich.
  - `criticality`: Critical, Important oder Optional.
- **Check:** `Check { kind, params, thresholds, enabled }` mit den Arten Ping, Tcp, Osc(preset), Mdns(service, name),
  Ndi(source), ArtNetNode, ArtNetUniverse(u, min_fps), Sacn(u), Agent und Snmp.
  - Presets pro Gerätetyp und Hersteller, z. B. X32/M32 auf UDP 10023 mit `/xinfo`, Wing auf UDP 2222 und
    Tablet-WLAN mit milderen Schwellen.
- **Ergebnis:** `ProbeResult { outcome: Ok | Degraded | Fail | Unknown, depth, metrics, message, raw }`.
  - `message` ist ein Klartext auf Englisch, `raw` sind die Rohdaten für die Diagnose.
  - `depth` (Prüftiefe) steht an jedem Ergebnis und in der UI: **Reachable** (Ping), **Port** (TCP), **Present**
    (mDNS/NDI-Ankündigung), **Answers** (OSC-/ArtPoll-Antwort) und **Data** (DMX-Frames kommen an, Prozess läuft).
    Die UI zeigt damit ehrlich, was belegt ist, z. B. „Present – Dante service announced (not clock/audio)“.
- **Status-Engine** (pure Funktionen, Zeit wird hineingereicht):
  - Rot erst nach N Fehlschlägen in Folge, Grün erst nach M Erfolgen. Flapping: mehr als K Wechsel in T Minuten
    ergibt Gelb mit der Begründung „flapping“.
  - Bereiche und Gesamtampel folgen den Regeln aus dem Auftrag. Ob optionale Geräte höchstens auf Gelb oder gar
    nicht wirken, ist einstellbar.
  - Quittierung: das Gerät bleibt sichtbar, zählt aber nicht mehr. Wird es Grün, endet die Quittierung von selbst.
  - Begründung: das schlimmste Gerät mit Name, Ursache und „seit hh:mm“.
  - Alles mit Unit-Tests.

## Netz: Probes und Scheduler

- `trait Probe { fn kind(&self); async fn run(&self, ctx: &ProbeCtx) -> ProbeResult }`. Jede Probe läuft in
  einer eigenen tokio-Task mit hartem Timeout. Ein Panic oder Hänger trifft nur dieses eine Ergebnis.
- Die UI liest einen Schnappschuss (`Arc<Mutex<Snapshot>>` bzw. `watch`-Kanal) und wartet nie auf das Netz.
- **Schonend:**
  - Die Intervalle sind pro Check einstellbar und versetzt geplant, damit nicht alle im selben Takt feuern.
  - Ein globales Limit liegt bei standardmäßig 50 Pakete/s. Bei Ausfällen gibt es Backoff.
  - ArtPoll alle 3 s, wie es die Spezifikation verlangt.
  - Kein Port-Scan. Der Subnetz-Sweep gilt nur für /24, läuft mit 20 Pings/s und erst nach einer Bestätigung.
  - Pulse sendet nur Abfragen (Ping, TCP-Verbindungsaufbau, `/xinfo`, ArtPoll, mDNS-Query), nie
    Steuerbefehle.
- **Ping ohne Admin-Rechte:**
  - macOS und Linux: ICMP über `SOCK_DGRAM` (`socket2`). Unter Linux geht das nur, wenn `ping_group_range` es
    erlaubt.
  - Windows: `IcmpSendEcho2` aus der Windows-API, ohne Admin-Rechte.
  - Geht beides nicht: Fallback auf TCP-Connect (Port 80/443 bzw. den Geräteport; auch „connection refused“
    gilt als erreichbar).
  - Die aktive Methode steht in der Statusleiste und am Ergebnis.
- **Interfaces** (`if-addrs`): Link, IP und IP-Wechsel werden selbst als Check angezeigt. Fällt eine Netzwerkkarte
  weg, werden die gebundenen Probes „Unknown – interface down“, ohne Absturz.
- **Art-Net:** Pulse hört passiv auf UDP 6454 (`SO_REUSEADDR`/`SO_REUSEPORT`, damit z. B. Beamweave auf demselben
  Rechner weiter läuft). Erfasst werden fps, letzter Frame, Absender und Konflikte pro Universum. Wichtig: Pulse
  sieht nur Broadcast-DMX oder DMX, das an diesen Rechner geht. Unicast an andere Nodes kann es im Switch nicht
  sehen. Das zeigt die UI als Prüftiefe an.
- **Netzlast:** gemessen wird mit 100 simulierten Geräten, dokumentiert in `docs/netzlast.md`. Die Schätzung im
  Show-Modus mit Ping alle 2 s: unter 10 kB/s.

## Oberfläche

- Startseite wie bei den anderen Programmen: Profile (Venues) mit Neu, Öffnen, Zuletzt und Demo.
- Danach oben die **Gesamtampel mit Begründung** und der Modus-Umschalter **CHECK · SHOW · DIAGNOSE** (wie
  EDIT/LIVE in Show Master). Darunter Kacheln nach Bereich, rechts das Detailpanel.
- Links die Navigation:
  - Monitor
  - Inventory (Tabelle mit Mehrfachauswahl, Duplizieren, Massenbearbeitung)
  - Discovery (gefunden / unbekannt / fehlt, mit einem Klick ins Inventar)
  - Log (Filter, CSV-Export)
- Status nie nur über Farbe: jede Statusfarbe hat ein gezeichnetes Symbol und ein Wort
  (✔/▲/✖/– werden gezeichnet, nicht als Zeichen gesetzt), dazu OK/WARN/DOWN/UNKNOWN.
- Show-Modus: ruhige Ansicht, nur Abweichungen groß. Ein betroffenes Gerät blinkt langsam. Ein Ton ist optional
  und standardmäßig aus. Es gibt keine Dialoge. Ein Fullscreen-Ampelbild lässt sich als eigener Viewport auf
  einen zweiten Monitor legen (Muster Lantern).
- Pre-Show-Check: eine Checkliste mit gründlicheren Proben. Am Ende „Technik freigegeben“ mit Name und Zeit, dazu
  ein Bericht.
- Diagnose: Graphen (egui-Painter, ohne Zusatz-Crate), Rohdaten, „Check now“, Traceroute, Port-Test (ein Port, auf
  Klick) und Klartext-Hinweise.

## Phasen (je eine Abstimmung mit dir)

| Phase | Inhalt | Version |
|---|---|---|
| 2 | Kern, Profile, Probe-Trait, Scheduler, Status-Engine, Ping/TCP, Event-Log, Unit-Tests | 0.1.0 |
| 3 | UI-Grundgerüst, Kacheln, Ampel, Detail, Inventar-Editor, Startseite, Icon. Launcher-Kachel (Diff vorher an dich) | 0.2.0 |
| 4 | Art-Net, mDNS/NDI, OSC-Presets, Interfaces | 0.3.0 |
| 5 | Pre-Show-Check mit Bericht, Show-Modus, Fullscreen-Ampel, Diagnose mit Graphen | 0.4.0 |
| 6 | Simulator, Integrationstests, Messung mit 100 Geräten | 0.5.0 |
| 7 | Statusseite, Blueprint-Import, Pulse Agent, optional sACN/SNMP | 0.6.0 |
| 8 | Packaging, `release.sh`, README, Anleitung. Erstes Release erst nach deiner Freigabe | 0.7.0 |

Innerhalb einer Phase erhöht jeder Commit die Patch-Version und bekommt einen CHANGELOG-Eintrag.

## Offene Fragen und Abweichungen (brauche deine Entscheidung)

1. **NDI-SDK:** Für die Präsenzprüfung (`_ndi._tcp` und TCP) braucht Pulse keine SDK. Für den NDI Discovery
   Server und eine optionale Stream-Prüfung müsste Pulse die NDI-Laufzeit wie Beamweave zur Laufzeit laden.
   Beamweave liefert sie bereits mit, unter der NDI-Lizenz, die du dort schon angenommen hast.
   **Vorschlag:** zuerst ohne SDK. Später ein Feature-Flag, das eine *installierte* NDI-Laufzeit nutzt und sie
   nicht mitliefert.
2. **`unsafe` unter Windows:** `IcmpSendEcho2` braucht einen FFI-Aufruf. Die Suite hat `unsafe_code = "deny"`
   (Beamweave nutzt `unsafe` für NDI).
   **Vorschlag:** ein einziges Modul `ping_windows.rs` mit `#[allow(unsafe_code)]` und Kommentar. Die
   Alternative ist ein TCP-Fallback ohne echtes ICMP unter Windows.
3. **Blueprint-Import:** Blueprint-Geräte haben keine IP.
   **Vorschlag für eine minimale Erweiterung:**
   - Blueprint: ein Feld `Device.address` (IP/Hostname, optional) und eine Spalte „Address“ in der
     Gerätetabelle.
   - Austauschdatei: ein neuer Abschnitt `devices: [{ name, kind, location, model, address }]` (laut den Regeln
     abwärtskompatibel, Version bleibt 1).
   - Pulse liest diese Austauschdatei und übernimmt Geräte mit Typ, Bereich und Adresse.

   Umsetzung erst in Phase 7 und nur, wenn du zustimmst.
4. **Bericht:** HTML-Datei (eigenständig, druckbar, als PDF über den Browser) und CSV, statt Typst
   (+35 MB, langer Build). Einverstanden?
5. **Gemeinsamer Code:** Ich kopiere nichts aus anderen Programmen in ein gemeinsames Crate. `theme.rs` und
   `scripts/` werden kopiert (Suite-Muster), `mdns-sd` wird direkt in der Version von `sm-mdns` eingebunden.
6. **Ports und Farbe:**
   - Statusseite auf TCP 7470, Agent auf TCP 7471. Pulse fragt den Agenten ab (HTTP GET), der Agent sendet von
     sich aus nichts.
   - Kachelfarbe Türkis `#2dd4bf`.
7. **Async:** tokio wie in Show Master (statt std-Threads wie in Lantern), weil viele gleichzeitige Probes mit
   Timeouts so einfacher und sicherer sind.
