//! Evaluation core: values, dimension-checked arithmetic and matrix algebra.
//!
//! Part 1 covers values and operations only. Environment, definitions and expression
//! evaluation are built on top of this module later.

pub mod builtins;
mod calculus;
mod closure;
mod constants;
mod display;
mod env;
mod error;
mod expr_eval;
mod func_forms;
mod func_value;
mod indexing;
mod lookup;
mod ode;
mod ode_block;
#[cfg(test)]
mod ode_block_tests;
mod ode_bvp;
mod ode_implicit;
mod ode_state;
mod ode_stiff;
mod ops;
mod optimize_form;
mod pde_block;
#[cfg(test)]
mod pde_block_tests;
mod pde_mol;
mod program;
#[cfg(test)]
mod program_tests;
mod range;
mod range_iter;
mod result_format;
#[cfg(test)]
mod result_format_tests;
mod root;
mod roots;
mod session;
mod solve_block;
#[cfg(test)]
mod solve_block_budget_tests;
#[cfg(test)]
mod solve_block_guess_tests;
mod solve_block_rows;
#[cfg(test)]
mod solve_block_tests;
mod solve_constraints;
mod special_forms;
mod symbolic_bridge;
#[cfg(test)]
mod tests;
mod unit_group;
mod value;
mod work;
mod work_table;

pub use builtins::{Arity, Builtin, BuiltinFn, Registry};
pub use constants::{all_constants, ConstantInfo};
pub use env::{Env, UserFunction, DEFAULT_TOLERANCE};
pub use error::EvalError;
pub use expr_eval::{evaluate, MAX_CALLS, MAX_CALL_DEPTH, MAX_NODE_DEPTH};
pub use ops::{
    abs, add, compare, conjugate, det, div, inverse, magnitude, map_abs, map_scalar, mul, neg, pow,
    require_dimensionless, sub, transpose, CmpOp,
};
pub use range_iter::{evaluate_series, evaluate_surface, MAX_RANGE_VARIABLES, NOT_REAL_PLOT_VALUE};
pub use result_format::{
    format_complex, format_complex_with, format_real, format_real_with, AngleUnit, ComplexForm,
    FormatError, MatrixDisplay, NumberFormat, NumberSeparators, Radix, ResultFormat,
};
pub use session::{Outcome, Session};

/// Whether an expression uses a name; the parser reads `root(f(x), x)` through it.
pub(crate) use closure::uses_variable;
pub use solve_block::{BlockMember, MemberKind, MAX_SOLVE_EVALUATIONS, MAX_SOLVE_ITERATIONS};
pub use value::{FuncValue, Matrix, Scalar, Value, MAX_ELEMENTS};
pub use work::Budget;
pub use work::{MAX_WORK, WORKSHEET_MAX_WORK};
