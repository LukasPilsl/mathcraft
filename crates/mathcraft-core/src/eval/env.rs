//! The evaluation environment: variables, user functions, built-in constants and the builtin
//! function registry.
//!
//! Variable keys are identifier texts. A subscripted name with a literal subscript such as `v_0`
//! is the variable keyed `v_0`, never an index (see `expr_eval::var_key`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::builtins::fileio::FileOptions;
use super::builtins::{elementary, statistics, Registry};
use super::closure::Closure;
use super::error::EvalError;
use super::expr_eval::range_in_expression;
use super::value::{Matrix, Scalar, Value};
use crate::expr::Expr;
use crate::units::Dimension;

/// Default value of the built-in tolerances `TOL` and `CTOL`.
pub const DEFAULT_TOLERANCE: f64 = 1e-3;

/// A user function: parameter names, the unevaluated body, and the names the body captured when
/// it was defined (see `closure`). Calls see the captured names, never the live worksheet.
///
/// Immutable once built. Captures and the worksheet share it through `Arc`, so copying a
/// captured function copies a pointer, never its body or its own captures.
#[derive(Debug, Clone, PartialEq)]
pub struct UserFunction {
    pub params: Vec<String>,
    pub body: Expr,
    pub closure: Closure,
}

/// Variables, user functions and the builtin registry visible to evaluation.
///
/// Values are held behind `Arc`: a closure captures a variable by sharing it (see `closure`),
/// so a large matrix is stored once however many functions use it.
#[derive(Clone)]
pub struct Env {
    variables: HashMap<String, Arc<Value>>,
    /// Range variables: names defined by a range expression (`t := 0, 1 .. 3`). They are kept
    /// apart from `variables`, since a range is not a vector: an expression that uses one is
    /// refused (see `expr_eval::range_in_expression`) rather than evaluated as a matrix.
    ranges: HashMap<String, Arc<Matrix>>,
    /// Names whose definition failed. They are worksheet names, so a built-in constant of the
    /// same name (`α`, `γ`) does not stand in for them: their uses are `Undefined`.
    failed: HashSet<String>,
    functions: HashMap<String, Arc<UserFunction>>,
    builtins: Registry,
    /// File access for the file functions. Disabled unless the caller sets it (see
    /// [`Env::set_file_options`]).
    files: FileOptions,
}

impl Env {
    /// An empty worksheet with the system variables set to their help defaults (`ORIGIN = 0`,
    /// `TOL` and `CTOL` = 0.001, `PRNPRECISION = 4`, `PRNCOLWIDTH = 8`) and the standard builtins.
    pub fn new() -> Self {
        let dimensionless = Dimension::dimensionless();
        let mut env = Self {
            variables: HashMap::new(),
            ranges: HashMap::new(),
            failed: HashSet::new(),
            functions: HashMap::new(),
            builtins: Registry::new(),
            files: FileOptions::disabled(),
        };
        let system = [
            ("ORIGIN", 0.0),
            ("TOL", DEFAULT_TOLERANCE),
            ("CTOL", DEFAULT_TOLERANCE),
            ("PRNPRECISION", 4.0),
            ("PRNCOLWIDTH", 8.0),
        ];
        for (name, value) in system {
            env.set_variable(
                name.to_string(),
                Value::Scalar(Scalar::real(value, dimensionless)),
            );
        }
        elementary::register(&mut env.builtins);
        super::builtins::integer_misc::register(&mut env.builtins);
        statistics::register(&mut env.builtins);
        super::builtins::special::register(&mut env.builtins);
        super::builtins::special_hyper::register(&mut env.builtins);
        super::builtins::coords_symbolic::register(&mut env.builtins);
        super::builtins::matrix::register(&mut env.builtins);
        super::builtins::matrix_extra::register(&mut env.builtins);
        super::builtins::register_data_tier(&mut env.builtins);
        super::builtins::register_fit_tier(&mut env.builtins);
        super::root::register(&mut env.builtins);
        super::special_forms::register(&mut env.builtins);
        super::builtins::register_finance_time_string_tier(&mut env.builtins);
        super::builtins::register_file_tier(&mut env.builtins);
        super::builtins::register_doe_tier(&mut env.builtins);
        super::builtins::register_image_tier(&mut env.builtins);
        super::builtins::register_pde_tier(&mut env.builtins);
        super::builtins::signal_tier2::register_signal_tier2(&mut env.builtins);
        super::builtins::data_tier2::register_data_tier2(&mut env.builtins);
        super::builtins::misc_tier2::register_misc_tier2(&mut env.builtins);
        super::builtins::image_tier2::register_image_tier2(&mut env.builtins);
        env
    }

    /// The file access the file functions use. Disabled by default.
    pub fn file_options(&self) -> &FileOptions {
        &self.files
    }

    /// Sets the file access for this worksheet's evaluation (see [`FileOptions`]).
    pub fn set_file_options(&mut self, files: FileOptions) {
        self.files = files;
    }

    /// The value of variable `key`, if defined.
    pub fn variable(&self, key: &str) -> Option<&Value> {
        self.variables.get(key).map(|v| &**v)
    }

    /// The shared handle of variable `key`, if defined. Cloning it copies a pointer.
    pub(super) fn variable_shared(&self, key: &str) -> Option<&Arc<Value>> {
        self.variables.get(key)
    }

    /// Binds `key` to `value`, replacing an earlier binding of either kind.
    pub fn set_variable(&mut self, key: String, value: Value) {
        self.failed.remove(&key);
        self.ranges.remove(&key);
        self.variables.insert(key, Arc::new(value));
    }

    /// Binds `key` as a range variable holding the column `values`, replacing an earlier binding
    /// of either kind.
    pub fn set_range(&mut self, key: String, values: Matrix) {
        self.failed.remove(&key);
        self.variables.remove(&key);
        self.ranges.insert(key, Arc::new(values));
    }

    /// Whether `key` is a range variable.
    pub fn is_range(&self, key: &str) -> bool {
        self.ranges.contains_key(key)
    }

    /// The column of values of range variable `key`, if it is one.
    pub fn range_variable(&self, key: &str) -> Option<&Matrix> {
        self.ranges.get(key).map(|m| &**m)
    }

    /// Leaves `key` undefined after its definition failed. The name stays a worksheet name: see
    /// [`Env::is_failed`].
    pub fn mark_failed(&mut self, key: &str) {
        self.variables.remove(key);
        self.ranges.remove(key);
        self.failed.insert(key.to_string());
    }

    /// Whether the definition of `key` failed and no later definition replaced it. Such a name
    /// is `Undefined` even where a built-in constant has the same name.
    pub fn is_failed(&self, key: &str) -> bool {
        self.failed.contains(key)
    }

    /// The user function `name`, if defined.
    pub fn function(&self, name: &str) -> Option<&UserFunction> {
        self.functions.get(name).map(|f| &**f)
    }

    /// The shared handle of user function `name`, if defined. Cloning it copies a pointer.
    pub(super) fn function_shared(&self, name: &str) -> Option<&Arc<UserFunction>> {
        self.functions.get(name)
    }

    /// Binds user function `name`, replacing an earlier definition.
    pub fn set_function(&mut self, name: String, function: UserFunction) {
        self.functions.insert(name, Arc::new(function));
    }

    /// The builtin function registry.
    pub fn builtins(&self) -> &Registry {
        &self.builtins
    }

    /// Mutable access to the builtin registry, for registering later function tiers.
    pub fn builtins_mut(&mut self) -> &mut Registry {
        &mut self.builtins
    }

    /// The current `ORIGIN`: the index of the first element of vectors and matrices. Defaults to
    /// 0 when the name is undefined. Must be a dimensionless integer.
    /// A range named `ORIGIN` is refused, not read as the absent (default 0) origin.
    pub fn origin(&self) -> Result<i64, EvalError> {
        if self.is_range("ORIGIN") {
            return Err(range_in_expression());
        }
        origin_of(self.variable("ORIGIN"))
    }
}

/// The `ORIGIN` held by an optional binding: 0 when unbound, otherwise a dimensionless integer.
pub(super) fn origin_of(value: Option<&Value>) -> Result<i64, EvalError> {
    match value {
        None => Ok(0),
        Some(Value::Scalar(s)) if s.dim.is_dimensionless() => integer_of(s)
            .ok_or_else(|| EvalError::Unsupported("ORIGIN must be an integer".to_string())),
        Some(_) => Err(EvalError::Unsupported(
            "ORIGIN must be a dimensionless integer".to_string(),
        )),
    }
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

/// The integer value of a real dimensionless scalar, when it is integral and within `i64`
/// range. Dimension is not checked here.
pub(super) fn integer_of(s: &Scalar) -> Option<i64> {
    let z = s.value;
    let limit = 9.0e15;
    if z.im == 0.0 && z.re.fract() == 0.0 && z.re.abs() <= limit {
        Some(z.re as i64)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use num_complex::Complex64;

    use super::*;

    #[test]
    fn new_env_has_default_origin_and_tolerances() {
        let env = Env::new();
        assert_eq!(env.origin(), Ok(0));
        assert!(matches!(env.variable("TOL"), Some(Value::Scalar(_))));
        assert!(matches!(env.variable("CTOL"), Some(Value::Scalar(_))));
    }

    #[test]
    fn integer_of_rejects_fractions_and_complex() {
        let d = Dimension::dimensionless();
        assert_eq!(integer_of(&Scalar::real(3.0, d)), Some(3));
        assert_eq!(integer_of(&Scalar::real(2.5, d)), None);
        assert_eq!(integer_of(&Scalar::new(Complex64::new(1.0, 1.0), d)), None);
    }

    #[test]
    fn system_variables_have_their_help_defaults() {
        let env = Env::new();
        for (name, expected) in [
            ("TOL", 0.001),
            ("CTOL", 0.001),
            ("PRNPRECISION", 4.0),
            ("PRNCOLWIDTH", 8.0),
        ] {
            match env.variable(name) {
                Some(Value::Scalar(s)) => assert_eq!(s.value.re, expected, "{name}"),
                other => panic!("{name}: {other:?}"),
            }
        }
    }
}
