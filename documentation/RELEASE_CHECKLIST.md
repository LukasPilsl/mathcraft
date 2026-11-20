# Release checklist (public open-source release)

Status: the repository is private. Nothing here has been published, and no remote exists. This
document is the plan for the day the shareholder decides to publish. Licence: MIT OR Apache-2.0,
permissive dependencies only (decided 2026-11-16).

Legend: [x] done in the repository, [ ] open.

## 1. Blockers: must all be true before the repository becomes public

- [ ] **Rewrite the git history (DESTRUCTIVE, needs the shareholder's approval).** PTC reference
  material and unknown-authorship samples are in past commits and must not reach the public
  repository. Files to purge from every commit, branch, tag and stash: `documentation/examples/`
  (all `*.mcdx` and `operations/Bild.png`), `documentation/mirror.py` if it was ever tracked, any
  `documentation/mathcad-prime-12-help/` or `screenshots/` blob. Recommended route: do not rewrite
  the private history at all. Export a clean tree of the chosen commit (`git archive`) into a new
  directory, commit it once as the first commit of a new repository, and publish that. If history
  must be kept, use `git filter-repo` on a fresh mirror clone, then verify with `git log --all
  --name-only | grep -E 'mcdx|mathcad-prime|screenshots|mirror.py'` returning nothing. Remove the
  old clones, worktrees and reflog. The current repository is not changed by this checklist.
- [ ] **Replace the sample-based test inputs** with worksheets made for this project. Until then
  the sample-based tests skip in the public repository (`require_samples!()`), so the golden
  comparison and the sample round trips are not run there. Add self-made worksheets under a test
  data folder with a clear licence, and point the tests at them.
- [ ] **Decide on internal files.** `CLAUDE.md` and `ROADMAP.md` are working files of the
  project's internal process. Keep, rewrite or remove them; `tools/release_check.py` warns about
  them until this is decided.
- [ ] **Review the documentation for copied PTC text.** `documentation/MCDX_FORMAT.md`,
  `REBUILD_GUIDE.md` and `SYMBOLIC_OPTIONS.md` are project notes. The overlap scan
  (`tools/release_check.py`, check b) covers only `.rs` and `.py` files, so read these by hand.
  Decided: `UI_DEFECTS.md` and `UNVERIFIED_FORMATS.md` are original notes and are allow-listed.
  `MISSING_FUNCTIONS.md` lists help page names per function and is excluded from the public tree
  (the check fails on it in the repository; the export drops it).
  Also check `tools/function_notes.tsv` and the UI strings in `crates/mathcraft-app/src` for
  copied help text.
- [ ] **Trademark and branding.** No PTC logo or name as a product name. Mentions of "Mathcad
  Prime" are factual (file format, compatibility) and stay with the trademark notice in the
  README. Check the window title, the About text and the PDF metadata.
- [ ] **Font metadata.** Read the copyright and name-table strings of the bundled fonts (STIX Two
  and the egui fonts) and put them in `documentation/THIRD_PARTY_LICENSES.md`; the document says
  they were not yet extracted. Confirm the Ubuntu Font Licence notice.
- [ ] **Placeholders filled in.** Repository URL (`repository` in `[workspace.package]` is the
  placeholder `https://github.com/OWNER/mathcraft`; `release_check.py` warns until it is replaced),
  a security contact in `SECURITY.md` (the text says
  "the address given on the project's page"), and the same contact route for the Code of Conduct.
  The bundle identifier `org.mathcraft.app` and the installer `AppId` GUID are placeholders too:
  the GUID must never change once released.
- [x] **`rust-version`** set in `[workspace.package]` to `1.90`, the highest `rust-version` in the
  locked dependency graph. Not yet verified: only Rust 1.94 is installed here, so no older
  toolchain has built the workspace. Verify on 1.90 before release (`rustup toolchain install 1.90`).
- [ ] **Clean-export check passes:** `python3 -I tools/release_check.py --public-tree DIR` prints
  `RESULT: PASS`, where DIR is the export from the first item. The check also runs the licence
  audit with `cargo metadata` in DIR. The export is `git archive HEAD | tar -x -C DIR`, then
  delete `DIR/documentation/examples`, `DIR/documentation/MISSING_FUNCTIONS.md` and any
  `screenshots/` or help tree (the check fails on each). The check does not build the export.
- [ ] **Tests pass in the clean export:** in DIR, `cargo fmt --all --check`, `cargo clippy
  --workspace --all-targets -- -D warnings` and `cargo test --workspace` all succeed, with no
  sample worksheets present (they skip with a message).
- [ ] **CI green** on Linux, macOS and Windows in the public repository (`.github/workflows/ci.yml`
  has not run anywhere yet).
- [ ] **Shareholder sign-off** on the licence, the public name, and the release scope.

## 2. What the tools check

`python3 -I tools/release_check.py` (default) checks the files git tracks in this repository and
reports what would have to be excluded. `--public-tree DIR` checks an exported tree instead.
`--help-dir PATH` points check (b) at a local help tree; `--skip-licences` skips check (c) when
cargo is unavailable. The script exits 1 on any FAIL. WARN lines do not change the exit status.

| check | what it does |
|---|---|
| (a) files | Allow-list of tracked files. Fails on help tree, screenshots, `mirror.py`, `documentation/examples`, any `.mcdx`, images, and `include_bytes!`/`include_str!` of those paths. |
| (b) overlap | With the help tree present locally: 200 identical characters in a row (whitespace collapsed) shared between a `.rs`/`.py` file and a help page: WARN. |
| (c) licences | Shipped graph of `mathcraft-app` on the five triples (same rules as `tools/gen_licenses.py`): FAIL on copyleft-only expressions, missing licences, and any non-permissive licence that is not a font licence. Build-time copyleft: WARN. `cargo-deny` is not used: it needs its own `deny.toml`. |
| (d) fonts | Every `.ttf` has its licence text and a row in the third-party document; the document names each font family; `NOTICE` points to the document. |
| (e) test skips | Every test file that names the samples uses `require_samples!()`. |
| (f) packaging | `packaging/` files exist and are described in `packaging/README.md`; icon sizes match; `Info.plist`, the desktop entry, the MIME file and the Inno script agree on `.mcdx`; deb and Inno sources exist; every crate inherits the Cargo metadata; the release workflow runs on `v*` tags for all three platforms. Placeholder `repository`: WARN. |

## 3. Release build

`[profile.release]` in the root `Cargo.toml`:

- `lto = "thin"`, `codegen-units = 1`, `opt-level = 3`: smaller and faster binary; the build is
  slower. Measure the build time on the CI runners before enabling this in CI.
- `panic = "unwind"`: keep it. The app catches panics in its background evaluation thread
  (`crates/mathcraft-app/src/loader.rs`), and the golden harness does the same
  (`crates/mathcraft-cli/src/golden.rs`). Switching to `"abort"` would turn a panic on one bad
  worksheet into a crash of the whole app.
- `strip = "debuginfo"`: keeps symbols, so crash backtraces stay readable, and drops debug info.

Build: `cargo build --release -p mathcraft-app`. Before shipping, check that the binary contains no
sample data: the samples are read from disk only by tests, so this is expected to hold.

## 4. Packaging

Written, see `packaging/README.md` for each file and the local build commands. Not yet built or
run for Linux and Windows.

- [x] **Icon:** original artwork drawn in code (`crates/mathcraft-app/src/icon.rs`), the window
  icon, and the files in `crates/mathcraft-app/assets/icons/` (PNG 16 to 1024 px, `mathcraft.ico`).
- [x] **macOS:** `packaging/macos/build-app.sh` makes `Mathcraft.app` and a DMG with Apple's own
  tools. Ad-hoc signed. Built and checked locally (structure, `plutil`, `codesign --verify`,
  `hdiutil verify`).
- [x] **Linux:** `.desktop` entry and shared-mime-info type `application/x-mathcraft-mcdx`
  (`packaging/linux/`), `cargo deb` metadata in `crates/mathcraft-app/Cargo.toml`, and an AppImage
  script (`appimagetool`, MIT). Not yet built.
- [x] **Windows:** Inno Setup script (`packaging/windows/mathcraft.iss`) with the `.mcdx`
  registration, and `build-installer.ps1`. Not yet built. Inno Setup is a free tool, not shipped.
- [x] **Release workflow:** `.github/workflows/release.yml` builds all three platforms on a `v*`
  tag and creates a draft GitHub release. Never run.
- [ ] **Code signing and notarisation.** Apple Developer ID, notarisation and a Windows
  code-signing certificate are paid services: the shareholder must approve them first. Until then
  Gatekeeper and SmartScreen warn on first launch.
- [ ] **Pin `appimagetool`** in the release workflow (it downloads the unpinned `continuous`
  release now) and check the Linux packages on a clean Ubuntu machine.
- [ ] **Cross-platform archives and installers:** `cargo-dist` could replace the hand-written
  workflow later. It needs the repository URL and would also generate its own workflow.
- Every package must include the licence texts and the font licence texts (see `NOTICE`). The
  attribution rules are in `documentation/THIRD_PARTY_LICENSES.md`, "What an open-source release
  must ship for the fonts".
- Packages are built from the public tree only. Never package from the private checkout.

## 5. CI

`.github/workflows/ci.yml` has two jobs:

- `check`: on Linux, macOS and Windows: `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets --locked -- -D warnings`, `cargo build --workspace --locked`, `cargo test
  --workspace --locked`. It does not need the samples or the help tree.
- `release-check`: on Linux: `python3 -I tools/release_check.py`. This fails on any file that must
  not be public, and on licence problems.

Not in CI yet: the golden comparison (needs samples; run locally). Release builds and packaging run
only in `.github/workflows/release.yml`, on a `v*` tag, and attach the packages to a draft release.

## 6. Local reference data (never published)

- `documentation/examples/` (sample worksheets, authorship unknown) and the Prime help tree and
  screenshots stay on the owner's machine. The samples are used by the golden tests, which skip
  when they are absent.
- The release check's overlap scan (b) reads the help Markdown from a local path when given
  `--help-dir`; nothing from it is copied into the repository.
