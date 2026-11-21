#!/bin/bash
# Builds dist/Pulse-x.y.z.dmg and dist/pulse-agent-x.y.z-macos.zip on a Mac,
# for this Mac's architecture (one compile). UNIVERSAL=1 builds Apple Silicon
# + Intel. Packages are only made again when the program changed. The list of what belongs to this
# build: dist/files-mac.txt.
set -euo pipefail
cd "$(dirname "$0")/../.."
# shellcheck source=../../scripts/ui.sh
. scripts/ui.sh
[ "$(uname -s)" = "Darwin" ] || ui_fail "This script must run on macOS."
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"
command -v cargo >/dev/null 2>&1 || export PATH="$HOME/.cargo/bin:$PATH"
V=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
mkdir -p dist
ui_header "Pulse $V" "Mac bauen"

if [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = "1" ]; then HOST=arm64; else HOST=x86_64; fi
ARCHS="${ARCHS:-$HOST}"
[ -n "${UNIVERSAL:-}" ] && ARCHS="arm64 x86_64"
BINS=()
AGENTS=()
for a in $ARCHS; do
  case "$a" in
    arm64|aarch64) a=arm64; t=aarch64-apple-darwin ;;
    *) a=x86_64; t=x86_64-apple-darwin ;;
  esac
  if [ "$a" = "$HOST" ] && [ "$(uname -m)" = "$HOST" ]; then
    ui_run "Programm für diesen Mac ($a)" cargo build --release --locked
    BINS+=("target/release/pulse")
    AGENTS+=("target/release/pulse-agent")
  else
    rustup target list --installed 2>/dev/null | grep -qx "$t" || ui_run "Rust-Ziel $t (einmalig)" rustup target add "$t"
    ui_run "Programm für $a" cargo build --release --locked --target "$t"
    BINS+=("target/$t/release/pulse")
    AGENTS+=("target/$t/release/pulse-agent")
  fi
done

DMG="dist/Pulse-$V.dmg"
KEY="$V-$(ui_fingerprint "${BINS[@]}" packaging/macos/Info.plist assets/icon.png)"
make_dmg() {
  WORK=$(mktemp -d)
  APP="$WORK/dmg/Pulse.app"
  mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
  lipo -create -output "$APP/Contents/MacOS/pulse" "${BINS[@]}"
  sed "s/APP_VERSION/$V/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
  ICONSET="$WORK/Pulse.iconset"
  mkdir -p "$ICONSET"
  for s in 16 32 128 256 512; do
    sips -z $s $s assets/icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
    sips -z $((s*2)) $((s*2)) assets/icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
  done
  iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Pulse.icns"
  # Ad-hoc signature: macOS asks once for "Open Anyway".
  codesign --force --deep -s - "$APP"
  ln -s /Applications "$WORK/dmg/Applications"
  rm -f "$DMG"
  hdiutil create -volname "Pulse" -srcfolder "$WORK/dmg" -ov -format UDZO "$DMG"
  rm -rf "$WORK"
}
if ui_fresh "$DMG" "$KEY"; then ui_skip "App und DMG" "unverändert"; else
  ui_run "App und DMG" make_dmg && ui_made "$DMG" "$KEY"
fi

# The agent for show computers: a plain program in a zip, signed ad hoc.
ZIP="dist/pulse-agent-$V-macos.zip"
AKEY="$V-$(ui_fingerprint "${AGENTS[@]}" packaging/agent/README.txt)"
make_agent_zip() {
  WORK=$(mktemp -d)
  mkdir -p "$WORK/pulse-agent"
  lipo -create -output "$WORK/pulse-agent/pulse-agent" "${AGENTS[@]}"
  codesign --force -s - "$WORK/pulse-agent/pulse-agent"
  cp packaging/agent/README.txt "$WORK/pulse-agent/"
  rm -f "$ZIP"
  (cd "$WORK" && ditto -c -k --norsrc --noextattr --keepParent pulse-agent agent.zip)
  mv "$WORK/agent.zip" "$ZIP"
  rm -rf "$WORK"
}
if ui_fresh "$ZIP" "$AKEY"; then ui_skip "Agent-Zip" "unverändert"; else
  ui_run "Agent-Zip" make_agent_zip && ui_made "$ZIP" "$AKEY"
fi
{ echo "$V"; echo "$DMG"; echo "$ZIP"; } > dist/files-mac.txt
ui_done
