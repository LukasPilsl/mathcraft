# Symbolic evaluation options for Mathcraft

Status: research report, 2026-11-16. Basis: sample files in `documentation/examples/*.mcdx`
(read directly), the local Prime help in `documentation/mathcad-prime-12-help/md/`, crates.io and
GitHub metadata fetched on 2026-11-16, and two throwaway probes (outside the repo, deleted).
Nothing in the engine was changed.

## 0. Recommendation (short)

1. **Do not adopt Symbolica.** Its licence (Symbolica Source-Available License 1.0) requires every
   person who runs it to hold a runtime licence unless they qualify for free use. That is incompatible
   with shipping Mathcraft binaries as open source.
2. **Build the MVP core ourselves on `num-rational` / `num-bigint` (MIT/Apache, the most used exact
   arithmetic in Rust).** It covers the sample results that matter most: exact rationals, linear
   systems solved into a vector of names, `float n`, and units kept as symbols. The probe solved all
   four linear-system checks exactly in 47 lines.
3. **Evaluate `symcas` (MIT/Apache-2.0, pure Rust, 0.1.1) as an optional expression kernel** for
   `expand`, `factor`, `diff`, `taylor`, `simplify`, behind our own trait. It is young (0.1.x, one
   release line) and has no `solve`, so it does not replace the solver. Decide after a 1-task spike.
4. **Postpone** SymEngine (C++ build, Rust wrapper unmaintained since 2020) and embedded SymPy
   (BSD-3, broadest coverage, but a Python runtime per OS). SymPy is the fallback for calculus
   (integrals, limits, transforms) if the shareholder accepts a bundled Python.
5. **Licence decides the GPL options.** Algebraeon (GPL-3), Giac (GPLv3), Maxima (GPL-2) and GiNaC
   (GPL) are only usable if Mathcraft's release licence is GPL-compatible. This is the first open
   question (section 6).

Effort: about 15-18 Haiku-sized tasks to cover the 38 harness-unsupported regions with the own-core
plan, or about 13-15 with a `symcas` adapter (section 5).

## Verified facts and corrections (fact-check 2026-11-16)

- **Coverage by stage** (38 unsupported regions): own exact-arithmetic core with linear solves
  ~18/38 (47%: 16 linear define-solves + 2 standalone solves); + rational/expand/power/`y(x) ->`
  ~25 (66%); + factor/assume/root/ln ~31 (82%). Units (6 Mechanik_3 regions) and the
  symbolic-parameter solve (2_rata r14) are the last 7.
- **Effort**: the 15-18 Haiku-task estimate is plausible up to ~31 regions and optimistic beyond.
- **Staging**: the staged recommendation stands. The own num-rational core is licence-neutral, so
  it can start before the licence decision.
- **Semantics**: unknown names stay symbolic (2_rata r14 keeps `C`). Standalone `y(x) ->`
  (3_rata r4) substitutes the solved a, b, c, while the definition form `y(x):=y(x) ->`
  (3_rata r3) stores `a*x^2+b*x+c` unsubstituted. The symbolic layer must match Prime per form.
- **Current state**: `SymEval` is refused at `crates/mathcraft-core/src/eval/expr_eval.rs:169`.
- **SHAREHOLDER DECISION** (neutral; consequences as in section 6):
  1. Release licence stance: permissive-only (MIT/Apache/BSD), GPL-compatible, or undecided.
     Permissive-only excludes GPL candidates. GPL-compatible admits them, with the combining rule
     in section 6. LGPL dependencies are usable with notice and relinking obligations.
  2. Embedded Python (SymPy): accept a bundled runtime (tens of MB per OS) for calculus features,
     or not. If not, Phase 2 is own tables only.

## 1. Sample symbolic regions (what Prime stores)

Source: every `ml:symEval` in the 24 `documentation/examples/*.mcdx` worksheets, extracted to
scratch dirs and parsed with a Python ElementTree script. Input text is the MathML rendered to
infix; the authoritative input text per region is the one `mathcraft-cli inspect` prints for the
same file (checked for `1_rata_1`: `(3+4)/(5^2-1) ->`, `(a+b)^2 -> expand`, `a^2+2 (ab)+b^2 -> factor`,
`(1/(x+2)) (2/(x+3)) -> factor`, `root(3+2*sqrt(4), 5) ->`).

Totals: **91 `ml:symEval` regions in 13 files** that carry a symbolic evaluation (the other
samples contain none). By command:

| Command kind | Count | Examples (input -> Prime stored result) |
|---|---|---|
| plain `->` (no keyword) | 55 | `(3+4)/(5^2-1) ->` gives `7/24`; `x^5*(6 x^3)/(x^(1/2) x^(5/3)) ->` gives `6*x^(35/6)`; `root(5, 3+2*sqrt(4)) ->` gives `7^(1/5)`; `sqrt(a^2)+sqrt(b^2) ->` with assume `a,b>0` gives `b + a` |
| `solve` (one variable, or vector of names) | 25 | `7x+3x=5, solve x` gives `1/2`; `[[solve,x],y]` on two lines gives `matrix{11/5;12/5}`; 3-unknown systems give exact `matrix{-(3/2);2;-(3/2)}` |
| `expand` | 4 | `(a+b)^2 -> expand` gives `a^2 + 2ab + b^2`; cube difference expands to `-45 c^4 d^2 + 7 c^6 + 75 c^2 d^4` (up to ordering) |
| `factor` | 3 | `a^2+2 scale(ab)+b^2 -> factor` gives `(a+b)^2` (ordering as stored); `a^2-b^2 -> factor` gives `-((b-a)(b+a))`; `(1/(x+2))(2/(x+3)) -> factor` gives `2/((x+2)(x+3))` |
| `assume` (ALL>0) | 1 | `sqrt(a^2)+sqrt(b^2)` gives `b+a` only under `assume, ALL>0` |
| `float n` | 3 float-only (15_rata r10-12) + 4 chained after solve (11 r7, 8 r6, 15 r13, 15 r17) | `float,3` on `(... x ...)` gives 3 significant figures (`0.0416`, `2.17`, `-1.85`); further `float` commands sit on non-symEval numeric regions in 11, 15 and 8 (not analysed) |

Shape of the 55 plain `->` regions: 48 are definitions `name(x):=expr ->` (the harness classes them
NoReference, not Unsupported); 7 are standalone (1_rata r0-r4 constant expressions; 3_rata r4 and
4_rata r6 `y(x) ->`). Most of the 48 are `x`-linear functions of a worksheet function
(`id([x, A, B])`, i.e. a line through two points, see appendix). The blocker "evaluate the definition
with `x` free" applies to about 2 of the 55 regions, not to the group as a whole.

Units: results keep unit symbols multiplied in (`833.56525 * N`, `625.1739375 * m * N`,
`((-625.1739375 * N) * m)`). Prime treats units as symbols in symbolic output. The `symcas` probe confirmed
it also keeps them as symbols (`625.1739375*N*m`). The help page `about_units_constants_in_symbolics`
explains the None/Unit/Constant labelling; only the label-free case matters for the samples.

Numbers: Prime writes symbolic rationals exactly (`7/24`, `-(21/2)`), and floats with decimals
(`833.56525`, `0.4705882352941176...`) when a `float` or numeric input is involved. Prime's
`float n` is significant figures, not decimal places.

Harness link: `crates/mathcraft-cli/tests/common/baseline.rs` counts 35 `symbolic evaluation`
and 3 `symbolic solve into a vector of names` entries as *unsupported*, i.e. the 38. Reconciliation: the 38 Unsupported symbolic regions are exactly 16 standalone symEvals + 22
`define`-with-solve; per sample: 1_rata 9, 2_rata 7, 3_rata 2, 4_rata 2, 10_rata 1, 11_rata 3,
15_rata 2, 16_rata 2, 7_rata 2, 8_rata 1, 9_rata 1, Mechanik_3 6 = 38. The symbolic samples also
have 22 EngineError + 16 EngineErrorNoRef regions, all `undefined name` chains downstream of the 22
solve-defines (UMP 8, C' 6, I 5, B' 4, SP 3, A 3, alpha 2, HSP 2, a, b). Solving the 22 define-solves
is therefore worth about 76 regions, not 38.

Appendix A (section 8) lists all 91 regions verbatim.

Note on the `id` element: in the MathML `ml:id` is used both for names and for function calls. In
the samples it carries `ln(...)` (kept unevaluated by Prime: `ln(7/3)/4 - 2/7` stays
`ln(7/3)/4 - 2/7`), `expand`/`factor`/`solve`/`float` keywords and user functions.

## 2. Prime's symbolic keywords (help: `about_keywords.md`)

Source: the keyword table in `documentation/mathcad-prime-12-help/md/about_keywords.md`. The
help also lists the operators (`->`, derivative `d/dx`, integral, `limit`) and the Symbolics
pages (about 34 in INDEX.md). Essential-for-MVP is judged against the samples and the core
Mathcad workflow.

| Keyword | Purpose (help) | Modifiers | Essential for MVP? |
|---|---|---|---|
| `solve` | Solve an equation symbolically; vector-of-names form `[[solve,x],y]` | real, fully, eval, generated, assume, using | **Yes** (25 regions) |
| `expand` | Multiply out powers and products | fully, using | **Yes** (4) |
| `factor` | Factor an expression | domain, real, complex, fully, using | **Yes** (3) |
| `simplify` / plain `->` | Algebraic simplification and evaluation | fully, using, max | **Yes** (55; basic rational + power rules) |
| `float` | Results reduced to a precision (significant figures) | fully, using | **Yes** (3) |
| `assume` | Assumptions on variables (ALL>0, real, integer, ...) | ALL, real, complex, even, odd, integer, RealRange | **Yes, minimal** (ALL>0 for sqrt(x^2)) |
| units as symbols | Units and constants keep their labels in output | - | **Yes** (Mechanik samples) |
| `substitute` | Replace a variable, function or expression | raw, fully, using | Phase 2 |
| `collect` | Collect like powers of a variable | fully, using | Phase 2 |
| `coeffs` | Coefficients of a polynomial | degree, fully, using | Phase 2 |
| `series` | Taylor/Laurent/Puiseux expansion around 0 | using | Phase 2 |
| derivative `d/dx` | Symbolic derivative (operator, help "To Evaluate Derivatives Symbolically") | - | Phase 2 |
| integral `int` | Symbolic integral (operator) | cauchy, standard, fully | Phase 2 (hard; needs tables or external CAS) |
| `limit` (operator) | Symbolic limit | - | Phase 2 |
| `parfrac` | Partial fractions | complex, domain, fully, using | Phase 2 |
| `confrac` | Continued fraction | fraction, matrix, fully, using | Phase 3 |
| `combine` | Combine using exp/log/trig identities | exp, log, sincos, atan, ... | Phase 3 |
| `rewrite` | Rewrite in terms of elementary functions | many (sin, exp, ln, ...) | Phase 3 |
| `rectangular` | Real/imaginary parts of complex results | fully, using | Phase 3 |
| `fully` | Step-by-step solution | max | Phase 3 (large output) |
| `explicit` | Substitute variable values without reducing | ALL, fully, using | Phase 3 |
| `decomp` | Decompose into coefficients, powers, generated variables | coeff, power, func, max, parts, expr | Phase 3 |
| `standard` | Integration constant / big-O format | - | Phase 3 |
| `using` | Replace generated variables in a solution | - | Phase 3 |
| `fourier`, `laplace`, `ztrans` and inverses | Transforms | nfact, ofact, fully, using, raw | Postpone (out of scope) |
| symbolic ODE via `solve` + `generated`; `odesolve` is numeric (solve blocks, help ODE_function.md) | Symbolic ODE solving | generated | Postpone |

Engine note from the help (`differences_between_legacy_and_new_engines.md`): Prime 7+ uses only
the "new" symbolic engine, which orders output canonically (`-b+a` shown as `a-b`), and returns
rearranged results. Exact output ordering therefore differs by design from any CAS, so tests
should compare values or canonical forms, not strings.

## 3. Candidate comparison

Licence claims are marked V (verified in this session from the cited URL) or U (unverified,
from memory or a secondary source). Downloads and dates are from crates.io or GitHub at
2026-11-16.

| # | Candidate | Licence | Maintenance (2026-11-16) | Rust / build | Coverage vs samples | Exactness | Verdict |
|---|---|---|---|---|---|---|---|
| 1 | **Symbolica 3.0.2** | Symbolica Source-Available License 1.0 (V). Not open source. Running needs free-use terms or a paid runtime licence per recipient; redistribution does not grant runtime rights (V). License.md s.3 allows distribution but grants no runtime rights (V). symbolica.io/license (V): free single-core academic/non-commercial use; hobbyist use only if not employed or affiliated; startups by agreement; standard EUR 3000/yr per developer machine. Section 4 forbids using it as blueprint for a "Competing Product". | crates.io updated 2026-10-30, 48k total downloads; GitHub pushed 2026-11-16, 1011 stars, 32 open issues (V) | Pure Rust plus optional native code generation (feature); MPFR/GMP backends selectable; Rust 1.96 (V) | Broad (CAS, pattern matching) | Exact | **Reject for an open-source release.** Acceptable only for internal research or with a paid licence |
| 2 | **SymEngine (C++)** | LICENSE file in repo reads MIT (V); bundles other licences (BSD variants, GPL with Bison exception) (V). Task brief assumed BSD. | C++ repo pushed 2026-11-13, 1430 stars, 250 open issues (V) | Needs C++ build and GMP. Rust wrappers: `symengine.rs` 0.2.2 (BSD-3 per crates.io, V), last release 2020-05-19, docs.rs build failed (V): **unmaintained**. `symengine.py` MIT (V) | Good core (diff, expand, series, solve for linear/polynomial, limited) | Exact rationals, symbolic constants | Postpone. Packaging cost high for 3 OS; Rust wrapper too old |
| 3 | **SymPy (embedded Python)** | BSD-3 (V) | Pushed 2026-11-15, 15.1k stars (V). Very active | Via PyO3 or subprocess. Python runtime plus SymPy shipped per OS (size: unmeasured here, expect tens of MB) | Broadest: solve (systems, polynomials), simplify, integrate, limit, series, dsolve, units module | Exact (symbolic constants pi, sqrt) | **Best coverage fallback for calculus.** Adopt only if shareholder accepts a bundled Python runtime. Startup and packaging cost not measured |
| 4 | **Maxima** | GPL-2 (Debian copyright and maxima list, U/secondary). Current COPYING not checked. | Active (sourceforge). Not measured here | Common Lisp runtime (SBCL/CLISP), subprocess | Good: solve, integrate, limit, ODE | Exact | Only if GPL-compatible release; Lisp runtime to ship |
| 5 | **GiNaC** | GPL (official site, V via search); version unclear (GPL-2+ per user post, U) | Not measured | C++; needs CLN and GMP; no solve for systems | Good for expand, diff, series, pattern | Exact | Only if GPL-compatible; heavy C++ dependency |
| 6 | **FLINT / Arb / Calcium** | LGPL-3.0 (GitHub SPDX) | Rust crate flint-sys 0.9.0, MIT/Apache, updated 2026-04-26, 47k downloads (V). Wrapper "Bindings to the FLINT C library" | C library; wrapper needs C toolchain and GMP/MPFR (U) | Only numeric and polynomial building blocks (factorization over Z/Q); no expression layer | Exact over Z, Q | Building block for factorization later. LGPL linking rules apply |
| 7 | **Malachite** | LGPL-3.0-only (V) | 0.14.0, updated 2026-10-13, 11.9M downloads (V) | Pure Rust arbitrary-precision integers and rationals | Numeric layer only | Exact | Usable as a numeric backend, but LGPL-3 static linking adds obligations. Prefer num-rational |
| 8 | **num-rational / num-bigint** | MIT/Apache-2.0 (V) | num-rational 0.4.2, 470M downloads (V) | Pure Rust | Numeric layer; used by probe 2 | Exact | **Adopt for the MVP numeric core** |
| 9 | **symcas 0.1.1** | MIT OR Apache-2.0 (V) | updated 2026-09-22, 42.6k downloads (V). Single project, young (V: 0.1.x). Repository link github.com/cislunarspace/cas returns 404 (checked 2026-11-16); crates.io MIT OR Apache-2.0, 0.1.1 created/updated 2026-09-22, 42,618 downloads, ~8k lines over cas-* crates (expand, cancel, factor, diff, taylor, simplify, subst); no solve/integrate/limit | Pure Rust (sub-crates `cas-domain`, `cas-expr`, `cas-parse`, `cas-poly`, `cas-print`); no C | Expand, factor complete for 1-2 variables, partial for >= 3 (cas-expr lib.rs:226), diff, Taylor series, cancel, subst, simplify (incl. trig), eval_rational, monomial_coeffs. **No solve, no integrate, no limit** | Exact rationals; floats as f64; units as symbols (probe) | **Best pure-Rust expression kernel candidate.** Spike before adopting |
| 10 | **Algebraeon 0.0.17** | GPL-3.0-only (V) | updated 2026-03-06, 9k downloads (V), pure Rust | Pure Rust; polynomial factoring, number theory, algebra | Algebra focus, not expression simplification | Exact | Only if GPL-3-compatible |
| 11 | **Giac / Xcas** | GPLv3 (V via packager pages, author offers dual licence on request); needs GMP (LGPL) | Active (2026 backup page) | C++ | Strong: solve, integrate, limit, series | Exact | Only if GPL-3-compatible; big C++ build |
| 12 | **Nerdamer (JS)** | Apache-2.0 (V, GitHub API) | pushed 2026-11-04, 564 stars (V) | JavaScript; would need an embedded JS engine | Good symbolic algebra in JS | Exact | Reject. Embedding a JS engine in a Rust desktop app costs more than it saves |
| 13 | **Mathics** | GPL (U, from memory) | Not measured | Python | Mathematica-like | Exact | Not evaluated further |
| 14 | **Own CAS (minimal subset)** | Ours | - | num-rational + our Expr | See section 5 | Exact | Recommended for MVP |
| 15 | **mathcore 0.3.1** | MIT (V) | Crate updated 2025-08-30, 4.8k downloads (V) | Claims diff, integrate, solve (crate description); quality unchecked | Unknown | Unknown | Not evaluated; check before any reliance |

Other crates surveyed: `symbolic-expressions` 5.0.3 (MIT/Apache, last update 2017, parser/writer
only; not a CAS); `cas` 0.0.1 (a content-addressable storage tool, not a CAS); `algebraeon`
(above); `symcas` (above). `rsymbolic`, `alge`, `calcify` were not checked (time
budget). Treat them as unevaluated. Stale crates: `symbolic-math` (2024) and `rusymbols` (2021) were not evaluated; `Tertius` (GitHub only, Van Hoeij factoring, licence unchecked).

Output-format and unit handling in brief:

- **Exactness**: symcas and Symbolica keep rationals exact (`7/24`) and symbolic constants as
  symbols. SymEngine/SymPy keep `sqrt(2)`, `pi` exact. num-rational alone is exact only for
  rationals (no surds): the sample set needs no surds except `7^(1/5)` and `sqrt` of perfect
  squares, which are rational-power cases.
- **Output format**: Prime's `-(21/2)`, `(1/2)*m*v^2`, `x - 1/6*x^3` are a printing question. Each
  CAS prints differently (symcas `1/2*m*v^2`, `-45*c^4*d^2 + ...`). Mapping the candidate tree
  to Mathcraft `Expr`: the `Expr` enum has Number, Id, Unary, Binary, Call, NthRoot, Parens,
  Matrix, Index, Range and Define (`crates/mathcraft-core/src/expr.rs` lines 135+). The pure-math
  subset (Number, Id, Unary, Binary, Call, NthRoot, Parens) maps directly. Matrix and Index map
  only for vector results. Estimated mapping cost: about 300 lines for a two-way converter plus
  tests (estimate, not built).
- **Units**: all candidates treat `m`, `N`, `s` as ordinary symbols unless told otherwise. The
  symcas probe confirmed it. Mathcraft's unit handling (`unit_system=si` in `inspect`) stays in our
  code: units are not a CAS feature.

## 4. Probe results

Probes live in the scratch dir (`cas_probe/symcas_probe`, `cas_probe/core_probe`), built with
`CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_INCREMENTAL=0`. Both target dirs were deleted
(32 MB and 17 MB during build). Cold compile times were not measured precisely; warm rebuilds
took under 1 s.

**Probe 1: symcas 0.1.1** (87 lines, `symcas_probe/src/main.rs`; the Gauss solver is 30 lines on
top of `monomial_coeffs` + `eval_rational`):

| Check (sample, region) | Prime stored | symcas output | Match |
|---|---|---|---|
| `(3+4)/(5^2-1)` simplify (1_rata_1 r0) | `7/24` | `7/24` | Yes |
| `x^5*6x^3/(x^(1/2)*x^(5/3))` (1_rata_1 r3) | `6*x^(35/6)` | `6*x^(35/6)` | Yes |
| `(a+b)^2` expand (1_rata_1 r10) | `a^2+2ab+b^2` | `a^2 + b^2 + 2*a*b` | Value yes, order differs |
| `a^2-b^2` factor (2_rata_2 r3) | `-((b-a)(b+a))` | `-(a + b)*(b - a)` | Value yes, order differs |
| cube-difference expand (1_rata_1 r12) | expanded polynomial | `-45*c^4*d^2 + 7*c^6 + 75*c^2*d^4` | Value yes |
| `2/((x+2)(x+3))` factor (1_rata_1 r13) | `2/((x+2)(x+3))` | `2*(2 + x)^-1*(3 + x)^-1` | **No**: denominator printed as negative powers; needs a printer rule |
| `diff sin x` | `cos(x)` | `cos(x)` | Yes |
| `833.56525*N`, `625.1739375*m*N` (Mechanik) | unit symbols kept | kept as symbols (`N*m` order) | Yes |
| `7x+3x=5`, solve x (2_rata_2 r0) | `1/2` | `1/2` (Gauss on coefficients) | Yes |
| 3-point parabola a,b,c (3_rata_3 r2) | `3/2, -21/2, 16` | `3/2, -21/2, 16` | Yes |
| x,y,z system (2_rata_2 r15) | `-3/2, 2, -3/2` | `-3/2, 2, -3/2` | Yes |
| x,y system (2_rata_2 r16) | `11/5, 12/5` | `11/5, 12/5` | Yes |
| `F` from `C = 5/9 (F-32)` (2_rata_2 r14) | `(9C+160)/5` | `-9/5*(160/9 + C)` | Equal but **not normalized**: needs an expand/collect pass |

Score: 4 linear systems and the rational/power/diff checks exact; 2 printing differences; 1 needs
normalization. `solve` is absent from symcas, so the solver is ours. Limitation: the probe did
not test `sqrt(a^2)` under `assume`, `ln`, transforms or user functions.

**Probe 2: hand-rolled exact solver on num-rational 0.4** (47 lines,
`core_probe/src/main.rs`): `(3+4)/(5^2-1) = 7/24`; `7x+3x=5 -> 1/2`; the 3-point parabola
gives `3/2, -21/2, 16`; the x,y,z system gives `-3/2, 2, -3/2`. All four linear checks match
Prime exactly. It does not handle symbolic coefficients, units, expansion, factoring, powers
with rational exponents or surds; those are the cost driver in section 5.

**Not probed** (budget): symbolica (licence rules it out for release anyway), SymEngine (C++
build, unmaintained wrapper), SymPy (Python runtime, size and startup unmeasured), Giac, Maxima,
GiNaC, Algebraeon (GPL). Their claims above are from metadata, docs or memory and are marked.

## 5. Build-vs-adopt estimate

Effort unit: one Haiku-sized task = one scoped PR (per CLAUDE.md worker rules). Sonnet-sized
tasks are marked S.

**Plan A: own core on num-rational (no CAS dependency)**

| Task | Scope | Haiku tasks |
|---|---|---|
| A1 | Mathcraft `Expr` to sparse polynomial over Q (with fallback to "unsupported") | 2 |
| A2 | Linear systems into vector of names (`solve, x, y`), harness regions | 2 |
| A3 | `float n` (significant figures) on symbolic results | 1 |
| A4 | `expand` and plain `->` simplify for rational expressions and power rules | 3 |
| A5 | `factor` over Q (difference of squares, rational roots, quadratics) | 3 |
| A6 | `assume, ALL>0` for `sqrt(x^2) -> x` | 1 |
| A7 | Unit symbols kept as symbols in symbolic output | 1 |
| A8 | Symbolic evaluation of worksheet user functions with free `x` (16 standalone + 22 solve-define regions) | 3 (S: 1) |
| A9 | Golden-harness wiring and tests per sample | 2 |
| | **Total** | **about 18 (S: 1 more)** |

**Plan B: same plus a `symcas` adapter** (replace A4 and A5 with an adapter; A8 still ours):
adapter 2, spike 1, A4/A5 replaced by 2 adapter tasks, other tasks unchanged: **about 13-15**.

**Plan C: calculus via embedded SymPy** (derivative, integral, limit, series): add 8-12 tasks plus
packaging work (Python runtime per OS, CI matrix, size budget): **+4 for packaging, +8 for
features**. Only if the shareholder accepts a bundled Python.

Known risks: symcas 0.1.x API churn; the `id` function-call semantics (A8) depend on how the
worksheet importer exposes user functions; output-ordering differences need a
canonical-form comparison in tests (section 2 note).

## 6. Open questions for the shareholder

1. **Licence stance.** Is the Mathcraft release GPL-compatible, permissive (MIT/Apache/BSD), or
   undecided? This decides the GPL candidates (Algebraeon, Giac, Maxima, GiNaC) and whether an
   LGPL dependency (malachite, FLINT) is acceptable. Without legal advice, the consequences as we
   understand them: a permissive release can depend on MIT/Apache/BSD code and combining GPL code
   in one binary requires GPL-compatible terms for the whole work; counsel should confirm; a GPL-3 release can link GPL-3 code but not GPL-2-only code (Maxima);
   LGPL code can be used but static linking needs notice and relinking obligations; counsel should confirm.
2. **Embedded Python.** Is a bundled Python runtime (size tens of MB per OS, PyO3 or subprocess)
   acceptable for calculus features? If not, Phase 2 is own tables only.
3. **Scope.** Is the MVP target the 38 harness-unsupported regions only, or full Prime symbolic
   parity (calculus, transforms, ODE)? The first is about 15-18 tasks; the second is open-ended.
4. **Presentation.** Should symbolic output match Prime's ordering byte-for-byte? The help
   confirms Prime's ordering is engine-specific, so we recommend value-level tests.
5. **symcas.** Accept a young MIT/Apache dependency behind an adapter, or own everything?

## 7. Sources

Retrieved 2026-11-16 unless stated.

- Local: `documentation/examples/*.mcdx` (24 files, extracted to scratch); `documentation/mathcad-prime-12-help/md/INDEX.md`, `about_keywords.md`, `about_symbolic_calculation.md`, `about_units_constants_in_symbolics.md`, `differences_between_legacy_and_new_engines.md`; `crates/mathcraft-cli/tests/common/baseline.rs` (38 count), `crates/mathcraft-core/src/expr.rs`.
- crates.io API: https://crates.io/api/v1/crates/symbolica ; /symengine ; /symcas ; /malachite ; /algebraeon ; /num-rational ; /flint-sys ; /symbolic-expressions ; /cas
- GitHub API: https://api.github.com/repos/symbolica-dev/symbolica ; /symengine/symengine ; /symengine/symengine.py ; /sympy/sympy ; /jiggzson/nerdamer
- Licence text: https://github.com/symbolica-dev/symbolica/blob/main/License.md ; https://raw.githubusercontent.com/symengine/symengine/master/LICENSE ; https://raw.githubusercontent.com/sympy/sympy/master/LICENSE
- Symbolica docs: https://symbolica.io/docs/get_started.html
- symcas docs and README: https://docs.rs/symcas/0.1.1/symcas/ ; https://github.com/cislunarspace/cas
- flint-rs: https://github.com/alex-ozdemir/flint-rs
- Search results (secondary, U): GiNaC licence (https://www.ginac.de/ ; https://lists.ginac.de/); Maxima GPL-2 (https://maxima.sourceforge.io/ext/list_archives/2007/msg03687.html ; Debian copyright https://metadata.ftp-master.debian.org/changelogs/main/m/maxima/stable_copyright); Giac GPLv3 (https://xcas.univ-grenoble-alpes.fr/backup/giac.html ; https://alternativeto.net/software/giac-xcas/about)

Licence and status notes (2026-11-16):

- symcas repo link 404 (checked 2026-11-16)
- SymEngine bundled licences unverified
- Algebraeon GPL-3.0, Malachite LGPL-3.0-only, Calcium LGPL-2.1, Nerdamer Apache-2.0 (verified)
- GiNaC GPL-2.0-or-later, Maxima GPLv2 ("or later" disputed), Giac GPL-3+ (secondary sources only)

Uncertainties (honest list):
- GiNaC version, Maxima GPL-2-only, Mathics GPL, SymEngine's exact licence text
  beyond the fetched LICENSE, and "1 of 3 OS packaging" costs are from memory or secondary
  sources, not verified against upstream.
- SymPy/Python bundle size and startup: not measured.
- `symcas` correctness beyond the 13 checks above: not tested; 0.1.x maturity is a judgement.
- The 91-region parse is from XML; inputs for `id(x, A, B)` user functions were not traced
  to their definitions (they depend on worksheet definitions).
- The 3 `float` symEval regions are confirmed; 7 further `float` commands on non-symEval
  regions were not analysed.
- Help images (`documentation/mathcad-prime-12-help/en/.../images`) were not opened; expected
  results come from the sample XML `symResult` nodes and the help text.

## 8. Appendix A: all 91 symbolic regions (verbatim from XML)

Format: `sample | region | input (MathML rendered to infix) | command | Prime symResult`.
`(arrow ->)` means no keyword. `id(...)` is a function call (ln, user function, or keyword).
Matrix results are column vectors.

```text
10_rata_18_UMP_Dreieck | r4 | IN: id([x, A, B]) | CMD: (arrow ->) | RES: symResult{((-((2 * ((3 * x) / 2))) - 5) / 2)}
10_rata_18_UMP_Dreieck | r5 | IN: id([x, B, C]) | CMD: (arrow ->) | RES: symResult{(((2 * (x / 2)) - 1) / 2)}
10_rata_18_UMP_Dreieck | r6 | IN: id([x, A, C]) | CMD: (arrow ->) | RES: symResult{((-((2 * (x / 6))) + 3) / 2)}
10_rata_18_UMP_Dreieck | r8 | IN: id([x, A, B, ((A + B) / 2)]) | CMD: (arrow ->) | RES: symResult{(((2 * x) / 3) + (11 / 6))}
10_rata_18_UMP_Dreieck | r9 | IN: id([x, B, C, ((B + C) / 2)]) | CMD: (arrow ->) | RES: symResult{(-((2 * x)) + 2)}
10_rata_18_UMP_Dreieck | r10 | IN: id([x, A, C, ((A + C) / 2)]) | CMD: (arrow ->) | RES: symResult{((3 / 2) + (6 * x))}
10_rata_18_UMP_Dreieck | r11 | IN: matrix{(y = id(x)); (y = id(x))} | CMD: [[solve, x], y] | RES: symResult{matrix{(1 / 16); (15 / 8)}}
11_rata_AA_eulersche_Gerade | r5 | IN: id([x, B, ((A + C) / 2)]) | CMD: (arrow ->) | RES: symResult{((-0.47058823529411764706 * x) + 2.3235294117647058824)}
11_rata_AA_eulersche_Gerade | r6 | IN: id([x, A, ((B + C) / 2)]) | CMD: (arrow ->) | RES: symResult{((0.038461538461538461538 * x) + 2.1538461538461538462)}
11_rata_AA_eulersche_Gerade | r7 | IN: matrix{(y = id(x)); (y = id(x))} | CMD: [[solve, x], y] | RES: symResult{matrix{0.333; 2.17}}
11_rata_AA_eulersche_Gerade | r10 | IN: id([x, B, C]) | CMD: (arrow ->) | RES: symResult{((-0.78571428571428571429 * x) + 4.2142857142857142857)}
11_rata_AA_eulersche_Gerade | r11 | IN: id([x, A, C]) | CMD: (arrow ->) | RES: symResult{(x + 6)}
11_rata_AA_eulersche_Gerade | r12 | IN: id([x, A, B]) | CMD: (arrow ->) | RES: symResult{((-0.25 * x) + 1.0)}
11_rata_AA_eulersche_Gerade | r14 | IN: id([x, A, B, C]) | CMD: (arrow ->) | RES: symResult{((4.0 * x) + 9.0)}
11_rata_AA_eulersche_Gerade | r15 | IN: id([x, B, C, A]) | CMD: (arrow ->) | RES: symResult{((1.2727272727272727273 * x) + 7.0909090909090909091)}
11_rata_AA_eulersche_Gerade | r16 | IN: id([x, C, A, B]) | CMD: (arrow ->) | RES: symResult{((-1.0 * x) + 5.5)}
11_rata_AA_eulersche_Gerade | r17 | IN: matrix{(y = id(x)); (y = id(x))} | CMD: [[solve, x], y] | RES: symResult{matrix{-0.7; 6.2}}
11_rata_AA_eulersche_Gerade | r20 | IN: id([x, A, B, ((A + B) / 2)]) | CMD: (arrow ->) | RES: symResult{((4.0 * x) - 3.25)}
11_rata_AA_eulersche_Gerade | r21 | IN: id([x, B, C, ((B + C) / 2)]) | CMD: (arrow ->) | RES: symResult{((1.2727272727272727273 * x) - 0.93181818181818181818)}
11_rata_AA_eulersche_Gerade | r22 | IN: id([x, A, C, ((A + C) / 2)]) | CMD: (arrow ->) | RES: symResult{(-x + 1)}
11_rata_AA_eulersche_Gerade | r23 | IN: matrix{(y = id(x)); (y = id(x))} | CMD: [[solve, x], y] | RES: symResult{matrix{0.850000000000000000009; 0.150000000000000000037}}
15_rata_Inkreismittelpunkt | r4 | IN: id([x, B, C]) | CMD: (arrow ->) | RES: symResult{((5 * x) - 5)}
15_rata_Inkreismittelpunkt | r5 | IN: id([x, A, C]) | CMD: (arrow ->) | RES: symResult{((x / 2) + 4)}
15_rata_Inkreismittelpunkt | r6 | IN: id([x, A, B]) | CMD: (arrow ->) | RES: symResult{((-((5 * ((2 * x) / 5))) + 2) / 5)}
15_rata_Inkreismittelpunkt | r10 | IN: id([x, A, ((A + c0) + b0)]) | CMD: [float, 3] | RES: symResult{((0.0416 * x) + 2.17)}
15_rata_Inkreismittelpunkt | r11 | IN: id([x, B, ((B + c0) - a0)]) | CMD: [float, 3] | RES: symResult{((-1.85 * x) + 1.85)}
15_rata_Inkreismittelpunkt | r12 | IN: id([x, C, ((C - a0) - b0)]) | CMD: [float, 3] | RES: symResult{((1.31 * x) + 2.38)}
15_rata_Inkreismittelpunkt | r13 | IN: matrix{(y = id(x)); (y = id(x))} | CMD: [[solve, x], y] | RES: symResult{matrix{-0.1692; 2.163}}
15_rata_Inkreismittelpunkt | r16 | IN: id([x, A, B, I]) | CMD: (arrow ->) | RES: symResult{((2.5 * x) + 2.586)}
15_rata_Inkreismittelpunkt | r17 | IN: matrix{(y = id(x)); (y = id(x))} | CMD: [[solve, x], y] | RES: symResult{matrix{-0.754; 0.702}}
16_rata_Dreieck_flaeche_winkel | r5 | IN: id([x, A, B]) | CMD: (arrow ->) | RES: symResult{((x / 2) + 1)}
16_rata_Dreieck_flaeche_winkel | r6 | IN: id([x, A, C]) | CMD: (arrow ->) | RES: symResult{((2 * x) + 7)}
16_rata_Dreieck_flaeche_winkel | r7 | IN: id([x, B, C]) | CMD: expand | RES: symResult{(-((2 * x) / 5) + (23 / 5))}
16_rata_Dreieck_flaeche_winkel | r14 | IN: id([x, A, B, C]) | CMD: (arrow ->) | RES: symResult{(-((2 * x)) + 3)}
16_rata_Dreieck_flaeche_winkel | r15 | IN: matrix{(y = id(x)); (y = id(x))} | CMD: [[solve, x], y] | RES: symResult{matrix{(4 / 5); (7 / 5)}}
16_rata_Dreieck_flaeche_winkel | r19 | IN: id([x, A, C, B]) | CMD: (arrow ->) | RES: symResult{(-(x / 2) + 5)}
16_rata_Dreieck_flaeche_winkel | r20 | IN: matrix{(y = id(x)); (y = id(x))} | CMD: [[solve, x], y] | RES: symResult{matrix{-(4 / 5); (27 / 5)}}
1_rata_1 | r0 | IN: ((3 + 4) / ((5 ^ 2) - 1)) | CMD: (arrow ->) | RES: symResult{(7 / 24)}
1_rata_1 | r1 | IN: (((3 / 4) + (1 / 3)) - (1 / 6)) | CMD: (arrow ->) | RES: symResult{(11 / 12)}
1_rata_1 | r2 | IN: ((id((2 + (1 / 3))) / 4) - (2 / 7)) | CMD: (arrow ->) | RES: symResult{((id((7 / 3)) / 4) - (2 / 7))}
1_rata_1 | r3 | IN: (((x ^ 5) * scale(6, (x ^ 3))) / ((x ^ (1 / 2)) * (x ^ (5 / 3)))) | CMD: (arrow ->) | RES: symResult{(6 * (x ^ (35 / 6)))}
1_rata_1 | r4 | IN: root(5,(3 + (2 * root(<ph>,4)))) | CMD: (arrow ->) | RES: symResult{(7 ^ (1 / 5))}
1_rata_1 | r10 | IN: (((a + b)) ^ 2) | CMD: expand | RES: symResult{(((b ^ 2) + ((2 * a) * b)) + (a ^ 2))}
1_rata_1 | r11 | IN: (((a ^ 2) + scale(2, ab)) + (b ^ 2)) | CMD: factor | RES: symResult{(((b ^ 2) + (2 * ab)) + (a ^ 2))}
1_rata_1 | r12 | IN: ((((scale(2, (c ^ 2)) - scale(5, (d ^ 2)))) ^ 3) - ((((c ^ 2) - scale(5, (d ^ 2)))) ^ 3)) | CMD: expand | RES: symResult{((((75 * (c ^ 2)) * (d ^ 4)) - ((45 * (c ^ 4)) * (d ^ 2))) + (7 * (c ^ 6)))}
1_rata_1 | r13 | IN: scale((1 / (x + 2)), (2 / (x + 3))) | CMD: factor | RES: symResult{(2 / (((x + 2)) * ((x + 3))))}
2_rata_2 | r0 | IN: ((scale(7, x) + scale(3, x)) = 5) | CMD: [solve, x] | RES: symResult{(1 / 2)}
2_rata_2 | r3 | IN: (((a ^ 2) - (b ^ 2))) | CMD: factor | RES: symResult{-((((b - a)) * ((b + a))))}
2_rata_2 | r4 | IN: (root(<ph>,(a ^ 2)) + root(<ph>,(b ^ 2))) | CMD: [assume, (ALL > 0)] | RES: symResult{(b + a)}
2_rata_2 | r14 | IN: (C = ((5 / 9) * ((F - 32)))) | CMD: [solve, F] | RES: symResult{(((9 * C) + 160) / 5)}
2_rata_2 | r15 | IN: matrix{(((x + scale(2, y)) - z) = 4); ((x + z) = -3); ((y - scale(2, z)) = 5)} | CMD: [[[solve, x], y], z] | RES: symResult{matrix{-(3 / 2); 2; -(3 / 2)}}
2_rata_2 | r16 | IN: matrix{((a + scale(2, b)) = 7); ((scale(2, a) - b) = 2)} | CMD: [[solve, a], b] | RES: symResult{matrix{(11 / 5); (12 / 5)}}
2_rata_2 | r19 | IN: matrix{((c + scale(2, d)) = 7); ((c - d) = 2)} | CMD: [[solve, c], d] | RES: symResult{matrix{(11 / 3); (5 / 3)}}
3_rata_3_Kurve_durch_3_Punkte | r2 | IN: matrix{(id(2) = 1); (id(4) = -2); (id(5) = 1)} | CMD: [[[solve, a], b], c] | RES: symResult{matrix{(3 / 2); -(21 / 2); 16}}
3_rata_3_Kurve_durch_3_Punkte | r3 | IN: id(x) | CMD: (arrow ->) | RES: symResult{(((a * (x ^ 2)) + (b * x)) + c)}
3_rata_3_Kurve_durch_3_Punkte | r4 | IN: id(x) | CMD: (arrow ->) | RES: symResult{(((3 * (x ^ 2)) / 2) + ((16 - ((21 * x) / 2))))}
4_rata_3_Kurve_durch_3_Punkte_V2 | r4 | IN: matrix{(id(indexer(P, 0)) = indexer(P, 1)); (id(indexer(Q, 0)) = indexer(Q, 1)); (id(indexer(R, 0)) = indexer(R, 1))} | CMD: [[[solve, a], b], c] | RES: symResult{matrix{(3 / 2); -(21 / 2); 16}}
4_rata_3_Kurve_durch_3_Punkte_V2 | r5 | IN: id(x) | CMD: (arrow ->) | RES: symResult{(((a * (x ^ 2)) + (b * x)) + c)}
4_rata_3_Kurve_durch_3_Punkte_V2 | r6 | IN: id(x) | CMD: (arrow ->) | RES: symResult{(((3 * (x ^ 2)) / 2) + ((16 - ((21 * x) / 2))))}
6_rata_18_gerade_durch_2_punkte | r4 | IN: id([x, A, B]) | CMD: (arrow ->) | RES: symResult{((x / 2) + 4)}
6_rata_18_gerade_durch_2_punkte | r5 | IN: id([x, A, C]) | CMD: (arrow ->) | RES: symResult{(-((2 * x) / 5) + (2 / 5))}
6_rata_18_gerade_durch_2_punkte | r6 | IN: id([x, B, C]) | CMD: (arrow ->) | RES: symResult{((5 * x) - 5)}
6_rata_18_gerade_durch_2_punkte | r12 | IN: id([x, A, B]) | CMD: (arrow ->) | RES: symResult{((-((3 * (x / 3))) - 4) / 3)}
6_rata_18_gerade_durch_2_punkte | r13 | IN: id([x, A, C]) | CMD: (arrow ->) | RES: symResult{(((7 * (x / 7)) + 24) / 7)}
6_rata_18_gerade_durch_2_punkte | r14 | IN: id([x, B, C]) | CMD: (arrow ->) | RES: symResult{(-((2 * x)) + 12)}
7_rata_18_Dreieck | r5 | IN: id([x, A, B]) | CMD: (arrow ->) | RES: symResult{((x / 2) + 1)}
7_rata_18_Dreieck | r6 | IN: id([x, A, C]) | CMD: (arrow ->) | RES: symResult{((2 * x) + 7)}
7_rata_18_Dreieck | r7 | IN: id([x, B, C]) | CMD: expand | RES: symResult{(-((2 * x) / 5) + (23 / 5))}
7_rata_18_Dreieck | r14 | IN: id([x, A, B, C]) | CMD: (arrow ->) | RES: symResult{(-((2 * x)) + 3)}
7_rata_18_Dreieck | r15 | IN: matrix{(y = id(x)); (y = id(x))} | CMD: [[solve, x], y] | RES: symResult{matrix{(4 / 5); (7 / 5)}}
7_rata_18_Dreieck | r19 | IN: id([x, A, C, B]) | CMD: (arrow ->) | RES: symResult{(-(x / 2) + 5)}
7_rata_18_Dreieck | r20 | IN: matrix{(y = id(x)); (y = id(x))} | CMD: [[solve, x], y] | RES: symResult{matrix{-(4 / 5); (27 / 5)}}
8_rata_18_Schwerpunkt_Dreieck | r4 | IN: id([x, B, ((A + C) / 2)]) | CMD: (arrow ->) | RES: symResult{(-((19 * x) / 4) + 14)}
8_rata_18_Schwerpunkt_Dreieck | r5 | IN: id([x, A, ((B + C) / 2)]) | CMD: (arrow ->) | RES: symResult{((-((7 * (x / 7))) + 12) / 7)}
8_rata_18_Schwerpunkt_Dreieck | r6 | IN: matrix{(y = id(x)); (y = id(x))} | CMD: [[solve, x], y] | RES: symResult{matrix{2.67; 1.33}}
8_rata_18_Schwerpunkt_Dreieck | r8 | IN: id([x, B, C]) | CMD: (arrow ->) | RES: symResult{((6 * x) - 29)}
8_rata_18_Schwerpunkt_Dreieck | r9 | IN: id([x, A, C]) | CMD: (arrow ->) | RES: symResult{(((4 * ((5 * x) / 8)) + 13) / 4)}
8_rata_18_Schwerpunkt_Dreieck | r10 | IN: id([x, A, B]) | CMD: (arrow ->) | RES: symResult{((-((3 * ((7 * x) / 6))) - 1) / 3)}
9_rata_18_HSP_Dreieck | r4 | IN: id([x, A, B]) | CMD: (arrow ->) | RES: symResult{((-((2 * ((3 * x) / 2))) - 5) / 2)}
9_rata_18_HSP_Dreieck | r5 | IN: id([x, B, C]) | CMD: (arrow ->) | RES: symResult{(((2 * (x / 2)) - 1) / 2)}
9_rata_18_HSP_Dreieck | r6 | IN: id([x, A, C]) | CMD: (arrow ->) | RES: symResult{((-((2 * (x / 6))) + 3) / 2)}
9_rata_18_HSP_Dreieck | r8 | IN: id([x, A, B, C]) | CMD: (arrow ->) | RES: symResult{(((2 * x) / 3) - 1)}
9_rata_18_HSP_Dreieck | r9 | IN: id([x, B, C, A]) | CMD: (arrow ->) | RES: symResult{(-((2 * x)) - 4)}
9_rata_18_HSP_Dreieck | r10 | IN: id([x, C, A, B]) | CMD: (arrow ->) | RES: symResult{((6 * x) + 5)}
9_rata_18_HSP_Dreieck | r11 | IN: matrix{(y = id(x)); (y = id(x))} | CMD: [[solve, x], y] | RES: symResult{matrix{-(9 / 8); -(7 / 4)}}
Mechanik_3_Spannungsberechnung | r8 | IN: (0 = Ax) | CMD: [solve, Ax] | RES: symResult{0}
Mechanik_3_Spannungsberechnung | r9 | IN: (0 = (Ay - FG)) | CMD: [solve, Ay] | RES: symResult{(833.56525 * N)}
Mechanik_3_Spannungsberechnung | r11 | IN: (0 = (MA - (FG * L1))) | CMD: [solve, MA] | RES: symResult{((625.1739375 * m) * N)}
Mechanik_3_Spannungsberechnung | r14 | IN: (0 = (Ax + N1)) | CMD: [solve, N1] | RES: symResult{0}
Mechanik_3_Spannungsberechnung | r16 | IN: (0 = (Ay + Q1)) | CMD: [solve, Q1] | RES: symResult{(-833.56525 * N)}
Mechanik_3_Spannungsberechnung | r18 | IN: (0 = ((MA + M1) - (Ay * s1))) | CMD: [solve, M1] | RES: symResult{((-625.1739375 * N) * m)}
```
