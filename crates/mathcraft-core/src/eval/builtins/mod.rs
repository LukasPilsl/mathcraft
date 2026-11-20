//! Built-in function registry.
//!
//! A builtin takes evaluated arguments and returns a value. The registry checks the argument
//! count before the call, so a builtin only sees a count its [`Arity`] accepted. Later function
//! tiers add their families with [`Registry::register`]; `elementary` is the first.
//!
//! Two kinds of builtin, both in the same registry:
//! - a plain builtin ([`BuiltinFn`], [`Registry::register`]) gets only its argument values. Most
//!   functions are this kind.
//! - a context builtin ([`BuiltinFnCtx`], [`Registry::register_ctx`]) also gets a [`BuiltinCtx`]:
//!   the worksheet's `ORIGIN`, `TOL` and `CTOL` as the call sees them. Inside a user function body
//!   these are the values captured at definition (see `closure`). Each value is read lazily: a
//!   context builtin that does not use `TOL` still runs when `TOL` is undefined or a range.

use std::collections::HashMap;

use super::error::EvalError;
use super::value::Value;
use super::work::Budget;

pub mod coords_symbolic;
pub mod data_hist;
pub mod data_interp;
pub mod data_outlier;
pub mod data_regress;
pub mod data_smooth;
pub mod data_thiele;
pub mod data_tier2;
pub mod data_util;
pub mod datetime;
pub mod distributions_cont;
pub mod distributions_disc;
pub mod doe;
pub mod doe_fit;
pub mod doe_screen;
pub mod elementary;
pub mod fileio;
pub mod finance;
pub mod fit_linear;
pub mod fit_nonlin;
pub mod image;
pub mod image_tier2;
pub mod integer_misc;
pub mod matrix;
mod matrix_decomp;
mod matrix_decomp2;
pub mod matrix_extra;
mod matrix_index;
pub mod matrix_linalg;
mod matrix_svd;
pub mod misc_tier2;
pub mod pde_grid;
pub mod random;
pub mod readwrite;
pub mod signal_fft;
pub mod signal_iir;
pub mod signal_tier2;
pub mod signal_transforms;
pub mod special;
pub mod special_hyper;
pub mod special_integrals;
pub mod statistics;
pub mod strings;

/// Registers the data-analysis and signal-processing tier: interpolation and prediction, straight-line
/// regression, smoothing and convolution, histograms and the discrete Fourier transforms.
pub fn register_data_tier(r: &mut Registry) {
    data_interp::register(r);
    data_regress::register(r);
    data_smooth::register(r);
    data_outlier::register(r);
    data_thiele::register(r);
    data_hist::register(r);
    signal_fft::register(r);
    signal_transforms::register(r);
    signal_iir::register(r);
}

/// Registers the curve-fitting tier: the plain fits (`lnfit`, `expfit`, `logfit`, `lgsfit`, `pwrfit`,
/// `sinfit`). `linfit` and `genfit` take a function argument and are registered as special forms.
pub fn register_fit_tier(r: &mut Registry) {
    fit_linear::register(r);
    fit_nonlin::register(r);
}

/// Registers the finance, date-and-time and string families (`finance`, `datetime`, `strings`).
pub fn register_finance_time_string_tier(r: &mut Registry) {
    finance::register(r);
    datetime::register(r);
    strings::register(r);
}

/// Registers the file-access functions (`READCSV`, `WRITECSV`, `READTEXT`, `WRITETEXT`), which read
/// the worksheet's [`FileOptions`](fileio::FileOptions).
pub fn register_file_tier(r: &mut Registry) {
    readwrite::register(r);
}

/// Registers the design-of-experiments matrices (`fullfact`, `boxwilson`, `doelabel`), the
/// polynomial regression `polyfit` and the factor screening `quickscreen` and `effects`.
pub fn register_doe_tier(r: &mut Registry) {
    doe::register(r);
    doe_fit::register(r);
    doe_screen::register(r);
}

/// Registers the image colour conversions and the matrix image operations (`rgb_to_hsv`, ...).
pub fn register_image_tier(r: &mut Registry) {
    image::register(r);
}

/// Registers the grid solvers of Poisson's equation (`relax`, `multigrid`).
pub fn register_pde_tier(r: &mut Registry) {
    pde_grid::register(r);
}

/// A builtin: evaluated arguments in, value out.
pub type BuiltinFn = fn(&[Value]) -> Result<Value, EvalError>;

/// A context builtin: the environment read-only, then the evaluated arguments.
pub type BuiltinFnCtx = fn(&BuiltinCtx, &[Value]) -> Result<Value, EvalError>;

/// The worksheet environment a context builtin may read. Each field is the outcome of its read:
/// an error (a range variable, an undefined name, a non-integer `ORIGIN`) is returned by the
/// accessor that needs it, so a builtin that does not read the field is not affected by it.
///
/// It also carries the file access of the evaluation and the evaluation's work budget, so a
/// builtin that reads a file charges its loops to the same budget as the rest of the statement.
#[derive(Clone)]
pub struct BuiltinCtx<'a> {
    origin: Result<i64, EvalError>,
    tol: Result<f64, EvalError>,
    ctol: Result<f64, EvalError>,
    files: fileio::FileOptions,
    budget: &'a Budget,
}

impl<'a> BuiltinCtx<'a> {
    /// Built by the evaluator from the current scope (see `expr_eval`).
    pub(super) fn from_scope(
        origin: Result<i64, EvalError>,
        tol: Result<f64, EvalError>,
        ctol: Result<f64, EvalError>,
        files: fileio::FileOptions,
        budget: &'a Budget,
    ) -> Self {
        Self {
            origin,
            tol,
            ctol,
            files,
            budget,
        }
    }

    /// A context with no file access and a full budget, for builtin tests.
    #[cfg(test)]
    pub(super) fn new(
        origin: Result<i64, EvalError>,
        tol: Result<f64, EvalError>,
        ctol: Result<f64, EvalError>,
    ) -> BuiltinCtx<'static> {
        // The budget lives for the rest of the test process; a test builds a few contexts only.
        let budget: &'static Budget = Box::leak(Box::new(Budget::new()));
        BuiltinCtx::from_scope(origin, tol, ctol, fileio::FileOptions::disabled(), budget)
    }

    /// The file access of this evaluation (disabled unless the caller opted in).
    pub fn files(&self) -> &fileio::FileOptions {
        &self.files
    }

    /// Charges `units` of work to the statement's budget. Fails with `WorkLimit` when it is spent.
    pub fn charge(&self, units: u64) -> Result<(), EvalError> {
        self.budget.charge(units)
    }

    /// `ORIGIN`: the index of the first element. An undefined `ORIGIN` (its definition failed, or it
    /// was never set) counts as the default 0, the same as indexing does; a bare read of `ORIGIN`
    /// is `Undefined`. A range `ORIGIN` is an error.
    pub fn origin(&self) -> Result<i64, EvalError> {
        self.origin.clone()
    }

    /// `TOL`, as a real number (the sign and finiteness are the builtin's to judge).
    pub fn tol(&self) -> Result<f64, EvalError> {
        self.tol.clone()
    }

    /// `CTOL`, as a real number.
    pub fn ctol(&self) -> Result<f64, EvalError> {
        self.ctol.clone()
    }
}

/// How a registered builtin is called.
#[derive(Debug, Clone, Copy)]
pub enum Call {
    Plain(BuiltinFn),
    WithEnv(BuiltinFnCtx),
}

/// The argument counts a builtin accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    Exact(usize),
    /// Inclusive range `min..=max`.
    Range(usize, usize),
    AtLeast(usize),
}

impl Arity {
    pub fn accepts(self, count: usize) -> bool {
        match self {
            Arity::Exact(n) => count == n,
            Arity::Range(lo, hi) => (lo..=hi).contains(&count),
            Arity::AtLeast(lo) => count >= lo,
        }
    }

    /// `Ok(())` when `count` arguments fit; otherwise an `ArgCount` error naming `name`.
    pub fn check(self, name: &str, count: usize) -> Result<(), EvalError> {
        if self.accepts(count) {
            return Ok(());
        }
        let expected = match self {
            Arity::Exact(n) => n.to_string(),
            Arity::Range(lo, hi) => format!("{lo} to {hi}"),
            Arity::AtLeast(lo) => format!("at least {lo}"),
        };
        Err(EvalError::ArgCount {
            name: name.to_string(),
            expected,
            found: count,
        })
    }
}

/// A registered builtin: its argument counts and its implementation.
#[derive(Debug, Clone, Copy)]
pub struct Builtin {
    pub arity: Arity,
    pub call: Call,
}

/// Name to builtin. Names are case-sensitive.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    functions: HashMap<String, Builtin>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a plain builtin `name`, replacing an earlier builtin of the same name.
    pub fn register(&mut self, name: &str, arity: Arity, call: BuiltinFn) {
        self.insert(name, arity, Call::Plain(call));
    }

    /// Adds a context builtin `name` (one that reads `ORIGIN`, `TOL` or `CTOL`), replacing an
    /// earlier builtin of the same name.
    pub fn register_ctx(&mut self, name: &str, arity: Arity, call: BuiltinFnCtx) {
        self.insert(name, arity, Call::WithEnv(call));
    }

    fn insert(&mut self, name: &str, arity: Arity, call: Call) {
        debug_assert!(
            !self.functions.contains_key(name),
            "builtin {name} is registered twice"
        );
        self.functions
            .insert(name.to_string(), Builtin { arity, call });
    }

    pub fn get(&self, name: &str) -> Option<&Builtin> {
        self.functions.get(name)
    }

    /// Every registered name, in no particular order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.functions.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arity_accepts_the_declared_counts() {
        assert!(Arity::Exact(1).accepts(1));
        assert!(!Arity::Exact(1).accepts(2));
        assert!(Arity::Range(1, 2).accepts(2));
        assert!(!Arity::Range(1, 2).accepts(3));
        assert!(Arity::AtLeast(1).accepts(7));
        assert!(!Arity::AtLeast(1).accepts(0));
    }

    #[test]
    fn arity_check_names_the_function() {
        let err = Arity::Range(1, 2).check("log", 3).unwrap_err();
        assert_eq!(
            err,
            EvalError::ArgCount {
                name: "log".to_string(),
                expected: "1 to 2".to_string(),
                found: 3,
            }
        );
    }

    fn constant_one(_: &[Value]) -> Result<Value, EvalError> {
        Ok(Value::Str("one".to_string()))
    }

    #[test]
    fn register_then_get() {
        let mut r = Registry::new();
        assert!(r.get("f").is_none());
        r.register("f", Arity::Exact(0), constant_one);
        assert_eq!(r.get("f").map(|b| b.arity), Some(Arity::Exact(0)));
    }
}
