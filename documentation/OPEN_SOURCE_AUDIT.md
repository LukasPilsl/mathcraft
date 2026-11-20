# Open-source audit of the public tree

Date: 2026-11-26 (branch `chore/open-source-prep`). Nothing was published, no history was touched.
This audit lists what in the tree comes from, or describes, PTC material, what was changed, and what
the shareholder must decide. The method and the decisions are written for the shareholder; the
steps to publish are in `OPEN_SOURCE_PLAN.md`. This is a technical review, not legal advice.

## 1. Gate results

| Check | Result |
|---|---|
| `python3 tools/release_check.py` in the repository | FAIL only for sample material: 26 files under `documentation/examples/` (24 worksheets, `operations/Bild.png`, `operations/Unbenannt.mcdx`) plus `documentation/MISSING_FUNCTIONS.md` (27 FAILs). The ten WARNs are the internal files and the placeholder repository URL. |
| Export (`git archive HEAD`, then delete `documentation/examples`, `MISSING_FUNCTIONS.md`, `screenshots/`, the help tree) with `release_check.py --public-tree` | `RESULT: PASS` |
| `cargo fmt`, `clippy -D warnings`, `cargo test --workspace --no-fail-fast --features mathcraft-app/render` in the export, no samples | see the final report; sample tests skip with a message |

## 2. Trademark use

All mentions name the product only to describe file-format compatibility. None is used as a name of
Mathcraft.

| Place | Text | Verdict |
|---|---|---|
| Window title (`crates/mathcraft-app/src/app.rs`, `window_title`) | `Mathcraft <version>` or file name | Own name. Fine. |
| About/disclaimer (`i18n.rs:560`) | "not affiliated with PTC or Mathcad. Mathcad is a trademark of its owner." | Fine. Suggest naming PTC Inc. to match the README. |
| File dialog filter (`i18n.rs:402`) | "Mathcad Prime worksheets" | Factual. Keep. |
| `packaging/` (Info.plist, desktop file, MIME XML, deb description) | "Mathcad Prime worksheet package", "not made by or affiliated with PTC" | Factual file-type description. Keep. |
| Crate descriptions (`mathcraft-import/Cargo.toml`) | "Reader and writer for Mathcad Prime .mcdx worksheet packages" | Factual. Keep. |
| Ribbon (`ribbon/resources.rs`, `ribbon/io.rs`) | Group captions "PTC Mathcad worksheets", "Hilfe in PTC Mathcad aufrufen", "Verbindung zur PTC Mathcad Community", help tooltip "PTC Mathcad Hilfe" | **Changed** in this branch: now "Worksheets (.mcdx)", "Hilfe in Mathcraft aufrufen", "Mit der Community verbinden", "Mathcraft Hilfe". They presented PTC's product as ours. |
| Ribbon tooltip `tutorial` (German) | Sentence about "PTC Mathcad Benutzer" | **Changed**: own wording. |
| Font names `Mathcad UniMath Prime`, `Mathcad UniMath Print` (`ribbon/text_format.rs`, `math_format.rs`) | Names of the fonts stored in worksheets | Factual (they must be written to files as is). Keep; mention in the trademark notice if desired. |
| Community button URL `https://community.ptc.com/...` (`ribbon/resources.rs:76`) | Link to PTC's forum | Decision: keep as a neutral link, or point to the project's own discussion page after publication. |
| PDF metadata (`export.rs`) | no PTC string found | Fine. |
| Comments citing "Prime", "UNVERIFIED: Prime ..." (e.g. `ribbon/math.rs`, `ribbon/document.rs`) | developer comments and two greyed-line tooltips | Factual. Two UI tooltips begin "UNVERIFIED:"; decide if users should see them. |

## 3. Verbatim overlap with the help pages

Method: 8-word shingles (lower case, words only) of every line in the files below against every
page of the help Markdown tree (474,801 distinct shingles). Hits are listed; the 200-character check
of `release_check.py` found none in code.

| File:line | Overlap | Action |
|---|---|---|
| `documentation/EDITING_PARITY.md` lines 19-26, 30, 33-34, 41, 51-52, 94 (14 lines) | The "Help text" column quotes the help sentences for the key tables (Enter, Home, End, Tab, Shift+Enter ...) | The whole document is a quotation table. Recommendation: remove from the public tree (private archive keeps it). Not rewritten: it would become a different document. |
| `documentation/SYMBOLIC_OPTIONS.md:129` | list of `decomp` option names (`coeff power func max parts expr`) | Option names are facts. No action. |
| `documentation/UI_PARITY_RIBBON.md:151` | list of calculation option names | Facts. No action. |
| `documentation/MCP_AI_INTERFACE.md:129-133` | a matrix `1 2 3 ... 8` | Numbers only. No action. |
| `tools/function_notes.tsv:11,12,227,263,265,269` | math identities ("integral from 0 to x of sin ...") | Standard mathematics, not creative text. No action. |
| `crates/mathcraft-app/src/ribbon/functions.rs:11,12` | `j0 j1 jn y0 y1 yn` function lists | Names only. No action. |
| `documentation/MISSING_FUNCTIONS.md` (6 hits) | help page names | The file is excluded from the public tree anyway. |
| `crates/mathcraft-mcp/src/{keywords,resources,prompts}.rs`, `i18n.rs` | no hits | Clean. |

Not detectable by this scan: UI text transcribed from the screenshots (see section 5). The German
ribbon captions and tooltips in `crates/mathcraft-app/src/ribbon/*.rs` and the strings in `i18n.rs`
follow Prime's German interface; a short label such as a button name is not copyrightable in
practice, but the full-sentence tooltips are the risk. One such sentence was rewritten (section 2).
The shareholder should have a human compare the remaining German tooltips with the screenshots.

## 4. Images, icons, fonts

- `crates/mathcraft-app/assets/icons/*`: PNG 16-1024 and `mathcraft.ico`, generated by
  `cargo run -p mathcraft-app --example make_icons` from `src/icon.rs` (a blue tile, a white "M", an
  orange baseline). Original, drawn in code.
- Ribbon icons: drawn with the egui painter from stroke lists in `ribbon_icons.rs`; no image files.
- Fonts (`assets/fonts`): Noto Sans, STIX Two (OFL). Licences are in `assets/fonts/LICENSES/`. The
  checklist item about extracting the fonts' copyright strings into `THIRD_PARTY_LICENSES.md` is
  still open.
- No `include_bytes!` or `include_str!` reaches outside `assets/fonts`; no PTC image is in the tree
  (the checker fails on any image outside the allow-list).
- `documentation/examples/operations/Bild.png` is sample material and is excluded.

## 5. Sample-derived expected values in tests (for the shareholder)

These tests contain numbers, names and structures that came from the stored results and contents of
the 24 sample worksheets (authorship unknown; they are PTC-format files of a third party). They all
need the samples to run and skip without them (`require_samples!()`), so the public tree runs none
of them. The values stay in the public source, so the question is whether pinned numbers and the
sample file names are acceptable.

| Where | What is pinned |
|---|---|
| `crates/mathcraft-cli/tests/common/baseline.rs` (354 lines) and `golden_samples.rs` | Per sample file: counts of Pass, Mismatch, EngineError, EngineErrorNoRef, NoReference, plot_ok and the unsupported lists. Contains the 24 sample file names (for example `17_rata_golfball_donau_lsg.mcdx`, `Mechanik_2_Wellendurchbiegung.mcdx`). Totals via `golden`: Pass 103, Mismatch 2, Unsupported 5, EngineError 0, EngineErrorNoRef 12, NoReference 267. |
| `crates/mathcraft-import/tests/`: `open_samples`, `read_results`, `worksheet`, `convert_math`, `furniture`, `operations_sample`, `plot_samples`, `plot_traces_samples`, `region_result_format`, `result_format_settings`, `roundtrip_samples`, `evaluate_samples`, `symbolic_samples`, `writer_samples` | Region counts, expected strings, stored results (SI values), plot parameters, formats, taken from named samples. |
| `crates/mathcraft-import/src/compare.rs`, `regions/tests.rs`, `regions/plot3d.rs`, `writer/plot.rs` | Comments and tests that name samples. |
| `crates/mathcraft-core/src/{solve,symbolic,eval}/...` tests and `crates/mathcraft-app/src/*_tests.rs` | Some tests and comments name a sample worksheet (grep for `rata_` or `Mechanik_`). Several have the inputs rewritten as own strings; those that read the files skip. |
| `crates/mathcraft-app/examples/screenshot.rs`, `src/dialogs.rs`, `src/app.rs` | Default paths or hints under `documentation/examples`. The default in `mathcraft-cli/src/args.rs` is exempt in the checker (runtime default only). |

Flag for the shareholder: the pinned numbers are results of computations on someone's engineering
worksheets (a triangle, a drive shaft, a golf ball). They are small facts, not the worksheets. The
safe route, already planned in the checklist, is to replace the sample-based tests step by step with
own worksheets. Decision needed: publish with the names and counts as they are (low risk, my view)
or strip the file names first.

## 6. Internal and process files

| File | Verdict | Reason |
|---|---|---|
| `CLAUDE.md` | Remove from the public tree (keep in the private archive) | Internal agent process ("company"), no use to contributors; names the PTC reference locations. |
| `ROADMAP.md` | Rewrite or remove | Internal state log with agent and shareholder talk, sample names, internal paths. A short public roadmap can be derived from its goals. Remove for the first release. |
| `documentation/TESTER_PROMPT.md` | Remove | Prompt for the QA agent; describes the PTC UI and screenshots. |
| `documentation/QA_FINDINGS_1.md` | Remove (or fold into issues) | Internal QA round; mentions the PTC UI behaviour; no value without context. |
| `documentation/EDITING_PARITY.md` | Remove | Quotes the help text verbatim (section 3). |
| `documentation/UI_PARITY_CANVAS.md`, `UI_PARITY_CHROME.md`, `UI_PARITY_PLOTS.md` | Remove | Describe Prime's UI by screenshot number (18-22 screenshot references each). Reference material, not Mathcraft documentation. |
| `documentation/UI_PARITY_RIBBON.md` | Keep, but rewrite first | Two tests in `crates/mathcraft-app/src/ribbon.rs` (`every_menu_line_of_the_three_tabs_is_wired_or_explained`, `unavailable_buttons_are_listed_in_the_parity_document`) read it. It is also the developer guide for the ribbon. If removed, those tests must skip or move the list into code. Screenshot numbers should be dropped from it. |
| `documentation/MCDX_FORMAT.md` | Keep | Notes on the file format derived by reading the samples; useful for contributors. It describes a format; it contains no sample content. The shareholder should read it once for copied text. |
| `documentation/REBUILD_GUIDE.md` | Remove or rewrite | Internal phase plan for agents; cites help pages and screenshots (12 references). |
| `documentation/SYMBOLIC_OPTIONS.md` | Keep | Own design notes; "per CLAUDE.md worker rules" at line 228 must be reworded if CLAUDE.md goes. |
| `documentation/UI_DEFECTS.md` | Keep (rewrite refs) | Own list of defects; cites screenshot numbers 8 times. |
| `documentation/UNVERIFIED_FORMATS.md` | Keep | Public honesty about guessed formats. Mentions "screenshots 0230-0232"; reword. |
| `documentation/MCP_AI_INTERFACE.md` | Keep | Tool design; line 248 mentions CLAUDE.md branching; reword. |
| `documentation/MCP_SECURITY_AUDIT.md`, `MCP_USER_GUIDE.md`, `THIRD_PARTY_LICENSES.md`, `RELEASE_CHECKLIST.md` | Keep | Public or maintainer documents. |
| `documentation/MISSING_FUNCTIONS.md` | Excluded already | Page names per function from the PTC help index. |
| `documentation/mirror.py` | Untracked, never commit | Help mirror script. |
| `.gitignore` line 13 cites `REBUILD_GUIDE.md §5` | Reword if that file goes | |

The release check marks all of the files above that exist as WARN (internal) rather than FAIL, so
the export passes with them present. The recommended export deletes the "remove" rows.

## 7. History

The history holds the sample worksheets and `operations/Bild.png` in past commits (206 commits). It
must not be published. See `OPEN_SOURCE_PLAN.md`: a fresh repository from the export avoids the
problem without a rewrite.

## 8. Placeholders still open

Repository URL (`Cargo.toml`, `OWNER`), security contact (`SECURITY.md`, `CODE_OF_CONDUCT.md`: both
say "the address given on the project's page"), bundle identifier `org.mathcraft.app` and UTI
`org.mathcraft.mcdx` (`packaging/macos/Info.plist.in`), Inno Setup `AppId` GUID
(`packaging/windows/mathcraft.iss`, line 13).
