#!/usr/bin/env bash
# Build a universal (Apple Silicon + Intel) macOS app and a DMG to share.
#
#   scripts/package-macos.sh            # creates dist/EV Showroom POS.app and dist/*.dmg
#   scripts/package-macos.sh --install  # also copies the app into /Applications
#
# Needs: Rust with both targets (rustup target add aarch64-apple-darwin x86_64-apple-darwin)
# and the Xcode command line tools. The app is signed ad-hoc (no Apple Developer ID),
# so on another Mac it must be opened once via System Settings → Privacy & Security → Open Anyway.
set -euo pipefail
cd "$(dirname "$0")/.."

APP_NAME="EV Showroom POS"
VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')
DIST=dist
APP="$DIST/$APP_NAME.app"
export MACOSX_DEPLOYMENT_TARGET=11.0

case "$(uname -m)" in
  arm64) HOST=aarch64-apple-darwin; OTHER=x86_64-apple-darwin ;;
  *)     HOST=x86_64-apple-darwin;  OTHER=aarch64-apple-darwin ;;
esac

echo "==> Building ($HOST and $OTHER)"
cargo build --release -p pos-app
cargo build --release -p pos-app --target "$OTHER"

echo "==> Assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create -output "$APP/Contents/MacOS/ev-pos" \
  target/release/ev-pos "target/$OTHER/release/ev-pos"
sed "s/__VERSION__/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"

echo "==> Making the icon"
WORK=$(mktemp -d)
ICONSET="$WORK/AppIcon.iconset"
mkdir -p "$ICONSET"
qlmanage -t -s 1024 -o "$WORK" packaging/macos/icon.svg >/dev/null 2>&1
SRC="$WORK/icon.svg.png"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" "$SRC" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) "$SRC" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

echo "==> Signing (ad-hoc)"
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"

echo "==> Creating DMG"
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
DMG="$DIST/EV-Showroom-POS-$VERSION-mac.dmg"
rm -f "$DMG"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$WORK"
echo "Created: $DMG ($(du -h "$DMG" | cut -f1))"

if [[ "${1:-}" == "--install" ]]; then
  echo "==> Installing to /Applications"
  rm -rf "/Applications/$APP_NAME.app"
  ditto "$APP" "/Applications/$APP_NAME.app"
  echo "Installed: /Applications/$APP_NAME.app"
fi
