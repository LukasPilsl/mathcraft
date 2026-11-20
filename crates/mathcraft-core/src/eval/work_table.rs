//! The cost classes of every builtin name (the table behind [`super::work::cost_of`]).
//!
//! Calibration (release build, measured 2026-11-16 on this machine). The unit is about 10 ns, so a
//! class with `k` units per element should take about `10 k` ns per element. The reviewer's
//! measurements are given as real nanoseconds per charged unit `r`, so the exact class is
//! `units = r * current_units / 10`. Each class below is that value rounded up, or a stated safety
//! margin where the measurement is noisy. Where the measurement disagreed with the old class by
//! more than 3x, the class was changed; the ratios are the reviewer's numbers, and the table gives
//! the new class.
//!
//! - Bessel `J0`, `Y1` (r 9-10) and `FresnelC` (r 9.5): unchanged, already exact.
//! - Gamma (r 0.18), erf (r 0.56), normal tails `pnorm` (r 1.5) and `qnorm` (r 3.0): were 200
//!   units, now 4, 12, 30 and 60. Reviewer's proposed "about 40" for all of them was not used:
//!   under the 10 ns convention it charged `pnorm` 7x too little.
//! - Elliptic `K` (r 0.17) and `E` (r 0.06): were 100 and 300, now 2 each.
//! - Psi (r 0.84): was 20, now 2.
//! - Mean and the other linear statistics (r 3.7): were 3, now 2.
//! - Sort and median (r 1.0 and 1.1): were 10, now 3 (the reviewer's target; a comparison sort is
//!   n log n, which this per-element class does not model).
//! - Elementary functions (sin r 18-23): were 1, now 3 (the reviewer's target, see `work.rs`).
//! - Element-wise arithmetic (add r 15-50): 3 units per element (see `work::binary_units`).
//! - Shape and copy functions: 3 units per element of input, which is also the output
//!   (augment, stack, reverse). `identity` and `diag` add their output size (see `work::output_units`).
//! - Dense elimination (det, rref, rank, r 0.7): `rows * cols * min / 21`, so one unit is about
//!   14 flops of LU. The inverse (r 0.65): `max^3 / 8`.
//! - Matrix multiply: 11 units per scalar multiply-add (r 110 ns), see `work::MATRIX_MULT_UNITS`.
//! - Lambert W (LOOPED, 100): not measured, unchanged. Ibeta (ITERATIVE, 200): not measured,
//!   unchanged. Orthogonal polynomials and the binomial and Poisson loops: not measured, unchanged.
//! - Distributions (not measured, estimated from their loops): log densities 20 units per element;
//!   cumulative functions through the incomplete gamma or beta (ITERATIVE, 200); quantiles found by
//!   bisection on those (SEARCH, 10000, about 60 to 120 evaluations); closed forms 3 units.
//! - Random generators: 20 units per draw (see `work::Cost::Draws` for the loop-driven ones).
//!
//! Release probe on this machine (2026-11-16), real ns per charged unit (exact is 10): matrix product
//! 9.9, det 500 7.3, det 1000 7.1, inverse 12.8, element-wise add 9.4, scalar power 10.0, sin 6.2,
//! gamma 10.7, psi 11.2, mean 6.5, elliptic K 12.1, elliptic E 13.2, identity 2.9 (3.4x pessimistic),
//! copies 1.0 and ranges 1.0 (10x pessimistic: the 3 units per element of the task). Erf measured 1.7
//! on inputs in [0.001, 1]: 6x pessimistic, kept at 12 for the reviewer's slower inputs. The normal
//! tails were not measured by this probe (its call had the wrong arity), and `sort` needs a column
//! vector to be measured.
//! - Eigen functions and `geninv` (not measured): `eigenvals`, `genvals` 4 n^3 complex multiply-adds;
//!   `eigenvec` n^3; `eigenvecs`, `genvecs` n^4 (see `work::Dense`). `geninv` is charged as least
//!   squares with an m x m right-hand side (see `work::builtin_units`).
//! - `Cholesky` (single-matrix and pivoted forms, n^3/3 flops) is charged as elimination
//!   (`Dense::Elimination`, about 2.3x the work by the formula, so an over-charge).
//! - `LU` is charged as elimination; `QR` as `Dense::Householder` (about 2 m n k + 2 m^2 k flops,
//!   with `k = min(m, n)`, for the reflections and the accumulated `Q`); `svds`, `cond2`, `norm2` as
//!   `Dense::Jacobi` (`4 k^2 m`) and `svd` as `Dense::JacobiVectors` (`5 k^2 m`, the same sweeps with
//!   `V` accumulated). The Jacobi classes were measured in release on random square matrices (svds
//!   0.23, 0.98, 2.29 s at k = 200, 300, 400; svd 0.25, 1.21, 2.44 s), not the one-sweep charge of
//!   the old class (2 k^2 m) that under-charged by about 1.8x. `QR` is not measured.
//!   `last` and `submatrix` were stubs until ORIGIN reached builtins; they are real now and classed
//!   as `SHAPE` (a count or a copy, 3 units per element). `csort` and `rsort` are real and classed
//!   as `LINE_SORT`.

use super::builtins::special::special_bessel::{MAX_ARG, MAX_FRESNEL_ARG};
use super::work::{Cost, Dense};

/// Medium: the default for a name missing from [`TABLE`].
pub(super) const MEDIUM: Cost = Cost::PerElement(20);
/// Matrix shape, copy and stubs: 3 units per input element (the output is no larger).
pub(super) const SHAPE: Cost = Cost::PerElement(3);
/// Matrix norms: one pass over the elements.
const NORM: Cost = Cost::PerElement(3);
/// Matrix sort: a comparison sort, charged per element (see the calibration note).
const MATRIX_SORT: Cost = Cost::PerElement(3);
/// Sorting the lines of a matrix by one key line (`csort`, `rsort`): 3 units per element per
/// comparison pass, with `ceil(log2 n)` passes (see `work::sort_passes`). Not measured.
const LINE_SORT: Cost = Cost::PerElementLog(3);
/// Elementary arithmetic, trigonometry, exp, rounding and the closed-form distribution functions.
const CHEAP: Cost = Cost::PerElement(3);
/// One pass over the data: means, variances, uniform and exponential distributions.
const LINEAR: Cost = Cost::PerElement(2);
/// Sorting-based statistics (n log n), charged per element.
const SORTING: Cost = Cost::PerElement(3);
/// Psi, the digamma function: a short series.
const PSI: Cost = Cost::PerElement(2);
/// Gamma: a Lanczos-type evaluation.
const GAMMA: Cost = Cost::PerElement(4);
/// Erf and erfc: a convergence loop, typical case about 12 units.
const ERF: Cost = Cost::PerElement(12);
/// Normal tail (`pnorm`, `cnorm`): a convergence loop, measured about 30 units.
const NORMAL_TAIL: Cost = Cost::PerElement(30);
/// Normal quantile (`qnorm`): measured about 60 units.
const NORMAL_QUANTILE: Cost = Cost::PerElement(60);
/// Kernels with convergence loops and no measurement yet (incomplete beta and gamma). Typical-case
/// figure: a pathological argument can run longer than its charge.
const ITERATIVE: Cost = Cost::PerElement(200);
/// Lambert W: a Newton loop of up to 100 steps. Not measured.
const LOOPED: Cost = Cost::PerElement(100);
/// Complete elliptic integral K (AGM): measured about 2 units.
const ELLIPTIC_K: Cost = Cost::PerElement(2);
/// Incomplete elliptic integrals E and F: measured about 2 units for E.
const ELLIPTIC: Cost = Cost::PerElement(2);
/// Bessel, Airy and Kelvin-type kernels (quadrature whose panel count grows with `|x|`).
const BESSEL: Cost = Cost::Oscillatory {
    base: 300,
    limit: MAX_ARG,
};
/// Fresnel integrals (quadrature whose panel count grows with `x^2`).
const FRESNEL: Cost = Cost::Quadratic {
    base: 200,
    limit: MAX_FRESNEL_ARG,
};
/// Orthogonal polynomials and the binomial and Poisson CDFs, which loop over their first argument.
const LOOP_ON_FIRST: Cost = Cost::Scaled { base: 20, arg: 0 };
/// Quantiles of the binomial and Poisson distributions, which loop over their second argument.
const LOOP_ON_SECOND: Cost = Cost::Scaled { base: 20, arg: 1 };
/// Log densities of the gamma, beta, t, chi-squared, F, log normal and negative binomial families:
/// a few log gamma calls per element. Not measured.
const DENSITY: Cost = Cost::PerElement(20);
/// Quantiles found by bracketing and bisection on a cumulative function: about 60 to 120
/// evaluations of an incomplete gamma or beta, estimated at about 1 us each. Not measured.
const SEARCH: Cost = Cost::PerElement(10_000);
/// Hypergeometric sums and quantiles: up to `n` terms (argument 3), each with three log gamma
/// calls. Measured about 100 ns per unit (ten times the 10 ns convention): `rhypergeom(1000, 500,
/// 500, 500)` ran 2 s on the old charge, `qhypergeom` 3.7 s.
const HYPERGEOMETRIC_SUM: Cost = Cost::Scaled { base: 400, arg: 3 };
/// A random draw from a closed form or a short series: 20 units per draw (the first argument).
const DRAW: Cost = Cost::Scaled { base: 20, arg: 0 };
/// A binomial or Poisson draw by inversion: a quantile search over the parameter in argument 1
/// (`n` or `lambda`) for each of the `m` draws. Measured about 45 ns per unit at `lambda = 1e10`
/// (`rpois(10, 1e10)` took 10 s on 2.2e8 units), so the base is 100 (about 4.5 units per old unit).
const DRAW_INVERSION: Cost = Cost::Draws { base: 100, arg: 1 };
/// A negative binomial draw: a gamma draw and a Poisson inversion at the mean `n (1 - p) / p`
/// (see `work::Cost::DrawsNegBinomial`). Same per-unit cost as `DRAW_INVERSION`.
const DRAW_NEGATIVE_BINOMIAL: Cost = Cost::DrawsNegBinomial { base: 100 };
/// A hypergeometric draw: a sum over up to `n` terms (argument 3) for each of the `m` draws.
/// Measured about 100 ns per unit (see `HYPERGEOMETRIC_SUM`).
const DRAW_HYPERGEOMETRIC: Cost = Cost::Draws { base: 400, arg: 3 };
/// Finance closed forms (`pv`, `fv`, `pmt`, `ipmt`, `nper`, `crate`, `nom`, `eff`, `fvc`, `npv`, ...): a
/// power and a few multiplications. Not measured.
const FINANCE_CLOSED: Cost = Cost::PerElement(10);
/// `rate` and `irr`: at most 20 Newton steps (see `finance`), each a few powers per flow. Not measured.
const FINANCE_SOLVER: Cost = Cost::PerElement(400);
/// `cumint` and `cumprn`: one interest term per period up to `end` (argument 4), about 20 units each.
const FINANCE_LOOP: Cost = Cost::Scaled { base: 20, arg: 4 };
/// String functions: a character scan or copy. The string's size is charged by `work::elements` (one
/// element per 64 bytes), so a 1 MiB string costs about 16 K elements. Not measured.
const STRING: Cost = Cost::PerElement(3);
/// The time functions: one clock read or one parse. Not measured.
const TIME: Cost = Cost::PerElement(3);
/// Spline construction (`cspline`, `lspline`, `pspline`): one tridiagonal solve, about 20 units per
/// point. Not measured.
const SPLINE_BUILD: Cost = Cost::PerElement(20);
/// Binary-search lookups (`linterp`, `interp`, `hist`, `histogram`): about 3 units per element per
/// comparison pass. Not measured.
const SEARCH_LOOKUP: Cost = Cost::PerElementLog(3);
/// Discrete Fourier transforms (`dft` and the deprecated `fft` family): about `n log n` butterflies,
/// 10 units per element per pass. Lengths that are not powers of 2 use Bluestein's method, which is
/// about six times slower (measured: `dft` of 2^20 values took 91 ms, of 2^20 + 1 took 540 ms), so
/// `work::builtin_units` charges them six times this class.
const FFT_CLASS: Cost = Cost::PerElementLog(10);
/// Burg prediction (`predict`): an order-`m` recursion over the data, 3 units per element and order.
/// Not measured. This class ignores the steps argument, so `work::builtin_units` charges `predict`
/// by its own formula, which includes the steps.
const BURG: Cost = Cost::Scaled { base: 3, arg: 1 };
/// Windowed median (`medsmooth`): a sort per window, 10 units per element and window point. Not
/// measured.
const WINDOW_MEDIAN: Cost = Cost::Scaled { base: 10, arg: 1 };
/// All-pairs Gaussian kernel (`ksmooth`): one exponential per pair, 20 units. Not measured.
const PAIRS_KERNEL: Cost = Cost::Pairwise(20);
/// Running-lines smoothing (`supsmooth`): three smooths, a residual smooth each, a choice and a
/// final smooth, each linear in the data: about 60 units per element. Not measured.
const RUNNING_SUPER: Cost = Cost::PerElement(60);
/// Full convolution (`convolve`): one multiply-add per pair, 10 units. Not measured.
const PAIRS_CONVOLVE: Cost = Cost::Pairwise(10);
/// Image 3 x 3 kernels and window statistics (builtins/image_tier2): about 30 units per pixel; the
/// loops inside charge the window sizes themselves. Not measured.
const IMAGE_KERNEL: Cost = Cost::PerElement(30);
/// Image compass finders: eight 3 x 3 kernels per pixel, about 80 units. Not measured.
const IMAGE_COMPASS: Cost = Cost::PerElement(80);
/// Image passes (maps, histograms, copies, transforms): a few units per element; the transforms and
/// the geometric maps add their own loop charges. Not measured.
const IMAGE_PASS: Cost = Cost::PerElement(3);
/// Binary morphology and shape functions: the structuring element and the hull or source points are
/// charged inside; this is the per-element base. Not measured.
const IMAGE_MORPH: Cost = Cost::PerElement(9);

/// Every builtin name with its class. Each name appears exactly once (a test checks this).
pub(super) const TABLE: &[(&[&str], Cost)] = &[
    (
        &[
            "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "exp", "ln",
            "log", "sqrt", "abs", "re", "im", "arg", "ln0", "floor", "ceil", "trunc", "round",
            "Floor", "Ceil", "Trunc", "Round", "sign", "min", "max",
        ],
        CHEAP,
    ),
    (
        &[
            "csc", "sec", "cot", "csch", "sech", "coth", "asec", "acsc", "acot", "asinh", "acosh",
            "atanh", "acsch", "asech", "acoth", "sinc", "Φ", "δ",
        ],
        CHEAP,
    ),
    (
        &["DAi", "DBi", "ellipticPi", "js", "ys", "bei", "ber"],
        CHEAP,
    ),
    (
        &[
            "mean", "gmean", "hmean", "var", "Var", "stdev", "Stdev", "skew", "kurt", "corr",
            "dnorm", "dunif", "punif", "qunif", "dexp", "pexp", "qexp",
        ],
        LINEAR,
    ),
    // Integer functions (builtins/integer_misc.rs). `gcd` and `lcm` run Euclid's loop per value
    // (about 40 steps for 64-bit values, so 20 units); `permut` and `combin` loop at most about 1100
    // times before they overflow (see their docs), so their class is per element, not per `k`.
    (&["if", "mod", "Re", "Im"], CHEAP),
    (&["gcd", "lcm"], Cost::PerElement(20)),
    (&["permut", "combin"], Cost::PerElement(3)),
    (&["median", "mode", "percentile", "cvar"], SORTING),
    (&["Psi"], PSI),
    (&["lnΓ", "dbinom", "dpois"], MEDIUM),
    (&["Γ"], GAMMA),
    (&["erf", "erfc"], ERF),
    (&["ibeta"], ITERATIVE),
    (&["pnorm", "cnorm"], NORMAL_TAIL),
    (&["qnorm"], NORMAL_QUANTILE),
    (&["LambertW"], LOOPED),
    // The integrals charge their own Gauss-Legendre panels; this is the per-element base.
    (&["Si", "Ci", "Shi", "Chi", "Ei", "li", "Li"], LOOPED),
    (&["atan2"], CHEAP),
    // Coordinate mappings and the double factorial (builtins/coords_symbolic).
    (
        &[
            "angle", "xy2pol", "pol2xy", "xyz2sph", "sph2xyz", "xyz2cyl", "cyl2xyz", "fact2",
        ],
        CHEAP,
    ),
    // Closed forms and series (the polylog series charges its own terms).
    (&["beta", "Zeta", "dilog", "polylog"], LOOPED),
    (&["horzflip", "vertflip"], SHAPE),
    (&["ellipticK"], ELLIPTIC_K),
    (&["ellipticE", "ellipticF"], ELLIPTIC),
    (
        &[
            "J0", "J1", "Jn", "Y0", "Y1", "Yn", "I0", "I1", "In", "K0", "K1", "Kn", "H1", "H2",
            "Ai", "Bi",
        ],
        BESSEL,
    ),
    (&["FresnelC", "FresnelS"], FRESNEL),
    (
        &[
            "Her", "Lag", "Leg", "Tcheb", "Ucheb", "Jac", "pbinom", "ppois",
        ],
        LOOP_ON_FIRST,
    ),
    (&["qbinom", "qpois"], LOOP_ON_SECOND),
    (
        &[
            "rows", "cols", "length", "identity", "diag", "augment", "stack", "reverse", "tr",
            "cross", "matrix", "vec",
        ],
        SHAPE,
    ),
    (&["last", "submatrix"], SHAPE),
    (&["sort"], MATRIX_SORT),
    (&["csort", "rsort"], LINE_SORT),
    (&["norm", "norm1", "norme", "normi"], NORM),
    (
        &["det", "rref", "rank", "Cholesky", "LU"],
        Cost::Dense(Dense::Elimination),
    ),
    (&["cond1", "condi", "conde"], Cost::Dense(Dense::Inverse)),
    (&["lsolve", "geninv"], Cost::Dense(Dense::LeastSquares)),
    (&["eigenvals", "genvals"], Cost::Dense(Dense::Eigenvalues)),
    (&["eigenvec"], Cost::Dense(Dense::EigenvectorOne)),
    (&["eigenvecs", "genvecs"], Cost::Dense(Dense::Eigenvectors)),
    (&["svds", "cond2", "norm2"], Cost::Dense(Dense::Jacobi)),
    (&["svd"], Cost::Dense(Dense::JacobiVectors)),
    (&["QR"], Cost::Dense(Dense::Householder)),
    // Continuous distributions (distributions_cont.rs).
    (
        &[
            "dgamma",
            "dbeta",
            "dt",
            "dchisq",
            "dF",
            "dlnorm",
            "dhypergeom",
            "dnbinom",
        ],
        DENSITY,
    ),
    (
        &["pgamma", "pbeta", "pt", "pchisq", "pF", "pnbinom"],
        ITERATIVE,
    ),
    (
        &["qgamma", "qbeta", "qt", "qchisq", "qF", "qnbinom"],
        SEARCH,
    ),
    (&["plnorm"], NORMAL_TAIL),
    (&["qlnorm"], NORMAL_QUANTILE),
    (
        &[
            "dcauchy", "pcauchy", "qcauchy", "dweibull", "pweibull", "qweibull", "dlogis",
            "plogis", "qlogis",
        ],
        CHEAP,
    ),
    // Discrete distributions (distributions_disc.rs).
    (&["dgeom", "pgeom", "qgeom"], CHEAP),
    (&["phypergeom", "qhypergeom"], HYPERGEOMETRIC_SUM),
    // Random numbers (random.rs).
    (&["rnd"], CHEAP),
    (
        &[
            "runif", "rnorm", "rexp", "rcauchy", "rlogis", "rlnorm", "rweibull", "rgamma",
            "rchisq", "rbeta", "rF", "rt", "rgeom",
        ],
        DRAW,
    ),
    (&["rbinom", "rpois"], DRAW_INVERSION),
    (&["rnbinom"], DRAW_NEGATIVE_BINOMIAL),
    (&["rhypergeom"], DRAW_HYPERGEOMETRIC),
    // Data analysis (data_interp, data_regress, data_smooth, data_hist).
    (&["linterp", "interp", "hist", "histogram"], SEARCH_LOOKUP),
    (&["cspline", "lspline", "pspline"], SPLINE_BUILD),
    (&["predict"], BURG),
    (&["line", "slope", "intercept", "stderr"], LINEAR),
    (&["medsmooth"], WINDOW_MEDIAN),
    (&["ksmooth"], PAIRS_KERNEL),
    (&["movavg", "expsmooth"], Cost::PerElement(3)),
    (&["supsmooth"], RUNNING_SUPER),
    (&["medfit"], Cost::PerElementLog(3)),
    (&["convolve", "deconvolve"], PAIRS_CONVOLVE),
    // Thielecoeff builds the table of inverse differences (n^2 pairs); Thiele is one pass per x.
    (&["Thielecoeff"], PAIRS_CONVOLVE),
    (&["Thiele"], Cost::PerElement(3)),
    // Outlier tests (data_outlier): one sort-free pass, plus one t quantile for the Grubbs tests.
    (&["Grubbs", "GrubbsClassic"], Cost::PerElement(20)),
    (&["ThreeSigma", "trim"], Cost::PerElement(3)),
    // Finance (finance.rs).
    (
        &[
            "fv", "fvadj", "fvc", "pv", "npv", "pmt", "ppmt", "ipmt", "nper", "cnper", "crate",
            "nom", "eff", "mirr",
        ],
        FINANCE_CLOSED,
    ),
    (&["rate", "irr"], FINANCE_SOLVER),
    (&["cumint", "cumprn"], FINANCE_LOOP),
    // Strings (strings.rs).
    (
        &[
            "concat", "substr", "search", "strlen", "num2str", "format", "str2num", "str2vec",
            "vec2str", "error",
        ],
        STRING,
    ),
    // Files (builtins/readwrite.rs): the reader and writer charge each line and cell themselves,
    // so the call's own charge is per argument element.
    (
        &["READCSV", "READTEXT", "WRITECSV", "WRITETEXT"],
        Cost::PerElement(1),
    ),
    // Design of experiments (builtins/doe.rs): the design is charged by its cells inside the call.
    (&["fullfact", "boxwilson", "doelabel"], SHAPE),
    // The polynomial fit (builtins/doe_fit.rs) charges its QR itself; the call pays per element read.
    (&["polyfit"], Cost::PerElement(2)),
    // Factor screening (builtins/doe_screen.rs): the design and the group means are charged inside.
    (&["quickscreen", "effects"], Cost::PerElement(2)),
    // Image (builtins/image.rs): colour conversions and intensity maps, a few units per pixel.
    (
        &[
            "rgb_to_hsv",
            "rgb_to_hls",
            "hsv_to_rgb",
            "hls_to_rgb",
            "invertimage",
            "clip",
            "scale",
        ],
        CHEAP,
    ),
    // Image tier 2 (builtins/image_tier2): each loop charges its own work; these classes are the
    // per-element base. A 3 x 3 kernel is about 30 units per pixel, the eight compass kernels 80.
    (
        &[
            "sobel",
            "prewitt",
            "roberts",
            "freichen",
            "centsmooth",
            "getnoise",
            "quantfilt",
            "wiener2d",
        ],
        IMAGE_KERNEL,
    ),
    (
        &["kirsch", "robinson3", "robinson5", "compgrad"],
        IMAGE_COMPASS,
    ),
    (
        &[
            "rowgrad",
            "colgrad",
            "threshold",
            "binarize",
            "equalize",
            "levelmap",
            "imhist",
            "imhist2",
            "hist2d",
            "translate",
            "zoom",
            "rotate",
            "putregion",
            "extract",
            "replace",
            "subcolor",
        ],
        IMAGE_PASS,
    ),
    (
        &[
            "erode",
            "dilate",
            "open",
            "close",
            "gray_erode",
            "gray_dilate",
            "gray_open",
            "gray_close",
            "thin",
            "skeleton",
            "cnvxhull",
            "distform",
        ],
        IMAGE_MORPH,
    ),
    (&["dct", "idct", "center", "matconv"], IMAGE_PASS),
    // Time (datetime.rs).
    (&["time", "hhmmss"], TIME),
    // Signal windows, FIR design, filtering and phase (builtins/signal_tier2.rs). Windows are a cosine
    // or exponential per sample (about 5 units); Kaiser sums a Bessel series per sample (20); the
    // Dolph-Chebyshev window is a double loop (Squared); design, filter and gain loops charge one
    // multiply-add per tap (10, or 20 for forward-backward filtering). The length argument is the
    // one that sets the loop count.
    (
        &[
            "blackman",
            "hamming",
            "hanning",
            "nuttall",
            "triangular",
            "taprect",
            "gaussian",
            "costaper",
        ],
        Cost::Scaled { base: 5, arg: 0 },
    ),
    (&["kaiser"], Cost::Scaled { base: 20, arg: 0 }),
    (&["cheby"], Cost::Squared { base: 5 }),
    (&["lowpass", "highpass"], Cost::Scaled { base: 10, arg: 1 }),
    (&["bandpass", "bandstop"], Cost::Scaled { base: 10, arg: 2 }),
    // IIR prototypes (signal_iir): a few terms per section, sections up to order 200.
    (&["butter", "cheby1"], Cost::Squared { base: 10 }),
    (&["iirlow", "iirhigh"], Cost::PerElement(20)),
    (&["response", "fftfilt", "gain"], Cost::Pairwise(10)),
    (&["filtfilt"], Cost::Pairwise(20)),
    (&["detrend", "quantize", "phasecor"], Cost::PerElement(6)),
    (&["phase"], Cost::PerElement(3)),
    // Discrete Fourier transforms (signal_fft).
    (
        &[
            "dft", "idft", "dftr", "idftr", "cfft", "icfft", "CFFT", "ICFFT", "fft", "ifft", "FFT",
            "IFFT", "dht", "idht", "hilbert",
        ],
        FFT_CLASS,
    ),
    // Polynomial roots (root): Durand-Kerner, about a term per pair of coefficients per iteration.
    (&["polyroots"], Cost::Pairwise(200)),
    // Fixed-form least-squares fits (fit_nonlin, fit_linear): Levenberg-Marquardt on the data, up to
    // 200 iterations of about n+1 model evaluations per point.
    (
        &["expfit", "logfit", "lgsfit", "pwrfit", "sinfit"],
        Cost::PerElement(800),
    ),
    (&["lnfit"], Cost::PerElement(20)),
    // Function-argument builtins (special_forms): each evaluation of the function argument is charged
    // by the form itself, so the table entry adds nothing.
    (
        &[
            "root",
            "genfit",
            "linfit",
            "rkfixed",
            "Rkadapt",
            "Stiffr",
            "Stiffb",
            "Bulstoer",
            "Adams",
            "BDF",
            "Radau",
            "AdamsBDF",
            "sbval",
            "bvalfit",
            "maximize",
            "minimize",
            "IsFunction",
            "funmap",
            "funconv",
            "funcdeconv",
            "numol",
            "statespace",
        ],
        Cost::PerElement(0),
    ),
    // Grid solvers (builtins/pde_grid): each sweep is charged inside the solver.
    (&["relax", "multigrid"], Cost::PerElement(0)),
    // A pdesolve solution is read by `pdesolve_interp`, charged per element of its arguments.
    (&["pdesolve_interp"], Cost::PerElement(3)),
    // Lookups and matches (matrix_extra): one comparison per element of the searched array.
    (
        &["lookup", "hlookup", "vlookup", "vhlookup", "match"],
        Cost::PerElement(3),
    ),
    // Ranking and ordering (matrix_extra): a comparison sort of the vector.
    (&["Rank", "order"], Cost::PerElementLog(3)),
    // Hypergeometric series (builtins/special_hyper): each term is charged inside the call.
    (&["fhyper", "mhyper"], Cost::PerElement(0)),
    // Kronecker product: one multiplication per output element (see `Pairwise`).
    (&["kronecker"], Cost::Pairwise(3)),
    // Log-spaced vectors (matrix_extra): a power per point; the points are charged by `output_units`.
    (&["logpts", "logspace"], Cost::PerElement(10)),
    // Second data tier (builtins/data_tier2.rs). `polycoeff`, `polyiter`, `localmax`, `localmin` and
    // `VSmooth` are charged by their loops in `work::builtin_units`; the entries here are the names.
    (&["conj", "nthRoot", "IsNaN", "DMS", "FIF"], CHEAP),
    (&["polyint"], Cost::Pairwise(3)),
    (
        &["polycoeff", "polyiter", "localmax", "localmin", "VSmooth"],
        SHAPE,
    ),
    (&["contingtbl"], Cost::PerElement(3)),
    // Second misc tier (builtins/misc_tier2): the type tests and sign functions are closed forms;
    // the DoE helpers charge their designs by cells inside the call; the file functions charge
    // each line, sample or row inside the call (the entries here are the per-argument-element base).
    (
        &[
            "IsArray",
            "IsScalar",
            "IsString",
            "SIUnitsOf",
            "csgn",
            "signum",
        ],
        CHEAP,
    ),
    (
        &[
            "fractfact",
            "fractruns",
            "fractresol",
            "foldover",
            "fractalias",
        ],
        SHAPE,
    ),
    (
        &["READPRN", "WRITEPRN", "READFILE", "WRITEFILE"],
        Cost::PerElement(1),
    ),
    (
        &[
            "READWAV",
            "WRITEWAV",
            "GETWAVINFO",
            "READBMP",
            "WRITEBMP",
            "READRGB",
            "WRITERGB",
        ],
        Cost::PerElement(1),
    ),
];
