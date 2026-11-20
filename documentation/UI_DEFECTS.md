# UI defects seen in headless screenshots

First look at the window, from `cargo run -p mathcraft-app --example screenshot` (see the module
doc in `crates/mathcraft-app/examples/screenshot.rs`). PNGs were written to
`/Users/lukaspilsl/development/mathcraft/screenshots/mathcraft/` (outside the repo, gitignored):
`mechanik1_{en,de}.png`, `kreisdiagramme_{en,de}.png`, `plot3d_{en,de}.png`, `rich_text_{en,de}.png`,
`operations_calc_{en,de}.png`, `dialog_insert_function_{en,de}.png`, `dialog_result_format_{en,de}.png`.

Reference for comparison: Prime 11 screens in `screenshots/` (index `Mathcad_Prime_11_Documentation_Index.md`):
0002 (empty main window), 0044 (`a:=3`, `2+3*4=14`, `f(x)`), 0170 (Einheitensystem dialog).
Prime's chrome is light, with large labelled icons and group captions; its results are black.

Ranked by severity (1 = worst). Fixed items are marked FIXED, partly fixed items PARTLY.

## High

1. **FIXED.** Error text runs into the next region. Mechanik_1 rows `F_Sy`/`F_x`, `F_Sx`/`F_y` and `F_R1`/`F_R2`
   show "undefined name ..." drawn over the next column's expression. The error is placed at the
   expression's right edge plus a fixed gap, with no width limit.
   Fix: `crates/mathcraft-app/src/canvas.rs` `draw_result` (about line 828) uses `RESULT_GAP` (line 27).
   Put long messages on their own line under the expression, or clip them to the region width
   (`viewmodel.rs` item rects tell the width).

2. **FIXED.** Content at the left edge loses its glyphs. Regions with x near 0 (`2_rata_2`: `atan(4.2)`, the
   matrix brackets, `[a b] :=`; `13_rata_plot1`: `x := 0, 0.2 .. 3`) are cut at the page edge. Prime
   keeps a margin.
   Fix: `canvas.rs` `show` (line 120, `origin = page.min + rect*zoom`), `draw_draft` (about line 315),
   and the click conversion (about lines 140-155) must all get the same inset. Only the origin is wrong;
   `viewmodel.rs` `MARGIN` (line 41) pads only right and bottom.

3. **FIXED.** Ribbon: every group has a frame and a caption under it ("Regions", "Templates",
   "Operators and symbols", ...), buttons are 92 x 66 pt with a vector icon (`ribbon_icons.rs`, our own
   strokes) above a 12 pt label, toggles look pressed (grid, calculation mode, language). Tooltips and
   the disabled "not implemented yet" style are kept. The row scrolls sideways when the window is
   narrower than the tab (see 21). Checked on all nine tabs in `screenshots/mathcraft/after_tab_*.png`.

4. **FIXED.** Computed results are green in the normal view. Matching and problem tones are colour-coded for
   the verification toggle, but they show all the time. Prime's results are black.
   Fix: `canvas.rs` `draw_result` (about line 845-850): `Tone::Match` should use `theme::INK` unless a
   verification mode is on. `theme.rs` `MATCH` (line 14) stays for that mode.

5. **FIXED.** Units show SI base units, not the named unit. `F_Sx = 879.15 m·kg/s^2` where the stored
   result is `879.15 SI [newton]` (CLI `inspect` on Mechanik_1). Prime shows N.
   Fix: `crates/mathcraft-app/src/format.rs` `unit_text` (line 119) prints the base composition. Map
   known compound units (N, J, W, Pa, ...) to their names, the same as the stored-result text
   (`stored_units_text`, line 155), and show the chosen unit. Display only; golden is unaffected.

## Medium

6. **FIXED (text).** Exponents and units are typed caret text. `1.015×10^3`, `2×10^-3i`, `m·kg/s^2`. Prime shows
   superscripts (`10³`). Results are drawn with a plain `LayoutJob` in `draw_result`, not the math layout.
   Fix: `format.rs` lines 126, 162 emit `^`. Give results a superscript-capable path (or a result
   `MathBox`) in `canvas.rs` `draw_result`.

7. **FIXED (geometry).** Round brackets are oversized and sink below the baseline. `sin(α_s)`, `cos(φ)`, `F(0 °)`,
   `(F_Sx + ...)/(...)`. The closing bracket drops under the line.
   Fix: `mathbox.rs` `bracket_pair` (line 253; `top`/`bottom` padding `0.08*size`, bulge
   `0.1*size + 0.03*height`) and `curve` (line 318). Centre the curve on the inner box's
   ascent/descent midpoint.

8. **FIXED.** Subscripts collide with brackets. `F_(φ)` and `F_(0 °)`: the subscript sits on the bracket's
   stroke. Mostly the same geometry as item 7.
   Fix: `mathlayout.rs` `attach` placement (subscript offset must clear the bracket's width).

9. **FIXED.** Text regions use Noto Sans (regular, italic, bold, bold italic; OFL 1.1, subsets in
   `assets/fonts/`, rows in `THIRD_PARTY_LICENSES.md`). `fonts::text_family` gives the faces; the window
   chrome's proportional family starts with Noto Sans as well. Math and results stay STIX Two.

10. **FIXED.** Red "undefined name" for equations with no value. Mechanik_1 rows `F_x - F_Sx - F_R2 == 0`
    and similar show red errors. These are engine gaps (the golden EngineErrorNoRef category: 12
    regions), but the window presents them as user errors.
    Fix: `viewmodel.rs` tone choice: an equation with no stored result and an unsupported cause should
    be `Tone::Unsupported` (grey, with the reason in the tooltip), not `Problem` (red).

11. **PARTLY.** Tick labels are 10.5 pt (`canvas.rs` `TICK_PT`, was 9 pt with a 7 px floor), margins
    34 / 20 pt for the labels. Still open: the two plot frames overlap in the bottom row of
    `12_rata_Kreisdiagramme` (about x 677). That comes from the stored item rectangles
    (`viewmodel.rs`), not from drawing; not changed here. No axis titles or legend are drawn yet
    (`plot.rs` has none), and Prime's line weights are not checked.

12. **PARTLY.** Fixed: the 3-D plot is clipped to its frame (`canvas.rs` `draw_plot3d` uses a clipped
    painter), and surface quads with a non-finite or zero-length edge are skipped
    (`plot3d::quad_is_drawable`). That removed the lines that ran across the whole window in
    `14_rata_plot2`. Tick labels are 10.5 pt.
    Slivers (fix/ui-polish3): quads that are not convex on screen (bow-ties from clamped corners)
    are now skipped (`plot3d.rs` `quad_is_drawable`). `14_rata_plot2` shows no coloured lines across
    the window any more; thin dark lines still radiate from the peak (see below).
    Open: the black line in `13_rata_plot1` is a real curve, not an axis. The worksheet has three
    `trace3D` curves with `<traceStyle color="#FF000000" automaticStyle="true">`. Prime picks automatic
    colours for those; the importer (`mathcraft-import/src/regions/plot3d.rs`) does not record
    `automaticStyle`, so the stored black is drawn. Fix there (record the flag, then use the palette in
    `plot3d.rs` `trace_view`). Also open: a few thin slivers still run from the peak in `14_rata_plot2`
    (quads whose corners lie far apart on screen); the cause is probably the grid or the clamping in
    `plot3d::to_unit`, not checked. The default region size comes from the stored layout.

13. **FIXED.** Results format labels are readable in light and dark mode (checked in
    `screenshots/mathcraft/after_dlg_result_dark.png`); the Insert function and About dialogs were
    checked in both themes too.

## Low

14. **FIXED.** `theme.rs` `GRID` is lighter (196, 220, 240); the spacing (24 pt) is unchanged.

15. **Insertion crosshair covers the first subscript.** On open, the crosshair sits on `m_1`'s `1`.
    Prime also starts with a crosshair, so only the position is a question.
    Fix: `canvas.rs` `draw_crosshair` (line 253).

16. **FIXED.** Calculus placeholders. The limit drew a second, stray `o` (the engine's symbolic value
    shown beside the body) and empty slots were filled squares. See "FIXED on fix/ui-math-layout"
    above.

17. **Document strip is cluttered.** A full path in a text box, plus Open and Save As, next to the
    file name. Prime shows the file name on a tab.
    Fix: `ribbon.rs` `document_strip` (line 407).

18. **FIXED.** The hint line above the sheet is 12 pt in the weak text colour (`app.rs` `central`).

19. **Selected text is not highlighted.** Selected regions now get a translucent tint under the outline
    (`theme.rs` `SELECTION_FILL`, `canvas.rs` `draw_overlay`). A selection inside a math draft or a text
    region is not drawn: `DraftView` (`edit/session.rs`) has no selection range, and `edit/math.rs`
    keeps the anchor but does not expose it. Medium. Fix: add the range to `DraftView`, then draw it in
    `canvas.rs` `draw_draft`. Not checked visually (the harness has no selection flag).
20. **PDF export sets text regions in STIX Two.** `export.rs` uses `Face::Roman` for text runs, so the
    PDF and the screen differ. Medium. Fix: `export.rs` text runs with `fonts::text_family` (the PDF
    writer then needs the Noto Sans files in `fonts::file_data`).
21. **FIXED (fix/ui-backlog).** The ribbon no longer scrolls sideways. Groups wrap to a second row
    when the window is narrower than the tab (`ribbon.rs` `group`, rows are `horizontal_wrapped`).
    Before: below about 1400 px at 100 % the last group of the Math tab was cut until the row was
    scrolled sideways.
22. **Plot frames have no title or legend.** Prime shows axis titles and a legend when given. Medium,
    part of 11.

## FIXED in this branch (feat/ui-polish-math)

- **Results are black** (item 4). `Tone::Match` and `Tone::Plain` draw in ink; green and red are for
  the stored comparison and real problems only.
- **Named units** (item 5). `format.rs` `dimension_text` shows N, Pa, J, W, C, V, Ω, F, S, T, Wb, H
  for a dimension-only value that matches one (factor 1 to SI, so the number is unchanged).
- **Superscripts** (item 6). `format.rs` `pow_text` writes exponents as superscript characters
  (`10³`, `10⁻¹²`, `m·kg/s²`). The golden is unaffected: the sample comparison reads numbers.
- **Text results beside the expression** (item 1). `viewmodel.rs` `result_room` gives the free width
  to the next region on the same rows. A result that does not fit goes on the line below, and is
  cut with an ellipsis when still too long (full text in the tooltip).
- **Left and top margin** (item 2). `canvas.rs` `CONTENT_INSET` (12 pt) offsets content, clicks,
  overlay and draft from the page edge. The grid and the page stay at the edge.
- **Round brackets** (items 7, 8). `mathbox.rs` `bracket_pair`: padding 0.04 em (was 0.08), bulge
  0.08 em + 0.02 height, and a `BRACKET_LEAD` gap of 0.1 em before the opening stroke, so a script
  on the content before it clears the stroke.
- **Equations with no stored result** (item 10). `viewmodel.rs` `result_line`: an engine gap without a
  stored result is grey, not red, and a definition without `=` shows nothing (Prime stores no
  result for it either). Engine errors that Prime stored stay red.

Tests: `mathbox::tests` (bracket extent, stroke bounds, lead), `format::display_units_tests` (named
units, superscripts, glyph coverage), `viewmodel_tests::a_result_has_the_room_up_to_the_next_region_on_its_rows`.

## Still open (math)

- Item 6: symbolic results stay math layout; numeric results use superscript characters, which the
  bundled serif face draws, but spacing around `×10³` is not the math layout's.

## New, found while fixing (math), status on `fix/ui-math-layout`

19. **FIXED.** Big operator limits collided with the operand. `Σ` and `Π` now stack their limits
    (`mathbox.rs` `stacked_limits`): the upper limit is centred above the sign, the lower one below
    it, and the operand follows the sign's width. The integral keeps its limits beside the sign, as
    Prime sets them (`mathlayout.rs` `Expr::Integral`). Checked in `operations/Unbenannt`.
20. **FIXED.** A matrix result after `=` is drawn as a grid in square brackets, with a unit after it
    when all cells share one (`format.rs` `matrix_cells`, `mathlayout.rs` `matrix_result`, shown by
    `viewmodel.rs` `result_line`). Results over 400 cells keep the shape text. Checked in
    `2_rata_2` (the `solve` results). A grid wider than the free room goes on the line below
    (`canvas.rs` `draw_result`).
21. **FIXED.** Brackets sat low when the content had a subscript or a descender-free text box:
    `(α_s)` drew the stroke centred on the box, which the subscript pulled down. Round and square
    brackets are now centred on the math axis, like fraction bars and matrices, and reach as far as
    the content on the farther side plus the pad (`mathbox.rs` `delimiter_extent`).
22. **FIXED.** Function names labelled as variables, or unlabelled, drew italic (`sin` in
    `Mechanik_1`). A call's name is always upright, and the standard function names (`sin`, `cos`,
    `ln`, `log`, `exp`, ...) are upright whatever their label (`mathlayout.rs` `STANDARD_FUNCTIONS`).

## FIXED on fix/ui-math-layout (also covers items 16, 19-22)

- **Limit placeholder** (item 16). A limit with no `=` or `->` shows no result, so the engine's
  symbolic value for it (the stray `o` in `operations/Unbenannt`) is no longer drawn
  (`viewmodel.rs` `shows_result`). Empty slots are now outlined boxes, not filled squares
  (`mathbox.rs` `placeholder`).

## Earlier notes (before the fixes above)

- **Selected ribbon tab text was unreadable in dark mode** (accent colour on accent fill). Removed the
  forced colour in `ribbon.rs` `tab_row`, so egui's selection colour applies. Found in dark renders.
- Items 3, 9, 13, 14, 18 above; 11 and 12 partly.

## Harness notes (not app defects)

- A headless egui context has no OS theme, so it defaults to dark. The harness sets light explicitly
  (Prime's windows are light); `--theme dark` renders the other one. The real window follows the OS.
- Windows fade in over time. Without a clock the dialogs stayed translucent with grid lines showing
  through. The harness now advances a simulated clock at 60 fps and runs 45 frames after staging.
- egui_kittest was not used: it forces egui's `accesskit` feature, which breaks egui-winit 0.33.3 to
  compile. The harness uses egui-wgpu directly (dev-dependency only).

## fix/ui-backlog

Status of the backlog items, with the checks that were made. Screenshots are in the gitignored
`screenshots/mathcraft/ui_*.png` (before: `ui_base_*`, after: `ui_after_*`).

- **Text selection, mouse (item 19 extended).** A press in the draft places the caret, a drag selects
  from the press, a double click selects the word (`canvas.rs` `show` reports `Output::draft`,
  `session.rs` `draft_pointer`, `text.rs` `place_caret`/`extend_to`/`select_word_at`, `math.rs`
  `place_caret`/`extend_to`/`select_word_at`). The math caret is found from the rendered text by
  recording each caret slot while rendering (`math.rs` `Writer::slots`, `slot_at`). A drag that starts
  in the draft selects text and no longer moves regions. Checked by unit tests only: the harness
  cannot send pointer events, so the live mouse path is not seen in a screenshot. A double click on a
  region that is not being edited still only selects it.
- **Symbolic keywords upright (item 22 extended).** `solve`, `factor`, `assume`, `simplify`, ... in a
  `-> command` are set upright (`mathlayout.rs` `SYMBOLIC_KEYWORDS`, `command_item`); their arguments
  stay italic. Test: `mathlayout_tests::symbolic_keywords_are_upright_and_their_arguments_italic`.
- **Ribbon at 1200 px (item 3, 21).** Before: Math tab cut at "Integral" with a horizontal scroll bar.
  After: the groups wrap to a second row; no horizontal scroll bar at 1200 px or at 1400 px.
- **Dialogs in dark theme and German (item 13 recheck).** Insert function, insert unit and result
  format were rendered in dark and German. No text was clipped and the labels are readable at
  1400 x 900. No colour change was needed; the disabled button in result format is low contrast
  by design (egui's disabled style).
- **Left edge (item 2 root cause).** Cause: `canvas::show` placed the content at the page's own edge,
  so glyphs that reach left of their origin (bracket lead, italic overhang, the stroke of a matrix
  bracket) were lost at x = 0. The fix is `CONTENT_INSET` (12 pt) for the content origin, which the
  draft, the clicks and the overlay share. Checked in `2_rata_2` (draft view and `--page-layout`) and
  `13_rata_plot1`: no clipping at 100 %. The remaining gap is that a click in the 12 pt margin is not
  taken as a click on the page (`canvas.rs` `inside`); not changed.
- **Matrix result columns (item 20 extended).** A result shows at most 20 columns; a wider one keeps
  19 and ends in a column of ellipses (`mathlayout.rs` `MAX_RESULT_COLUMNS`, `matrix_result`). A 1 x 400
  result is no wider than a 20-column grid. Test: `mathlayout_tests::a_wide_matrix_result_is_capped_at_the_column_limit`.
- **Plot legend (item 22 extended).** The legend is drawn only when the plot has its `<legend>` flag
  (`plot.rs` `PlotView::legend`, `legend_entries`). Trace names are still empty in the samples, so the
  legend stays hidden there. Test: `plot::tests::no_legend_is_drawn_unless_the_plot_has_the_legend_flag`.

## fix/ui-polish3 (round 3, light and dark, en and de)

Rendered all 25 samples (`screenshots/mathcraft/p3_*_light_en.png`) and a dark and a German set.

23. **FIXED.** 3-D surface slivers. Quads whose projected corners cross (bow-ties) filled as long
    lines across the plot (`14_rata_plot2`, `13_rata_plot1`). Before: `p3_14_rata_plot2_light_en.png`.
    After: `p3_14_rata_plot2_after_convex.png`. Fix: `plot3d.rs` `quad_is_drawable` also requires a
    convex quad (same turn sign at each corner). Test: `plot3d::tests::degenerate_quads_are_not_drawn`
    (bow-tie case).
24. **FIXED.** The 2-D plot note ("1 of 4 traces not drawn") sat on the top tick label and the frame
    (`3_rata_3_Kurve_durch_3_Punkte`, dark, de). Before: `p3_3_dark_de.png`. After:
    `p3_3_after_note.png`. Fix: `canvas.rs` `draw_plot` gives the note its own row above the frame
    (a title moves down under it).

## Open after round 3

- **Plot notes are English in German.** `plot.rs`/`plot3d.rs` build "n of m traces not drawn" with
  `format!`, not through `i18n`. Medium. Fix: a key in `i18n.rs` and the note text from there.
- **Plot axis titles**: checked in the samples. `<axisLabel />` is present on every axis but empty
  in all 24 samples, so there is no text to read or draw. Not read (see `plot.rs`). Low.
- **Overlapping plot frames** in `17_rata_schiefer_wurf`: confirmed in the XML. Region 10 spans
  x 0 to 323.06 and region 11 starts at x 302.36, both at top 529.13. Kept faithful to the stored
  rects, not a drawing bug. Medium.
- **Black trace in `13_rata_plot1`**: resolved. `automaticStyle="true"` is read for 3-D traces and
  they use the automatic palette (`plot3d.rs`). The only 2-D-looking automatic traces are in
  `plot3D` regions. No 2-D `xyPlot` in the samples carries `automaticStyle`, so 2-D keeps its
  stored colour.
- **Stray result text in `Unbenannt`**: the limit body `o` is the user's content, not an engine
  value (checked, item 16 is correct).

