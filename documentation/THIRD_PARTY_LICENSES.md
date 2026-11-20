# Third-party licences of mathcraft-app

Scope: every crate that `mathcraft-app` links into the shipped binary on the five target triples
below (the "shipped" table), the crates used only at build time (the "build-time only" table), and
the fonts that `eframe` compiles in. Nothing is downloaded or run for this report.

## How the crate table was generated

- Generator: `tools/gen_licenses.py` (tracked; Python 3 standard library only). Re-run it with
  `python3 -I tools/gen_licenses.py` from anywhere in the repository. `--offline` uses only the
  local registry; `--stdout` prints the table instead of writing it.
- Date of the generation below: 2026-11-16. Source: `Cargo.lock` of this commit.
- Triples: `x86_64-unknown-linux-gnu` (linux-x64), `x86_64-pc-windows-msvc` (windows-x64),
  `aarch64-apple-darwin` (macos-arm64), `x86_64-apple-darwin` (macos-x64),
  `aarch64-unknown-linux-gnu` (linux-arm64). Not covered: `aarch64-pc-windows-msvc` and the
  UEFI, wasm and other targets. Add a triple to `TRIPLES` in the script to cover it.
- Membership: `cargo tree -p mathcraft-app --target <triple>`, which is the graph that
  `cargo build -p mathcraft-app --target <triple>` compiles. Shipped = `-e normal,no-proc-macro`
  (normal dependencies, proc-macros not expanded). Build-time only = the rest of `-e normal,build`
  (proc-macros, build-script dependencies). Dev-dependency edges are not followed.
- Licences: the `license` field of each package in `cargo metadata --locked` (or `license_file`
  when `license` is empty). The script executes only `cargo`; it never runs build scripts or code
  from a crate.
- The first version of this report listed 111 crates from macOS only. It was incomplete for Linux
  and Windows and is replaced by this table.

## Flags

- **Copyleft option, not linked for these triples:** `r-efi` 5.3.0 and 6.0.0, `MIT OR Apache-2.0 OR
  LGPL-2.1-or-later`. It is not linked for the five triples (it is pulled only by `getrandom` for
  the `uefi` target). If a UEFI build is ever shipped, the licence is MIT, chosen as the OR
  alternative; LGPL is not used. The generated table flags any entry like this in the Cargo.lock
  section below it.
- **Copyleft-only crates:** none. The generator reports BLOCKING for an expression with no
  non-copyleft option; the current run reports zero.
- **Non-standard terms (read the licence text):** `epaint_default_fonts` (the UI fonts, see the
  font section). Its licence is `(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0`, so every part
  applies.
- **Permissive, with conditions:**
  - `Unicode-3.0` (every row of the table with that term: the `icu_*`, `litemap`, `potential_utf`,
    `tinystr`, `writeable`, `yoke`, `zerofrom`, `zerotrie`, `zerovec` crates and their derive crates). It needs its copyright and
    licence notice kept with the distributed source or binary. It does not block shipping.
  - `bzip2-1.0.6` (`libbz2-rs-sys`). The bzip2 BSD-style licence; it
    needs its notice kept with the binary.
  - `Apache-2.0` only crates (13 rows, for example `ab_glyph`, `winit`, `glutin`, `zopfli`): the
    licence text must accompany a binary release.
  - `0BSD OR MIT OR Apache-2.0` (`adler2`): MIT chosen. Keep the MIT notice.
- **zip:** `zip` 9.6.0 is a normal dependency of `mathcraft-import`, so it ships in the binary. The
  `zip` line in `crates/mathcraft-app/Cargo.toml` `[dev-dependencies]` is redundant with it.
- **Sample worksheets (`documentation/examples/*.mcdx`):** the authorship of these files is
  UNKNOWN; the project owner found them and it is not established who made them. The `.mcdx`
  extension belongs to PTC's Mathcad Prime, but that does not say who authored these files, and
  this report does not claim they are PTC's. Their redistribution rights are not confirmed. Before a
  public release, either confirm the redistribution rights in writing, or exclude
  `documentation/examples` from the public release build and replace the tests' inputs with
  worksheets made for this project. No licence in the crate table covers these files.

## Bundled fonts

`epaint_default_fonts` 0.33.3 compiles four fonts into the binary (the `default_fonts` feature
of `eframe`). The file names below are in the crate's `fonts/` directory.

| font file | used for | licence | licence text in the crate |
|---|---|---|---|
| `Ubuntu-Light.ttf` | window chrome (ribbon, menus, dialogs, plot and 3-D tick labels), and the last fallback for other scripts | Ubuntu Font Licence 1.0 | `fonts/UFL.txt` |
| `NotoEmoji-Regular.ttf` | emoji fallback | SIL Open Font License 1.1 | `fonts/OFL.txt` |
| `Hack-Regular.ttf` | monospace | MIT (Hack project; DejaVu parts are public domain) | `fonts/Hack-Regular.txt` |
| `emoji-icon-font.ttf` | icon fallback | MIT, "Copyright (c) 2014 John Slegers" | `fonts/emoji-icon-font-mit-license.txt` |

Math and results use the STIX Two faces below. Text regions and the window chrome use the Noto Sans
faces below (the chrome's proportional family starts with Noto Sans; Ubuntu-Light is its fallback).
The other three are egui's defaults and are compiled in whether or not the window draws them.

## Fonts bundled by mathcraft-app (sheet math and text)

`mathcraft-app` compiles nine font files into the binary with `include_bytes!`
(`crates/mathcraft-app/src/fonts.rs`): five STIX Two files (math, results) and four Noto Sans files
(text regions, window chrome). They are subsets; the files are in
`crates/mathcraft-app/assets/fonts/`, and their licence texts in `assets/fonts/LICENSES/`.

| font file (in `assets/fonts/`) | face | used for | bytes | licence | licence text |
|---|---|---|---|---|---|
| `STIXTwoText-Regular.ttf` | STIX Two Text, roman | upright names, numbers, units, text runs, results | 143,048 | SIL OFL 1.1 | `LICENSES/STIX-Two-OFL-1.1.txt` |
| `STIXTwoText-Italic.ttf` | STIX Two Text, italic | math variables (Latin and Greek), italic text runs | 155,440 | SIL OFL 1.1 | `LICENSES/STIX-Two-OFL-1.1.txt` |
| `STIXTwoText-Bold.ttf` | STIX Two Text, bold | bold text runs | 142,916 | SIL OFL 1.1 | `LICENSES/STIX-Two-OFL-1.1.txt` |
| `STIXTwoText-BoldItalic.ttf` | STIX Two Text, bold italic | bold italic text runs | 155,352 | SIL OFL 1.1 | `LICENSES/STIX-Two-OFL-1.1.txt` |
| `STIXTwoMath-Regular.ttf` | STIX Two Math, roman | operators, relations, arrows, `∞`, `∫`, `Σ`, `Π`; fallback for symbols the text faces lack | 381,976 | SIL OFL 1.1 | `LICENSES/STIX-Two-OFL-1.1.txt` |
| `NotoSans-Regular.ttf` | Noto Sans, roman | text regions, window chrome, plot and 3-D tick labels | 83,796 | SIL OFL 1.1 | `LICENSES/NotoSans-OFL-1.1.txt` |
| `NotoSans-Italic.ttf` | Noto Sans, italic | italic text runs | 88,408 | SIL OFL 1.1 | `LICENSES/NotoSans-OFL-1.1.txt` |
| `NotoSans-Bold.ttf` | Noto Sans, bold | bold text runs | 83,716 | SIL OFL 1.1 | `LICENSES/NotoSans-OFL-1.1.txt` |
| `NotoSans-BoldItalic.ttf` | Noto Sans, bold italic | bold italic text runs | 88,300 | SIL OFL 1.1 | `LICENSES/NotoSans-OFL-1.1.txt` |

Total added to the binary: 1,322,952 bytes (about 1.26 MiB).

- Noto Sans: copyright "Copyright 2022 The Noto Project Authors (https://github.com/notofonts/latin-greek-cyrillic)"
  (the licence text's header). Source: the variable font `NotoSans[wdth,wght].ttf` and
  `NotoSans-Italic[wdth,wght].ttf` from `google/fonts` (`ofl/notosans`), instanced with fonttools
  4.68.0 at wdth 100 and wght 400 (regular, italic) or 700 (bold, bold italic), then subset to Basic
  Latin, Latin-1 and Latin Extended, Greek, general punctuation, currency, letterlike, number forms,
  arrows and mathematical operators, keeping the kern, liga and calt features. Unmodified in name;
  the instancing and subsetting are modifications under OFL 1.1 section 3.

- Versions: STIX Two Text 2.13 (build b171), STIX Two Math 2.12 (build b168a). Copyright line of
  both: "Copyright 2001-2021 The STIX Fonts Project Authors (https://github.com/stipub/stixfonts)".
- Source: the TTF files of the npm packages `@expo-google-fonts/stix-two-text` 0.4.2 and
  `@expo-google-fonts/stix-two-math` 0.4.0, which contain the STIX Two releases unchanged. The
  npm wrappers are MIT; the fonts are OFL 1.1 (their `LICENSE_FONT`).
- Subsetting: `pyftsubset` from fonttools 4.68.0 (Python), keeping the Basic Latin, Latin-1 and
  Latin Extended, Greek and Greek Extended, general punctuation, super- and subscripts, currency,
  letterlike, number forms, arrows, mathematical operators, miscellaneous technical, geometric
  shapes, supplemental arrows, miscellaneous mathematical symbols, supplemental mathematical
  operators, and mathematical alphanumeric ranges (text faces); and only the symbol ranges above
  (math face). All layout features are kept. The subset is a modified version under OFL 1.1 section
  3. Its names stay "STIX Two Text" and "STIX Two Math". The licence reserves the font name
  "TM Math", which no file uses.
- Coverage is checked by `fonts::tests` and by `viewmodel_tests::every_character_drawn_for_the_samples_has_a_glyph_in_its_face`.
- Adding or replacing a font file: put it in `assets/fonts/`, its licence text in
  `assets/fonts/LICENSES/`, a row in this section, and an entry in `FONT_LICENCES` of
  `tools/gen_licenses.py`. The script exits with status 2 when these disagree.

### What an open-source release must ship for the fonts

1. Copy these four files from the crate's `fonts/` directory into the release, under their own
   names, in a `LICENSES/` (or similar) directory next to the binary and the source:
   `UFL.txt` (Ubuntu-Light), `OFL.txt` (NotoEmoji), `Hack-Regular.txt` (Hack and the DejaVu
   notice), `emoji-icon-font-mit-license.txt` (emoji-icon-font). Do not edit them. Also copy
   `crates/mathcraft-app/assets/fonts/LICENSES/STIX-Two-OFL-1.1.txt` and the five STIX Two files
   listed above, which the binary contains in full. Also copy
   `crates/mathcraft-app/assets/fonts/LICENSES/NotoSans-OFL-1.1.txt` and the four Noto Sans files.
2. Each font's copyright line. `UFL.txt` and `OFL.txt` require the copyright notice of the font
   to be kept with the font. The copyright holder strings are in each font's `name` table
   (name ID 0); they were not extracted in this review and must be read from the `.ttf` files
   before the release. The Ubuntu font is expected to carry a Canonical Ltd. notice; confirm it
   from the name table.
3. The Ubuntu and Noto fonts were not modified, so the Ubuntu name restriction and the OFL
   reserved-name rule are met. A modified font must be renamed before it is shipped.
4. This file (`documentation/THIRD_PARTY_LICENSES.md`) goes with the release. Its crate table
   must be regenerated when `Cargo.lock` changes (for example with `cargo about` or
   `cargo tree` plus the registry `Cargo.toml` licence fields).

## Crate table

<!-- BEGIN GENERATED CRATE TABLE -->
Generated by `tools/gen_licenses.py` on 2026-11-18 with `cargo metadata --locked --filter-platform <triple>` for the triples linux-x64 = `x86_64-unknown-linux-gnu`, windows-x64 = `x86_64-pc-windows-msvc`, macos-arm64 = `aarch64-apple-darwin`, macos-x64 = `x86_64-apple-darwin`, linux-arm64 = `aarch64-unknown-linux-gnu`.

Rules: *shipped* = normal dependency closure of `mathcraft-app` (linked into the binary; proc-macros and workspace crates excluded). *build-time only* = proc-macros and build-script dependencies (they run at build time and are not shipped). Dev-dependencies are not followed. *Review needed*: a copyleft term anywhere in the expression (even as an OR alternative, with the chosen alternative stated), a non-standard term, or no licence field.

Per triple (shipped / build-time only): linux-x64 223 / 39; windows-x64 144 / 28; macos-arm64 161 / 26; macos-x64 161 / 26; linux-arm64 223 / 39.
Union over the triples: 262 shipped, 41 build-time only. Flagged for review: 1. Blocking (copyleft only): 0.

### Shipped crates

| crate | version | licence (as declared) | used on | Review needed | notes |
|---|---|---|---|---|---|
| ab_glyph | 0.2.32 | Apache-2.0 | all five | no |  |
| ab_glyph_rasterizer | 0.1.10 | Apache-2.0 | all five | no |  |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | all five | no |  |
| aes | 0.9.4 | MIT OR Apache-2.0 | all five | no |  |
| ahash | 0.8.12 | MIT OR Apache-2.0 | all five | no |  |
| arboard | 3.7.0 | MIT OR Apache-2.0 | all five | no |  |
| as-raw-xcb-connection | 1.0.1 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| ashpd | 0.11.1 | MIT | linux-x64, linux-arm64 | no |  |
| async-broadcast | 0.7.2 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| async-channel | 2.5.0 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| async-executor | 1.14.1 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| async-fs | 2.2.0 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| async-io | 2.6.0 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| async-lock | 3.5.0 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| async-net | 2.0.0 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| async-process | 2.5.0 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| async-signal | 0.2.14 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| async-task | 4.7.1 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| bitflags | 1.3.2 | MIT/Apache-2.0 | macos-arm64, macos-x64 | no |  |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | all five | no |  |
| block-buffer | 0.12.1 | MIT OR Apache-2.0 | all five | no |  |
| block2 | 0.5.1 | MIT | macos-arm64, macos-x64 | no |  |
| block2 | 0.6.2 | MIT | macos-arm64, macos-x64 | no |  |
| blocking | 1.7.0 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| bumpalo | 3.20.4 | MIT OR Apache-2.0 | all five | no |  |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | all five | no |  |
| byteorder-lite | 0.1.0 | Unlicense OR MIT | all five | no |  |
| bzip2 | 0.6.1 | MIT OR Apache-2.0 | all five | no |  |
| calloop | 0.13.0 | MIT | linux-x64, linux-arm64 | no |  |
| calloop | 0.14.5 | MIT | linux-x64, linux-arm64 | no |  |
| calloop-wayland-source | 0.3.0 | MIT | linux-x64, linux-arm64 | no |  |
| calloop-wayland-source | 0.4.1 | MIT | linux-x64, linux-arm64 | no |  |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | all five | no |  |
| cgl | 0.3.2 | MIT / Apache-2.0 | macos-arm64, macos-x64 | no |  |
| cipher | 0.5.2 | MIT OR Apache-2.0 | all five | no |  |
| clipboard-win | 5.4.3 | BSL-1.0 | windows-x64 | no |  |
| cmov | 0.5.4 | Apache-2.0 OR MIT | all five | no |  |
| concurrent-queue | 2.5.0 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| const-oid | 0.10.3 | Apache-2.0 OR MIT | all five | no |  |
| constant_time_eq | 0.6.1 | CC0-1.0 OR MIT-0 OR Apache-2.0 | all five | no |  |
| core-foundation | 0.9.4 | MIT OR Apache-2.0 | macos-arm64, macos-x64 | no |  |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | macos-arm64, macos-x64 | no |  |
| core-graphics | 0.23.2 | MIT OR Apache-2.0 | macos-arm64, macos-x64 | no |  |
| core-graphics-types | 0.1.3 | MIT OR Apache-2.0 | macos-arm64, macos-x64 | no |  |
| cpubits | 0.1.1 | MIT OR Apache-2.0 | all five | no |  |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 | all five | no |  |
| crc32fast | 1.5.5 | MIT OR Apache-2.0 | all five | no |  |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| crypto-common | 0.2.2 | MIT OR Apache-2.0 | all five | no |  |
| ctutils | 0.4.8 | Apache-2.0 OR MIT | all five | no |  |
| cursor-icon | 1.2.0 | MIT OR Apache-2.0 OR Zlib | all five | no |  |
| deflate64 | 0.1.12 | MIT | all five | no |  |
| deranged | 0.5.9 | MIT OR Apache-2.0 | all five | no |  |
| digest | 0.11.3 | MIT OR Apache-2.0 | all five | no |  |
| dispatch | 0.2.0 | MIT | macos-arm64, macos-x64 | no |  |
| dispatch2 | 0.3.1 | Zlib OR Apache-2.0 OR MIT | macos-arm64, macos-x64 | no |  |
| dlib | 0.5.3 | MIT | linux-x64, linux-arm64 | no |  |
| downcast-rs | 1.2.1 | MIT/Apache-2.0 | linux-x64, linux-arm64 | no |  |
| dpi | 0.1.2 | Apache-2.0 AND MIT | all five | no |  |
| ecolor | 0.33.3 | MIT OR Apache-2.0 | all five | no |  |
| eframe | 0.33.3 | MIT OR Apache-2.0 | all five | no |  |
| egui | 0.33.3 | MIT OR Apache-2.0 | all five | no |  |
| egui-winit | 0.33.3 | MIT OR Apache-2.0 | all five | no |  |
| egui_glow | 0.33.3 | MIT OR Apache-2.0 | all five | no |  |
| emath | 0.33.3 | MIT OR Apache-2.0 | all five | no |  |
| endi | 1.1.1 | MIT | linux-x64, linux-arm64 | no |  |
| enumflags2 | 0.7.12 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| epaint | 0.33.3 | MIT OR Apache-2.0 | all five | no |  |
| epaint_default_fonts | 0.33.3 | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 | all five | non-standard term(s) OFL-1.1, Ubuntu-font-1.0: read the licence text |  |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | all five | no |  |
| errno | 0.3.15 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| error-code | 3.4.0 | BSL-1.0 | windows-x64 | no |  |
| event-listener | 5.4.2 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| event-listener-strategy | 0.5.4 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| fastrand | 2.5.0 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| fax | 0.2.7 | MIT | macos-arm64, macos-x64 | no |  |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 | all five | no |  |
| flate2 | 1.1.11 | MIT OR Apache-2.0 | all five | no |  |
| foreign-types | 0.5.0 | MIT/Apache-2.0 | macos-arm64, macos-x64 | no |  |
| foreign-types-shared | 0.3.1 | MIT/Apache-2.0 | macos-arm64, macos-x64 | no |  |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 | all five | no |  |
| futures-channel | 0.3.34 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| futures-io | 0.3.34 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| futures-lite | 2.6.1 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| futures-util | 0.3.32 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| gethostname | 1.1.0 | Apache-2.0 | linux-x64, linux-arm64 | no |  |
| getrandom | 0.3.4 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | all five | no |  |
| glow | 0.16.0 | MIT OR Apache-2.0 OR Zlib | all five | no |  |
| glutin | 0.32.3 | Apache-2.0 | all five | no |  |
| glutin-winit | 0.5.0 | MIT | all five | no |  |
| glutin_egl_sys | 0.7.1 | Apache-2.0 | linux-x64, windows-x64, linux-arm64 | no |  |
| glutin_glx_sys | 0.6.1 | Apache-2.0 | linux-x64, linux-arm64 | no |  |
| glutin_wgl_sys | 0.6.1 | Apache-2.0 | windows-x64 | no |  |
| half | 2.7.1 | MIT OR Apache-2.0 | macos-arm64, macos-x64 | no |  |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | all five | no |  |
| hex | 0.4.3 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| hmac | 0.13.0 | MIT OR Apache-2.0 | all five | no |  |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 | all five | no |  |
| icu_collections | 2.4.0 | Unicode-3.0 | all five | no |  |
| icu_locale_core | 2.4.0 | Unicode-3.0 | all five | no |  |
| icu_normalizer | 2.4.0 | Unicode-3.0 | all five | no |  |
| icu_normalizer_data | 2.4.0 | Unicode-3.0 | all five | no |  |
| icu_properties | 2.4.0 | Unicode-3.0 | all five | no |  |
| icu_properties_data | 2.4.0 | Unicode-3.0 | all five | no |  |
| icu_provider | 2.4.0 | Unicode-3.0 | all five | no |  |
| idna | 1.1.0 | MIT OR Apache-2.0 | all five | no |  |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT | all five | no |  |
| image | 0.25.10 | MIT OR Apache-2.0 | all five | no |  |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | all five | no |  |
| inout | 0.2.6 | MIT OR Apache-2.0 | all five | no |  |
| libbz2-rs-sys | 0.2.6 | bzip2-1.0.6 | all five | no |  |
| libc | 0.2.191 | MIT OR Apache-2.0 | linux-x64, macos-arm64, macos-x64, linux-arm64 | no |  |
| libloading | 0.8.9 | ISC | all five | no |  |
| linux-raw-sys | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| linux-raw-sys | 0.4.15 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| litemap | 0.8.3 | Unicode-3.0 | all five | no |  |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | all five | no |  |
| log | 0.4.35 | MIT OR Apache-2.0 | all five | no |  |
| lzma-rust2 | 0.25.0 | Apache-2.0 | all five | no |  |
| memchr | 2.8.3 | Unlicense OR MIT | all five | no |  |
| memmap2 | 0.9.12 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| memoffset | 0.9.1 | MIT | all five | no |  |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 | all five | no |  |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 | all five | no |  |
| moxcms | 0.8.1 | BSD-3-Clause OR Apache-2.0 | all five | no |  |
| nix | 0.32.0 | MIT | macos-arm64, macos-x64 | no |  |
| nohash-hasher | 0.2.0 | Apache-2.0 OR MIT | all five | no |  |
| num-bigint | 0.4.8 | MIT OR Apache-2.0 | all five | no |  |
| num-bigint | 0.5.1 | MIT OR Apache-2.0 | all five | no |  |
| num-complex | 0.4.6 | MIT OR Apache-2.0 | all five | no |  |
| num-conv | 0.2.2 | MIT OR Apache-2.0 | all five | no |  |
| num-integer | 0.1.47 | MIT OR Apache-2.0 | all five | no |  |
| num-rational | 0.4.2 | MIT OR Apache-2.0 | all five | no |  |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | all five | no |  |
| objc-sys | 0.3.5 | MIT | macos-arm64, macos-x64 | no |  |
| objc2 | 0.5.3 | MIT | macos-arm64, macos-x64 | no |  |
| objc2 | 0.6.5 | MIT | macos-arm64, macos-x64 | no |  |
| objc2-app-kit | 0.2.2 | MIT | macos-arm64, macos-x64 | no |  |
| objc2-app-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | macos-arm64, macos-x64 | no |  |
| objc2-core-foundation | 0.3.2 | Zlib OR Apache-2.0 OR MIT | macos-arm64, macos-x64 | no |  |
| objc2-core-graphics | 0.3.2 | Zlib OR Apache-2.0 OR MIT | macos-arm64, macos-x64 | no |  |
| objc2-encode | 4.1.0 | MIT | macos-arm64, macos-x64 | no |  |
| objc2-foundation | 0.2.2 | MIT | macos-arm64, macos-x64 | no |  |
| objc2-foundation | 0.3.2 | MIT | macos-arm64, macos-x64 | no |  |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | all five | no |  |
| ordered-stream | 0.2.0 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| owned_ttf_parser | 0.25.1 | Apache-2.0 | all five | no |  |
| parking | 2.2.1 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | all five | no |  |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | all five | no |  |
| pbkdf2 | 0.13.0 | MIT OR Apache-2.0 | all five | no |  |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | all five | no |  |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | all five | no |  |
| piper | 0.2.5 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| png | 0.18.1 | MIT OR Apache-2.0 | all five | no |  |
| polling | 3.11.0 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| pollster | 0.4.0 | Apache-2.0/MIT | linux-x64, linux-arm64 | no |  |
| potential_utf | 0.1.6 | Unicode-3.0 | all five | no |  |
| powerfmt | 0.2.1 | MIT OR Apache-2.0 | all five | no |  |
| ppmd-rust | 1.5.2 | CC0-1.0 OR MIT-0 | all five | no |  |
| ppv-lite86 | 0.2.21 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| profiling | 1.0.18 | MIT OR Apache-2.0 | all five | no |  |
| pxfm | 0.1.30 | BSD-3-Clause OR Apache-2.0 | all five | no |  |
| quick-error | 2.0.1 | MIT/Apache-2.0 | macos-arm64, macos-x64 | no |  |
| quote | 1.0.47 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| rand | 0.9.5 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| rand_chacha | 0.9.0 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| rand_core | 0.9.5 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib | all five | no |  |
| rfd | 0.16.1 | MIT | all five | no |  |
| roxmltree | 0.21.2 | MIT OR Apache-2.0 | all five | no |  |
| rustix | 0.38.44 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| rustix | 1.1.6 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| scoped-tls | 1.0.1 | MIT/Apache-2.0 | linux-x64, linux-arm64 | no |  |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | all five | no |  |
| serde | 1.0.229 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| sha1 | 0.11.0 | MIT OR Apache-2.0 | all five | no |  |
| sha2 | 0.11.1 | MIT OR Apache-2.0 | all five | no |  |
| signal-hook-registry | 1.4.8 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| simd-adler32 | 0.3.10 | MIT | all five | no |  |
| slab | 0.4.12 | MIT | linux-x64, linux-arm64 | no |  |
| smallvec | 1.16.5 | MIT OR Apache-2.0 | all five | no |  |
| smithay-client-toolkit | 0.19.2 | MIT | linux-x64, linux-arm64 | no |  |
| smithay-client-toolkit | 0.20.0 | MIT | linux-x64, linux-arm64 | no |  |
| smithay-clipboard | 0.7.3 | MIT | linux-x64, linux-arm64 | no |  |
| smol_str | 0.2.2 | MIT OR Apache-2.0 | all five | no |  |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | all five | no |  |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | all five | no |  |
| syn | 3.0.10 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| thiserror | 1.0.69 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | all five | no |  |
| tiff | 0.11.3 | MIT | macos-arm64, macos-x64 | no |  |
| time | 0.3.55 | MIT OR Apache-2.0 | all five | no |  |
| time-core | 0.1.9 | MIT OR Apache-2.0 | all five | no |  |
| tinystr | 0.8.5 | Unicode-3.0 | all five | no |  |
| tracing | 0.1.44 | MIT | all five | no |  |
| tracing-core | 0.1.36 | MIT | all five | no |  |
| ttf-parser | 0.25.1 | MIT OR Apache-2.0 | all five | no |  |
| typed-path | 0.13.0 | MIT OR Apache-2.0 | all five | no |  |
| typenum | 1.20.1 | MIT OR Apache-2.0 | all five | no |  |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | linux-x64, linux-arm64 | no |  |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 | all five | no |  |
| url | 2.5.8 | MIT OR Apache-2.0 | all five | no |  |
| urlencoding | 2.1.3 | MIT | linux-x64, linux-arm64 | no |  |
| utf8_iter | 1.1.0 | Apache-2.0 OR MIT | all five | no |  |
| utf_types | 1.0.0 | Apache-2.0 OR MIT | all five | no |  |
| uuid | 1.29.3 | Apache-2.0 OR MIT | linux-x64, linux-arm64 | no |  |
| wayland-backend | 0.3.17 | MIT | linux-x64, linux-arm64 | no |  |
| wayland-client | 0.31.15 | MIT | linux-x64, linux-arm64 | no |  |
| wayland-csd-frame | 0.3.0 | MIT | linux-x64, linux-arm64 | no |  |
| wayland-cursor | 0.31.14 | MIT | linux-x64, linux-arm64 | no |  |
| wayland-protocols | 0.32.13 | MIT | linux-x64, linux-arm64 | no |  |
| wayland-protocols-experimental | 20250721.0.1 | MIT | linux-x64, linux-arm64 | no |  |
| wayland-protocols-misc | 0.3.12 | MIT | linux-x64, linux-arm64 | no |  |
| wayland-protocols-plasma | 0.3.12 | MIT | linux-x64, linux-arm64 | no |  |
| wayland-protocols-wlr | 0.3.12 | MIT | linux-x64, linux-arm64 | no |  |
| wayland-sys | 0.31.11 | MIT | linux-x64, linux-arm64 | no |  |
| web-time | 1.1.0 | MIT OR Apache-2.0 | all five | no |  |
| webbrowser | 1.2.7 | MIT OR Apache-2.0 | all five | no |  |
| weezl | 0.1.12 | MIT OR Apache-2.0 | macos-arm64, macos-x64 | no |  |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | windows-x64 | no |  |
| windows-sys | 0.52.0 | MIT OR Apache-2.0 | windows-x64 | no |  |
| windows-sys | 0.60.2 | MIT OR Apache-2.0 | windows-x64 | no |  |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | windows-x64 | no |  |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | windows-x64 | no |  |
| windows-targets | 0.53.5 | MIT OR Apache-2.0 | windows-x64 | no |  |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 | windows-x64 | no |  |
| windows_x86_64_msvc | 0.53.1 | MIT OR Apache-2.0 | windows-x64 | no |  |
| winit | 0.30.13 | Apache-2.0 | all five | no |  |
| winnow | 1.0.5 | MIT | linux-x64, linux-arm64 | no |  |
| writeable | 0.6.4 | Unicode-3.0 | all five | no |  |
| x11-dl | 2.21.0 | MIT | linux-x64, linux-arm64 | no |  |
| x11rb | 0.13.2 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| x11rb-protocol | 0.13.2 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| xcursor | 0.3.11 | MIT | linux-x64, linux-arm64 | no |  |
| xkbcommon-dl | 0.4.2 | MIT | linux-x64, linux-arm64 | no |  |
| xkeysym | 0.2.1 | MIT OR Apache-2.0 OR Zlib | linux-x64, linux-arm64 | no |  |
| yoke | 0.8.4 | Unicode-3.0 | all five | no |  |
| zbus | 5.19.0 | MIT | linux-x64, linux-arm64 | no |  |
| zbus_names | 4.3.5 | MIT | linux-x64, linux-arm64 | no |  |
| zcheapstr | 1.2.0 | MIT | linux-x64, linux-arm64 | no |  |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | all five | no |  |
| zerofrom | 0.1.8 | Unicode-3.0 | all five | no |  |
| zeroize | 1.9.1 | Apache-2.0 OR MIT | all five | no |  |
| zerotrie | 0.2.5 | Unicode-3.0 | all five | no |  |
| zerovec | 0.11.9 | Unicode-3.0 | all five | no |  |
| zip | 9.6.0 | MIT | all five | no |  |
| zlib-rs | 0.6.8 | Zlib | all five | no |  |
| zopfli | 0.8.4 | Apache-2.0 | all five | no |  |
| zstd | 0.14.4 | BSD-3-Clause | all five | no |  |
| zstd-safe | 8.3.0 | BSD-3-Clause | all five | no |  |
| zstd-sys | 2.1.3+zstd.1.5.7 | BSD-3-Clause AND BSD-2-Clause | all five | no |  |
| zune-core | 0.5.3 | MIT OR Apache-2.0 OR Zlib | all five | no |  |
| zune-jpeg | 0.5.15 | MIT OR Apache-2.0 OR Zlib | all five | no |  |
| zvariant | 5.15.1 | MIT | linux-x64, linux-arm64 | no |  |
| zvariant_utils | 4.2.0 | MIT | linux-x64, linux-arm64 | no |  |

### Build-time only (not linked into the binary)

| crate | version | licence (as declared) | used on | Review needed | notes |
|---|---|---|---|---|---|
| async-recursion | 1.2.0 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| async-trait | 0.1.92 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | all five | no |  |
| bytemuck_derive | 1.12.1 | Zlib OR Apache-2.0 OR MIT | all five | no |  |
| cc | 1.11.2 | MIT OR Apache-2.0 | all five | no |  |
| cfg_aliases | 0.2.2 | MIT | all five | no |  |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 | all five | no |  |
| document-features | 0.2.12 | MIT OR Apache-2.0 | all five | no |  |
| enumflags2_derive | 0.7.12 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 | all five | no |  |
| foreign-types-macros | 0.2.4 | MIT/Apache-2.0 | macos-arm64, macos-x64 | no |  |
| futures-macro | 0.3.32 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| gl_generator | 0.14.0 | Apache-2.0 | linux-x64, windows-x64, linux-arm64 | no |  |
| jobserver | 0.1.35 | MIT OR Apache-2.0 | all five | no |  |
| khronos_api | 3.1.0 | Apache-2.0 | linux-x64, windows-x64, linux-arm64 | no |  |
| litrs | 1.0.1 | MIT OR Apache-2.0 | all five | no |  |
| pkg-config | 0.3.34 | MIT OR Apache-2.0 | all five | no |  |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| quick-xml | 0.41.0 | MIT | linux-x64, linux-arm64 | no |  |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | all five | no |  |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| serde_repr | 0.1.21 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| shlex | 2.0.1 | MIT OR Apache-2.0 | all five | no |  |
| syn | 2.0.119 | MIT OR Apache-2.0 | all five | no |  |
| synstructure | 0.14.0 | MIT | all five | no |  |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | all five | no |  |
| toml_datetime | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| toml_edit | 0.25.19+spec-1.1.0 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | linux-x64, linux-arm64 | no |  |
| tracing-attributes | 0.1.31 | MIT | linux-x64, linux-arm64 | no |  |
| version_check | 0.9.5 | MIT/Apache-2.0 | all five | no |  |
| wayland-scanner | 0.31.11 | MIT | linux-x64, linux-arm64 | no |  |
| xml-rs | 0.8.29 | MIT | linux-x64, windows-x64, linux-arm64 | no |  |
| yoke-derive | 0.8.4 | Unicode-3.0 | all five | no |  |
| zbus_macros | 5.19.0 | MIT | linux-x64, linux-arm64 | no |  |
| zerocopy-derive | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | macos-arm64, macos-x64 | no |  |
| zerofrom-derive | 0.1.8 | Unicode-3.0 | all five | no |  |
| zeroize_derive | 1.5.0 | Apache-2.0 OR MIT | all five | no |  |
| zerovec-derive | 0.11.7 | Unicode-3.0 | all five | no |  |
| zvariant_derive | 5.15.1 | MIT | linux-x64, linux-arm64 | no |  |

### In Cargo.lock but not linked for the five triples (flagged entries only)

| crate | version | licence (as declared) | Review needed | notes |
|---|---|---|---|---|
| r-efi | 5.3.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | copyleft alternative present; chosen: MIT | copyleft option(s): LGPL-2.1-or-later |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | copyleft alternative present; chosen: MIT | copyleft option(s): LGPL-2.1-or-later |
<!-- END GENERATED CRATE TABLE -->

## Release checklist for the licences

1. Ship the licence texts of the crates that need them (Apache-2.0, Unicode-3.0, bzip2, MIT
   notices) with the binary and the source, following the rows of the table.
2. Copy the four font licence texts in the next section into a `LICENSES/` directory.
3. Regenerate the table with `tools/gen_licenses.py` whenever `Cargo.lock` changes, then check
   the "Review needed" column and the `BLOCKING` count (must be zero).
4. Resolve the sample-worksheet question above before the public release.
