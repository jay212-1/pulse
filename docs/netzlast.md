# Netzlast mit 100 Geräten

Gemessen am 2026-11-21 mit Pulse 0.5.0 gegen den Simulator (`crates/pulse-sim`).

```bash
cargo test --release -p pulse-sim --test sim -- --ignored --nocapture
```

## Aufbau

- 100 Geräte, alle mit Standard-Intervallen (Ping alle 2 s, andere Prüfungen alle 5 s), normaler Modus (ein
  Echo-Request pro Ping):
  - 70 nur mit Ping,
  - 20 mit Ping und TCP-Port (simulierter Dienst),
  - 5 Pulte (X32-Abfrage per OSC an den simulierten Pult),
  - 5 Dante-Geräte (Ping und mDNS-Ankündigung, passiv).
- Alles auf 127.0.0.1. Die Paketzahl ist dieselbe wie im echten Netz, die Antwortzeiten nicht.
- Paketlimit von Pulse: 50 Pakete/s (Token-Bucket, Spitzen bis zu einer Sekunde). Passive Prüfungen (mDNS,
  Art-Net) kosten nichts.
- Rechner: Intel Core i7-9750H, macOS 26.6.2, Release-Build. Dauer: 60 s.

## Ergebnis

| Wert | gemessen |
|---|---|
| gewünschte Prüfungen | 56,0 pro s (100 Pings / 2 s + 30 Prüfungen / 5 s) |
| gelaufene Prüfungen | 2883 = 48,0 pro s |
| gesendete Pakete | 3047 = 50,8 pro s |
| gesendete Bytes (Schätzung, Ethernet + IP + ICMP) | 2,88 kB/s |
| vom Limit verschobene Läufe | 803 (28 %), im Mittel 0,9 s später |
| Timeouts, Abstürze | 0, 0 |
| Status am Ende | 100 von 100 OK |
| CPU (Testprozess mit Simulator) | 1,2 s in 60 s = 2,1 % eines Kerns |

Die Antworten der Geräte kommen ungefähr in derselben Größe dazu. Hin und zurück sind das also rund 6 kB/s.
Die Schätzung aus dem Plan (unter 10 kB/s) stimmt.

## Was das heißt

- **Das Paketlimit greift bei 100 Geräten.** Gewünscht sind 56 Prüfungen pro Sekunde, Pulse lässt rund 50 Pakete
  pro Sekunde zu. Ein gutes Viertel der Läufe wartet deshalb, im Mittel knapp eine Sekunde. Jeder Ping läuft dann
  etwa alle 2,3 s statt alle 2 s. Das Limit hält, wie es soll (50,8/s mit der erlaubten Spitze am Anfang).
- Bis etwa 85 Geräte mit Ping alle 2 s und ein paar TCP- oder Pult-Prüfungen bleibt Pulse unter dem Limit.
- Wer mehr Geräte hat und die 2 s braucht, kann das Ping-Intervall unwichtiger Geräte auf 3 bis 5 s stellen.
  Das Limit selbst (`Options::max_rate`) ist absichtlich nicht in der Oberfläche einstellbar.
- Im Pre-Show-Check (5 Echo-Requests pro Ping, alles sofort) dauert der erste Durchlauf bei 100 Geräten wegen
  des Limits etwa 10 s.
- Die CPU-Last ist klein. Die meiste Zeit wartet der Scheduler auf Antworten.
