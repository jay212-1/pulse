# Architektur-Analyse der Showtime Suite (für Pulse)

Stand: 2026-11-21. Untersucht: `show-master`, `lantern`, `beamweave`, `blueprint`, `showtime-suite` in `~/Desktop/dev`.
Die allgemeine Analyse vom 2026-11-19 steht in `blueprint/docs/blueprint/ARCHITEKTUR-ANALYSE.md` und gilt
weiter. Hier steht die Kurzfassung und alles, was für Pulse dazukommt.

## 1. Repo- und Workspace-Struktur

- Ein Repo pro Programm, Ordner klein geschrieben nebeneinander in `~/Desktop/dev`, dazu ein Release-Repo
  (`<name>-releases`), das nur Downloads und eine README enthält. Pulse: `jay212-1/pulse` und
  `jay212-1/pulse-releases` (beide öffentlich, noch leer). Das ist dasselbe Muster wie bei Blueprint.
- Größere Programme sind Workspaces mit `crates/*`, so wie Show Master (`sm-core` rein ohne I/O, `sm-server`
  Laufzeit und Netz, `sm-desktop` egui, `showmaster` Binary) und Blueprint (`bp-core`, `bp-print`, `blueprint`).
  Kleinere Programme bestehen aus einem einzigen Crate (Lantern, Beamweave, Launcher).
- **Kein gemeinsames Crate-Repo.** Geteilt wird durch Kopien: `sm-mdns`, `sm-rtpmidi` und `sm-timecode` liegen
  in Show Master und in Lantern. `scripts/ui.sh` und `scripts/setup.sh` sind identische Kopien in allen Repos.
  Theme, Settings, Logging und Updater sind pro Programm nach demselben Muster gebaut.
- Gemeinsames Workspace-Muster: `[workspace.package]` mit Version, Edition 2021, `rust-version = "1.85"`,
  Lizenz `MIT OR Apache-2.0`. Lints: `unsafe_code = "deny"`, Clippy `unwrap_used`/`expect_used`/`panic` = warn,
  `cargo clippy --all-targets -- -D warnings`. Release-Profil: `opt-level 3`, `lto thin`, `codegen-units 4`,
  `strip`.

## 2. UI-Framework und Design

- **egui/eframe 0.37** (`wgpu`, `default_fonts`), nur die Standardschriften von egui, keine Icon-Font. Symbole
  werden mit dem Painter gezeichnet. In UI-Texten stehen nur Zeichen aus den egui-Standardschriften (andere
  lösen in Tests einen Panic aus).
- Dunkles Theme mit festen Konstanten in `theme.rs`. Blueprint verwendet `BG (12,14,17)`, `PANEL (17,19,24)`,
  `CARD (24,27,34)`, `LINE (44,48,58)`, `TEXT (232,235,239)` und `MUTED (140,146,160)`.
  **Die Bedeutungsfarben sind überall gleich und passen direkt als Ampel:**
  - Grün `#22c55e` = ok
  - Amber/Gelb `#f59e0b` bzw. `#facc15` = Achtung
  - Rot `#ef4444` = Fehler
  - Blau `#3b82f6` = aktiv/ausgewählt
  - `MUTED` = grau
- Fenster: `ViewportBuilder` (ca. 1400×860, Mindestgröße, Icon per `include_bytes!`). Startseite mit Banner und
  Aktionskarten (Neu, Öffnen, Zuletzt, Demo), danach links Navigation und rechts Inhalt bzw. Detail. Show Master
  hat zusätzlich einen EDIT/LIVE-Umschalter mit ruhiger Live-Ansicht und Touch-Größen. Lantern öffnet eigene
  Ausgabe-Viewports auf einem zweiten Monitor. Dieses Muster passt für das Fullscreen-Ampelbild.
- Lantern hat in `health.rs` bereits ein „Was ist gerade falsch“-Modell: `Level` Warn/Error, ein stabiler `key`
  pro Problem und eine Klartext-Meldung, angezeigt in der oberen Leiste. Pulse übernimmt diese Sprache und
  diesen Stil.
- UI-Sprache ist Englisch und schlicht. Doku und Skriptausgaben sind Deutsch.

## 3. Konventionen

- Fehler: `thiserror` in der Logik. Kein `unwrap`/`expect`/`panic` in Code, der auf Eingaben, Dateien oder Netz
  reagiert.
- Logging: `tracing` (Show Master) bzw. ein eigenes Log (Lantern).
- Daten: **serde + JSON**, pretty gedruckt, mit `schema_version` und Migrationsfunktionen auf
  `serde_json::Value`. Atomares Speichern (tmp-Datei, fsync, rename), Autosave und Wiederherstellung.
- Speicherorte:
  - Einstellungen: `dirs::config_dir()/<programm>/settings.json`. Für Tests wird der Ordner per
    Umgebungsvariable umgelenkt (z. B. `BLUEPRINT_CONFIG_DIR`).
  - Dokumente: standardmäßig in `Documents/<Programm>`.
- Async: Show Master nutzt **tokio + axum 0.8** (HTTP, JSON, WebSocket) für Server und Fernbedienung. Lantern
  nutzt std-Threads und ein kleines HTTP/1.1 auf std.
- Tests: Unit-Tests in den Core-Crates, UI-Tests mit `egui_kittest` und Screenshots per Umgebungsvariable.
  Tests fassen nie die echten Einstellungen oder den echten Dokumente-Ordner an.
- Version und Änderungen: jede Änderung erhöht die Version und bekommt einen `## x.y.z`-Abschnitt im
  `CHANGELOG.md` (Englisch). Entscheidungen stehen in `decisions.md` (Deutsch, eine Zeile pro Entscheidung).

## 4. Launcher-Integration

- Der Katalog ist `showtime-suite/apps.json`. Pro App gibt es dort:
  - `id`, `name`, `tagline`, `color`, `releases` (Release-Repo) und `icon_url`
  - `macos` mit `asset_suffix` und `app`
  - `windows` mit `asset_prefix`, `asset_suffix`, `inno_app_id` und `exe`
  - `linux` mit `asset_suffix` und `bin`
  - `dev` mit `repo`, `folder`, `release`, `install_macos` und `install_linux`
- Das Icon liegt im Launcher unter `assets/apps/<id>.png` und als PNG im Launcher-Release-Repo (`icons/<id>.png`).
  Installierte Launcher lesen den Katalog aus dem Default-Branch von `Cheter0410/showtime-suite-releases`.
- Eine neue App braucht deshalb keinen neuen Launcher. Die Farben der vorhandenen Apps:
  - Show Master `#22c55e`
  - Beamweave `#e94f8a`
  - Lantern `#ffc45c`
  - Blueprint `#3898ec`

  Für Pulse ist ein Türkis (`#2dd4bf`) noch frei.
- Startparameter: keine. Die Programme werden ohne Argumente gestartet und unterstützen nur `--version`.

## 5. Build- und Release-Skripte

- `install-mac.sh`: Pull, **Release-Build** (immer `--release`, kein schnelleres Profil, Wunsch von Jan),
  Installation nach „Programme › Showtime Suite“. `install.sh` für Linux ohne sudo.
- `packaging/macos/build-dmg.sh` + `Info.plist` (Bundle-ID `com.cheter.<bin>`, Ad-hoc-Signatur, keine
  Notarisierung, DMG nur neu bauen, wenn sich etwas geändert hat).
- `packaging/build-on-linux.sh`: auf dem Arch-PC werden Linux-`tar.gz` und ein Windows-Installer (Inno Setup
  unter Wine, feste `AppId`) gebaut.
- `release.sh`: ein Befehl pro Rechner, dazu `status` und `dry-run`. Erhöht die Version bei Änderungen, erzeugt
  „What's new“ aus den Commits, setzt den Tag `vX.Y.Z` und lädt per `gh release` ins Release-Repo hoch.
  Asset-Namen: `<App>-<v>.dmg`, `<App>-Setup-<v>.exe`, `<bin>-<v>-linux-x86_64.tar.gz`.
- Zielplattformen: macOS (Beamweave universal), Windows x64, Linux x86_64. Es gibt kein CI.

## 6. Was Pulse vorfindet

| Thema | Vorhanden | Folgerung für Pulse |
|---|---|---|
| mDNS | `sm-mdns` (Show Master/Lantern) auf `mdns-sd =0.21.7`. Kündigt eigene Dienste an und sucht nach genau zwei Typen | Pulse braucht allgemeines Browsen beliebiger Typen (Dante, NDI, …). Ich nutze direkt `mdns-sd` in **derselben Version** und kopiere `sm-mdns` nicht |
| Art-Net | Beamweave `dmx.rs` sendet nur (ArtDmx, sACN-Ausgabe), es gibt kein Empfangen und kein ArtPoll | ArtPoll/ArtPollReply und den Empfang baue ich neu (kleines, reines Parser-Modul mit Tests) |
| NDI | Beamweave und Lantern laden die NDI-Laufzeit zur Laufzeit (`libloading`). Beamweave lädt sie beim Bauen (`fetch-ndi.sh`) und liefert sie mit | Für die Präsenzprüfung ist keine SDK nötig: `_ndi._tcp` per mDNS plus TCP auf den angekündigten Port reicht. Für den Discovery Server und die Stream-Prüfung siehe die offenen Fragen |
| OSC | `rosc 0.11` (Lantern, Beamweave). Show Master nimmt OSC-Befehle an | Für die Pult-Abfragen (`/xinfo`, `/status`) wird `rosc` wiederverwendet |
| Kommunikation zwischen Programmen | **Es gibt keinen Status-Bus.** Die Programme finden sich per mDNS (`_lantern._udp`) und steuern sich per OSC oder HTTP-Befehl | Pulse bietet seinen Status als HTTP/WS an (und kündigt `_pulse._tcp` an). OSC-Meldungen an ein frei eingestelltes Ziel sind optional. Einen Empfänger in den anderen Programmen gibt es noch nicht |
| Statusseite | Show Master: axum + WebSocket. Lantern: Seite per `include_str!` | Pulse: axum, read-only, eine eingebaute Seite |
| Blueprint-Geräte | `patch.rs` `Device { name, kind (Stagebox, IoRack, Console, Splitter, Amp, Recorder, Iem, Other), location, model, inputs, outputs, connector, notes, color }`, dazu `Location`. **Kein IP-, Hostname- oder MAC-Feld.** Die Austauschdatei `showtime-exchange` v1 enthält Inputs, Outputs, Sender und Szenen, aber **keine Geräteliste** | Ohne Erweiterung kann Pulse nur Namen, Typ und Location übernehmen. Vorschlag siehe Plan |
| Druck/Bericht | Blueprint: Typst eingebettet (+ ca. 35 MB, langer Build) | Für einen Pre-Show-Bericht ist Typst zu schwer. Vorschlag: HTML-Bericht (druckbar) und CSV |
