# Pulse – Anleitung

Pulse prüft vor und während der Show, ob die Technik erreichbar ist und läuft: Pult, Stageboxen,
NDI-Quellen, Art-Net- und sACN-Universen, Tablets, Zuspiel- und Backup-Rechner, Switches. Daraus
wird eine Ampel mit Begründung. Pulse fragt nur ab; es schreibt nie etwas auf ein Gerät.

Die Oberfläche ist englisch. Begriffe aus dem Programm stehen hier in `Code-Schrift`.

## 1. Installieren

- **Über Showtime Suite** (empfohlen): Kachel Pulse, Installieren. Updates bietet der Launcher an.
- **Mac von Hand:** `Pulse-x.y.z.dmg` öffnen und Pulse in den Programme-Ordner ziehen. Beim ersten
  Start Rechtsklick › Öffnen (das Programm ist nicht notarisiert).
- **Windows:** `Pulse-Setup-x.y.z.exe`. Braucht keine Administratorrechte.
- **Linux:** `pulse-x.y.z-linux-x86_64.tar.gz` entpacken und `pulse` starten, oder aus dem
  Quellcode mit `./install.sh`.

**Firewall:** Beim ersten Start fragt macOS (und Windows), ob Pulse eingehende Verbindungen annehmen
darf. Bitte erlauben. Sonst sieht Pulse keine Art-Net-, sACN- und mDNS-Pakete, und die Statusseite
ist vom Handy aus nicht erreichbar. Ping und TCP-Abfragen gehen auch ohne.

## 2. Ein Profil anlegen

Ein Profil (`.pulse`-Datei) beschreibt einen Ort oder eine Show: welche Geräte da sind, wo sie
stehen und was geprüft wird.

1. Startseite: `New profile`, Namen und `Venue or show` eintragen, Ort wählen, `Create profile`.
2. `Inventory`: Geräte hinzufügen. Am schnellsten mit einem Preset, das die passenden Prüfungen
   schon mitbringt. Dann Name, Adresse (IP oder Hostname) und Bereich (`Area`, z. B. FOH, Bühne,
   Licht) eintragen.
3. Für jedes Gerät die Wichtigkeit wählen:
   - `Critical`: fällt es aus, wird die Ampel rot.
   - `Important`: Ausfall macht die Ampel gelb.
   - `Optional`: wird angezeigt, ändert die Ampel nicht.
4. Speichern mit Cmd+S (Windows/Linux: Strg+S). Änderungen werden außerdem kurz nach der letzten
   Eingabe automatisch gesichert.

Öffnen: `Open profile`, `RECENT PROFILES`, Doppelklick auf eine `.pulse`-Datei oder die Datei ins
Fenster ziehen. Zum Ausprobieren gibt es `Try the demo`: eine erfundene Halle mit simulierten Geräten,
bei denen ab und zu etwas ausfällt.

### Geräte aus Blueprint übernehmen

In Blueprint die Austauschdatei exportieren (`<name>.showtime.json`), dann in Pulse unter
`Inventory` › `Import from Blueprint…`. Pulse übernimmt Name, Ort (als Bereich), Modell und Adresse
und wählt das Preset aus dem Modell. Geräte ohne Adresse kommen ausgeschaltet an, bis eine Adresse
eingetragen ist. Ein zweiter Import ändert nichts doppelt; bekannte Geräte bekommen nur, was ihnen
noch fehlt.

### Geräte finden lassen

`Discovery` sucht im Netz nach Pulten (X32, X Air, Wing), Art-Net-Nodes, Dante- und NDI-Geräten.
Der Ping-Sweep über das ganze Subnetz startet erst, wenn du `Start sweep` klickst. Gefundene Geräte
übernimmst du mit einem Klick ins Inventar.

## 3. Die Presets

| Preset | Was geprüft wird |
|---|---|
| Behringer X32 / Midas M32, X Air / MR, Wing | Ping und Abfrage über OSC (Name, Firmware) |
| Soundcraft Si Impact / Expression / Performer | Ping und TCP 3804 (der HiQnet-Control-Port, mit dem sich ViSi Remote verbindet) |
| Other console | Ping |
| Dante device | Ping und Dante-Ankündigung im Netz |
| NDI source | Ping, NDI-Ankündigung und ihr Port |
| Art-Net node | Ping und Antwort auf ArtPoll |
| Lighting desk (Art-Net) | Ping und Art-Net-Universum 0 (mindestens 20 Bilder pro Sekunde, Konflikte) |
| grandMA3 (Art-Net) | Ping, Antwort auf ArtPoll und Universum 0 (mindestens 20 Bilder pro Sekunde) |
| Lighting desk (sACN) | Ping und sACN-Universum 1 (Bildrate, Priorität) |
| Tablet on Wi-Fi | Ping mit WLAN-Grenzen (mehr Verzögerung erlaubt, Verluste zählen) |
| Show Master computer | Ping und der Web-Port 7400 |
| Lantern computer | Ping und die Lantern-Ankündigung (mDNS) |
| Network switch | Ping |
| Managed switch (SNMP) | Ping und Antwort auf SNMP v2c; mit eingetragenem Port auch dessen Zustand und Fehlerzähler |
| Show computer with Pulse Agent | Rechnerzustand und laufende Programme über den Agent |
| Wi-Fi access point, Other device | Ping |

Jede Prüfung lässt sich im Inventar anpassen (Port, Universum, Grenzwerte), dazu pro Gerät das Interface.

### Soundcraft Si Impact

Pulse prüft, ob das Pult im Netz antwortet und der Control-Port (TCP 3804) offen ist. Mehr fragt
Pulse beim Si nicht ab. Wenn das Pult rot bleibt: unter `DIAGNOSE` den Port-Test auf 3804 laufen
lassen, und am Pult prüfen, ob die Netzwerk-Fernsteuerung an ist.

### grandMA3

Pulse sieht die MA3 als Art-Net-Gerät (ArtPoll) und misst ein Universum. Stell im Inventar das
Universum ein, das die Konsole wirklich sendet. **Wichtig:** Schickt die MA3 Art-Net per Unicast
direkt an die Nodes, kommen die Pakete nicht bei Pulse an; dann meldet das Universum `no frames`,
obwohl das Licht läuft. Pulse sieht nur Art-Net, das als Broadcast gesendet wird oder an den
Pulse-Rechner geht. In dem Fall die Universums-Prüfung abschalten und nur den Node prüfen, oder in
der MA3 Pulse als zusätzliches Ziel eintragen.

### sACN

sACN läuft über Multicast. Pulse tritt den Gruppen der eingetragenen Universen bei. Senden zwei
Quellen dasselbe Universum mit derselben höchsten Priorität, meldet Pulse einen Konflikt. Eine
Quelle mit niedrigerer Priorität gilt als Backup und ist in Ordnung.

### Switches über SNMP

Für `Managed switch (SNMP)` im Switch SNMP v2c zum Lesen einschalten, die Community (oft `public`)
und die Port-Nummer (ifIndex) im Inventar eintragen. Manche Switches antworten nur bestimmten
Absendern; dann den Pulse-Rechner dort erlauben. Pulse liest nur, es setzt nichts.

## 4. Pulse Agent für Show-Rechner

Der Agent ist ein kleines Programm für Zuspiel-, Backup- und Medienrechner. Pulse fragt ihn, wie
es dem Rechner geht: CPU, Speicher, freier Plattenplatz und ob die Show-Programme laufen.

1. Das passende Paket aus den Downloads holen: `pulse-agent-x.y.z-macos.zip`,
   `pulse-agent-x.y.z-windows-x86_64.zip` oder `pulse-agent-x.y.z-x86_64-linux.tar.gz`.
2. Entpacken und `pulse-agent` starten (Mac: beim ersten Mal Rechtsklick › Öffnen; die Firewall
   fragt, bitte erlauben). Er hört auf TCP 7471.
3. In Pulse das Gerät mit dem Preset `Show computer with Pulse Agent` anlegen, die Adresse und die
   Programme eintragen, die laufen müssen (z. B. `QLab, Ableton Live`).

Läuft ein Programm nicht oder antwortet der Agent nicht, wird das Gerät rot. Wird die Platte voll
(Standard: unter 5 GB frei), ist die CPU dauerhaft ausgelastet oder der Speicher knapp, wird es gelb.
Von Hand prüfen: im Browser `http://<adresse>:7471/status` öffnen. Wie der Agent beim Anmelden von
selbst startet, steht in der `README.txt` im Paket.

## 5. Vor der Show: CHECK

Oben links stehen die drei Modi. `CHECK` ist zum Einrichten und Prüfen.

- `Monitor`: die große Ampel mit Begründung, darunter jede Prüfung als Kachel. Sortieren nach
  Bereich, Problemen oder Name. Klick auf eine Kachel öffnet die Details.
- `Pre-show check`: `Start pre-show check` lässt jede Prüfung einmal laufen. Bekannte Probleme
  kannst du quittieren (sie bleiben im Bericht). Dann `Your name` eintragen und
  `Technology released`. Der Bericht wird als HTML und CSV gespeichert (`Open report`,
  `Show in folder`).
- `Event log`: alles, was passiert ist, mit Uhrzeit; als CSV exportierbar.

**Quittieren:** In den Details eines Geräts `Acknowledge`. Ein quittiertes Problem zählt nicht mehr
für die Ampel, bis das Gerät wieder in Ordnung war. So bleibt die Ampel aussagekräftig, wenn ein
bekanntes Problem nicht behoben werden kann.

## 6. Während der Show: SHOW

`SHOW` ist ruhig: nur die Ampel und, wenn etwas ist, die Probleme groß. Keine Dialoge. Probleme,
die noch nicht quittiert sind, blinken langsam. `Sound on new problems` spielt zusätzlich einen Ton. `Light window` öffnet
die Ampel als eigenes kleines Fenster, z. B. für einen zweiten Bildschirm.

Die Ampel:

- **Grün** (`ALL OK`): alles antwortet wie erwartet.
- **Gelb** (`WARNING`): etwas ist auffällig (Verluste, lange Antwortzeiten, niedrige Bildrate,
  ein wichtiges Gerät fehlt), die Show läuft aber.
- **Rot** (`PROBLEM`): ein kritisches Gerät oder eine kritische Prüfung ist ausgefallen.

Pulse wartet ein paar Prüfungen ab, bevor es die Farbe wechselt, damit ein einzelnes verlorenes
Paket nicht gleich Alarm macht. Ein Gerät, das ständig zwischen gut und schlecht wechselt, wird
als `flapping` gemeldet.

## 7. Wenn etwas nicht stimmt: DIAGNOSE

`DIAGNOSE` zeigt ein Gerät genau: Graphen für Antwortzeit und Verluste, die Rohdaten der letzten
Prüfungen und Hinweise in einfachen Worten (z. B. „antwortet auf Ping, aber der Port ist zu“).
Dazu ein Port-Test und Traceroute.

Typische Ursachen:

- **Alles rot auf einmal:** falsches Netz oder falsches Interface. Hat der Rechner mehrere
  Netzwerke (z. B. WLAN und Show-Netz), im Inventar beim Gerät das `Interface` festlegen.
- **Ping geht, Port nicht:** Gerät läuft, aber der Dienst ist aus oder eine Firewall blockt.
- **Art-Net/sACN ohne Bilder:** Konsole sendet nicht, anderes Universum, Unicast (siehe grandMA3)
  oder die Firewall des Pulse-Rechners blockt.

## 8. Statusseite fürs Handy

Unten in der Statusleiste `Status page` einschalten. Pulse zeigt dann die Adresse an, z. B.
`http://192.168.1.20:7470`. Diese Adresse im Handy-Browser öffnen (gleiches Netz). Die Seite zeigt
die Ampel, die Probleme und alle Geräte, aktualisiert sich alle zwei Sekunden und kann nichts
verändern. Standardmäßig ist sie aus. Beim ersten Einschalten fragt die Firewall des Rechners.

## 9. Gut zu wissen

- Pulse schickt Broadcasts und den Ping-Sweep nur, wenn die `Discovery`-Seite offen ist oder du es
  bestätigt hast. Im Normalbetrieb erzeugt Pulse wenig Netzlast (gemessen: etwa 3 kB/s bei 100
  Geräten).
- Ping funktioniert ohne Administratorrechte. Wo das System es nicht erlaubt (manche Linux-
  Systeme), prüft Pulse stattdessen per TCP.
- Tastenkürzel: Cmd/Strg+N neues Profil, Cmd/Strg+O öffnen, Cmd/Strg+S speichern.
