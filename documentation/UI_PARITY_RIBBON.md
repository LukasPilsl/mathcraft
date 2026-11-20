# Ribbon parity with Mathcad Prime 11

Source of truth: the Prime screenshots (`screenshots/0003`, `0016`-`0024`, `0025`-`0039`,
`0072`-`0096`, `0243`, `0244`, `0115`) and `Mathcad_Prime_11_Documentation_Index.md` section 3 and 4.
Code: `crates/mathcraft-app/src/ribbon.rs` (tab list, `Action`, `RibbonState`), `ribbon/spec.rs`
(declarative types), `ribbon/render.rs` (drawing, one fixed-height row), one `ribbon/<tab>.rs` per tab.

Status: Rechnen, Eingabe/Ausgabe and Funktionen are complete in layout and content. The other seven
tabs have Prime's groups, captions and buttons; wired buttons are listed below. A follow-up fills them.

## How to add a tab, a group or a button

1. A tab is `pub static TAB: TabSpec` in `ribbon/<tab>.rs` (key into `i18n.rs` `tab.*`, a list of
   `Group`). The ten tabs are registered in `spec::tabs()` and `ribbon::TABS`; the order is Prime's.
2. A `Group::new(en, de, cols)` has a caption and columns. `large(button)` is a large button (icon above
   a one or two line label); `stack(&[b, b, b])` is up to three small buttons stacked. `.collapsed(icon)`
   makes the group one dropdown button whose content opens as a panel (Steuerelemente, Integration).
3. A button is `Button::new(id, "English", "Deutsch", Icon::..)` plus modifiers (all `const fn`):
   - `.act(Action::..)` what a click does. **No action means "not available yet"**: the button stays
     visible, greyed, tooltip "not available yet". List it in the table below (a test fails otherwise).
   - `.rule(Rule::..)` when it is enabled: `Doc` (default), `NoRegion` (Textblock, Textfeld, Bild: greyed
     while a region is selected or edited, 0061), `Selection`, `Plot`, `MathDraft`, `NoDraft` (x-y chart:
     greyed while a matrix or text is edited, 0115/0224), `Always`, `Never`.
   - `.menu(Menu::..)` a dropdown, `.split(Menu::..)` a split button (click = action, arrow = menu),
     `.toggle(Flag::..)` pressed while the flag is on, `.check(Flag::..)` a checkbox,
     `.number("500")` a number field, `.combo(&[..])` a combo box, `.below()` arrow under the label,
     `.tip(en, de)` extra tooltip text.
   - Menus: `Menu::Items(&[Item])` (`Item::act`, `Item::missing`, `Item::sub`), `Menu::Palette(&[Section])`
     of `Cell::text/typed/with/missing`, `Menu::Functions(&[names])` (Prime's list; a name missing from
     `functions_table.rs` is greyed) and `Menu::Category("Statistics")` (all functions of a table category).
4. A new command: add a variant to `ribbon::Action` and handle it in `App::ribbon_action` (`app.rs`).
   Enabled-rules that need new state: add a field to `RibbonState` and a `Rule` in `spec.rs`.
5. Icons: `Icon::Text("fx")` draws a short text; stroke icons live in `ribbon_icons.rs`.
6. Narrow windows: the renderer first hides the text of small buttons, then moves the trailing groups into
   a `»` dropdown. The row never wraps and keeps `render::RIBBON_H`. Check with the screenshot harness:
   `cargo run -p mathcraft-app --example screenshot -- <file.mcdx> <out.png> --tab Rechnen --width 900`
   (`--tab` takes the English or German name, e.g. `Functions`, `Eingabe/Ausgabe`, `math_format`).

## Wired in the stub tabs

Matrizen/Tabellen: Insert matrix, insert/delete row and column (while a math region is edited).
Diagramme: Insert chart (x-y, polar), Edit chart. Mathematische Formatierung: result format.
Textformatierung: nothing.

Berechnung (complete, 0022, 0134-0137, 0190-0197): Automatic calculation (AUTO/MANUAL shared with the
status bar), Stop all calculations (cancels a running evaluation), Calculate worksheet and Calculate now
(active only in MANUAL, 0194), error trace First/Previous/Next/Last (select the erroneous regions, greyed
when there are none), ORIGIN, TOL and CTOL (lists 0 and 1, 10^-1 to 10^-7, stored in `builtInVariables`,
the current value is checked), Calculation options (Mathcraft's Worksheet options window plus Prime's
greyed list), File access.

Dokument (complete, 0023, 0138-0144, 0197-0212): Section, Text block, Image; Separate regions
(vertically/horizontally moves the selected overlapping regions apart), Add page break, Insert blank line
(single line, page), Remove blank line (single line, empty area; Mathcraft adds Page break); Seite
(paper size A3, A4, A5, Letter, US-Legal, Ledger/US-Tabloid, orientation, margins Standard 25 mm, Narrow
12.5 mm, Wide 50 mm, Legacy 30.48 mm, custom margins opens the page setup window, show grid, grid size),
stored in `pageModel`; Ansicht (page/draft, plus Mathcraft's zoom lines); Kopfzeilen und Fußzeilen (header,
footer, page number formats, save date formats, file name and path: they type `{page}`, `{pages}`,
`{date:...}`, `{filename}`, `{path}` into the footer, or the part being edited); Bereichsformatierung
(Show border, plus Mathcraft's area-around-selection, align and distribute lines); Zwischenablage (cut,
copy, paste). The paper codes `Tabloid` and `A5`, the slash date formats and the field tokens are
Mathcraft's own spelling (UNVERIFIED, see `UNVERIFIED_FORMATS.md`).

Ressourcen (0024, 0145-0149, 0248): every button shows a small window with the web address or the help
topic and a copy button; Mathcraft never opens a browser. The web addresses are not in the screenshots
(UNVERIFIED). Keyboard shortcuts, About and the Language group are Mathcraft's own windows.

Einheiten dropdown (0175-0187): the unit palette window, and Unit system, which opens the Einheitensystem
window (SI, USCS, CGS stored in `currentUnitSystem`; Benutzerdefiniert with Based on, Change, Insert,
Remove edits the tables in the window only, OK keeps the system it is based on; Cancel marks the worksheet
modified as Prime does, 0184).
Matrizen/Tabellen: Insert matrix (Prime's 12 by 12 size grid, hover highlights, click inserts into the
draft or at the insertion point), operator palette (index, transpose, cross product), function list,
insert above/below/left/right and delete row/column (while a math region is edited, `MatrixOp`), Show
indices and Shrink nested matrices (undoable, on the selected or edited math region's result display,
`MatrixDisplay`). Diagramme: Insert chart (x-y, polar; greyed while a region is edited), Edit chart,
Change type (line = x-y, polar), Add/Remove trace, Add vertical/horizontal marker, Delete marker (greyed
until the plot has one), Styles (Symbol, Line style, Trace colour with Prime's palette, Trace weight),
Axes (logarithmic x or y, grid lines, axis expressions and limits open the plot window). All plot
commands are one undo step (`EditSession::plot_op`). Style changes apply to the last trace: Mathcraft
cannot select a single trace yet. Markers are display only (not saved, see `UNVERIFIED_FORMATS.md`).
Screenshot harness: `--select-plot`, `--select-math`, `--popup <button id>`, `--grid-hover ROW,COL`. Berechnung: automatic calculation, calculate
worksheet/now, calculation options, file access. Dokument: section, text block, image, page break,
blank lines, page layout/grid/region borders/zoom, header and footer fields, align and distribute.
Mathematische Formatierung: result format. Ressourcen: shortcuts, About, language (Mathcraft's own
group). Textformatierung: font, size, bold, italic, underline, text colour, alignment, Textstile
(Normal, Titel, Überschrift 1 and 2) and Link / Link entfernen on the selected text of a text region
being typed. Greyed until text is selected (0215). Colour palette and "Mehr Farben" (colour picker)
are wired. The Link window is display only: the link is never opened.

## Enabled in the QA round (fix/qa-ui-controls)

Solve block (Rechnen; greyed while a region is selected, 0067): inserts a skeleton block at the
insertion point (guess `x:=1`, constraint `x^2==4`, `find(x)`) through `Doc::insert_solve_block`, the
same call as the MCP tool; the user edits the members. Text field (Rechnen, Dokument): starts a text
region as Textblock does (Prime's text field has no stored shape in the samples). Image was already
wired. Disable region (Berechnung): the selected regions are not evaluated (`Document::disabled`,
`eval_worksheet`); the flag lives in the session and a save warns that it is lost (UNVERIFIED, no sample
has a disabled region). Calculate worksheet / Calculate now stay greyed in AUTO mode on purpose: Prime
greys them there too (0194); they enable in MANUAL.

## Not available yet (greyed, tooltip "not available yet")

Rechnen: Chart component menu (Insert chart component, X-axis expression,
Y-axis expression); Stil menu; Einheiten: Base units (Prime opens no window, UNVERIFIED); palette cells without an
action (shown greyed): Gleichungsumbruch (display-only line wrap, no model), in Analysis the
bold `=` between lim and ∂/∂x, in Ingenieurtechnisch z̄ (overbar), ∠ and ∘, in Vektor und Matrix
[‖] (vector of a range), M⟨⟩ (column), M⌒ (row), 1,3..n and V⃗ (vectorize: the model has no such
operators), in Vergleich ∈ (needs `for`), the whole Programmierung palette (programs have no
editing syntax yet), the Physik constants, in Rechensymbole ∠ ∀ ∃ ∩ ∪ (no sets or quantifiers),
Geldsymbole and Tags (the parser rejects these characters in names). Working since the operator
batch: x! (the key `!`, Prime's only shortcut for it), ∇ (Ctrl+Shift+G), ∂/∂x (Ctrl+D), x′ (Ctrl+'), × (Ctrl+8), ‖x‖, Mᵀ, Mᵢ, the
comparison `=` (boolean equal, typed `==`), ℂ ℕ ℚ ℝ ℤ and ℵ ℶ ℷ ℸ (letters of a name). Two extra
Mathcraft cells, dⁿ/dxⁿ and ∂ⁿ/∂xⁿ, insert the operators with an order slot. The Modifikatoren
list is only what screenshot 0034 shows.

Eingabe/Ausgabe: Embed worksheet, Save worksheet in cache, XMCD, MCD converter, Delete annotations,
Data file name, READEXCEL, Excel component (menu), Insert object, Keep aspect ratio, Reset object size,
Update link, Simple and Advanced controls, Edit, Hide left side, Shrink left side, Assign inputs,
Assign outputs, Show as list, Show in worksheet.

Funktionen: working. Names in Prime's lists that `functions_table.rs` lacks are greyed (for example
the `.sc` Bessel variants). Prime's Alle Funktionen opens a docked pane (0096); here it opens the
Insert Function window.

Matrizen/Tabellen: Insert table (Mathcraft reads a Prime table region, `RegionKind::Table`, but cannot create one: the writer has no table format and no sample shows what a new table stores), Delete cells (no cell selection); in the operator
palette: matrix column, matrix row, vectorize (the parser has no such operators); the norm ‖x‖ and
the cross product × work (Ctrl+8).

Diagramme: Number of points, Format chart values, the chart types other than x-y and polar (column,
bar, stem, waterfall, error, box plot, impact), in Styles: Colour scheme, Area fill, Chart background,
Contour values, Perspective; in Axes: Axes cross at 0,0, Tick marks, Tick values; Contour plot, 3D chart.

Mathematische Formatierung: Math font, Size, Text colour, Fill colour, Clear format, Bold, Italic,
Underline. Not done: the samples hold the document-wide `mathStyles` (Variable, Unit, Constant, Function,
System, Keyword: family, size, weight, slant, colour) in `presentation.xml`, but Mathcraft's model keeps
none of them and the math layout draws with fixed STIX faces, so the group needs a model field, a
writer patch of `presentation.xml` and a style-aware layout first.

Textformatierung: Bullets, Numbering, Tag, Spelling, Background colour (Hintergrundfarbe), Subtitle
(Untertitel). Mathcraft keeps no lists, tags, spelling or text background yet. Missing from 0021:
indents, sub and superscript, Textstile beyond the four in the gallery.

Mathematische Formatierung, the reason for the greying: Prime's math font, size, colours and label
styles are kept per region or per name occurrence in a place the samples do not show (the samples
only have the document-wide `mathStyles` in `presentation.xml`). See `UNVERIFIED_FORMATS.md`.

Berechnung: the Redefinition menu
(Built-in, User-defined, Functions, Units, Constants, Variables, Scalar variables, Vectors and matrices:
Mathcraft gives no redefinition warnings); the Calculation options lines Approximate equality, ORIGIN in
strings, Strict singularity check, Multithreading, Units/constants in symbolic math, Bin/Oct/Hex mode
(no place in the worksheet model yet). The Berechnung dropdowns Neudefinition and Berechnungsoptionen
open; only their lines are greyed.

Dokument: in the Seite menu Show frame and Show major grid lines (not stored in the samples,
UNVERIFIED). The Regions group has no Area around selection button any more: it is in Bereichsformatierung.

Ressourcen: the graduation-cap and ? icons at the far right of the tab row are inert. The Tutorial and
Help buttons show the help topic only: Mathcraft has no help viewer for them.
