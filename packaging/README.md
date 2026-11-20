# Packaging

How Mathcraft is turned into installers. Nothing here is published: the repository is private,
and the release workflow only runs on a `v*` tag in the public repository.

Every package is built from the public tree only, and contains only files on the allow-list of
`tools/release_check.py`: the binary, the icons, the desktop and MIME files, and the licence texts
(`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`, `documentation/THIRD_PARTY_LICENSES.md`, and the font
licence texts in `crates/mathcraft-app/assets/fonts/LICENSES/`). No PTC reference data goes in.

## Versions and tooling

| Output | Built by | Runs on | Tool and licence |
|---|---|---|---|
| `Mathcraft-<v>-macos-<arch>.dmg` | `macos/build-app.sh` | macOS | `iconutil`, `codesign`, `hdiutil`, `plutil` (Apple, shipped with macOS) |
| `Mathcraft-<v>-linux-<arch>.AppImage` | `linux/build-appimage.sh` | Linux | `appimagetool` (MIT) |
| `mathcraft-app_<v>-1_amd64.deb` | `cargo deb` (cargo-deb, MIT or Apache-2.0) | Linux | config in `crates/mathcraft-app/Cargo.toml` (`[package.metadata.deb]`) |
| `Mathcraft-<v>-windows-x64-setup.exe` | `windows/build-installer.ps1` | Windows | Inno Setup 6 (free tool, not shipped; the installer contains only Mathcraft files) |

### The MCP server in every package

Each package also carries `mathcraft-mcp`, the server that lets AI assistants (Claude Code, Claude
Desktop, other MCP clients) work on worksheets. It is a second binary of the `mathcraft-app` crate.
Every package is built with the `render` feature (`cargo build --release --locked -p mathcraft-app
--features render`), so the window and `mathcraft-mcp` both serve the render tools (`render_page`,
`render_region`, `app_screenshot`). The GPU device is created by the first render, not at start-up.
Where the binary lands:

| Package | Path of `mathcraft-mcp` | Stable? |
|---|---|---|
| DMG | `/Applications/Mathcraft.app/Contents/MacOS/mathcraft-mcp` (ad hoc signed like the app) | yes |
| `.deb` | `/usr/bin/mathcraft-mcp` | yes |
| AppImage | `usr/bin/mathcraft-mcp` inside the image | no: the mount path changes each run |
| Windows installer | `{app}\mathcraft-mcp.exe` next to `mathcraft-app.exe`; optional task "Add to PATH" | yes |

The user guide for AI clients is `documentation/MCP_USER_GUIDE.md`; the DMG carries a short
read-me that points to the binary.

### Claude Desktop extension (`.mcpb`)

`mcpb/build-mcpb.sh macos|windows|custom BIN OUT` zips an already-built `mathcraft-mcp` into a
Desktop Extension (needs `zip`): `manifest.json` (from `mcpb/manifest.json.in`, MCPB manifest 0.3,
server type `binary`, a `directory` user setting passed as `--root`, a `win32` command override),
`icon.png` (`icon-256.png`), `server/mathcraft-mcp[.exe]` (mode 755) and the licence texts.
Outputs: `target/packaging/Mathcraft-<v>-macos-<arch>.mcpb` (binary from
`target/packaging/Mathcraft.app`, so build with `KEEP_APP=1`, else `target/release`) and
`target/Mathcraft-<v>-windows-x64.mcpb` (binary from `target/xwin/**/release/mathcraft-mcp.exe`,
or `MCP_EXE`). The bundle is unsigned. Without the extension, `mathcraft-mcp
--install-claude-desktop` writes the config entry directly (see `documentation/MCP_USER_GUIDE.md`).

Not used: cargo-wix (WiX is under the Microsoft Reciprocal Licence, not a permissive licence) and
cargo-bundle (unmaintained; the `.app` is assembled by hand, which is short).

All scripts run from anywhere: they find the repository root themselves. Versions come from
`[workspace.package] version` in the root `Cargo.toml`.

## Files

Icons (`crates/mathcraft-app/assets/icons/`), generated from code by
`cargo run -p mathcraft-app --example make_icons -- crates/mathcraft-app/assets/icons`:

- `logo-master.png`: the logo artwork (1024 px, transparent corners); `icon-16.png` ... `icon-1024.png` are scaled from it, at 16, 32, 48, 64, 128, 256, 512 and 1024 px.
- `mathcraft.ico`: the Windows icon (16, 32, 48 and 256 px, PNG payloads).
- The same artwork is the window icon (`crates/mathcraft-app/src/icon.rs`), so the window and the
  installers match. The artwork is original (a blue tile, a white "M" and an orange baseline);
  it carries no PTC mark.

Linux:

- `linux/mathcraft.desktop`: the desktop entry. It declares `MimeType=application/x-mathcraft-mcdx`.
- `linux/mathcraft-mime.xml`: defines `application/x-mathcraft-mcdx` (glob `*.mcdx`, a subclass of
  `application/zip`). Installed to `/usr/share/mime/packages/`.
- `linux/AppRun`: the AppImage entry point.
- `linux/build-appimage.sh`: assembles the AppDir and runs `appimagetool`. Set `APPIMAGETOOL` to
  its path. `SKIP_BUILD=1` packages the existing release binaries (`mathcraft-app` and
  `mathcraft-mcp`).
- `linux/deb/postinst`, `linux/deb/postrm`: refresh the MIME, desktop and icon caches (cargo-deb
  `maintainer-scripts`).

macOS:

- `macos/Info.plist.in`: the template for `Info.plist`. `@VERSION@` is replaced at build time.
  It declares the `.mcdx` document type and the imported UTI `io.github.lukaspilsl.mathcraft.mcdx` (conforms to
  `public.zip-archive`). The UTI is imported, not exported, because PTC owns the format.
- `macos/build-app.sh`: builds the release binary (unless `SKIP_BUILD=1`), assembles
  `Mathcraft.app` (executable, `mathcraft-mcp`, `Info.plist`, `AppIcon.icns` made with `iconutil`,
  licence texts), signs both executables and the bundle ad hoc, and writes the DMG with an
  `/Applications` link and a `README.txt`. `hdiutil verify` checks the DMG. `KEEP_APP=1` also leaves
  the `.app` in `target/packaging/` for tests.

Windows:

- `windows/mathcraft.iss`: the Inno Setup script. It installs for all users (admin), adds the
  Start menu entry and an optional desktop shortcut, and registers `.mcdx` under `HKLM\Software\Classes`
  (HKA, admin install) with the ProgID `Mathcraft.Worksheet` and the icon. Uninstall removes the
  registration. The AppId GUID is fixed: never change it, or upgrades break.
- `windows/build-installer.ps1`: builds the release binaries (`mathcraft-app` and `mathcraft-mcp`,
  both with the render feature) and runs `ISCC.exe` with the version from `Cargo.toml` (or `-Version`).
  The installer also has an optional task that appends `{app}` to the machine PATH; uninstall
  removes that entry again.

Metadata and CI:

- `Cargo.toml` (workspace): `description`, `license`, `repository` (placeholder
  `https://github.com/lukaspilsl/mathcraft`, replace before the public release), `keywords`, and
  `rust-version`. Each crate inherits them with `.workspace = true`.
- `.github/workflows/release.yml`: on a `v*` tag, a matrix job (ubuntu, macos, windows) builds the
  packages and uploads them as artifacts; a final job creates a **draft** GitHub release with them.
  The AppImage step downloads `appimagetool` from its `continuous` release, which is not pinned.

## Building locally

```sh
# macOS
packaging/macos/build-app.sh                     # target/packaging/Mathcraft-<v>-macos-<arch>.dmg

# Linux
cargo install cargo-deb --locked
cargo build --release --locked -p mathcraft-app --features render
cargo deb -p mathcraft-app --no-build --output target/packaging/
APPIMAGETOOL=/path/to/appimagetool packaging/linux/build-appimage.sh

# Windows (PowerShell, with Inno Setup 6 installed)
packaging\windows\build-installer.ps1
```

## Checks

`python3 -I tools/release_check.py` checks the packaging files: that every file listed above
exists, that the icon sizes match their names, that `Info.plist` parses and declares `.mcdx`, that
the desktop entry and the MIME file agree, that the Inno Setup script registers `.mcdx`, that the
deb assets exist, that each crate carries its Cargo metadata, and that the release workflow has
the three platforms.

## Known limits

- The macOS app and the Windows installer are not code-signed or notarised (paid certificates).
  Gatekeeper and SmartScreen warn on first launch. The shareholder must decide on the cost.
- The app takes a `.mcdx` path as its first argument. Windows (`%1`) and Linux (`%F`) pass the
  path that way. On macOS a file opened from Finder arrives as an Apple event, not an argument;
  that path is untested.
- None of the Linux or Windows packages has been built or run in this repository yet. The macOS
  script was run locally (see the release notes in the pull request).
- The `.deb` runtime dependencies come from `$auto` (dpkg-shlibdeps), which needs `dpkg-shlibdeps`
  on the build machine.
