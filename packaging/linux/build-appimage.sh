#!/bin/sh
# Builds a Linux AppImage from the release binary. The AppDir is assembled here; appimagetool
# (MIT licence, https://github.com/AppImage/appimagetool) turns it into the image.
#
#   APPIMAGETOOL=/path/to/appimagetool packaging/linux/build-appimage.sh
#   SKIP_BUILD=1 ... to package target/release/mathcraft-app as it is
#
# Output: target/packaging/Mathcraft-<version>-linux-<arch>.AppImage
set -eu

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
ARCH=$(uname -m)
APPIMAGETOOL="${APPIMAGETOOL:-appimagetool}"
OUT="$ROOT/target/packaging"
APPDIR="$OUT/staging/Mathcraft.AppDir"
ICONS="$ROOT/crates/mathcraft-app/assets/icons"

command -v "$APPIMAGETOOL" >/dev/null 2>&1 || {
    echo "appimagetool not found; set APPIMAGETOOL to its path" >&2
    exit 1
}
if [ -z "${SKIP_BUILD:-}" ]; then
    # Both binaries (the window and mathcraft-mcp) carry the offscreen renderer for the render tools.
    cargo build --release --locked -p mathcraft-app --features render
fi
[ -x target/release/mathcraft-mcp ] || { echo "missing binary: target/release/mathcraft-mcp" >&2; exit 1; }

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/mime/packages" \
    "$APPDIR/usr/share/icons/hicolor/256x256/apps" "$APPDIR/usr/share/doc/mathcraft/fonts" \
    "$OUT"

install -m 755 target/release/mathcraft-app "$APPDIR/usr/bin/mathcraft-app"
# The MCP server. Inside an AppImage its path changes on every run (the image is mounted under
# /tmp), so AI clients should use the copy that the .deb installs at /usr/bin/mathcraft-mcp.
install -m 755 target/release/mathcraft-mcp "$APPDIR/usr/bin/mathcraft-mcp"
install -m 755 packaging/linux/AppRun "$APPDIR/AppRun"
# The desktop entry and icon sit at the AppDir root (required) and in the FreeDesktop tree.
install -m 644 packaging/linux/mathcraft.desktop "$APPDIR/mathcraft.desktop"
install -m 644 packaging/linux/mathcraft.desktop "$APPDIR/usr/share/applications/mathcraft.desktop"
install -m 644 packaging/linux/mathcraft-mime.xml "$APPDIR/usr/share/mime/packages/mathcraft.xml"
install -m 644 "$ICONS/icon-256.png" "$APPDIR/mathcraft.png"
install -m 644 "$ICONS/icon-256.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/mathcraft.png"

# Licence texts and attribution.
DOC="$APPDIR/usr/share/doc/mathcraft"
install -m 644 LICENSE-MIT LICENSE-APACHE NOTICE documentation/THIRD_PARTY_LICENSES.md "$DOC/"
install -m 644 crates/mathcraft-app/assets/fonts/LICENSES/*.txt "$DOC/fonts/"

ARCH="$ARCH" "$APPIMAGETOOL" "$APPDIR" "$OUT/Mathcraft-$VERSION-linux-$ARCH.AppImage"
echo "wrote $OUT/Mathcraft-$VERSION-linux-$ARCH.AppImage"
