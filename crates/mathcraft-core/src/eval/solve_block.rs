//! Solve blocks: guess values, constraints and a `find` or `minerr` call, run as one unit.
//!
//! A block is a list of [`BlockMember`]s in document order, each a guess (an ordinary definition),
//! a constraint (an equation or an inequality) or the solver call. [`Session::run_solve_block`]
//! runs them as follows:
//!
//! - Guesses and the names they define are local to the block. They run in a scratch copy of the
//!   worksheet, so they shadow outer names inside the block only. Prime help ("To Define a Solve
//!   Block"): variables defined inside a solve block have no values outside it.
//! - Constraints are collected in any order and not evaluated until the solver runs (after all
//!   guesses). Each is `lhs == rhs` or `lhs < rhs`, `<=`, `>`, `>=`. Logical operators and `≠`
//!   are refused, as the help says (see `solve_constraints`).
//! - The solver member is `find(x, y, ...)` or `minerr(...)`, optionally assigned to a name or to
//!   a vector of names (`[a, b] := (find(x, y)=)`). Its unknowns start at the current guess
//!   values, with their dimensions. The solve runs in SI value space, one `f64` per unknown.
//! - `find` returns a scalar for one unknown and a column for several. A vector target takes its
//!   elements in order and has the target's shape. `minerr` also sets `ERR` to the residual norm.
//! - Results are bound in the worksheet after the block. A failed solve leaves the target names
//!   undefined, the same as a failed definition.
//!
//! Every user-function call and every builtin's work made while a block's solver members run is
//! charged to one budget per block (see `work`), shared by all solver members: a block with two
//! `find`s gets the same total as one. It is per block, not per member, so a block cannot multiply
//! its cost by adding solver members. The solver also has an evaluation limit
//! ([`MAX_SOLVE_EVALUATIONS`]), so a hostile block cannot hang.
//!
//! **Guesses and outer names.** A solver's unknowns start at the value the name has in the
//! worksheet when the block runs, so a guess defined inside the block shadows an outer one. The
//! help says guess values can be defined "above the solve block region or inside the solve block"
//! (`working_with_guess_values.md`, with `working_with_guess_values2.png`: `a := -5` above
//! `maximize(f, a) = -4.613`), and the "About Guess Values" pages require a guess for every
//! unknown. So a name defined above the block with no guess inside it serves as the guess (`x :=
//! 1.5` above `find(x)` with `x^2 == 4` gives 2). A guess that fails leaves its name broken: a
//! solver member that names that unknown reports the guess's error, and a later successful
//! definition of the same name replaces it (last definition wins; the help does not say
//! otherwise). Other solver members are not affected by a broken guess they do not name.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use super::env::{Env, UserFunction};
use super::error::EvalError;
use super::expr_eval::var_key;
use super::ode_block;
use super::pde_block;
use super::session::{Outcome, Session};
use super::solve_block_rows::{iteration_charge, objective_fn, row_fn};
use super::solve_constraints::Constraint;
use super::value::{Matrix, Scalar, Value};
use super::work::Budget;
use crate::expr::{Expr, Id, IdKind};
use crate::solve::{find, minerr, optimize, Limits, Problem, DEFAULT_CTOL};
use crate::units::Dimension;

/// Outer iterations allowed to the solver for one solve.
pub const MAX_SOLVE_ITERATIONS: usize = 200;
/// BFGS iterations allowed to one `maximize` or `minimize`, over all its augmented-Lagrangian rounds.
pub const MAX_OPT_ITERATIONS: usize = 1000;
/// Constraint-closure calls allowed to the solver for one solve.
pub const MAX_SOLVE_EVALUATIONS: usize = 200_000;
/// Unknowns allowed in one solve block. A Jacobian costs `4 n` closure evaluations, each writing `n`
/// unknowns, so a block of `n` unknowns costs about `40 n^2` units per iteration, and the dense
/// elimination `n^3 / 21` more. Prime solve blocks have a handful of unknowns; 200 keeps any block
/// well inside the per-statement budget (`work::MAX_WORK`) and the solver's linear algebra under a
/// second. A larger block is refused with a message rather than run to the budget.
pub const MAX_BLOCK_UNKNOWNS: usize = 200;

/// What a solve-block member is, from its `solve-block-category`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    /// An ordinary definition that gives an unknown its starting value.
    Guess,
    /// An equation or inequality the solution must satisfy.
    Constraint,
    /// The `find` or `minerr` call.
    Solver,
}

/// One member of a solve block, in document order.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockMember {
    pub kind: MemberKind,
    /// The member's statement, or the reason it cannot be run (reported for the member).
    pub body: Result<Expr, String>,
}

/// The solver call of a solver member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Solver {
    Find,
    Minerr,
    Maximize,
    Minimize,
}

/// A solver member's call: its kind, its unknowns, and for `maximize` and `minimize` the objective
/// applied to the unknowns, `f(x, y, ...)`, which is evaluated at each point of the solve.
struct SolverCall {
    solver: Solver,
    unknowns: Vec<String>,
    objective: Option<Expr>,
}

/// Where a solver member's result goes.
#[derive(Debug, Clone, PartialEq)]
enum Target {
    /// `find(...)` alone: the value is shown, nothing is bound.
    Discard,
    /// `name := find(...)`.
    Name(String),
    /// `[a, b] := find(...)`: the names in row-major order, and the matrix shape.
    Matrix {
        rows: usize,
        cols: usize,
        names: Vec<String>,
    },
}

/// A solved member, ready to bind: the names and values to set, and the shown outcome.
struct Answer {
    bindings: Vec<(String, Value)>,
    /// `ERR` for `minerr`: the residual norm.
    err: Option<f64>,
    shown: Outcome,
}

/// Block-level state gathered while the members run.
#[derive(Default)]
struct BlockState {
    constraints: Vec<Constraint>,
    /// The first block-wide problem: a constraint that cannot be parsed or a member that cannot
    /// run. Every solver member fails with it.
    failure: Option<EvalError>,
    /// Guesses that failed, by the name they define. A later successful guess for the same name
    /// removes its entry (the last definition wins). A solver member fails with the entry of
    /// its first unknown that has one, so `find` reports the guess's own error, not `Undefined`.
    broken_guesses: BTreeMap<String, EvalError>,
}

impl Session {
    /// Runs the members of one solve block and returns one outcome per member, in order.
    ///
    /// Guesses and constraints are collected first; each solver member is then run against all
    /// of the block's constraints. See the module docs for the rules.
    pub fn run_solve_block(&mut self, members: &[BlockMember]) -> Vec<Outcome> {
        let budget = Rc::new(Budget::new());
        let outcomes = self.run_block_budget(members, Rc::clone(&budget));
        self.work_spent += budget.spent();
        outcomes
    }

    /// [`Session::run_solve_block`] with the block's budget supplied by the caller (tests read what
    /// the block spent).
    pub(super) fn run_block_budget(
        &mut self,
        members: &[BlockMember],
        budget: Rc<Budget>,
    ) -> Vec<Outcome> {
        let mut scratch = Session {
            env: self.env.clone(),
            work_spent: 0,
        };
        let mut outcomes: Vec<Option<Outcome>> = members.iter().map(|_| None).collect();
        let mut state = BlockState::default();
        // The block's one budget (the caller's): guesses and solver members all charge it.
        for (slot, member) in outcomes.iter_mut().zip(members) {
            if member.kind == MemberKind::Solver {
                continue;
            }
            let outcome = match (&member.body, member.kind) {
                (Err(reason), _) => {
                    let e = EvalError::Unsupported(reason.clone());
                    state.failure.get_or_insert(e.clone());
                    Outcome::Error(e)
                }
                (Ok(expr), MemberKind::Guess) => {
                    // The guess shares the block's budget with the solver members.
                    let outcome = scratch.run_budget(expr, &budget);
                    for name in target_names(expr) {
                        match &outcome {
                            Outcome::Error(e) => {
                                state.broken_guesses.insert(name, e.clone());
                            }
                            _ => {
                                state.broken_guesses.remove(&name);
                            }
                        }
                    }
                    outcome
                }
                (Ok(expr), _) => match Constraint::parse(expr) {
                    Ok(c) => {
                        state.constraints.push(c);
                        Outcome::Accepted
                    }
                    Err(e) => {
                        state.failure.get_or_insert(e.clone());
                        Outcome::Error(e)
                    }
                },
            };
            *slot = Some(outcome);
        }
        for (slot, member) in outcomes.iter_mut().zip(members) {
            if member.kind != MemberKind::Solver {
                continue;
            }
            // odesolve returns functions, not values, and has its own rules (see `ode_block`).
            if let Ok(body) = &member.body {
                if pde_block::is_pdesolve(member_rhs(body)) {
                    let names = target_names(body);
                    let result = match state.failure.clone() {
                        Some(e) => Err(e),
                        None => pde_block::solve_pdesolve(
                            &scratch.env,
                            member_rhs(body),
                            &names,
                            &state.constraints,
                            &budget,
                        ),
                    };
                    *slot = Some(self.commit_functions(result, &names));
                    continue;
                }
                if ode_block::is_odesolve(member_rhs(body)) {
                    let names = target_names(body);
                    let result = match state.failure.clone() {
                        Some(e) => Err(e),
                        None => ode_block::solve_odesolve(
                            &scratch.env,
                            member_rhs(body),
                            &names,
                            &state.constraints,
                            &budget,
                        ),
                    };
                    *slot = Some(self.commit_functions(result, &names));
                    continue;
                }
            }
            let result = match &member.body {
                Err(reason) => Err(EvalError::Unsupported(reason.clone())),
                Ok(body) => {
                    // A broken guess counts only for the unknowns this member names.
                    let unknown_error = solver_call(member_rhs(body)).ok().and_then(|call| {
                        call.unknowns
                            .iter()
                            .find_map(|n| state.broken_guesses.get(n).cloned())
                    });
                    match state.failure.clone().or(unknown_error) {
                        Some(e) => Err(e),
                        None => solve_member(&scratch.env, body, &state.constraints, &budget),
                    }
                }
            };
            let names = match &member.body {
                Ok(body) => target_names(body),
                Err(_) => Vec::new(),
            };
            *slot = Some(self.commit(result, &names));
        }
        outcomes
            .into_iter()
            .map(|o| {
                o.unwrap_or(Outcome::Error(EvalError::Unsupported(
                    "member not run".into(),
                )))
            })
            .collect()
    }

    /// Binds the solution functions of an `odesolve` member, or marks its target names as failed.
    /// One function shows as a definition; a vector of them shows nothing.
    fn commit_functions(
        &mut self,
        result: Result<Vec<(String, UserFunction)>, EvalError>,
        names: &[String],
    ) -> Outcome {
        match result {
            Ok(functions) => {
                let single = match functions.as_slice() {
                    [(name, _)] => Some(name.clone()),
                    _ => None,
                };
                for (name, function) in functions {
                    self.env.set_function(name, function);
                }
                match single {
                    Some(name) => Outcome::Defined {
                        name,
                        display: None,
                    },
                    None => Outcome::Accepted,
                }
            }
            Err(e) => {
                for name in names {
                    self.env.mark_failed(name);
                }
                Outcome::Error(e)
            }
        }
    }

    /// Binds a solved member in the worksheet, or marks its target names as failed.
    fn commit(&mut self, result: Result<Answer, EvalError>, names: &[String]) -> Outcome {
        match result {
            Ok(answer) => {
                for (name, value) in answer.bindings {
                    self.env.set_variable(name, value);
                }
                if let Some(err) = answer.err {
                    let dimensionless = Dimension::dimensionless();
                    self.env.set_variable(
                        "ERR".to_string(),
                        Value::Scalar(Scalar::real(err, dimensionless)),
                    );
                }
                answer.shown
            }
            Err(e) => {
                for name in names {
                    self.env.mark_failed(name);
                }
                Outcome::Error(e)
            }
        }
    }
}

/// The right-hand side of a solver member: the call itself, without the target.
fn member_rhs(body: &Expr) -> &Expr {
    match body {
        Expr::Define { expr, .. } => expr,
        other => other,
    }
}

/// Runs one solver member against the block's constraints, in the scratch worksheet `env`.
fn solve_member(
    env: &Env,
    body: &Expr,
    constraints: &[Constraint],
    budget: &Rc<Budget>,
) -> Result<Answer, EvalError> {
    let (target, rhs) = match body {
        Expr::Define { target, expr } => (target_of(Some(&**target))?, &**expr),
        other => (target_of(None)?, other),
    };
    let SolverCall {
        solver,
        unknowns,
        objective,
    } = solver_call(rhs)?;
    let optimizing = matches!(solver, Solver::Maximize | Solver::Minimize);
    // Find and minerr need constraints; an optimisation without any is an unconstrained search.
    if constraints.is_empty() && !optimizing {
        return Err(EvalError::Unsupported(
            "solve block has no constraints".to_string(),
        ));
    }
    let mut dims = Vec::with_capacity(unknowns.len());
    let mut guess = Vec::with_capacity(unknowns.len());
    for name in &unknowns {
        let (x, dim) = guess_of(env, name)?;
        guess.push(x);
        dims.push(dim);
    }
    let n = unknowns.len();
    if n > MAX_BLOCK_UNKNOWNS {
        return Err(EvalError::Unsupported(format!(
            "a solve block takes at most {MAX_BLOCK_UNKNOWNS} unknowns, this one has {n}"
        )));
    }
    if let Target::Matrix { rows, cols, .. } = target {
        if rows * cols != n {
            return Err(EvalError::Unsupported(format!(
                "a vector target of {} elements cannot take {n} unknowns",
                rows * cols
            )));
        }
    }

    let equalities: Vec<Constraint> = constraints
        .iter()
        .filter(|c| c.is_equality())
        .cloned()
        .collect();
    let inequalities: Vec<Constraint> = constraints
        .iter()
        .filter(|c| !c.is_equality())
        .cloned()
        .collect();
    let work = Rc::new(RefCell::new(env.clone()));
    let failure: Rc<RefCell<Option<EvalError>>> = Rc::new(RefCell::new(None));
    let ctol = ctol_of(env)?;
    let limits = Limits {
        max_iterations: if optimizing {
            MAX_OPT_ITERATIONS
        } else {
            MAX_SOLVE_ITERATIONS
        },
        max_evaluations: Some(MAX_SOLVE_EVALUATIONS),
    };
    let mut problem = Problem::new(guess).ctol(ctol).limits(limits);
    problem = problem.on_iteration(iteration_charge(budget, &failure));
    if !equalities.is_empty() {
        let f = row_fn(&work, &unknowns, &dims, equalities, budget, &failure);
        problem = problem.equations(f.0, f.1);
    }
    if !inequalities.is_empty() {
        let g = row_fn(&work, &unknowns, &dims, inequalities, budget, &failure);
        problem = problem.inequalities(g.0, g.1);
    }

    // (values, ERR) of a solved member: `minerr` reports its residual, the others report none.
    let solved: Result<(Vec<f64>, Option<f64>), crate::solve::SolveError> = match solver {
        Solver::Find => find(&problem).map(|sol| (sol.x, None)),
        Solver::Minerr => minerr(&problem).map(|sol| {
            let residual = sol.residual;
            (sol.x, Some(residual))
        }),
        Solver::Maximize | Solver::Minimize => {
            let sign = if solver == Solver::Maximize {
                -1.0
            } else {
                1.0
            };
            let Some(call) = objective else {
                return Err(EvalError::Unsupported(
                    "maximize and minimize need an objective function".to_string(),
                ));
            };
            problem = problem.objective(objective_fn(
                &work, &unknowns, &dims, call, budget, &failure, sign,
            ));
            optimize(&problem).map(|o| (o.x, None))
        }
    };
    if let Some(e) = failure.borrow_mut().take() {
        return Err(e);
    }
    let (x, err) = solved.map_err(|e| EvalError::Solve(e.to_string()))?;
    let values: Vec<Scalar> = x
        .iter()
        .zip(&dims)
        .map(|(x, d)| Scalar::real(*x, *d))
        .collect();
    build_answer(target, values, err)
}

/// Shapes the solution as the target asks and builds the bindings and the shown outcome.
fn build_answer(
    target: Target,
    values: Vec<Scalar>,
    err: Option<f64>,
) -> Result<Answer, EvalError> {
    let n = values.len();
    let column = |values: &[Scalar]| -> Result<Value, EvalError> {
        if n == 1 {
            Ok(Value::Scalar(values[0]))
        } else {
            Ok(Value::Matrix(Matrix::new(n, 1, values.to_vec())?))
        }
    };
    match target {
        Target::Discard => Ok(Answer {
            bindings: Vec::new(),
            err,
            shown: Outcome::Value(column(&values)?),
        }),
        Target::Name(name) => {
            let value = column(&values)?;
            Ok(Answer {
                bindings: vec![(name.clone(), value.clone())],
                err,
                shown: Outcome::Defined {
                    name,
                    display: Some(Box::new(Outcome::Value(value))),
                },
            })
        }
        Target::Matrix { rows, cols, names } => {
            let matrix = Matrix::new(rows, cols, values.clone())?;
            let bindings = names
                .into_iter()
                .zip(values)
                .map(|(name, s)| (name, Value::Scalar(s)))
                .collect();
            Ok(Answer {
                bindings,
                err,
                shown: Outcome::Value(Value::Matrix(matrix)),
            })
        }
    }
}

/// The solver call inside a solver member: its kind and the names of its unknowns. Parentheses
/// and `=` around the call are ignored.
fn solver_call(expr: &Expr) -> Result<SolverCall, EvalError> {
    let mut e = expr;
    while let Expr::Parens(inner) | Expr::Eval { expr: inner, .. } = e {
        e = inner;
    }
    let Expr::Call { func, args } = e else {
        return Err(EvalError::Unsupported(
            "a solve block member must be a guess, a constraint, or a solver call".to_string(),
        ));
    };
    let solver = match func.name.to_ascii_lowercase().as_str() {
        "find" => Solver::Find,
        "minerr" => Solver::Minerr,
        "maximize" => Solver::Maximize,
        "minimize" => Solver::Minimize,
        _ => {
            return Err(unavailable_solver(&func.name).unwrap_or_else(|| {
                EvalError::Unsupported(
                    "a solve block member must be a guess, a constraint, or a solver call"
                        .to_string(),
                )
            }))
        }
    };
    // maximize(f, x, ...) and minimize(f, x, ...) take the objective first; its name is kept, the
    // unknowns follow as for find.
    let (objective, unknown_args): (Option<Expr>, &[Expr]) =
        if matches!(solver, Solver::Maximize | Solver::Minimize) {
            let Some((first, rest)) = args.split_first() else {
                return Err(EvalError::Unsupported(
                    "maximize and minimize need the objective function and its unknowns"
                        .to_string(),
                ));
            };
            let Expr::Id(id) = first else {
                return Err(EvalError::Unsupported(
                    "the objective of maximize or minimize must be the name of a function"
                        .to_string(),
                ));
            };
            let key = var_key(id)?;
            (Some(Expr::call(&key, unknown_variables(rest)?)), rest)
        } else {
            (None, args)
        };
    let mut names: Vec<String> = Vec::with_capacity(unknown_args.len());
    for arg in unknown_args {
        match arg {
            // A unit-kind name here is still the unknown: Prime labels a contextual `v` as the
            // volt unit (17_rata_golfball_donau_lsg_2), but inside find(...) it names a variable.
            Expr::Id(id) if matches!(id.kind, IdKind::Variable | IdKind::Unit) => {
                let key = var_key(id)?;
                if names.contains(&key) {
                    return Err(EvalError::Unsupported(format!(
                        "unknown `{key}` is listed twice"
                    )));
                }
                names.push(key);
            }
            _ => {
                return Err(EvalError::Unsupported(
                    "find and minerr take the names of their unknowns".to_string(),
                ))
            }
        }
    }
    if names.is_empty() {
        return Err(EvalError::Unsupported(
            "find, minerr, maximize and minimize need at least one unknown".to_string(),
        ));
    }
    Ok(SolverCall {
        solver,
        unknowns: names,
        objective,
    })
}

/// The unknowns of a solver call as variable names in an expression: each argument must be a name.
fn unknown_variables(args: &[Expr]) -> Result<Vec<Expr>, EvalError> {
    args.iter()
        .map(|arg| match arg {
            Expr::Id(id) => Ok(Expr::Id(Id {
                kind: IdKind::Variable,
                ..id.clone()
            })),
            _ => Err(EvalError::Unsupported(
                "find, minerr, maximize and minimize take the names of their unknowns".to_string(),
            )),
        })
        .collect()
}

/// The error for a solver name used where no solver runs: `find` and `minerr` outside a solve
/// block, or a solve function that is not implemented yet. `None` for any other name. The builtin
/// lookup uses this for a name it does not know. `maximize` and `minimize` outside a block are
/// functions of their own (`optimize_form`), so they are not listed here.
pub(super) fn unavailable_solver(name: &str) -> Option<EvalError> {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "find" | "minerr" | "odesolve" => Some(EvalError::OutsideSolveBlock(title_case(&lower))),
        "pdesolve" => Some(EvalError::OutsideSolveBlock(title_case(&lower))),
        _ => None,
    }
}

/// `find` to `Find`.
fn title_case(lower: &str) -> String {
    let mut chars = lower.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The target of a solver member: `None` for a bare call, a name, or a vector of names.
fn target_of(target: Option<&Expr>) -> Result<Target, EvalError> {
    let name_of = |e: &Expr| match e {
        Expr::Id(id) if id.kind == IdKind::Variable => var_key(id),
        _ => Err(EvalError::Unsupported(
            "a solve result goes to a name or a vector of names".to_string(),
        )),
    };
    match target {
        None => Ok(Target::Discard),
        Some(Expr::Matrix {
            rows,
            cols,
            elements,
        }) => Ok(Target::Matrix {
            rows: *rows,
            cols: *cols,
            names: elements.iter().map(name_of).collect::<Result<_, _>>()?,
        }),
        Some(other) => Ok(Target::Name(name_of(other)?)),
    }
}

/// The names a solver member would bind, for marking them failed. Empty when the target is not
/// a name or a vector of names.
fn target_names(body: &Expr) -> Vec<String> {
    let Expr::Define { target, .. } = body else {
        return Vec::new();
    };
    match target_of(Some(target)) {
        Ok(Target::Name(name)) => vec![name],
        Ok(Target::Matrix { names, .. }) => names,
        _ => Vec::new(),
    }
}

/// The starting value of unknown `name`: its current scalar value and dimension.
fn guess_of(env: &Env, name: &str) -> Result<(f64, Dimension), EvalError> {
    if env.is_range(name) {
        return Err(EvalError::Unsupported(format!(
            "range variable `{name}` cannot be an unknown"
        )));
    }
    match env.variable(name) {
        Some(Value::Scalar(s)) if s.value.im == 0.0 => Ok((s.value.re, s.dim)),
        Some(Value::Scalar(_)) => Err(EvalError::Unsupported(format!(
            "complex guess value for `{name}`"
        ))),
        Some(_) => Err(EvalError::Unsupported(format!(
            "vector-valued unknown `{name}` is not supported yet"
        ))),
        None => Err(EvalError::Undefined(name.to_string())),
    }
}

/// CTOL from the worksheet: a positive real scalar, or the solver default when CTOL is not defined.
/// Any other value (a vector, text, a complex or non-positive number) is an error, not a silent
/// default, as for `TOL` and `ORIGIN` in the builtins.
fn ctol_of(env: &Env) -> Result<f64, EvalError> {
    match env.variable("CTOL") {
        None => Ok(DEFAULT_CTOL),
        Some(Value::Scalar(s)) if s.value.im == 0.0 && s.value.re > 0.0 => Ok(s.value.re),
        Some(_) => Err(EvalError::Unsupported(
            "CTOL must be a positive real number".to_string(),
        )),
    }
}
