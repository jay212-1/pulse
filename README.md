# Pulse

Netzwerk- und Gerätemonitor der Showtime Suite. Pulse prüft vor und während einer Show, ob Mischpult, Stageboxen, NDI-Quellen, Art-Net-Universen, Tablets, Backup-Rechner und das Netzwerk erreichbar und gesund sind, und zeigt das als Ampel mit Begründung.

Pulse schreibt nie auf Geräte. Es fragt nur, schonend und mit Timeouts.

In Arbeit (Phase 2 von 8, siehe `docs/PLAN.md` und `docs/FORTSCHRITT.md`).

## Entwickeln

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p pulse-net --example watch -- 127.0.0.1 192.0.2.1
```
