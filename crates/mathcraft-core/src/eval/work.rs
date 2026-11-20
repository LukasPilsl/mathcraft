//! Work budget: a deterministic bound on the arithmetic one top-level statement may do.
//!
//! `MAX_CALLS` counts user-function calls, but a builtin does all its work inside one call: `J0`
//! over a 1000 x 1000 matrix is one call and seconds of quadrature. So every builtin call, every
//! operator, every matrix copy and every solver step is charged, in abstract units, before it runs.
//! A charge is computed from shapes and argument magnitudes only, never from timing, so the same
//! statement always gets the same decision. The budget is per top-level evaluation (a worksheet
//! statement), and it is shared by everything that statement runs: nested user functions, the
//! guesses and the constraint closures of a solve block (one budget per block).
//!
//! Unit: about 10 ns in a release build. The cost classes (see `work_table`) were fitted to kernel
//! times measured on this machine:
//! - J0 over 1000 x 1000 on [0, 100] took 6.0 s; the class charges about 6.8e8 units.
//! - J0 over 30 x 30 on [0, 1e5] took 3.1 s; it is charged about 3.4e8 units (refused).
//! - FresnelC over 30 x 30 on [0, 1000] took 11.6 s; it is charged about 1.2e9 units (refused).
//! - A matrix multiply-add costs about 110 ns: 11 units ([`MATRIX_MULT_UNITS`]). A 500 x 500
//!   product is refused, a 200 x 200 product (about 0.9 s) is admitted.
//! - Element-wise arithmetic, copies and `sin` measured 15-50 ns per element: 3 units each.
//!
//! [`MAX_WORK`] is therefore about 3 s of kernel time. Products and determinants are charged by
//! their scalar multiply-adds (see [`cubic_units`]), and the same per-operation cap that
//! [`check_units`] applies, so one matrix product is judged by one rule whether it is alone or part
//! of a sheet.
//!
//! Every builtin name must have a class in `work_table::TABLE`. A name missing from it is charged as
//! medium (see `work_table::MEDIUM`); the `every_registered_builtin_has_a_cost_class` test lists the
//! gaps. Output size: a builtin that allocates a result larger than its inputs also pays for the
//! output (see [`output_units`]), before it allocates.
//!
//! Known limits (documented, not built):
//! - A per-worksheet budget is not implemented: a worksheet of 1000 heavy cells can take 1000 times
//!   the per-statement budget. The GUI must evaluate on a background thread with cancellation (the
//!   viewer already uses one). A worksheet-level budget is future work.
//! - `exponent_bits` caps the exponent at 2^31. A matrix power with a larger exponent is charged as
//!   2^31 squarings, which under-charges by up to 2x.
//! - The environment is cloned once per solver member and once per solve block. The clone is O(the
//!   worksheet) and is not charged.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::OnceLock;

use super::error::EvalError;
use super::expr_eval::MAX_CALLS;
use super::value::{Matrix, Scalar, Value, MAX_ELEMENTS};
use super::work_table::{MEDIUM, TABLE};
use crate::expr::BinaryOp;

/// Units one top-level statement may spend. About 3 s of release-build kernel time (see the module
/// documentation for the calibration).
pub const MAX_WORK: u64 = 300_000_000;

/// Units one worksheet may spend in all, summed over its statements (see `Session::work_spent`).
/// About 50 statements at the per-statement cap. A worksheet that reaches it stops with the
/// regions after it not run.
pub const WORKSHEET_MAX_WORK: u64 = 50 * MAX_WORK;

/// Units per scalar multiply-add of a matrix product (about 110 ns: a checked complex operation).
pub(super) const MATRIX_MULT_UNITS: u64 = 11;

/// Units per element for copies, element-wise arithmetic and shape functions (1.5-5 x 10 ns).
pub(super) const ELEMENT_UNITS: u64 = 3;

/// Units per element for a scalar or element-wise power (47-157 ns per element).
pub(super) const POW_ELEMENT_UNITS: u64 = 16;

/// Largest order or parameter magnitude that a charge counts. Kernels refuse larger ones anyway.
const MAX_PARAM: f64 = 1e6;

/// Largest distribution parameter (a Poisson rate, a negative binomial mean) that a random draw
/// charges. Above `MAX_PARAM` so that `rpois(10, 1e10)` is charged its real cost, not 1e6's.
const MAX_DRAW_PARAM: f64 = 1e15;

/// Divisor of the argument term in an oscillatory kernel: `base * (1 + |x| / 40)`.
const OSCILLATION_SCALE: f64 = 40.0;

/// The two budgets of one top-level evaluation: user-function calls and work units. Both start
/// full and are never refilled inside the evaluation.
pub struct Budget {
    calls: Cell<usize>,
    work: Cell<u64>,
}

impl Default for Budget {
    fn default() -> Self {
        Self::new()
    }
}

impl Budget {
    /// A fresh budget: the full call and work limits of one evaluation.
    pub fn new() -> Self {
        Self {
            calls: Cell::new(MAX_CALLS),
            work: Cell::new(MAX_WORK),
        }
    }

    /// Takes one user-function call, or fails with `CallLimit` once the calls are spent.
    pub(super) fn take_call(&self) -> Result<(), EvalError> {
        let left = self.calls.get();
        if left == 0 {
            return Err(EvalError::CallLimit(MAX_CALLS));
        }
        self.calls.set(left - 1);
        Ok(())
    }

    /// Charges `units` before the work they pay for runs. Over the budget, the budget is spent and
    /// the call fails with `WorkLimit`.
    pub(super) fn charge(&self, units: u64) -> Result<(), EvalError> {
        let left = self.work.get();
        if units > left {
            self.work.set(0);
            return Err(EvalError::WorkLimit(MAX_WORK));
        }
        self.work.set(left - units);
        Ok(())
    }

    /// Units charged so far. A refused charge spends the whole budget, so it counts as `MAX_WORK`.
    pub(super) fn spent(&self) -> u64 {
        MAX_WORK - self.work.get()
    }

    /// Units still available (tests only).
    #[cfg(test)]
    pub(super) fn remaining(&self) -> u64 {
        self.work.get()
    }
}

/// How a builtin's charge grows with its arguments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Cost {
    /// `k` units per element of every argument.
    PerElement(u64),
    /// Bessel-type quadrature: `base * (1 + (|x| + |order|) / 40)` per element of the last argument,
    /// where `order` is the first argument when there are two. The panels grow with `|x| + |nu|`.
    /// Beyond `limit` in `|x|` the kernel refuses at once, so only `base` is charged.
    Oscillatory { base: u64, limit: f64 },
    /// Fresnel-type quadrature: `base + 4 x^2` per element of the last argument, since the panels
    /// grow with `x^2`. Beyond `limit`, only `base`.
    Quadratic { base: u64, limit: f64 },
    /// A loop that runs `p` times, with `p` the magnitude of argument `arg` (a degree or a term
    /// count): `base * (1 + p)` per element of every argument.
    Scaled { base: u64, arg: usize },
    /// A random generator with `m` draws (the first argument) and an inner loop of length `p`
    /// (argument `arg`, such as the trial count of a binomial draw): `base * (1 + m) * (1 + p)`
    /// units in all. The draw count is not an element of the arguments, so `Scaled` cannot charge it.
    Draws { base: u64, arg: usize },
    /// A negative binomial generator `rnbinom(m, n, p)`: `base * (1 + m) * (1 + mean)` with the
    /// mean `n (1 - p) / p`, which is the Poisson rate the inversion searches (not `n`).
    DrawsNegBinomial { base: u64 },
    /// `base * (1 + p)^2` on the magnitude `p` of the first argument: a double loop over that count
    /// (the Dolph-Chebyshev window's `n^2` cosines). Beyond [`MAX_PARAM`] the charge is capped, which
    /// is still over the budget.
    Squared { base: u64 },
    /// `k` units per element per comparison pass of a sort: `ceil(log2(n))` passes, where `n` is
    /// the larger dimension of the first argument (an upper bound on the lines a sort orders).
    PerElementLog(u64),
    /// `k` units per pair of elements of the first two arguments: an all-pairs kernel or a full
    /// convolution (`a * b` for the element counts `a`, `b`). With one argument, the pairs are its own
    /// elements (`a * a`). See `work_table`.
    Pairwise(u64),
    /// Dense linear algebra, charged on the shape of the first argument (see [`dense_units`]).
    Dense(Dense),
}

/// The dense kernels, each with its own shape formula.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Dense {
    /// Gauss-Jordan or LU elimination on `rows x cols`: `rows cols min(rows, cols) / 21` units
    /// (about 0.07 units per flop, measured 0.5-0.9 ns per charged flop). Used by `det`, `rref`
    /// and `rank`.
    Elimination,
    /// The inverse of an `n x n` matrix: `max^3 / 8` units (measured 0.65 ns per charged unit of
    /// the old `2 n^3` formula). Used by `cond1`, `condi`, `conde` and matrix division.
    Inverse,
    /// Least squares of an `m x n` system with `p` right-hand columns: `2/3 m n (n + p)` units.
    /// Used by `lsolve`. Not measured.
    LeastSquares,
    /// Jacobi singular values of `rows x cols`: `4 k^2 max(rows, cols)` units, with `k` the
    /// smaller dimension. Calibrated on release timings of random square matrices (see
    /// [`Dense::JacobiVectors`] for the measurements): `svds` took 0.23 s at 200, 0.98 s at 300 and
    /// 2.29 s at 400, which is 3.6 units per `k^2 m` at 10 ns. Used by `svds`, `cond2` and `norm2`.
    Jacobi,
    /// Jacobi SVD with the vectors (`svd`): `5 k^2 max(rows, cols)` units. The rotations also
    /// accumulate `V`, and the sweeps are the same. Measured in release on random square matrices:
    /// 0.25 s at 200, 1.21 s at 300, 2.44 s at 400 (about 3.8 to 4.5 units per `k^2 m`). The charge
    /// refuses from k = 392 (about 3e8 units, the budget), where the kernel takes about 2.3 s.
    JacobiVectors,
    /// Householder QR of `rows x cols` with `k = min` reflections, forming the `rows x rows` `Q`:
    /// `2 k (rows cols + rows^2)` flops, divided by 21 as in [`Dense::Elimination`] (one unit per
    /// about 14 flops). Used by `QR`. Not measured.
    Householder,
    /// All eigenvalues of an `n x n` matrix (Hessenberg reduction and shifted QR): `4 n^3` complex
    /// multiply-adds, each charged [`MATRIX_MULT_UNITS`]. Used by `eigenvals` and `genvals`. Not
    /// measured.
    Eigenvalues,
    /// One eigenvector by inverse iteration: one LU factorization, charged as `n^3` multiply-adds
    /// (`n^3 / 3` rounded up). Used by `eigenvec`. Not measured.
    EigenvectorOne,
    /// All eigenvectors by inverse iteration: `n` times [`Dense::EigenvectorOne`]. Used by `eigenvecs`
    /// and `genvecs`. Not measured.
    Eigenvectors,
}

/// Looks up the class of every builtin name, once.
fn table() -> &'static HashMap<&'static str, Cost> {
    static MAP: OnceLock<HashMap<&'static str, Cost>> = OnceLock::new();
    MAP.get_or_init(|| {
        TABLE
            .iter()
            .flat_map(|(names, cost)| names.iter().map(move |name| (*name, *cost)))
            .collect()
    })
}

/// The class of builtin `name`: its table entry, or [`MEDIUM`] when it has none.
pub(super) fn cost_of(name: &str) -> Cost {
    table().get(name).copied().unwrap_or(MEDIUM)
}

/// Whether `name` has an explicit entry in the table.
#[cfg(test)]
pub(super) fn is_listed(name: &str) -> bool {
    table().contains_key(name)
}

/// Every name in the table, once per entry. Used by the duplicate check in the tests.
#[cfg(test)]
pub(super) fn listed_names() -> impl Iterator<Item = &'static str> {
    TABLE.iter().flat_map(|(names, _)| names.iter().copied())
}

/// The cost class of a name (test helper for the stub re-classing list).
#[cfg(test)]
pub(super) fn class_of_listed(name: &str) -> Option<Cost> {
    table().get(name).copied()
}

/// Units for one builtin call with its evaluated arguments: the class of its inputs, plus the
/// output it allocates (see [`output_units`]).
pub(super) fn builtin_units(name: &str, args: &[Value]) -> u64 {
    let class = match (name, args) {
        // `geninv(A)` solves `A X = I` with an m x m identity, so its right-hand side has m columns.
        // The table's class sees one argument and would charge no right-hand side at all.
        ("geninv", [Value::Matrix(a), ..]) => {
            dense_units(Dense::LeastSquares, a.rows(), a.cols(), a.rows())
        }
        // `predict(v, m, n)`: Burg's recursion costs about m per data point, and the `n` steps cost m
        // each, so the order and the steps both multiply. The table's class ignores the steps.
        ("predict", [v, m, s]) => {
            let order = 1.0 + scalar_magnitude(m).min(MAX_PARAM);
            let units = 3.0 * order * (elements(v) as f64 + scalar_magnitude(s));
            units.ceil() as u64
        }
        // `hist(P, data)` and `histogram(P, data)` build P + 1 edges before they count.
        ("hist" | "histogram", [s @ Value::Scalar(_), _]) => class_units(cost_of(name), args)
            .saturating_add(scalar_magnitude(s).min(MAX_ELEMENTS as f64) as u64),
        // Second data tier (builtins/data_tier2.rs): the loops the table's class cannot see.
        // `polycoeff`: the product `prod (z - x_i)` and one division per point, each O(n): O(n^2).
        ("polycoeff", [x, _]) => {
            let n = elements(x) as f64;
            (3.0 * n * n).min(u64::MAX as f64) as u64
        }
        ("polyiter", [x, _, _, nmax, _]) => {
            let n = elements(x) as f64;
            let steps = 1.0 + scalar_magnitude(nmax).min(MAX_PARAM);
            (3.0 * n * n * steps).min(u64::MAX as f64) as u64
        }
        ("localmax" | "localmin", _) => {
            // Two-column form: a window of 2w + 1 per element of the matrix. Grid form: (2w + 1)^2.
            let (grid, w_at) = if args.len() <= 2 {
                (args.first(), 1)
            } else {
                (args.get(2), 3)
            };
            let w = args.get(w_at).map_or(1.0, scalar_magnitude).min(MAX_PARAM);
            let span = 2.0 * w + 1.0;
            let factor = if args.len() <= 2 { span } else { span * span };
            let n = grid.map_or(0, elements) as f64;
            (3.0 * factor * n).min(u64::MAX as f64) as u64
        }
        ("VSmooth", [v, w]) => {
            // Each width runs at most n + 2 median passes (`VSmooth` stops after n + 1 changing
            // passes). A pass copies and sorts the window of each of the n elements: about
            // m (1 + log2 m) operations for width m. Widths 0 and 1 run no pass.
            let n = elements(v) as f64;
            let mut per_pass = 0.0;
            for_each_magnitude(w, &mut |m| {
                let m = m.min(MAX_PARAM);
                if m >= 2.0 {
                    per_pass += m * (1.0 + m.log2());
                }
            });
            (n * (n + 2.0) * per_pass).min(u64::MAX as f64) as u64
        }
        _ if FFT_NAMES.contains(&name) => {
            class_units(cost_of(name), args).saturating_mul(bluestein_factor(name, args))
        }
        _ => class_units(cost_of(name), args),
    };
    class.saturating_add(output_units(name, args))
}

/// The names of the transforms of `signal_fft`, which share the `FFT_CLASS` cost class.
const FFT_NAMES: &[&str] = &[
    "dft", "idft", "dftr", "idftr", "cfft", "icfft", "CFFT", "ICFFT", "fft", "ifft", "FFT", "IFFT",
    "dht", "idht", "hilbert",
];

/// Cost factor of Bluestein's method (lengths that are not powers of 2), measured against radix-2.
const BLUESTEIN_FACTOR: u64 = 6;

/// [`BLUESTEIN_FACTOR`] when a length that the transform runs over is not a power of 2, else 1.
fn bluestein_factor(name: &str, args: &[Value]) -> u64 {
    let (rows, cols) = match args.first() {
        Some(Value::Matrix(a)) => (a.rows(), a.cols()),
        _ => (1, 1),
    };
    let (r, c) = match name {
        // These kernels run radix-2 only (their own checks refuse other lengths), so no Bluestein.
        "fft" | "FFT" | "ifft" | "IFFT" => return 1,
        // `idftr` runs over the full lengths 2(L - 1) (see `signal_fft::idftr`).
        "idftr" if cols == 1 => (2 * rows.saturating_sub(1), 1),
        "idftr" => (rows, 2 * cols.saturating_sub(1)),
        _ => (rows, cols),
    };
    if r.is_power_of_two() && c.is_power_of_two() {
        1
    } else {
        BLUESTEIN_FACTOR
    }
}

/// Units from a builtin's class, on its arguments.
fn class_units(cost: Cost, args: &[Value]) -> u64 {
    match cost {
        Cost::PerElement(k) => k.saturating_mul(total_elements(args)),
        Cost::PerElementLog(k) => k
            .saturating_mul(total_elements(args))
            .saturating_mul(sort_passes(args)),
        Cost::Pairwise(k) => {
            let a = args.first().map_or(1, elements);
            let b = args.get(1).map_or(a, elements);
            k.saturating_mul(a).saturating_mul(b)
        }
        Cost::Oscillatory { base, limit } => {
            let order = match args {
                [m, _] => scalar_magnitude(m).min(MAX_PARAM),
                _ => 0.0,
            };
            let Some(z) = args.last() else {
                return base;
            };
            let mut sum = 0.0;
            for_each_magnitude(z, &mut |x| {
                sum += if x <= limit {
                    base as f64 * (1.0 + (x + order) / OSCILLATION_SCALE)
                } else {
                    base as f64
                };
            });
            sum.ceil() as u64
        }
        Cost::Quadratic { base, limit } => {
            let Some(z) = args.last() else {
                return base;
            };
            let mut sum = 0.0;
            for_each_magnitude(z, &mut |x| {
                sum += if x <= limit {
                    base as f64 + 4.0 * x * x
                } else {
                    base as f64
                };
            });
            sum.ceil() as u64
        }
        Cost::Scaled { base, arg } => {
            let p = args.get(arg).map_or(0.0, scalar_magnitude).min(MAX_PARAM);
            let units = base as f64 * (1.0 + p) * total_elements(args) as f64;
            units.ceil() as u64
        }
        Cost::Draws { base, arg } => {
            let m = args.first().map_or(0.0, scalar_magnitude).min(MAX_PARAM);
            let p = args
                .get(arg)
                .map_or(0.0, scalar_magnitude)
                .min(MAX_DRAW_PARAM);
            (base as f64 * (1.0 + m) * (1.0 + p)).ceil() as u64
        }
        Cost::Squared { base } => {
            let p = args.first().map_or(0.0, scalar_magnitude).min(MAX_PARAM);
            (base as f64 * (1.0 + p) * (1.0 + p)).ceil() as u64
        }
        Cost::DrawsNegBinomial { base } => {
            let m = args.first().map_or(0.0, scalar_magnitude).min(MAX_PARAM);
            let n = args.get(1).map_or(0.0, scalar_magnitude);
            let p = args.get(2).map_or(1.0, scalar_magnitude);
            let mean = if p > 0.0 {
                (n * (1.0 - p) / p).clamp(0.0, MAX_DRAW_PARAM)
            } else {
                MAX_DRAW_PARAM
            };
            (base as f64 * (1.0 + m) * (1.0 + mean)).ceil() as u64
        }
        Cost::Dense(kind) => match args {
            [Value::Matrix(a), rest @ ..] => {
                let p = match rest.first() {
                    Some(Value::Matrix(b)) => b.cols(),
                    _ => 0,
                };
                dense_units(kind, a.rows(), a.cols(), p)
            }
            _ => 1,
        },
    }
}

/// Units for the output a builtin allocates beyond its inputs, charged before it allocates.
/// `identity(n)` and `diag(v)` build an `n x n` matrix from a scalar or a vector of length `n`; the
/// count is capped at [`MAX_ELEMENTS`], so an oversized request is refused by the builtin itself
/// (`TooLarge`) rather than by the budget. Every other builtin's output is no larger than its input,
/// which its class already charges.
pub(super) fn output_units(name: &str, args: &[Value]) -> u64 {
    let square = match (name, args) {
        ("identity", [v, ..]) => {
            let n = scalar_magnitude(v).min(MAX_ELEMENTS as f64) as u64;
            n.saturating_mul(n)
        }
        ("diag", [Value::Matrix(v)]) if v.cols() == 1 => {
            let n = v.rows() as u64;
            n.saturating_mul(n)
        }
        // `logpts(minexp, dec, dnpts)` builds `dec * dnpts` points; `logspace(min, max, npts)` builds `npts`.
        ("logpts", [_, dec, per]) => {
            let (d, p) = (scalar_magnitude(dec), scalar_magnitude(per));
            (d.min(MAX_ELEMENTS as f64) * p.min(MAX_ELEMENTS as f64)) as u64
        }
        ("logspace", [_, _, n]) => scalar_magnitude(n).min(MAX_ELEMENTS as f64) as u64,
        _ => 0,
    };
    ELEMENT_UNITS.saturating_mul(square.min(MAX_ELEMENTS as u64))
}

/// Units for a dense kernel on a `rows x cols` matrix, with `extra` right-hand columns for
/// [`Dense::LeastSquares`] (zero otherwise). The formulas are in [`Dense`]. Saturating, so an
/// absurd shape is simply over any budget.
pub(super) fn dense_units(kind: Dense, rows: usize, cols: usize, extra: usize) -> u64 {
    let (r, c, p) = (rows as u64, cols as u64, extra as u64);
    let m = r.max(c);
    let k = r.min(c);
    match kind {
        Dense::Elimination => r.saturating_mul(c).saturating_mul(k) / 21,
        Dense::Inverse => m.saturating_mul(m).saturating_mul(m) / 8,
        Dense::LeastSquares => {
            2u64.saturating_mul(r)
                .saturating_mul(c)
                .saturating_mul(c.saturating_add(p))
                / 3
        }
        Dense::Jacobi => 4u64.saturating_mul(k).saturating_mul(k).saturating_mul(m),
        Dense::JacobiVectors => 5u64.saturating_mul(k).saturating_mul(k).saturating_mul(m),
        Dense::Householder => {
            let flops = 2u64
                .saturating_mul(k)
                .saturating_mul(r.saturating_mul(c).saturating_add(r.saturating_mul(r)));
            flops / 21
        }
        Dense::Eigenvalues => MATRIX_MULT_UNITS
            .saturating_mul(4)
            .saturating_mul(m.saturating_mul(m).saturating_mul(m)),
        Dense::EigenvectorOne => {
            MATRIX_MULT_UNITS.saturating_mul(m.saturating_mul(m).saturating_mul(m))
        }
        Dense::Eigenvectors => MATRIX_MULT_UNITS
            .saturating_mul(m.saturating_mul(m).saturating_mul(m).saturating_mul(m)),
    }
}

/// Refuses one operation whose units alone exceed [`MAX_WORK`]. Called before a kernel runs, so the
/// same rule holds whether the operation is a builtin, an operator, or a product inside a sheet.
pub(super) fn check_units(units: u64) -> Result<(), EvalError> {
    if units > MAX_WORK {
        Err(EvalError::WorkLimit(MAX_WORK))
    } else {
        Ok(())
    }
}

/// [`check_units`] for a matrix product or other kernel counted in scalar multiply-adds.
pub(super) fn check_mults(mults: u64) -> Result<(), EvalError> {
    check_units(cubic_units(mults))
}

/// Units for a unary operator: [`ELEMENT_UNITS`] per element.
pub(super) fn unary_units(v: &Value) -> u64 {
    ELEMENT_UNITS.saturating_mul(elements(v))
}

/// Units for copying a value out of the worksheet or a closure: [`ELEMENT_UNITS`] per matrix element,
/// and nothing for a scalar or text. Charged before the copy (see `expr_eval::lookup`).
pub(super) fn copy_units(v: &Value) -> u64 {
    match v {
        Value::Matrix(_) | Value::List(_) | Value::Symbolic(_) => {
            ELEMENT_UNITS.saturating_mul(elements(v))
        }
        Value::Scalar(_) | Value::Str(_) | Value::Func(_) => 0,
    }
}

/// Units for a binary operator, or the per-operation refusal of [`check_units`] for a matrix product
/// too large on its own. Matrix products, quotients by a matrix and matrix powers cost by their
/// multiply-adds (see [`cubic_units`]); a power costs [`POW_ELEMENT_UNITS`] per element and every
/// other element-wise operator [`ELEMENT_UNITS`] per element.
pub(super) fn binary_units(op: BinaryOp, a: &Value, b: &Value) -> Result<u64, EvalError> {
    match (op, a, b) {
        (BinaryOp::Mul, Value::Matrix(x), Value::Matrix(y)) if !is_dot_pair(x, y) => {
            // The element count is judged first: an oversized result is `TooLarge`, not work.
            let count = x.rows().saturating_mul(y.cols());
            if count > MAX_ELEMENTS {
                return Err(EvalError::TooLarge(count));
            }
            product_units(mults(x.rows(), x.cols(), y.cols()))
        }
        (BinaryOp::Div, Value::Matrix(x), Value::Matrix(y)) if x.shape() != y.shape() => {
            let inverse = dense_units(Dense::Inverse, y.rows(), y.cols(), 0);
            check_units(inverse)?;
            product_units(mults(x.rows(), x.cols(), y.cols())).map(|p| p.saturating_add(inverse))
        }
        (BinaryOp::Pow, Value::Matrix(m), Value::Scalar(e)) if m.rows() == m.cols() => {
            let n = m.rows() as u64;
            let cube = n.saturating_mul(n).saturating_mul(n);
            check_mults(cube)?;
            Ok(cubic_units(cube.saturating_mul(2 * exponent_bits(e))))
        }
        _ => {
            let per_element = match op {
                BinaryOp::Pow => POW_ELEMENT_UNITS,
                _ => ELEMENT_UNITS,
            };
            Ok(per_element.saturating_mul(elements(a).max(elements(b))))
        }
    }
}

/// `2 * mults` units, after the per-operation check (a single product above [`MAX_WORK`] is refused).
fn product_units(scalar_mults: u64) -> Result<u64, EvalError> {
    check_mults(scalar_mults)?;
    Ok(cubic_units(scalar_mults))
}

/// Units for `scalar_mults` multiply-adds of a matrix kernel.
pub(super) fn cubic_units(scalar_mults: u64) -> u64 {
    scalar_mults.saturating_mul(MATRIX_MULT_UNITS)
}

fn mults(rows: usize, inner: usize, cols: usize) -> u64 {
    (rows as u64)
        .saturating_mul(inner as u64)
        .saturating_mul(cols as u64)
}

/// Two column vectors of one length, longer than one element: the dot product (see `ops::mul`).
fn is_dot_pair(x: &Matrix, y: &Matrix) -> bool {
    x.cols() == 1 && y.cols() == 1 && x.rows() == y.rows() && x.rows() > 1
}

/// Bits in the exponent magnitude: the number of squarings of repeated squaring. The magnitude is
/// capped at 2^31 (see the module documentation).
fn exponent_bits(e: &Scalar) -> u64 {
    let k = e.value.re.abs().min(2_147_483_648.0) as u64;
    u64::from(64 - k.leading_zeros()).max(1)
}

/// `ceil(log2(n))` comparison passes (at least one) for a sort of the first argument's lines, where
/// `n` is its larger dimension.
fn sort_passes(args: &[Value]) -> u64 {
    match args.first() {
        Some(Value::Matrix(m)) => {
            let lines = m.rows().max(m.cols()) as u64;
            u64::from(u64::BITS - lines.saturating_sub(1).leading_zeros()).max(1)
        }
        _ => 1,
    }
}

/// Number of scalars a value holds: a matrix's elements, or one for a scalar or text.
fn elements(v: &Value) -> u64 {
    match v {
        Value::Matrix(m) => (m.rows() as u64).saturating_mul(m.cols() as u64),
        Value::List(items) => items.iter().map(elements).fold(0u64, u64::saturating_add),
        Value::Scalar(_) => 1,
        // A string counts one element per 64 bytes, so the string functions charge for their size.
        Value::Str(s) => 1 + (s.len() as u64) / 64,
        Value::Symbolic(s) => s.entries.len() as u64,
        Value::Func(_) => 1,
    }
}

fn total_elements(args: &[Value]) -> u64 {
    args.iter().map(elements).fold(0u64, u64::saturating_add)
}

/// Magnitude of a scalar argument, or zero for anything else.
fn scalar_magnitude(v: &Value) -> f64 {
    match v {
        Value::Scalar(s) => s.value.norm(),
        _ => 0.0,
    }
}

/// Calls `f` with the magnitude of every scalar of `v`: the scalar itself, each matrix element, or
/// zero for text. Linear in the size of `v`, which the kernel itself must also read.
fn for_each_magnitude(v: &Value, f: &mut dyn FnMut(f64)) {
    match v {
        Value::Scalar(s) => f(s.value.norm()),
        Value::Matrix(m) => m.data().iter().for_each(|s| f(s.value.norm())),
        Value::List(items) => items.iter().for_each(|item| for_each_magnitude(item, f)),
        Value::Str(_) | Value::Symbolic(_) | Value::Func(_) => f(0.0),
    }
}
