#!/bin/bash
# Builds Pulse for Linux and Windows on one Linux PC:
#   dist/pulse-<v>-linux-x86_64.tar.gz          portable Linux program
#   dist/pulse-agent-<v>-x86_64-linux.tar.gz    Pulse Agent for Linux
#   dist/Pulse-Setup-<v>.exe                    Windows installer (Inno Setup under Wine)
#   dist/pulse-agent-<v>-windows-x86_64.zip     Pulse Agent for Windows
# The list of what belongs to this build: dist/files-linux-pc.txt (release.sh
# uploads exactly these). Nothing is done twice: tests once per commit,
# packages only when the program changed. SKIP_WINDOWS=1 skips Windows.
set -euo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=../scripts/ui.sh
. scripts/ui.sh
V=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
mkdir -p dist target
FILES=()
ui_header "Pulse $V" "Linux + Windows bauen"

key=""
[ -z "$(git status --porcelain --untracked-files=no 2>/dev/null)" ] && key=$(git rev-parse HEAD 2>/dev/null || true)
if [ -n "$key" ] && [ -f "target/.tested/$key" ]; then
  ui_skip "Tests" "für diesen Stand schon bestanden"
else
  ui_run "Tests" cargo test --workspace --locked
  [ -n "$key" ] && mkdir -p target/.tested && touch "target/.tested/$key"
fi

ui_run "Linux-Programm" cargo build --release --locked
BIN=target/release/pulse
KEY="$V-$(ui_fingerprint "$BIN")"
NAME="pulse-$V-linux-x86_64"
TAR="dist/$NAME.tar.gz"
make_tar() {
  rm -rf "dist/$NAME" && mkdir -p "dist/$NAME"
  cp "$BIN" README.md packaging/linux/pulse.desktop "dist/$NAME/"
  cp assets/icon.png "dist/$NAME/pulse.png"
  tar -C dist -czf "$TAR" "$NAME" && rm -rf "dist/$NAME"
}
if ui_fresh "$TAR" "$KEY"; then ui_skip "Linux-Archiv" "unverändert"; else
  ui_run "Linux-Archiv" make_tar && ui_made "$TAR" "$KEY"
fi
FILES+=("$TAR")

# The agent for show computers (its names must not end like the app's
# packages: the launcher picks the app by name ending).
ABIN=target/release/pulse-agent
AKEY="$V-$(ui_fingerprint "$ABIN" packaging/agent/README.txt)"
ATAR="dist/pulse-agent-$V-x86_64-linux.tar.gz"
make_agent_tar() {
  rm -rf dist/pulse-agent && mkdir -p dist/pulse-agent
  cp "$ABIN" packaging/agent/README.txt dist/pulse-agent/
  tar -C dist -czf "$ATAR" pulse-agent && rm -rf dist/pulse-agent
}
if ui_fresh "$ATAR" "$AKEY"; then ui_skip "Agent für Linux" "unverändert"; else
  ui_run "Agent für Linux" make_agent_tar && ui_made "$ATAR" "$AKEY"
fi
FILES+=("$ATAR")

if [ -z "${SKIP_WINDOWS:-}" ]; then
  ui_run "Windows-Programm" cargo build --release --locked --target x86_64-pc-windows-gnu
  EXE=target/x86_64-pc-windows-gnu/release/pulse.exe
  WKEY="$V-$(ui_fingerprint "$EXE")"
  ISCC="${ISCC:-$HOME/.wine/drive_c/InnoSetup/ISCC.exe}"
  WINE=$(command -v wine || command -v wine64 || true)
  export WINEDEBUG=-all WINEDLLOVERRIDES="mscoree,mshtml="
  SETUP="dist/Pulse-Setup-$V.exe"
  make_setup() {
    rm -rf stage && mkdir -p stage && cp "$EXE" README.md stage/
    "$WINE" "$ISCC" "/DAppVersion=$V" "Z:$(pwd)/packaging/windows/pulse.iss"
    rm -rf stage
  }
  if [ -n "$WINE" ] && [ -f "$ISCC" ]; then
    if ui_fresh "$SETUP" "$WKEY"; then ui_skip "Windows-Installer" "unverändert"; else
      ui_run "Windows-Installer" make_setup && ui_made "$SETUP" "$WKEY"
    fi
    FILES+=("$SETUP")
  else
    ui_warn "Windows-Installer übersprungen: Wine oder Inno Setup fehlt (release.sh richtet beides ein)"
  fi
  AEXE=target/x86_64-pc-windows-gnu/release/pulse-agent.exe
  AWKEY="$V-$(ui_fingerprint "$AEXE" packaging/agent/README.txt)"
  AZIP="dist/pulse-agent-$V-windows-x86_64.zip"
  make_agent_zip() {
    rm -rf dist/pulse-agent && mkdir -p dist/pulse-agent
    cp "$AEXE" dist/pulse-agent/
    sed 's/$/\r/' packaging/agent/README.txt > dist/pulse-agent/README.txt
    rm -f "$AZIP"
    (cd dist && python3 -m zipfile -c "$(basename "$AZIP")" pulse-agent) && rm -rf dist/pulse-agent
  }
  if ui_fresh "$AZIP" "$AWKEY"; then ui_skip "Agent für Windows" "unverändert"; else
    ui_run "Agent für Windows" make_agent_zip && ui_made "$AZIP" "$AWKEY"
  fi
  FILES+=("$AZIP")
fi

{ echo "$V"; printf '%s\n' "${FILES[@]}"; } > dist/files-linux-pc.txt
ui_done
