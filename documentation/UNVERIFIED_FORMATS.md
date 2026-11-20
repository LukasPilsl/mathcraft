# Unverified .mcdx formats

Formats Mathcraft reads or writes that no sample in `documentation/examples/` shows. Each one is
a guess at Prime's schema until it is checked against a file saved by Prime. The code that relies
on a guess carries an `UNVERIFIED` comment that points here.

Check each entry against a real Prime 11 file, then either confirm it (remove the comment and the
entry) or correct the code and this list.

## Implemented

| Feature | Where | Guessed shape | What a sample shows |
|---|---|---|---|
| Hyperlink in a text region | `crates/mathcraft-import/src/rich_text.rs` (`RunStyle::link`, `Hyperlink` wrapper), `crates/mathcraft-import/src/writer/text.rs` | XAML `<Hyperlink NavigateUri="...">` wrapping one `<Run>` per linked run | No sample has a `Hyperlink`. The FlowDocument is XAML, so the element and attribute names are the WPF ones. Prime may store the target elsewhere. |
| Hyperlink drawing | `crates/mathcraft-app/src/canvas.rs` (`run_format`, `LINK_BLUE`) | Link runs are drawn underlined in blue unless the run sets its own colour | Visual only. The colour is Mathcraft's choice, not Prime's. Not drawn in PDF or export. |
| Page header and footer text (read, draw, edit, write) | `crates/mathcraft-import/src/regions.rs` (`Builder::furniture`, `Worksheet::header`/`footer`), `crates/mathcraft-import/src/writer/worksheet.rs` (`write_furniture`), `writer/text.rs` (`plan_furniture`), `crates/mathcraft-app/src/viewmodel.rs` (`furniture_of`), `canvas.rs` (`draw_pages`), `export.rs` (`paginate`), `edit/session.rs` (`start_furniture`) | `mathcad/header.xml` and `mathcad/footer.xml` hold `<regions>` in the worksheet namespace, each `<region>` read like a worksheet region with `<text item-idref>` naming a flow document. Positions are page-local points. The writer writes a part only when its regions changed; a new part gets a `<header>` or `<footer>` root in the worksheet namespace. | All 24 samples have `<regions />` in both parts, so no real header or footer content has been seen. |
| Page break (read, draw, insert, delete, layout) | `crates/mathcraft-import/src/regions.rs` (`RegionKind::PageBreak`, `classify`), `crates/mathcraft-app/src/pages.rs` (`break_layout`), `viewmodel.rs` (`build`, `to_stored`), `canvas.rs`, `export.rs` (`paginate`, `dashed_ops`) | A worksheet `<region>` whose content element is `<page-break />`. Content below it starts the next page in the window, page layout and PDF; the marker is a dashed rule at its height. | No sample has one. Only the element name is guessed. |
| Header and footer field tokens | `crates/mathcraft-app/src/furniture.rs` (`expand_str`) | `{page}`, `{pages}`, `{date}`, `{filename}` are plain text in the flow document, replaced when drawn. Prime's field syntax is unknown. | Mathcraft's own spelling. Prime probably stores a field differently, and a file saved by Prime would show its own field as text. |
| Page-break writer | `crates/mathcraft-import/src/writer/worksheet.rs` (`RegionKind::PageBreak`) | A new break is written as `<region ...><page-break /></region>`. A break from the source keeps its source XML. | Round trip of a file with a page break is untested against Prime. |
| Plot markers (vertical and horizontal) | `crates/mathcraft-import/src/regions/plot.rs` (`Plot2d::markers`), `crates/mathcraft-app/src/plot.rs`, `canvas.rs` (`draw_plot`) | Display only, not read and not written: the writer emits `<markers />` as in every sample. Prime's screenshots 0230-0232 show a black line with a boxed value label; the file element is unknown. | No sample has a marker. A marker added in Mathcraft is lost on save. |
| Trace symbol and line style names | `crates/mathcraft-app/src/edit/plot.rs` (`SYMBOLS`, `LINE_STYLES`, `WEIGHTS`), `plot.rs` (`symbol_shape`) | `symbol` is `circle`, `square`, `diamond`, `triangle` or `cross`; `line-style` is `Dash` or `Dot`; weights 1, 2, 4, 6 pt. | The samples only have `symbol="none"`, `line-style="Solid"`, weights 1 and 6. |

## Page-break layout

The content below a page break moves so that it starts the top of the next page. This is a display
change only, as for a collapsed area: stored positions do not change. Clicks and insertion points
are mapped back to stored positions with `ViewModel::to_stored`. Items that cross a break are drawn
across it, not moved whole. The mapping ignores collapsed areas above a break.

## Implemented, guessed shape (area and image)

No sample has an `<area>` or an `<image>` region, and no sample has an image part. The
shapes below are guesses, so the reader, the writer and the canvas all depend on them. Check them
against a file saved by Prime 11 first.

| Feature | Where | Guessed shape | What a sample shows |
|---|---|---|---|
| Area | `crates/mathcraft-import/src/regions.rs` (`area_model`, `AreaModel`), `writer/worksheet.rs` (`RegionKind::Area`) | `<region>` whose content is `<area collapsed="true\|false" locked="true\|false" hide-border="true\|false">`, with its member regions in a `<regions>` wrapper inside `<area>` | No sample has an area. The help (`about_areas.md`, `to_protect_unprotect_an_area.md`) describes the features (collapse, lock, hide border), not the file. |
| Image region | `regions.rs` (`RegionKind::Image`, `ImageModel`), `writer/worksheet.rs` | `<region>` whose content is `<image item-idref="rId"/>`. The size is the region's `actualWidth`/`actualHeight`. | No sample has an image region. |
| Image part | `package.rs` (`Package::images`, `linked_images`), `writer/image.rs` | A relationship of type `.../relationships/image` whose target is a `.png`, `.jpg` or `.jpeg` part. New images are written to `mathcad/media/ImageN.png` or `.jpeg`, with `Default` content types for the extension. | No sample has an image part. The `image` relationship type and the `mathcad/media/` folder are the OPC standard type and an invented folder. |
| Image in the PDF | `crates/mathcraft-app/src/pdf.rs` (`Op::Image`) | Standard PDF `/XObject` image, decoded pixels as `/DeviceRGB` with no filter (no `FlateDecode`), flattened onto white. | Not a file-format question, but no sample exercises it. |
| Area members in the file | `crates/mathcraft-app/src/edit/session.rs` (`area_around_selection`), `viewmodel.rs` (`collect_positions`) | Members of an area keep their absolute page positions (`Frame::Page`) in the area's `<regions>`. The area's own `pos`/`size` are its frame, with the header bar above the members. | No sample has an area. Prime may store members relative to the area. |
| Image resize | `crates/mathcraft-app/src/edit/session.rs` (`begin_resize`, `resize_region`) | Resizing rewrites the image region's `actualWidth`/`actualHeight` only; the image bytes are not changed. | No sample has an image region. |

## Plot creation (new and edited 2D plots)

A plot read from a file and not changed is copied from the source byte for byte. A new or edited
x-y or polar plot is generated by `crates/mathcraft-import/src/writer/plot.rs` in the shape of the
samples (`3_rata_3_Kurve_durch_3_Punkte`, `17_rata_schiefer_wurf`, polar plot of
`12_rata_Kreisdiagramme`). The new trace has no `resultRef`, so `result.xml` is written empty and
Prime calculates the plot on opening. Not in any sample:

| Feature | Where | Guessed shape | What a sample shows |
|---|---|---|---|
| Fixed attributes of a new plot | `writer/plot.rs` | `<graph-size width="230.4" height="230.4" />`, `positionticmark="3"`, `legendWidth="71.4666666666667"`, `gridFrequency` 11 | Samples vary these per plot (graph size 81 to 230, ticks 0 to 6, legend width 38 to 139). Prime recomputes them; only the sample defaults (230.4, 11) are confirmed. |
| Region size of a new plot | `crates/mathcraft-app/src/edit/plot.rs` (`PLOT_DEFAULT_SIZE`) | 345 x 330 points | Chosen near the 4-trace plot of sample 3 (362 x 381). Prime's default is not stored anywhere. |
| Plot title | `writer/plot.rs` | `<title>text</title>` | Every sample has `<title />`. |
| Trace name | `writer/plot.rs` | `name="..."` attribute on `<trace>` | No sample names a trace (the app's plot window cannot set it; only kept if read). |
| Grid lines off | `writer/plot.rs` | `<gridLines />` absent inside `<axisGrid>` | Every sample has `<gridLines />`. |
| Log axis | `writer/plot.rs` | `scale-type="log"` on the domain | Every sample is `linear`. |
| New trace attributes | `edit/plot.rs` | No `num-of-points` (samples have both forms); colours cycle dark blue, black, red, yellow, orange, green, blue | Colours and the missing `num-of-points` are in the samples. |
| Default domain of a new x-y plot | `edit/plot.rs` | `startValue`/`endValue` real -10 and 10 without `resultRef` (polar angle 0 and 360) | Prime shows -10 to 10 for an empty plot (screenshot 0053); the stored values of a never-calculated plot are not seen. |
| `origin-positioning="true"` | `writer/plot.rs` | On x-y plots, not on polar plots | Matches the samples. |
| Polar plot insertion | `edit/session.rs` (`insert_plot`) | Same generator with `angularAxis`/`radialAxis` | The one polar sample has the angular axis with a single `<math>`; the generator does the same. |
| Text run formats (bold, italic, underline, colour, family, size) | `writer/text.rs` (`push_run_attrs`) | Run attributes `FontWeight`, `FontStyle`, `TextDecorations`, `Foreground`, `FontFamily`, `FontSize` on `<Run>` | The samples' text regions use the same XAML attributes (`rich_text.rs` reads them); not every combination is in a sample. |
| Text link | `writer/text.rs` | `<Hyperlink NavigateUri="...">` around the run | No sample has a hyperlink. Display only; Mathcraft never opens it. |
| Text alignment per paragraph | `writer/text.rs` | `TextAlignment` on `<Paragraph>` (Center, Right, Justify) | Read from the samples' `FlowDocument` attributes; Justify and Right are not in a sample. |
| Textstile looks (Normal, Titel, Überschrift 1 and 2) | `ribbon/text_format.rs` (`style_change`) | Sizes and colours copied from `presentation.xml` `textStyle` elements (Tahoma 14.67, 37.33, 21.33, 17.33 pt; `#FF2E3192` for the headings) | The values are verified in the sample, but the gallery applies them to the selection only: the named styles are not written back to `presentation.xml` (session only, not stored). |
| Text font list | `ribbon/text_format.rs` | Tahoma, Arial, Calibri, Times New Roman, Courier New, Mathcad UniMath Prime | Prime's exact list is in screenshot 0216 (not checked against the file). Mathcraft draws text in its bundled Noto Sans whatever the name. |
| Textfarbe palette | `ribbon/text_format.rs` (`colour_rows`) | Ten standard colours and tints/shades of each; values are guesses | Prime's exact palette values are not in the screenshot (0129 shows the layout only). |
| Disabled region (Bereich deaktivieren) | `crates/mathcraft-doc/src/document.rs` (`disabled`), `crates/mathcraft-app/src/edit/save.rs` | Session only: not read, not written; a save warns | No sample has a disabled region. |
| Math font, size, colours and label styles | not written | Prime keeps them per region or per name occurrence; samples only have the document-wide `mathStyles` in `presentation.xml` | Greyed in the ribbon until a region-level style is found in a sample. |

## Ribbon: Berechnung, Dokument, Ressourcen, Einheiten

Guessed, with no sample or Prime screenshot to check them against:

- The web addresses shown by the Ressourcen buttons (Community, Blogs, Support-Center, LearningConnector, Neue Benutzer). They are display text only; Mathcraft never opens them.
- The paper codes `Tabloid` and `A5` written to the page model.
- The slash date formats of the header and footer date menu, and the field tokens `{date:..}` and `{path}` that Mathcraft's furniture code writes for them.
- The unit-table rows of the Einheitensystem window beyond what screenshots 0176-0183 show. SI, USCS and CGS are stored in the worksheet; the Benutzerdefiniert tables live in the window only and are not saved.
- Literal subscript names (`range_m`, `g_acc`, `v_max`, `x_1`): written in the same `Span` + `pw:Subscript` form the samples use for digit subscripts, with the subscript text as typed. The samples show only digit and plain-letter subscripts; a subscript that is a unit name (`m`) is assumed to be stored as plain text too. It reads back as a variable subscript.
- The Margin presets Narrow (12.5 mm), Wide (50 mm) and Legacy (30.48 mm) are read from the Ränder list (screenshot 0200); only Standard (25 mm) is measured on the page itself (0003).

## Programs and solve blocks made by the MCP tools

- **Programs (`insert_program`): not written.** No sample holds a programming region, so the XML Prime uses for one (the element names, how local assignments, `if`, `for`, `while`, `return` and `try` nest) is unknown. The reader maps no program element and the writer refuses `Expr::Program` (`programs are not written yet`). Mathcraft does not guess a shape: a sheet that holds a program works in memory (calculate, transcript, `read_region`), `insert_program` answers `saveable: false` with a warning, and `document_save` fails with a message that names the region. Nothing corrupt is ever written. Remove the program region, or keep the sheet open, to save the rest. Writing and reading it needs one sample sheet with a program.
- **Solve blocks (`insert_solve_block`): written like the sample blocks.** The block is a `<solveblock>` region whose members are `<region>` children with `solve-block-category` `guess-value`, `constraint` or `solver`, as in samples 5 and 6; the keyword (`find`, `minerr`, `minimize`, `maximize`, `odesolve`, `pdesolve`) is the `KEYWORD` identifier of the solver member. Guessed parts: the block's size (360 x rows) and the member positions (18.9 pt left, one 18.9 pt row each, the solver one row lower), which the samples vary; and the solver of `odesolve` and `pdesolve`, written as a plain definition `y := odesolve(...)` without the `=` of an evaluation (no sample has either call). `Given` is not written: the samples have no such element, the block region is the Given.
- **Symbolic evaluations (`insert_symbolic`)** use the same `<symEval>` form as the samples. Keywords the samples do not contain (`ztrans`, `invztrans`, `fourier`, `rewrite`, `assume`, `rectangular`) are written the same way, as `KEYWORD` words, and are not checked against a Prime file.

## Result number format and digit grouping

Read by `crates/mathcraft-import/src/regions.rs` (`number_attr`, `general_format`). Written only for a region's own `resultFormat/general` (`writer/result_format.rs`). Guessed shape: attribute `display` on `general` with `general`, `decimal`, `scientific`, `engineering` or `percent`, and `digit-grouping="true"`. Both are written only when they differ from the default. No sample states either; Prime may spell them differently (a file saved by Prime shows the real names). The reader reads back what the writer writes.

- **Worksheet `presentation.xml` (the sheet's default format): refused, not written.** `presentation.xml` is in the schema-typed `worksheet50` namespace, and an attribute no sample states could make Prime refuse the whole file. A changed number format or digit grouping there is a `WriteError::Settings` ("number format and digit grouping are not stored in presentation.xml (unverified)"). The save fails and nothing is written.
- **Region `resultFormat/general`: written, with a warning.** Written so the number format can be used at all. Every save that writes one of these attributes reports `unverified file format written: result number format (region N); Prime may not read it` for each such region: `write_package_with_report` returns the warnings, `document_save` returns them in `warnings`, and the GUI status line shows the first one after a save. A save with defaults, or with only verified changes, reports nothing.
## Refused by the writer (no stored form known)

Factorial `x!`, cross product `×`, norm `‖x‖`, partial derivative `∂/∂x` and gradient `∇` are read, evaluated and drawn by Mathcraft, but no sample shows how Prime stores them. `crates/mathcraft-import/src/writer/expr.rs` refuses them with `WriteError::Unsupported` (nothing is guessed or corrupted), so a worksheet containing one cannot be saved as .mcdx yet. Text form: `partial(x, f)`, `gradient(f, a, b)`, `u×v`, `‖x‖`. Check against a Prime file, then add the elements.
