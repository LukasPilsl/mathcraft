#!/usr/bin/env bash
# Builds a Claude Desktop extension (.mcpb, a zip) around an already-built mathcraft-mcp binary.
#
#   packaging/mcpb/build-mcpb.sh macos                  # target/packaging/Mathcraft-<v>-macos-<arch>.mcpb
#   packaging/mcpb/build-mcpb.sh windows                # target/Mathcraft-<v>-windows-x64.mcpb
#   packaging/mcpb/build-mcpb.sh custom BIN OUT.mcpb    # any binary (name decides .exe or not)
#
# macOS takes the binary from target/packaging/Mathcraft.app (run build-app.sh with KEEP_APP=1)
# or, failing that, target/release/mathcraft-mcp. Windows takes the first
# target/xwin/**/release/mathcraft-mcp.exe (override with MCP_EXE). Needs `zip`.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION=$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p;}' "$ROOT/Cargo.toml" | head -1)
[ -n "$VERSION" ] || { echo "cannot read version from Cargo.toml" >&2; exit 1; }

case "${1:-}" in
macos)
    BIN="$ROOT/target/packaging/Mathcraft.app/Contents/MacOS/mathcraft-mcp"
    [ -x "$BIN" ] || BIN="$ROOT/target/release/mathcraft-mcp"
    OUT="$ROOT/target/packaging/Mathcraft-$VERSION-macos-$(uname -m).mcpb"
    ;;
windows)
    BIN="${MCP_EXE:-$(find "$ROOT/target/xwin" -path '*release/mathcraft-mcp.exe' 2>/dev/null | head -1)}"
    OUT="$ROOT/target/Mathcraft-$VERSION-windows-x64.mcpb"
    ;;
custom)
    BIN="${2:?binary path}"
    OUT="${3:?output path}"
    ;;
*)
    echo "usage: $0 macos | windows | custom BIN OUT.mcpb" >&2
    exit 2
    ;;
esac
[ -n "$BIN" ] && [ -f "$BIN" ] || { echo "missing binary: ${BIN:-<none found>}" >&2; exit 1; }

case "$BIN" in
*.exe) NAME=mathcraft-mcp.exe ;;
*) NAME=mathcraft-mcp ;;
esac

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/server" "$(dirname "$OUT")"
sed "s/@VERSION@/$VERSION/g" "$ROOT/packaging/mcpb/manifest.json.in" > "$STAGE/manifest.json"
cp "$ROOT/crates/mathcraft-app/assets/icons/icon-256.png" "$STAGE/icon.png"
cp "$BIN" "$STAGE/server/$NAME"
chmod 755 "$STAGE/server/$NAME"
cp "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$ROOT/NOTICE" "$STAGE/"

python3 -I -c 'import json,sys; json.load(open(sys.argv[1]))' "$STAGE/manifest.json"
rm -f "$OUT"
# -X drops extra attributes; zip stores the Unix mode bits, so the binary stays executable.
(cd "$STAGE" && zip -q -X -r "$OUT" manifest.json icon.png server LICENSE-MIT LICENSE-APACHE NOTICE)
echo "$OUT"
