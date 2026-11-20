#!/bin/sh
# Builds Mathcraft.app and a compressed DMG on macOS. Uses only Apple's own tools (plutil,
# iconutil, codesign, hdiutil); no third-party packaging tool.
#
#   packaging/macos/build-app.sh                 build the release binaries, then package them
#   SKIP_BUILD=1 packaging/macos/build-app.sh    package target/release/mathcraft-app and
#                                                target/release/mathcraft-mcp as they are
#   KEEP_APP=1 packaging/macos/build-app.sh      also leave target/packaging/Mathcraft.app (bundle id
#                                                io.github.lukaspilsl.mathcraft) next to the DMG, for testing
#
# Output: target/packaging/Mathcraft-<version>-macos-<arch>.dmg (the .app is in the DMG).
# Staging files live in target/packaging/staging and are removed at the end.
#
# The app also carries the MCP server: Mathcraft.app/Contents/MacOS/mathcraft-mcp, a stable path
# for AI clients (see documentation/MCP_USER_GUIDE.md). It is built with the render feature.
#
# The app is signed ad hoc (codesign --sign -). That is not a Developer ID signature: Gatekeeper
# blocks the first launch until the user opens it from the context menu. Notarisation needs a
# paid Apple Developer account and is not done here.
set -eu

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
ARCH=$(uname -m)
OUT="$ROOT/target/packaging"
STAGING="$OUT/staging"
APP_NAME="Mathcraft.app"
BIN="$ROOT/target/release/mathcraft-app"
MCP_BIN="$ROOT/target/release/mathcraft-mcp"
ICONS="$ROOT/crates/mathcraft-app/assets/icons"

if [ -z "${SKIP_BUILD:-}" ]; then
    # Both binaries (the window and mathcraft-mcp) carry the offscreen renderer for the render tools.
    cargo build --release --locked -p mathcraft-app --features render
fi
[ -x "$BIN" ] || { echo "missing binary: $BIN" >&2; exit 1; }
[ -x "$MCP_BIN" ] || { echo "missing binary: $MCP_BIN" >&2; exit 1; }

rm -rf "$STAGING"
mkdir -p "$STAGING/dmg" "$OUT"
APP="$STAGING/dmg/$APP_NAME"
CONTENTS="$APP/Contents"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources/licenses/fonts"

# Executable and metadata.
cp "$BIN" "$CONTENTS/MacOS/mathcraft-app"
chmod 755 "$CONTENTS/MacOS/mathcraft-app"
cp "$MCP_BIN" "$CONTENTS/MacOS/mathcraft-mcp"
chmod 755 "$CONTENTS/MacOS/mathcraft-mcp"
sed "s/@VERSION@/$VERSION/g" "$ROOT/packaging/macos/Info.plist.in" > "$CONTENTS/Info.plist"
plutil -lint "$CONTENTS/Info.plist"
printf 'APPL????' > "$CONTENTS/PkgInfo"

# AppIcon.icns from the PNG set. iconutil wants exact file names inside an .iconset folder.
ICONSET="$STAGING/AppIcon.iconset"
mkdir -p "$ICONSET"
cp "$ICONS/icon-16.png" "$ICONSET/icon_16x16.png"
cp "$ICONS/icon-32.png" "$ICONSET/icon_16x16@2x.png"
cp "$ICONS/icon-32.png" "$ICONSET/icon_32x32.png"
cp "$ICONS/icon-64.png" "$ICONSET/icon_32x32@2x.png"
cp "$ICONS/icon-128.png" "$ICONSET/icon_128x128.png"
cp "$ICONS/icon-256.png" "$ICONSET/icon_128x128@2x.png"
cp "$ICONS/icon-256.png" "$ICONSET/icon_256x256.png"
cp "$ICONS/icon-512.png" "$ICONSET/icon_256x256@2x.png"
cp "$ICONS/icon-512.png" "$ICONSET/icon_512x512.png"
cp "$ICONS/icon-1024.png" "$ICONSET/icon_512x512@2x.png"
iconutil -c icns "$ICONSET" -o "$CONTENTS/Resources/AppIcon.icns"

# Licence texts and attribution (see NOTICE and documentation/THIRD_PARTY_LICENSES.md).
LIC="$CONTENTS/Resources/licenses"
cp LICENSE-MIT LICENSE-APACHE NOTICE "$LIC/"
cp documentation/THIRD_PARTY_LICENSES.md "$LIC/"
cp crates/mathcraft-app/assets/fonts/LICENSES/*.txt "$LIC/fonts/"

# Ad hoc signature: the bundle must be sealed, and an arm64 binary must carry a signature.
# The helper binary is signed on its own first, the same way as the app.
codesign --force --sign - "$CONTENTS/MacOS/mathcraft-mcp"
codesign --force --sign - "$APP"
codesign --verify --strict --verbose=2 "$APP"

# DMG: the app, a link to /Applications so the user can drag it across, and a short read-me.
ln -s /Applications "$STAGING/dmg/Applications"
cat > "$STAGING/dmg/README.txt" <<EOF
Mathcraft $VERSION

Drag Mathcraft.app to Applications, then open it. The first launch needs
right-click > Open, because the app is not notarised.

Also inside the app: the MCP server for AI assistants (Claude Code, Claude Desktop,
other MCP clients). Its path is stable:

    /Applications/Mathcraft.app/Contents/MacOS/mathcraft-mcp

Connect it with, for example:

    claude mcp add mathcraft -- /Applications/Mathcraft.app/Contents/MacOS/mathcraft-mcp --root ~/Documents

The user guide, with the safety model and the config snippets, is in the
documentation folder of the source code.
EOF
DMG="$OUT/Mathcraft-$VERSION-macos-$ARCH.dmg"
rm -f "$DMG"
hdiutil create -quiet -volname "Mathcraft $VERSION" -srcfolder "$STAGING/dmg" \
    -ov -format UDZO "$DMG"
hdiutil verify -quiet "$DMG"

if [ -n "${KEEP_APP:-}" ]; then
    rm -rf "$OUT/$APP_NAME"
    cp -R "$APP" "$OUT/$APP_NAME"
    echo "wrote $OUT/$APP_NAME"
fi
rm -rf "$STAGING"
echo "wrote $DMG"
