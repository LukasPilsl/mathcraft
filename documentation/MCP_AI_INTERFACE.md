# Mathcraft AI Interface (MCP)

Goal: any MCP client (Claude, Claude Code, other agents) can **read, write, format, calculate and draw** in Mathcraft worksheets, and **look at the result** (rendered page images), with the same engine and renderer the GUI uses. No second implementation, no PTC data involved.

## 1. Principles

1. **One core, three front ends.** The GUI, the CLI and the MCP server all call the same document model, evaluator and renderer. The MCP server never re-implements maths or layout.
2. **The AI sees what a human sees.** Every edit can be followed by a render (PNG of a page or region) plus a structured read-back (values, errors, units). Closing this loop is the point of the feature.
3. **Structured first, pixels second.** Tools return JSON (region ids, text, results, errors) and, where useful, an image. The AI should rarely need to guess from pixels.
4. **Refuse rather than wrong.** Existing safety limits (work budget, 60 s deadline, region caps, image limits, file layer off by default) apply unchanged and surface as tool errors with a reason.
5. **Deterministic and stateless-friendly.** Every tool is addressed by `document_id` and stable `region_id`. Same input, same output; no hidden cursor.
6. **Human stays in control.** In live mode the user watches the AI edit, can undo it, and can pause the connection.

## 2. Architecture

```
MCP client (Claude, ...)
      | stdio (default) or Streamable HTTP on 127.0.0.1
      v
crates/mathcraft-mcp  (new)  -- tool/resource/prompt definitions, JSON schemas, session registry
      |
      v
mathcraft-core (engine)  mathcraft-import (mcdx read/write)  mathcraft-render (new, split out of app)
```

- **`mathcraft-mcp`** (new crate, binary `mathcraft-mcp`, also `mathcraft --mcp`). Candidate SDK: the official Rust MCP SDK (`rmcp`, Apache-2.0); verify licence and maturity before adopting, fall back to a small hand-written JSON-RPC layer if not suitable.
- **`mathcraft-render`** (new, extracted from `mathcraft-app`): offscreen egui/wgpu or software rasteriser that turns a document page into RGBA/PNG. Today this exists only as the `screenshot` example; it must become a library with no window. Also usable by the CLI (`mathcraft render`).
- **Document model API** (in core/import, or a thin `mathcraft-doc` crate): the edit operations the GUI performs (insert region, edit text, move, delete, format, undo) exposed as plain functions on a `Document`, not buried in egui code. Region ids must be stable across edits. The GUI's `edit/session.rs` is refactored to call this API; MCP calls the same functions.

### Two modes

| Mode | How | Use |
|------|-----|-----|
| **Headless** (default) | `mathcraft-mcp` owns documents in memory, loads/saves `.mcdx`, renders offscreen. | Batch work, CI, servers, agents without a display. |
| **Live** | The running app hosts the MCP endpoint (Unix socket / named pipe / `127.0.0.1` port with token). Tools act on the open window's documents. | User and AI collaborate; the user sees edits appear and can undo them. |

**Routing (decided by the Shareholder):** live is the primary experience, headless always exists.
- Mathcraft running and the request names a file (or "the open file / this sheet") that is open in the app -> the tools act on that **live document**; the user sees the edits and can undo them.
- Request has no open file ("calculate X from these instructions") or no app is running -> **headless**: build in memory and return a `.mcdx` (path, plus the rendered pages if asked).
- The client can force either with `target: "live" | "headless"` on `document_open`/`document_new`. `document_list` shows both kinds with a `live` flag.
- The standalone `mathcraft-mcp` binary auto-detects a running app (local socket) at start and per call; it never starts the GUI on its own.

*Implemented (package 10, `crates/mathcraft-mcp/src/live/`, `crates/mathcraft-app/src/{live.rs,app/live_host.rs}`):* the app hosts a loopback TCP endpoint with a per-session token in a 0600 discovery file (chosen over Unix socket/named pipe: one mechanism, no extra crate on all three OSes). Deviation from the plan: tool handlers take a closure over `&mut Doc`, which cannot cross a process boundary, so the tool table runs inside the app on a `LiveBackend` (documents lent from the tabs for one call, on the UI thread) and the standalone binary is a router that forwards whole JSON-RPC calls; the binary and its tests moved into the app crate to avoid a crate cycle (render is injected via `RenderBackend`). Routing as above; ids `live<N>` / `doc<N>`; one undo step `AI: <tool>` per call; `user_busy` for a tab with an open draft; `paused`, `rate_limited` (20 mutating calls/s), `live_unavailable`; status bar indicator with pause, region flash 1.5 s, Settings switch and `--no-ai`/`--ai`; `app_status`, `app_focus_document`, `app_select_region`, `app_screenshot` (the page via the renderer, not an OS window capture). Build the binary with `cargo build --release -p mathcraft-app --bin mathcraft-mcp --features render` (render tools need `render`; `cargo run -p mathcraft-app --bin mathcraft-mcp --features render`). Hardening: total 5 s auth deadline, 30 s per request line, 16 pre-auth sockets, 8 authenticated, 20/s heavy and 100/s read-only calls, one job per UI frame, queued jobs answer `paused` when paused, stale `live.json` (dead port) means no app, 0700 discovery folder, OS CSPRNG token. Renders (`render_page`, `render_region`, `render_plot_data`, `app_screenshot`) run on their own thread and are answered on a later UI frame, so the window does not wait for them; the GUI is built with `--features render` in every package. Not yet: audit log; calculations still run on the UI thread (a long calculation blocks the window up to its deadline).

Both modes expose the **same tools**. Live mode only adds `app_*` tools (open tab, focus region, screenshot of the actual window) and an "AI is editing" indicator in the status bar with a pause button.

## 3. Tool catalogue

Naming: `verb_noun`. All take `document_id` unless creating/opening. All edits return the changed regions plus any new errors, so the AI gets immediate feedback.

### Documents
- `document_new {page_setup?}` -> `document_id`
- `document_open {path}` / `document_save {document_id, path?}` / `document_close` / `document_list`
- `document_info` -> page count, page setup (size, margins, grid), unit system, calculation mode, region count, dirty flag
- `document_undo` / `document_redo` (shared with the GUI stack in live mode). Each reports the label of the undone step (`step`, undo only) and the ids of the regions it added, removed or changed (`changed_regions`, `changed_count`)

### Reading
- `read_outline {document_id, page?}` -> ordered regions with `region_id`, kind, position, size, one-line summary
- `read_region {document_id, region_id}` -> full content: math as Mathcraft text notation *and* a structured tree, text with runs and styles, plot with series/axes/options, result (value, unit, display format, error)
- `read_worksheet_text {document_id}` -> the whole sheet as a compact plain-text transcript (top-to-bottom, left-to-right, with results), cheapest way for an AI to understand a sheet
- `read_variables {document_id}` -> defined names, kinds, units, defining region
- `read_errors {document_id}` -> all current errors/warnings with region and message

### Writing
- `insert_math {document_id, text|tree, x?, y?, after_region?}` -> region id and result. Input is Mathcraft's linear text syntax (the same the keyboard produces), e.g. `v:=len/dur`, `M:=[1, 2; 3, 4]` (commas between items; `A` is ampere, so do not name a matrix `A`). Position defaults to "below the last region, aligned to the grid".
- `insert_text {…, runs|markdown}` text region with basic styling
- `insert_plot {kind: xy|polar|contour|surface|…, series[], axes, options}` -> region id
- *Implemented (layout, `crates/mathcraft-mcp/src/tools/areas.rs`):* `insert_area {x?, y?, after_region?, width?, height?, locked?, collapsed?, hide_border?}` (an empty area, 360 x 120 pt by default), `insert_page_break {x?, y?, after_region?}`, `insert_image {path | data_base64, x?, y?, after_region?, width?, height?}`. The image is a PNG or JPEG: `path` inside a root (`.png`, `.jpg`, `.jpeg`, same root rules as documents), or `data_base64` (strict base64, at most 1.4 MB of text so a live request stays under 2 MiB). Limits: 4 MB file, 16 megapixels per image, 128 megapixels per document; the header gives the size, then one full decode proves it is an image. A missing width or height keeps the proportions; without either the size comes from the pixels at 96 dpi, at most 400 pt wide, as the GUI does. All three are `batch` ops and one undo step each. Hyperlinks: `link` on a run of `insert_text` (`http://`, `https://` or `mailto:`, no spaces); read back by `read_region`. Saving a sheet with an area, a new page break, an image or a hyperlink writes a file form that is a guess (`UNVERIFIED_FORMATS.md`), so `document_save` returns an `unverified file format written` warning for each.
- *Not offered (decided in batch 2):* `insert_table` (the only table in the model is a `<spec-table>` presentation of define regions, not a grid of literal values, and no sample shows a data table to copy, so creation would be a guess); `set_calculation_mode` and `calculate_now` (AUTO and manual are the app window's session setting, not stored in the document, so headless there is nothing to set; `calculate` already runs the sheet); `insert_hyperlink` and `read_page_setup` (a hyperlink is a run field, see above; `document_info` already gives the page setup, header and footer). Undo and redo are the `document_undo` / `document_redo` tools that exist, not new tools.
- *Implemented (blocks, `crates/mathcraft-mcp/src/tools/blocks.rs`):* `insert_symbolic {expression, keyword?}` (the `expr -> keyword` evaluation), `insert_solve_block {given[], find: {kind: Find|Minerr|Minimize|Maximize|Odesolve|Pdesolve, vars[], guesses?, target?}}` (guesses, constraints and the solver call as one region, one undo step) and `insert_program {name, params[], body}` (a function defined by a multi-line program). All three are `batch` ops, atomic, labelled `AI: <tool>`, and answer with the results of every region they made. `describe_function` and `list_functions` answer for keywords too, and `mathcraft://reference/keywords` lists them with a tested example each.
- `edit_region {region_id, text|tree}`, `move_region`, `resize_region`, `delete_region`
- `batch {ops[]}` -> apply several edits atomically, one render, one result. Strongly preferred for building a sheet in one call.
- *Implemented (search, `crates/mathcraft-mcp/src/tools/search.rs`, core in `crates/mathcraft-doc/src/search.rs`):* `search_document {document_id, query, match_case?, whole_word?}` -> `count` and the matches (region id, kind math/text/plot_title, character offsets, matched text), changes nothing; `replace_in_document {document_id, query, replacement, match_case?, whole_word?}` -> replaces every match, math only where it still reads (others listed in `refused`), one undo step "AI: replace_in_document".

### Calculating
- `calculate {document_id, region_ids?}` -> results and errors (worksheet or subset); honours AUTO/manual mode and all safety limits
- `evaluate_expression {expression, document_id?}` -> quick scratch evaluation, optionally in the scope of a document, without adding a region
- `set_calculation_options {mode, precision?, tolerance?, units?, …}`
- *Implemented (package 5, `crates/mathcraft-mcp/src/tools/{write,calculate}.rs`):* insert_math, insert_text (text or runs), edit_region, move_region, resize_region, delete_region, batch (atomic, max 200 ops, one undo step "AI: batch"), calculate, evaluate_expression, set_calculation_options (array_origin, unit_system, tolerances, prn_precision; no AUTO/manual mode yet).
- `convert_units {value, from, to}`, `symbolic {operation: simplify|solve|diff|integrate|series|…, expression}` as thin wrappers around the engine

### Formatting
- `format_result {region_id, format}` (precision, notation, units shown, matrix display)
- `format_text {region_id, range?, style}` (font, size, bold, colour, paragraph style)
- `set_page_setup {size, orientation, margins}`, `set_header_footer {…}`
- `set_plot_options {region_id, …}` (axes, traces, labels, legend, markers)
- *Implemented (package 7, `crates/mathcraft-mcp/src/tools/{plots,format}.rs`):* insert_plot (kind xy or polar; series as `expr` or `x_expr`+`y_expr`, max 32, 4 KiB each; limits, title, grid, log axes, size, trace styles), set_plot_options (series, limits, title, grid, log, polar, trace styles, markers; one undo step), format_result (precision, notation, trailing zeros, thresholds, radix, digit grouping, j, matrix window, reset; shown back by read_region `display`; not in undo history), format_text (bold, italic, underline, size, colour, paragraph alignment on a range; one undo step), set_page_setup (paper, orientation, margin preset, grid size; not undoable), set_header_footer (plain text; not undoable). Not yet, for the Doc API: trace names, legend, axis labels, marker values, units shown, font family and hyperlinks in format_text, custom margin values (only presets), 3D plots, undo for format_result, page and header/footer.

### Seeing (the key loop)
- `render_page {document_id, page, dpi?|width?, show_grid?}` -> PNG image content block
- `render_region {document_id, region_id, scale?}` -> PNG of one region (a plot, a matrix result) with padding
- `render_plot_data {region_id}` -> the sampled series as numbers, so the AI can verify a plot without reading pixels
- Images are returned as MCP `image` content (base64 PNG) with a short alt-text summary; size is capped (default longest side 1600 px) to keep token cost bounded.

### Discovery
- `list_functions {query?, category?}` and `describe_function {name}` -> signature, description, units behaviour, example (from Mathcraft's own function table, never the PTC help text)
- `list_units {query?}`, `list_constants`, `list_plot_types`
- `capabilities` -> engine version, enabled features, limits, which tools are mutating

### Scenarios (package 12, agent evaluation)

`crates/mathcraft-app/tests/mcp_scenarios.rs`: ten scripted sessions, no LLM. Each is the call sequence a competent client makes after reading `MCP_USER_GUIDE.md` and `mathcraft://reference/syntax`, with assertions on results, numbers, undo steps, PNGs (decoded, not blank, within `max_side`) and `.mcdx` round trips (saved, reopened by the engine and by a fresh server, same transcript). Renders are skipped with a note when there is no GPU.

| # | Scenario | Checks |
|---|---|---|
| 1 | Projectile range from a problem statement | One `batch` (text, inputs with units, formulas, results); `range = 40.775 m`, `h_max = 10.194 m`; plot of height against distance matches the formula point by point; one undo removes the batch, one redo restores it |
| 2 | Find and fix a unit error | `read_errors` names both lines and the reason; `edit_region` fixes the definition, then the result line (`39.24 N`); each edit is one undo step |
| 3 | Two plots and a polar plot | `render_plot_data` numbers: sin, cos, `x/2`; polar points satisfy `r = 1 + cos t`; marker and series changes undo one at a time; `resize_region` shows in the outline |
| 4 | Format a report | Result precision (`read_region` display), heading bold/centred/16 pt, A4 landscape narrow, header and footer; `move_region` above a definition reports `undefined name` and a second move repairs it; formats survive save and reopen |
| 5 | Live: the open sheet | Attach by path (no second tab); batch and single edit are `AI: batch` and `AI: insert_math`; undo removes one AI edit at a time; the window's region count, flash and focus follow; save writes the file the user has open |
| 6 | Headless: calculate from instructions | `--no-live`; discovery (`list_units`, `list_constants`, `evaluate_expression`); `tension = 39.24 N`; save, close, reopen in a new process |
| 7 | Matrices and symbolic commands | `[1, 2; 3, 4]`, transpose, element, product, `sum`, `integral`, `expand`, `factor`; `set_calculation_options` ORIGIN 1; `find` outside a solve block is an error result |
| 8 | Error handling | Unknown document, unknown region, missing argument, bad op (lists the ops), all-or-nothing batch, 201 ops, text over 64 KB, `exists` names `overwrite: true`, `path_denied`, `unsaved_changes` names `discard`, page 99999, unknown tool |
| 9 | Solve: solve block and symbolic | `describe_function solve` is a keyword, a typo gets a close match, `using` says keyword; `insert_solve_block` Find with guesses in m gives `[[2 m], [1 m]]`, target `[p, q]` used by later lines, Minimize; `insert_symbolic` `solve, x`, a two-unknown system, `expand`, `series`; refusals (bad keyword, a constraint that is not an equation, a failed block leaves nothing); save, reopen, same transcript |
| 11 | Search and replace | `search_document` finds `LEN` ignoring case (math and text), `whole_word` narrows `v` from 4 to 2; `replace_in_document` refuses the two formulas that would not read (listed in `refused`) and replaces the text; one `document_undo` restores; a whole-word replace of `dur` in all three places leaves the sheet calculating |
| 10 | Programs | `insert_program` factorial (`for`), `if` / `else if` / `otherwise`, `try` / `on error`, each called with `insert_math`; line-numbered refusals; `document_save` refuses a sheet with a program and writes no file; after deleting them a piecewise `if(...)` function saves and reopens |
| 12 | Layout, links, undo and warnings | One batch: a link in a text run, a locked area, a page break and an image by path; the link is read back. A data_base64 image is inserted, undone and redone (`document_undo` names `AI: insert_image` and the changed region); 18 bad arguments and a 16 MP image are refused with codes and change nothing; a batch with a refused op applies none; `document_save` warns for area, page break, image and hyperlink; save, reopen, same transcript |

Run: `cargo test -p mathcraft-app --features render --test mcp_scenarios -- --nocapture`. The last test in the file runs all ten in parallel, asserts that every tool in `tools/list` was called by a scenario (or is allow-listed with a reason; the allow-list is empty), and writes the table below to `$CARGO_TARGET_TMPDIR/mcp-tool-coverage.md`. Last run (batch 2, with scenario 12): 12 of 12 scenarios pass, 49 of 49 tools exercised.

### Parity table (tool -> GUI feature -> scenario)

Numbers are the scenarios above. A tool counts as covered when a scenario calls it; a call that is refused on purpose (scenario 8) counts too.

| Tool | GUI feature | Scenarios |
|---|---|---|
| `capabilities` | Help / About (engine, limits, roots) | 6 |
| `document_new` | File > New | 1, 2, 3, 4, 6, 7, 8 |
| `document_open` | File > Open (attaches to an open tab in live mode) | 1, 2, 3, 4, 5, 6, 7, 8 |
| `document_save` | File > Save / Save As (with overwrite and .bak) | 1, 2, 3, 4, 5, 6, 7, 8 |
| `document_close` | File > Close (unsaved changes ask first) | 6, 8 |
| `document_list` | Window list of open tabs | 5, 6 |
| `document_info` | Page Setup dialog, document properties | 1, 2, 3, 4, 5, 6, 7, 8 |
| `document_undo` | Edit > Undo | 1, 2, 3, 5, 8, 12 |
| `document_redo` | Edit > Redo | 1, 2, 5, 12 |
| `read_outline` | Region list of a page | 1, 2, 3, 4, 6, 7 |
| `read_region` | Region properties (selection) | 1, 2, 4, 6, 7, 8 |
| `read_worksheet_text` | Whole-sheet text view (no GUI view, a transcript) | 1, 2, 3, 4, 6, 7 |
| `read_variables` | Variables list | 1, 2 |
| `read_errors` | Error markers in the sheet | 2 |
| `list_functions` | Insert > Function browser | 6 |
| `describe_function` | Function help | 6 |
| `list_units` | Units browser | 6 |
| `list_constants` | Constants list | 6 |
| `render_page` | Print preview / page view (picture) | 1, 2, 3, 4, 6, 7, 8 |
| `render_region` | Copy region as picture | 1, 2, 3, 4, 6, 7, 8 |
| `render_plot_data` | Plot trace values (no GUI equivalent; the plot is drawn) | 1, 3 |
| `insert_math` | Math region (click and type) | 5, 7, 8 |
| `insert_text` | Text region (click and type) | 3, 8, 12 |
| `edit_region` | Edit a region in place | 2, 8 |
| `move_region` | Drag a region | 4, 8 |
| `resize_region` | Drag a region's handle | 3 |
| `delete_region` | Delete region | 7 |
| `batch` | Several edits in one step (no GUI equivalent; one undo) | 1, 2, 4, 5, 6, 7, 8 |
| `search_document` | Find: Suchen field and arrows in the status bar | 11 |
| `replace_in_document` | Replace: Ersetzen durch field and Replace all | 11 |
| `calculate` | Calculate worksheet (F9) | 1, 2, 3, 4, 6, 7 |
| `evaluate_expression` | Scratch calculation (no GUI equivalent) | 6 |
| `set_calculation_options` | Calculation options (ORIGIN, units, precision) | 7 |
| `format_result` | Result format (number format dialog) | 4, 7 |
| `format_text` | Text formatting (bold, size, alignment) | 4 |
| `set_page_setup` | Page Setup dialog (paper, orientation, margins) | 4 |
| `set_header_footer` | Header and footer dialog | 4 |
| `insert_plot` | Insert > Plot (x-y or polar) | 1, 3 |
| `set_plot_options` | Plot properties dialog (traces, limits, markers) | 3 |
| `insert_symbolic` | Symbolic palette: the `->` evaluation with a keyword | 9 |
| `insert_solve_block` | Solve block (Given / Find / Minerr / Minimize / Maximize / odesolve / pdesolve) | 9 |
| `insert_program` | Programming palette (multi-line program; no GUI button yet) | 10 |
| `insert_area` | Insert > Area (empty collapsible group, lock, hide border) | 12 |
| `insert_page_break` | Insert > Page break | 12 |
| `insert_image` | Insert > Picture (PNG or JPEG, size from the pixels) | 12 |
| `app_status` | Status bar (AI indicator) | 5 |
| `app_focus_document` | Switch tab | 5 |
| `app_select_region` | Click a region | 5 |
| `app_screenshot` | Screenshot of the window | 5 |

### GUI features without a tool equivalent (backlog)

- **Regions:** spec tables (`<spec-table>`, not creatable, see section 3 above), putting regions into an area and out again (areas are created empty), image crop, controls, grouping, z-order (bring to front).
- **Text:** font family, hyperlinks on existing text (only `insert_text` runs set a `link`), paragraph styles beyond alignment, spell-check.
- **Plots:** trace names, legend, axis labels, marker values (a marker can be added but not placed), units shown on axes, 3D plots (contour, surface).
- **Formats:** undo for `format_result`, custom margin values (presets only), reading back header and footer, the worksheet-wide format as a whole (`set_calculation_options` has no display format).
- **Calculation:** AUTO and manual mode, calculate in a chosen order.
- **Undo:** the undo history as a list of labels (only the label of the step being undone is reported).
- **Editing:** cut, copy and paste, zoom and view state, document properties beyond the title.

- **Solve blocks and programs:** done as tools (`insert_solve_block`, `insert_program`, `insert_symbolic`), but the GUI has no button for either yet, and a sheet that holds a program cannot be saved to `.mcdx` (the file form of programs is unknown, see `UNVERIFIED_FORMATS.md`): `insert_program` says so in its answer and `document_save` refuses with the region id.

### Evaluation findings (what an LLM trips on)

Found by running the scenarios. Each is a fix to make or a decision to take; fixed ones are marked.

1. **Saving fails for some names.** `range_m` or `dist_m` (a single-letter subscript after an underscore) calculate fine but `document_save` fails with `document_error: ... subscript m cannot be written; the reader reads only digits or a name`. The guide recommended these names. Scenarios use plain names. Fix in the writer or refuse the name when it is written. Engine owner, not MCP.
2. **Fixed: `batch` holds plots.** `insert_plot` and `set_plot_options` are batch ops (one undo step `AI: batch`, atomic rollback).
3. **Fixed: plot x is unitless.** The plot reference has a units section and `insert_plot` takes `x_unit: "m"`, which writes every `x` as `(x*m)` (axis numbers stay plain; parametric traces get it on both expressions). `x*m` in the expression still works.
4. **Fixed: `format_result` shows in the transcript.** A region with its own format reports its value in that format in `calculate` (`value`), `read_region` (`result.value`, `display.result_text`), `read_worksheet_text` and the `format_result` reply. Regions that follow the worksheet keep the worksheet format.
5. **Moving a result above its definition breaks it.** Mathcraft calculates top to bottom, left to right. The `move_region` description already says that position matters, and the reply lists the new `undefined name` in `new_errors`. No change needed; a client has to read both.
6. **`A` is ampere.** `A:=[1, 2; 3, 4]` then `A =` shows `1 A`. The guide's trap list (section 9) lacked it; added. Also `pi` is undefined (`π` works): the error says `undefined name \`pi\`` with no hint. The syntax reference now says so.
7. **`->` without the closing `=`** is read as a symbolic command: `unsupported operation: symbolic command \`m\` with these words`. Not actionable. The syntax reference now states the `=`.
8. **Matrix literal with spaces** (`[1 2;3 4]`, as this document had it) fails with `expected ']', found number '2'`. Fixed in this document (commas).
9. **Values stay in SI** unless the line asks for a unit: `10 kN` reads back as `1×10^4 N`. Correct per the guide; an LLM needs `-> kN =` to answer in kN.
10. **`read_variables` drops the subscript:** `g_acc` is listed as `g`, `range_m` as `range`. The engine's variable name is the base name. Engine owner.
11. **Fixed: plot summaries name the traces.** `xy plot: y1 = sin(x); y2 = cos(x); y3 = x/2 over x` (parametric: `(x, y) = (cos(t), sin(t))`), in `read_outline` and the transcript.
12. **Fixed: header and footer are read back** by `document_info` (`header`, `footer`). No new tool.
13. **Fixed: every headless write step is labelled `AI: <tool>`** (`AI: insert_math`, `AI: edit_region`, `AI: format_text`, ...), like batch and plot calls and live mode.
14. **Fixed: a refused path leads with the hint** (`use a .mcdx path inside a folder given with --root (capabilities lists the folders); reason: ...`), for `..` too.
15. **Fixed: `evaluate_expression` has no `region_id`.** It returns `scratch: true`.
16. **Fixed: `solve` is a keyword, not a function.** `describe_function solve` used to answer `not_found`. It now describes the keyword (syntax, a tested example, common mistakes); `list_functions` lists keywords (marked `keyword: true`); a name that is nothing gets close matches (`did you mean simplify?`); a palette word the engine lacks (`using`) says it is a keyword, not a function. Solve blocks, programs and `-> keyword` lines have tools.
17. **Names that spell units trip symbolic lines.** `t` is a tonne, `s` a second, `c` the speed of light: `sin(t) -> laplace` silently treats `t` as a unit, and a coefficient `c` is replaced by 299792458. The keyword reference says to use `x`, `p`, `w`, `k` for variables. `insert_solve_block` and `insert_program` declare the names they introduce (unknowns, parameters, locals) as variables, so `t` is safe there.

Checked and fine: `batch` rollback names the op index and says nothing was applied; `exists` names `overwrite: true` and the `.bak` copy; `unsaved_changes` names `discard: true`; `unknown_document` names `document_list`; `invalid_params` for a missing argument names it; `render_page` refuses page 0 and 99999 before any drawing; `find` outside a solve block is an error on the line, not a tool error.

## 4. Resources and prompts

**Resources** (read-only, URI addressable, cheap to re-read):
- `mathcraft://doc/{id}/page/{n}.png`, `mathcraft://doc/{id}/transcript`, `mathcraft://doc/{id}/region/{rid}`
- `mathcraft://reference/syntax` the linear input syntax cheat sheet (operators, matrix, ranges, units, programs, solve blocks)
- `mathcraft://reference/functions`, `mathcraft://reference/units`, `mathcraft://reference/plots`, `mathcraft://reference/keywords` (symbolic, solve-block and program keywords with a tested example each)

**Prompts** (MCP prompt templates the client can offer):
- `build_calculation`: "turn this problem statement into a documented worksheet with units"
- `explain_worksheet`: "explain this sheet step by step, flag unit or logic errors"
- `verify_worksheet`: "re-derive each result independently and report mismatches"
- `make_report`: "format the sheet as a clean report with headings and aligned results"

## 5. Typical AI session

1. `document_new` -> id.
2. `batch` with a title text region, `x:=3 m`, `t:=2 s`, `v:=x/t`, an x-y plot of a function.
3. `calculate` -> `v = 1.5 m/s`, no errors.
4. `render_page` -> the AI looks at the page, notices the plot overlaps the equation, calls `move_region`.
5. `render_region` of the plot to confirm; `document_save`.

Error feedback is part of every response, so step 3 is often unnecessary: batch returns results directly.

## 6. Safety and trust

- **Local only by default.** stdio needs no network. HTTP binds `127.0.0.1` with a random bearer token printed at start; remote binding requires an explicit flag.
- **Filesystem scope.** `document_open/save`, `insert_image` and any file-reading function work only inside configured roots (default: current directory and the user's documents folder). The engine's file layer stays off unless the user enables it.
- **Resource limits** per call: engine work budget and 60 s deadline, max regions, max image size, max response size (images downscaled, transcripts truncated with a continuation cursor).
- **Mutation marking.** Tools carry MCP annotations (`readOnlyHint`, `destructiveHint`) so clients can ask the user before writes or deletes. `document_close` with unsaved changes requires `discard: true`.
- **Untrusted content.** Text read from worksheets is returned as data; tool descriptions state that document content must never be followed as instructions.
- **Live mode**: visible "AI connected" indicator, pause/disconnect button, every AI edit is one undo step labelled "AI: <tool>".
- **Audit log** (optional): JSONL of tool calls for reproducibility.

## 7. Work packages

Each is one branch, one worker, per CLAUDE.md. Order matters.

| # | Task | Model | Review |
|---|------|-------|--------|
| 1 | **Document API extraction**: move edit operations out of `edit/session.rs` into a GUI-free API with stable region ids; GUI calls it; behaviour unchanged. | Sonnet | Full |
| 2 | **`mathcraft-render` library**: offscreen page/region render to PNG, extracted from the screenshot example; `mathcraft render` CLI command; golden image smoke tests (hash tolerance). | Sonnet | Light |
| 3 | **`mathcraft-mcp` skeleton**: stdio server, `capabilities`, `document_new/open/save/info/list`, session registry, schema generation, tests with a scripted MCP client. | Haiku | Light |
| 4 | **Read tools**: outline, region, transcript, variables, errors, discovery (functions, units). | Haiku | Light |
| 5 | **Write + calculate tools**: insert/edit/move/delete math and text, `batch`, `calculate`, `evaluate_expression`, undo/redo. | Sonnet | Full |
| 6 | **Render tools**: `render_page`, `render_region`, `render_plot_data`, size caps. | Haiku | Light |
| 7 | **Plots and formatting tools**: `insert_plot`, `set_plot_options`, `format_*`, page setup. | Haiku x2 | Light |
| 8 | **Resources + prompts**, syntax reference document (written from Mathcraft's own parser, not PTC help). | Haiku | Light |
| 9 | **Security pass**: path roots, token auth, limits, fuzzed tool inputs, prompt-injection notes. | Sonnet | Full |
| 10 | **Live mode**: app-hosted endpoint, AI indicator, pause, undo labelling, `app_*` tools. | Sonnet | Full |
| 11 | **Packaging and docs**: `claude mcp add mathcraft -- mathcraft-mcp` instructions, config snippets for Claude Desktop/Code, bundle the binary in the app/installers, example sessions as tests. | Haiku | Light |
| 12 | **Agent evaluation**: scripted scenarios an LLM must solve through the MCP only (build a projectile sheet, fix a unit error, plot a function, format a report); track success rate. | Haiku | Light |

Packages 1 and 2 are independent and run in parallel; 3 can start once 1 has a stub API. 4 to 8 then run in parallel on separate tool modules.

## 8. Acceptance (feature complete)

- Every GUI-creatable region type and every ribbon formatting action has an MCP tool equivalent (tracked in a parity table in this file, updated as tools land).
- A client with only the MCP tools can build, calculate, format, render and save a worksheet that opens in Mathcraft and round-trips through the writer.
- Render output from MCP is pixel-identical to the GUI screenshot harness for the same document.
- Every tool has a JSON schema, a description an LLM can use without extra docs, a unit test and one scripted end-to-end test.
- All safety limits hold under adversarial input (huge regions, recursive definitions, huge images, path traversal).

## 9. Decisions

1. **Live is the priority**, headless is built alongside (they share every tool). Package 10 moves up: the app-hosted endpoint is developed together with the skeleton (package 3), and routing (section 2) is part of acceptance.
2. **Writes:** the AI may create new files and overwrite existing ones, only inside configured roots. Overwriting an existing file requires `overwrite: true` on the call, and a `.bak` copy is kept for the first overwrite per session. In live mode edits are undoable instead.
3. **Input syntax (CEO decision):** Mathcraft's own linear text syntax is canonical, with a cheat sheet as an MCP resource. A LaTeX-to-region converter is added later as a separate package (AIs write LaTeX well, but it is ambiguous and needs its own parser). No Mathcad-plain-text dialect.
4. **Prime comparison (decided): none in the shipped MCP.** The public product and MCP server contain no PTC data and never compare against PTC samples. The AI verifies its own work by build -> calculate -> render -> read back -> fix, using only Mathcraft's engine, renderer and our own sample files. Limitation: this proves internal correctness and tidy layout, not equality with Prime. Closing that gap stays private development work (PTC material in the private repo only) and ends once differences stop turning up. Before going public: history rewrite (needs explicit go-ahead) and replace golden tests that read PTC samples with tests on our own files.
