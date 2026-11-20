# How a Mathcad Prime `.mcdx` file works

A `.mcdx` file is a Mathcad Prime worksheet. Inside it is a set of XML files, zipped together.
Mathcraft reads these files in `crates/mathcraft-import`. The description below comes from the
files of Mathcad Prime 11.0.0 that were inspected while building that importer. It is not an
official specification.

## 1. The file is a ZIP package

Rename a `.mcdx` to `.zip` and open it. You will find this layout:

```text
[Content_Types].xml          list of the content types of each part
_rels/.rels                  points to the parts that matter
docProps/core.xml            title, author and dates
docProps/app.xml             application information
mathcad/worksheet.xml        the regions and their math (the sheet itself)
mathcad/result.xml           the calculated results, one entry per result
mathcad/header.xml           page header
mathcad/footer.xml           page footer
mathcad/integration.xml      regions that link to other programs (empty in simple sheets)
mathcad/settings/presentation.xml   display settings
mathcad/settings/calculation.xml    calculation and unit settings
```

Each part is a plain XML file in UTF-8. `[Content_Types].xml` and `_rels/.rels` follow the Open
Packaging Conventions used by Office files, so any ZIP tool can read the package.

## 2. The worksheet: `mathcad/worksheet.xml`

The root element is `<worksheet>`. It declares these XML namespaces:

| Prefix | Namespace | Holds |
|---|---|---|
| `ml` | `http://schemas.mathsoft.com/math50` | the math: definitions, evaluations, operators, numbers |
| `u` | `http://schemas.mathsoft.com/units10` | unit names and powers |
| `ws` | `http://schemas.mathsoft.com/worksheet50` | the worksheet's own elements |
| `p` | `http://schemas.mathsoft.com/provenance10` | where a region came from |

Every region is a `<region>` inside `<regions>`. It has a position on the page and holds one
`<math>` element:

```xml
<region region-id="0" actualWidth="60.04" actualHeight="25.6" top="37.8" left="18.9">
  <math resultRef="0">
    <ml:define>...</ml:define>
  </math>
</region>
```

- `region-id` identifies the region. It is unique in the sheet.
- `top` and `left` give the position in points from the top-left corner of the margin box (the
  printable area inside the page margins), not the paper's corner. In the 24 top-level samples
  `left` and `top` are never negative and are 0 at the first grid corner, so 0 is the margin box
  corner. With the Standard margins (25 mm) a region at 0 sits inside the grid, as Prime draws it.
- `resultRef` links the region to its result in `result.xml`. Both use the same number.

A region holds one of these:

- **Definition** `<ml:define>`: `name := expression`. The name is an `<ml:id>` with
  `labels="VARIABLE"` (or `FUNCTION` for functions with arguments). The expression follows it.
- **Evaluation** `<ml:eval>`: `expression =`. The result is shown after the `=`. It can carry a
  `<ml:unitOverride>` to show the result in a chosen unit (an empty `<ml:placeholder/>` means
  none).
- **Text**: prose regions.
- **Solve block, program, plot, symbolic**: these have their own elements. A solve block is a
  region with `solve-block-category="solver"` and a `KEYWORD` such as `find` or `minerr` in the
  expression.

### Expressions are trees

Math is stored as a tree, not as text. Each operator is an `<ml:apply>` with the operator as its
first child, then the operands:

```xml
<ml:apply>
  <ml:mult />
  <ml:real>3</ml:real>
  <ml:id labels="UNIT" label-is-contextual="true">m</ml:id>
</ml:apply>
```

Common pieces:

| Element | Meaning |
|---|---|
| `<ml:real>3</ml:real>` | a number |
| `<ml:id labels="VARIABLE">x</ml:id>` | a variable |
| `<ml:id labels="UNIT">m</ml:id>` | a unit |
| `<ml:id labels="CONSTANT">h</ml:id>` | a constant (`h` is Planck's constant in Prime) |
| `<ml:id labels="FUNCTION">sin</ml:id>` | a function, used as the first child of an `apply` |
| `<ml:mult/>`, `<ml:div/>`, `<ml:plus/>`, `<ml:minus/>` | arithmetic |
| `<ml:neg/>` | unary minus |
| `<ml:pow/>` | power |
| `<ml:nthRoot/>` with `<ml:placeholder/>` | square root (the empty degree means 2) |
| `<ml:range>` | a range `from..to` |
| `<ml:matrix rows="2" cols="1">` | a matrix, with its elements inside |

Subscripts are stored as a `Span` with a `pw:Subscript` in the label text, for example `v_0`.
Prime may also write a unary minus into a number (`-1`) instead of a `neg` element.

## 3. The results: `mathcad/result.xml`

`result.xml` holds the calculated value of each region. Entries are matched to regions by number:

```xml
<resultsList xmlns:ml="http://schemas.mathsoft.com/math50"
             xmlns:u="http://schemas.mathsoft.com/units10"
             xmlns="http://schemas.mathsoft.com/result10">
  <resultData result-id="6" calculation-status="Synchronized">
    <ml:result>
      <ml:unitedValue>
        <ml:real>2.0394324259558561</ml:real>
        <u:unitMonomial>
          <u:unitReference unit="second" power-numerator="1" power-denominator="1" />
        </u:unitMonomial>
      </ml:unitedValue>
    </ml:result>
  </resultData>
</resultsList>
```

- `result-id` matches `resultRef` in `worksheet.xml`.
- `calculation-status` is `Synchronized` when the value is current. It is `Pending` when the
  value has not been calculated yet.
- The value in `<ml:real>` is stored in **SI base units**, whatever unit is shown on screen. A
  result shown in liters is stored in cubic metres, for example. The unit names in
  `<u:unitReference>` say which unit was displayed.
- A result that failed has `<engineErrors>` with an error code and text instead of a value.
- A plot result has a `<ml:Trace2dResult>` with the range of its values and the trace points.

Mathcraft's golden check compares its own calculation with these stored values.

### Plot traces: `<ml:Trace2dResult>`

Each `<trace resultRef="N">` of a plot points at `resultData` N, which holds a `Trace2dResult`
instead of an `<ml:result>` (found in the samples: 85 entries in 15 sheets, checked by
`crates/mathcraft-import/tests/plot_traces_samples.rs`):

- `TraceType` is `Range`, `Parametric` or `Vector`. `RangeInfo` gives `Min` and `Max` of the
  parameter (with a `unitMonomial` when it has units). `ResultInfo` gives `Min` and `Max` of the
  plotted values.
- `Data/RangePoints/DataVectors` and `Data/ResultPoints/DataVectors` hold comma-separated lists in
  square brackets, of equal length. The first list is the first coordinate of each point (x) and
  the second is the second coordinate (y), for every trace type. On a polar plot the pair is
  (angle in radians, radius), before the conversion to x and y.
- A `Vector` trace has one point. A `NaN` in either list is a gap (for example `sqrt` of a negative
  value on part of the range); the polyline breaks there.
- The count is one fewer than the trace's `num-of-points`: 500 gives 499 values, in every stored
  trace of the samples. The values are evenly spaced over the range, first to last.
- Values are in SI base units, as for other stored results (the units of the ranges are in
  `unitMonomial`). Mathcraft compares its own series with them to 1e-9 absolute and relative,
  point by point.

### 3D plot data (`plot3D`)

The 3D plot is a region `<plot><plot3D>` in `worksheet.xml`. Its traces are `<trace3D resultRef="N">`
with `num-of-points`, and the plot has `xAxis`, `yAxis`, `zAxis`, one `<plotEquations>` list (trace
`i` uses plot equation `i`, each with one `<math>` per slot; the second slot is a placeholder in the
samples) and a 4 x 4 `CameraTransformMatrix`. Found in 2 samples (`13_rata_plot1`: two curves and a
surface; `14_rata_plot2`: a matrix surface).

The stored data of trace `N` is a `<ml:result>` (not `Trace2dResult`): a matrix of four.

- element 0: the kind, `0` for a curve and `2` for a surface;
- element 1: a matrix of three, the `X`, `Y`, `Z` coordinate matrices (`rows` x `cols`,
  row-major). A curve has `rows` x 1;
- element 2: a matrix of six, `[xmin, xmax, ymin, ymax, zmin, zmax]`;
- element 3: `1` in every sample (meaning not known).

Sample 13: a curve `x` over the range `x` has `X = x`, `Y = 0`, `Z = x`; the surface `f(x, y)` has
`X[r][c] = x_c` and `Y[r][c] = y_r` (the x range runs along the columns), and its values match the
evaluated `f`. Sample 14: the matrix surface `A(i, j)` is stored with `X` and `Y` evenly spaced from
-10 to 45 (41 values), which is not the stated axis (-15 to 45), and its `Z` values do not follow
the sheet's own definitions (see the golden baseline note). Checked by `mathcraft-cli golden`.

## 4. Settings: `mathcad/settings/`

`calculation.xml` (namespace `calculation10`) holds:

- `builtInVariables`: `array-origin` (0 or 1), convergence and constraint tolerances, the print
  precision (`prn-precision`) and the random seed.
- `calculationBehavior`: automatic recalculation, string origin, and symbolic options.
- `units`: `currentUnitSystem` (`si`, `us`, `cgs`, or a custom system), and any custom unit
  definitions in `customUnitSystem`.

`presentation.xml` holds the display settings: fonts, colours and page layout.

Its page layout is `<pageModel paper-code="A4" orientation="Portrait" margin-type="Standard"
grid-size="Standard" />`. The file stores the margin type's name only, not its margins. Every
sample is A4, portrait, standard margins. Standard margins are 25 mm on each side: Prime 11's
screenshot 0003 shows the grid starting 24.8 mm and ending 25.0 mm inside the A4 edges. The other
margin types are not measured yet. `header.xml` and `footer.xml` are empty (`<regions />`)
in all 25 samples, so their content format is not known yet. No sample has a page break.

## 5. How the parts fit together

1. **Opening**: Prime reads `worksheet.xml` to build the regions, then loads `result.xml`. If a
   stored value is still `Synchronized`, Prime shows it without calculating again.
2. **Editing**: changing a region marks the results that depend on it as `Pending`. Prime then
   recalculates them in the background (automatic calculation is set in `calculation.xml`).
3. **Saving**: Prime writes the worksheet, the settings and the current results. A saved file
   therefore always contains Prime's own values.

Two things follow from this:

- **Results can be recalculated outside Prime.** If `result.xml` is emptied, Prime calculates
  every region when the file is opened and then calls `ResumeCalculation` and `Synchronize`
  (through the automation object). Saving then writes fresh results. The comparison scripts in
  `tools/prime-compare` use this to test Mathcraft against Prime.
- **The stored results are a reference.** Mathcraft reads them to check its own results. It
  never needs them to calculate.

## 6. How Mathcraft reads a `.mcdx`

`crates/mathcraft-import` does this in four steps:

1. `package.rs` opens the ZIP and reads `worksheet.xml`, `result.xml` and the settings.
2. `dom.rs` turns the XML into a tree that is easy to walk.
3. `expr.rs` converts each math tree into Mathcraft's text syntax (for example
   `<ml:mult/>` becomes `*`, and `<ml:nthRoot/>` with a placeholder becomes `sqrt(...)`).
4. `regions.rs` converts each region: definitions, evaluations, solve blocks and text. A region
   that cannot be converted becomes a text note, and the reason is listed in the import report.

`results.rs` reads `result.xml` for the golden check. The import status is in `docs/FEATURES.md`.

Command line:

```bash
mathcraft import my_sheet.mcdx -o my_sheet.mcraft
```

## 7. Writing a `.mcdx` yourself

A `.mcdx` can be written by copying an existing package and replacing `worksheet.xml`. The other
parts can be kept. The generator in `tools/prime-compare/New-PrimeBattery.ps1` does this: it takes
a template package, writes one evaluated region per line, and clears `result.xml`. Prime accepts
the result when it is opened.

Caveats:

- The element names above were read from Prime 11.0.0 files. Other Prime versions may write
  different details.
- Only the subset listed here was written and tested. Solve blocks, programs, plots and
  symbolic regions are read from existing files, but not generated.
- Integration regions (`integration.xml`) link a worksheet to other programs. Their format was
  not documented, so Mathcraft does not read them.

## 8. Calculus operators and tables

Source: `documentation/examples/operations/Unbenannt.mcdx`, saved by Prime 11.0.0.0 (engine
11.0.0.21, math schema 5.10.9), with its screenshot `Bild.png`. It is the only sample with these
elements. The 24 top-level samples have none. Everything below is read from that one file. Points
that one sample cannot settle are marked **not in sample**.

The file holds five regions: a derivative, a definite integral, a summation with an empty lower
bound, a limit and a table of three defines. Their free names (`r`, `i`, `f`, `x`, `y`, `o`) are
not defined anywhere in the sheet. `result.xml` stores `calculation-status="Synchronized"` for
all seven results, and no value for any of them.

### Calculus operators

Each operator is an `<ml:apply>` with the operator element first. Its operands follow: an
`<ml:lambda>`, then the wrapper elements the operator takes.

```xml
<ml:apply>
  <ml:integral />
  <ml:lambda>
    <ml:boundVars><ml:id xml:space="preserve">p</ml:id></ml:boundVars>
    <ml:id xml:space="preserve">x</ml:id>
  </ml:lambda>
  <ml:lowerBound><ml:id xml:space="preserve">i</ml:id></ml:lowerBound>
  <ml:upperBound><ml:id xml:space="preserve">f</ml:id></ml:upperBound>
</ml:apply>
```

| Element | Operands after the operator | Meaning | Mathcraft |
|---|---|---|---|
| `<ml:derivative />` | `ml:lambda`, `ml:degree` | d/dx of the body | `Derivative` |
| `<ml:integral />` | `ml:lambda`, `ml:lowerBound`, `ml:upperBound` | definite integral | `Integral` |
| `<ml:summation />` | `ml:lambda`, `ml:lowerBound`?, `ml:upperBound` | sum over the bound variable | `Accumulate` (sum) |
| `<ml:product />` | as summation | product | `Accumulate` (product); **not in sample** |
| `<ml:limit />` | `ml:lambda`, then the bare target expression | limit of the body | `Limit` |

- **`ml:lambda`** holds `<ml:boundVars>` with exactly one `ml:id` (the bound variable), then the
  body as the next child. The bound variable and the body have no `labels` attribute in the
  sample, so they read as variables.
- **`ml:degree`** is the derivative's order. `<ml:placeholder />` inside it is the first
  derivative (`order: None`). A number or expression as the degree is the nth derivative, but
  **no sample has one**: the reader accepts it, and the writer writes only the placeholder or an
  expression.
- **Empty bound (placeholder):** a bound with no element at all is empty. The summation of the
  sample has only `<ml:upperBound>2</ml:upperBound>`, so its lower bound is empty. The reader
  turns an absent bound into `Placeholder`, and the writer omits the element for a placeholder.
  The sample shows this for the lower bound of a summation only. The same rule for an integral
  is **inferred, not seen**.
- **`ml:limit`** has no attributes in the sample, and its target is a bare expression
  (`<ml:real>9</ml:real>`). The help (`limit_operator.md`) gives three limit sides: right (`+`),
  left (`-`) and two-sided (empty). Their storage is **not in sample**. The reader treats a limit
  with no attributes as two-sided and refuses any attribute. The writer writes two-sided only.
  Encoding of the one-sided forms: **unknown, to verify in Prime** (no sample has one; a guess
  would be an attribute on `ml:limit` or an extra operand, unverified). The core implements the
  semantics (`Side::Right`, `Side::Left` in `symbolic/limit.rs`); the AST has no side field yet.
  Infinite targets: the sample's `∞` / `inf` spelling is read as the infinity name (see
  `calculus.rs` `is_infinity`); the sign of `-∞` is a unary minus, as the reader builds it.
- **`ml:product`** is **not in sample**. It is read by analogy with `ml:summation`. The writer
  refuses it.
- The vector forms `sum(v)` / `product(v)` have no element in any sample. The writer refuses them.

Evaluation: the numeric evaluator refuses a limit (the help says limits are symbolic and the `=`
sign cannot evaluate them). A lower bound of `i` is the imaginary unit, Mathcad's built-in
constant, so the sample integral's lower bound is complex and refused as "a complex limit of an
integral". Prime's stored result for that region cannot confirm this: `result.xml` has no value.

### Tables: `<spec-table>`

A table is a region whose content is `<spec-table>`, not `<math>`. The table region has no
`resultRef`. Each of its members is a `<math>` child of `<spec-table>`, with its own `resultRef`.
The members are not regions: they have no `region-id`.

```xml
<region region-id="4" top="264.56..." left="188.97...">
  <spec-table>
    <math resultRef="4">
      <ml:define>
        <ml:id labels="VARIABLE" xml:space="preserve">a</ml:id>
        <ml:apply>
          <ml:scale />
          <ml:matrix rows="3" cols="1">
            <ml:real>1</ml:real><ml:real>4</ml:real><ml:real>7</ml:real>
          </ml:matrix>
          <ml:id labels="UNIT" xml:space="preserve">m</ml:id>
        </ml:apply>
      </ml:define>
    </math>
    <!-- resultRef 5 (b, unit s) and 6 (c, unit m/s) follow the same shape -->
  </spec-table>
</region>
```

- A member is `ml:define` with a `VARIABLE` target and a value of `ml:scale` (matrix, unit).
  The unit of `c` is `ml:apply` `ml:div` of two `UNIT` ids (`m/s`).
- `rows`/`cols` on `ml:matrix` are as for any matrix. The table here has 3x1 matrices.
- The screenshot shows a header row (`a`, `b`, `c`), a unit row (`(m)`, `(s)`, and `c`'s unit
  stacked as a fraction), a rule, and the values. Only the defines are stored: the header, unit
  row, rule and value rows are the display, drawn from the defines.
- A table is a presentation of its defines. Prime keeps no separate table data in the sample
  (`result.xml` has no table value: each member has its own `resultRef`).
- Position and size are on the table region (`actualWidth`, `actualHeight`, `top`, `left`), as for
  other regions.
- **Not in sample:** whether a table can hold anything but defines, what `resultFormat` inside a
  table member means (none is present), and whether `spec-table` carries attributes beyond the
  region's own.

Mathcraft: `RegionKind::Table` holds one member per `<math>`, each converted or kept with the
reason. Evaluation runs each member like a define region, in document order, and joins it with its
own `resultRef`. The reader is permissive about attributes. The writer writes the members from the
model, with `resultRef` as read, and copies the `spec-table` attributes it does not model.

### What Prime stored

`result.xml` has `<resultData result-id="N" calculation-status="Synchronized" />` for ids 0 to 6,
with no child element. Read as the golden harness does, the derivative fails with an undefined name
(`EngineErrorNoRef`), the integral, summation and limit are `Unsupported`, and the three table members
are `NoReference`. No region reaches `Pass`.

### Open questions

Each of these needs one Prime test in the shareholder's install:

1. The element name and operands of a product.
2. How a one-sided limit is stored (an attribute on `ml:limit`, or an extra operand).
3. The stored form of an nth derivative (`ml:degree` with a number) and of an integral with an
   empty bound.
4. Whether a value appears in `result.xml` for a calculus region whose body is defined (this file
   has only undefined names).
5. Whether a table can hold non-define members.
