#!/bin/bash
# Pulse on the Mac: update this checkout, build the DMG and install the app.
#
#   mkdir -p ~/Desktop/dev
#   git clone https://github.com/jay212-1/pulse.git ~/Desktop/dev/pulse
#   cd ~/Desktop/dev/pulse
#   bash install-mac.sh
#
# Run it again later to update; only what is needed is done.
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=scripts/ui.sh
. scripts/ui.sh
# shellcheck source=scripts/setup.sh
. scripts/setup.sh
[ "$(uname -s)" = "Darwin" ] || ui_fail "Dieses Skript ist für macOS (Linux: ./install.sh)."
setup_no_root
[ -n "${SM_PULLED:-}" ] || ui_header "Pulse" "Installieren / Aktualisieren"

if ! xcode-select -p >/dev/null 2>&1; then
  xcode-select --install || true
  ui_fail "Im Fenster, das sich geöffnet hat, auf 'Installieren' klicken und warten. Dann nochmal: bash install-mac.sh"
fi
if [ -z "${SM_PULLED:-}" ]; then
  if setup_pull; then SM_PULLED=1 exec bash "$(pwd)/$(basename "$0")" "$@"; fi
fi
setup_rust

ui_section "Bauen"
./packaging/macos/build-dmg.sh
V=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
DMG="dist/Pulse-$V.dmg"

ui_section "Installieren"
[ -n "${NO_INSTALL:-}" ] && { ui_ok "Installer: $DMG"; ui_done; exit 0; }
STAMP=dist/.made/installed-app
# All show tools share one folder (as Showtime Suite installs them).
APPDIR=$(mac_app_dir)
WANT=$(cat "dist/.made/$(basename "$DMG")" 2>/dev/null || echo none)
if [ -d "$APPDIR/Pulse.app" ] && [ "$(cat "$STAMP" 2>/dev/null)" = "$WANT" ]; then
  ui_skip "In $APPDIR" "Pulse $V ist schon installiert"
elif pgrep -x pulse >/dev/null 2>&1; then
  ui_warn "Pulse läuft gerade – nicht ersetzt (ungespeicherte Änderungen gehen sonst verloren)."
  ui_info "Beenden (Cmd+Q), dann nochmal: bash install-mac.sh"
else
  ui_run "In $APPDIR" mac_app_put "$DMG" Pulse.app "$APPDIR"
  printf '%s' "$WANT" > "$STAMP"
fi

ui_section "Fertig"
ui_ok "Pulse ist in Programme › Showtime Suite (Launchpad, Spotlight)"
ui_done
open "$APPDIR/Pulse.app"
