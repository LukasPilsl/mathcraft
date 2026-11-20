//! Programming regions, checked against the help examples (`about_programs`, `using_programs`,
//! `example_looping_and_control`, `example_recursive_functions`, `example_using_the_try_on_error_operator`,
//! `using_conditional_statements`). The text syntax is not parsed yet, so each program is built
//! as its syntax tree.

use crate::eval::{EvalError, Outcome, Session, Value};
use crate::expr::{BinaryOp, BranchKind, Expr, Id, IdKind, Stmt, UnaryOp};

fn n(x: f64) -> Expr {
    Expr::num(x)
}

fn v(name: &str) -> Expr {
    Expr::var(name)
}

fn bin(op: BinaryOp, a: Expr, b: Expr) -> Expr {
    Expr::binary(op, a, b)
}

fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::call(name, args)
}

fn mat(rows: usize, cols: usize, elements: Vec<Expr>) -> Expr {
    Expr::matrix(rows, cols, elements)
}

fn prog(stmts: Vec<Stmt>) -> Expr {
    Expr::Program(stmts)
}

fn line(e: Expr) -> Stmt {
    Stmt::Line(e)
}

fn local(name: &str, e: Expr) -> Stmt {
    Stmt::Local {
        target: v(name),
        expr: e,
    }
}

fn element(name: &str, indices: Vec<Expr>) -> Expr {
    Expr::index(v(name), indices)
}

fn branch(kind: BranchKind, cond: Option<Expr>, body: Vec<Stmt>) -> Stmt {
    Stmt::Branch { kind, cond, body }
}

fn iff(c: Expr, body: Vec<Stmt>) -> Stmt {
    branch(BranchKind::If, Some(c), body)
}

fn else_if(c: Expr, body: Vec<Stmt>) -> Stmt {
    branch(BranchKind::ElseIf, Some(c), body)
}

fn also_if(c: Expr, body: Vec<Stmt>) -> Stmt {
    branch(BranchKind::AlsoIf, Some(c), body)
}

fn else_(body: Vec<Stmt>) -> Stmt {
    branch(BranchKind::Else, None, body)
}

fn for_in(var: &str, range: Expr, body: Vec<Stmt>) -> Stmt {
    Stmt::For {
        var: Id::new(var, IdKind::Variable),
        range,
        body,
    }
}

fn while_(cond: Expr, body: Vec<Stmt>) -> Stmt {
    Stmt::While { cond, body }
}

fn ret(e: Expr) -> Stmt {
    Stmt::Return(e)
}

/// `a .. b`, the range a `for` loop runs over.
/// `a mod b` as `a - b·floor(a/b)`; the `mod` builtin is not implemented yet.
fn modulo(a: Expr, b: Expr) -> Expr {
    bin(
        BinaryOp::Sub,
        a.clone(),
        bin(
            BinaryOp::Mul,
            b.clone(),
            call("floor", vec![bin(BinaryOp::Div, a, b)]),
        ),
    )
}

fn upto(a: f64, b: f64) -> Expr {
    Expr::range(n(a), None, n(b))
}

/// `name := rhs` on the worksheet.
fn def(name: &str, rhs: Expr) -> Expr {
    Expr::Define {
        target: Box::new(v(name)),
        expr: Box::new(rhs),
    }
}

/// `name(params) := body`
fn def_fn(name: &str, params: &[&str], body: Expr) -> Expr {
    Expr::Define {
        target: Box::new(call(name, params.iter().map(|p| v(p)).collect())),
        expr: Box::new(body),
    }
}

/// `expr =`
fn shown(expr: Expr) -> Expr {
    Expr::Eval {
        expr: Box::new(expr),
        unit_override: None,
    }
}

/// Runs statements in order on one session and returns the outcome of the last.
fn run_last(stmts: &[Expr]) -> Outcome {
    let mut s = Session::new();
    let mut out = None;
    for st in stmts {
        out = Some(s.run(st));
    }
    out.expect("at least one statement")
}

/// A real scalar value, or a panic naming the outcome.
fn real(out: Outcome) -> f64 {
    match out {
        Outcome::Value(Value::Scalar(s)) => {
            assert!(s.value.im == 0.0, "expected a real value, got {}", s.value);
            s.value.re
        }
        other => panic!("expected a real scalar, got {other:?}"),
    }
}

fn error(out: Outcome) -> EvalError {
    match out {
        Outcome::Error(e) => e,
        other => panic!("expected an error, got {other:?}"),
    }
}

/// The element values of a matrix outcome, row-major.
fn elements(out: Outcome) -> Vec<f64> {
    match out {
        Outcome::Value(Value::Matrix(m)) => m.data().iter().map(|s| s.value.re).collect(),
        other => panic!("expected a matrix, got {other:?}"),
    }
}

fn close(actual: f64, expected: f64, tol: f64) {
    assert!(
        (actual - expected).abs() <= tol,
        "{actual} is not {expected} (tolerance {tol})"
    );
}

/// Runs the definitions in `prefix`, then `last`, on one session.
fn with_defs(prefix: Vec<Expr>, last: Expr) -> Outcome {
    let mut stmts = prefix;
    stmts.push(last);
    run_last(&stmts)
}

// --- Value of a program -------------------------------------------------------------------

#[test]
fn value_is_the_last_line_that_gives_one() {
    // s ← 0; for i ∈ 0..3: s ← s + i; s  (0 + 1 + 2 + 3)
    let body = prog(vec![
        local("s", n(0.0)),
        for_in(
            "i",
            upto(0.0, 3.0),
            vec![local("s", bin(BinaryOp::Add, v("s"), v("i")))],
        ),
        line(v("s")),
    ]);
    close(real(run_last(&[shown(body)])), 6.0, 0.0);
}

#[test]
fn loop_without_a_closing_line_gives_the_last_assignment() {
    // about_programs: with no return value the result is the last calculation, PE_2.
    // for i ∈ 0..2: t ← i+2; PE ← 10·t  →  the last PE is 10·4
    let body = prog(vec![for_in(
        "i",
        upto(0.0, 2.0),
        vec![
            local("t", bin(BinaryOp::Add, v("i"), n(2.0))),
            local("PE", bin(BinaryOp::Mul, n(10.0), v("t"))),
        ],
    )]);
    close(real(run_last(&[shown(body)])), 40.0, 0.0);
}

#[test]
fn return_ends_the_program_with_its_value() {
    // return 5; x ← 1  → 5, the line after the return never runs.
    let body = prog(vec![ret(n(5.0)), local("x", n(1.0))]);
    close(real(run_last(&[shown(body)])), 5.0, 0.0);
}

#[test]
fn if_without_a_taken_branch_leaves_the_earlier_value() {
    // x ← 1; if 0: return 9  → the if gives nothing, so the value is x = 1.
    let body = prog(vec![local("x", n(1.0)), iff(n(0.0), vec![ret(n(9.0))])]);
    close(real(run_last(&[shown(body)])), 1.0, 0.0);
}

#[test]
fn program_with_no_value_is_an_error() {
    let body = prog(vec![iff(n(0.0), vec![line(n(1.0))])]);
    assert!(matches!(
        error(run_last(&[shown(body)])),
        EvalError::Unsupported(_)
    ));
}

// --- Conditionals (using_conditional_statements) ----------------------------------------

/// `r ← 0; <branches>; r` with `x` defined as `x_value`.
fn chain_result(x_value: f64, branches: Vec<Stmt>) -> f64 {
    let mut stmts = vec![local("r", n(0.0))];
    stmts.extend(branches);
    stmts.push(line(v("r")));
    real(with_defs(vec![def("x", n(x_value))], shown(prog(stmts))))
}

fn else_if_chain() -> Vec<Stmt> {
    vec![
        iff(
            bin(BinaryOp::Gt, v("x"), n(1.0)),
            vec![local("r", bin(BinaryOp::Add, v("r"), n(1.0)))],
        ),
        else_if(
            bin(BinaryOp::Gt, v("x"), n(2.0)),
            vec![local("r", bin(BinaryOp::Add, v("r"), n(10.0)))],
        ),
        else_(vec![local("r", bin(BinaryOp::Add, v("r"), n(100.0)))]),
    ]
}

fn also_if_chain() -> Vec<Stmt> {
    vec![
        iff(
            bin(BinaryOp::Gt, v("x"), n(1.0)),
            vec![local("r", bin(BinaryOp::Add, v("r"), n(1.0)))],
        ),
        also_if(
            bin(BinaryOp::Gt, v("x"), n(2.0)),
            vec![local("r", bin(BinaryOp::Add, v("r"), n(10.0)))],
        ),
        else_(vec![local("r", bin(BinaryOp::Add, v("r"), n(100.0)))]),
    ]
}

#[test]
fn else_if_stops_at_the_first_true_branch() {
    // The help's f(3) = ["x>1", 0, 0, 0]: only the first true branch runs.
    close(chain_result(3.0, else_if_chain()), 1.0, 0.0);
    close(chain_result(2.0, else_if_chain()), 1.0, 0.0);
    close(chain_result(0.0, else_if_chain()), 100.0, 0.0);
}

#[test]
fn also_if_keeps_every_true_branch_and_skips_else() {
    // f(3) with also if: both "x>1" and "x>2" run; else does not.
    close(chain_result(3.0, also_if_chain()), 11.0, 0.0);
    close(chain_result(2.0, also_if_chain()), 1.0, 0.0);
    close(chain_result(0.0, also_if_chain()), 100.0, 0.0);
}

#[test]
fn else_must_follow_an_if() {
    let body = prog(vec![local("x", n(0.0)), else_(vec![line(n(1.0))])]);
    assert!(matches!(
        error(run_last(&[shown(body)])),
        EvalError::Unsupported(_)
    ));
}

// --- Loops (example_looping_and_control, operators_for_exiting_loops_and_programs) ----------

#[test]
fn continue_skips_the_rest_of_the_pass() {
    // s ← 0; for i ∈ 0..7: if mod(i, 2) = 0: continue; s ← s + i; s  →  1+3+5+7 = 16
    let body = prog(vec![
        local("s", n(0.0)),
        for_in(
            "i",
            upto(0.0, 7.0),
            vec![
                iff(
                    bin(BinaryOp::Eq, modulo(v("i"), n(2.0)), n(0.0)),
                    vec![Stmt::Continue],
                ),
                local("s", bin(BinaryOp::Add, v("s"), v("i"))),
            ],
        ),
        line(v("s")),
    ]);
    close(real(run_last(&[shown(body)])), 16.0, 0.0);
}

#[test]
fn break_leaves_the_loop_and_keeps_the_last_value() {
    // s ← 0; for i ∈ 0..10: s ← s + 1; if i = 2: break; s  → 3 passes
    let body = prog(vec![
        local("s", n(0.0)),
        for_in(
            "i",
            upto(0.0, 10.0),
            vec![
                local("s", bin(BinaryOp::Add, v("s"), n(1.0))),
                iff(bin(BinaryOp::Eq, v("i"), n(2.0)), vec![Stmt::Break]),
            ],
        ),
        line(v("s")),
    ]);
    close(real(run_last(&[shown(body)])), 3.0, 0.0);
}

#[test]
fn while_repeats_until_the_condition_is_zero() {
    // sqrt(37) by Newton: estsqrt ← 1; while |estsqrt² − a| ≥ ε: estsqrt ← ½(estsqrt + a/estsqrt)
    let a = 37.0;
    let eps = 0.00005;
    let test = bin(
        BinaryOp::Ge,
        Expr::unary(
            UnaryOp::Abs,
            bin(BinaryOp::Sub, bin(BinaryOp::Pow, v("e"), n(2.0)), n(a)),
        ),
        n(eps),
    );
    let step = bin(
        BinaryOp::Mul,
        n(0.5),
        bin(BinaryOp::Add, v("e"), bin(BinaryOp::Div, n(a), v("e"))),
    );
    let body = prog(vec![
        local("e", n(1.0)),
        while_(test, vec![local("e", step)]),
        line(v("e")),
    ]);
    close(real(run_last(&[shown(body)])), 37f64.sqrt(), 1e-4);
}

#[test]
fn while_with_break_stops_at_the_bound() {
    // i ← 0; while 1: i ← i + 1; if i = 5: break; i  → 5 (the infinite test ends by break)
    let body = prog(vec![
        local("i", n(0.0)),
        while_(
            n(1.0),
            vec![
                local("i", bin(BinaryOp::Add, v("i"), n(1.0))),
                iff(bin(BinaryOp::Eq, v("i"), n(5.0)), vec![Stmt::Break]),
            ],
        ),
        line(v("i")),
    ]);
    close(real(run_last(&[shown(body)])), 5.0, 0.0);
}

#[test]
fn infinite_while_ends_with_the_work_limit() {
    // while 1: c ← 1  never ends by itself; the budget ends it.
    let body = prog(vec![while_(n(1.0), vec![local("c", n(1.0))])]);
    assert!(matches!(
        error(run_last(&[shown(body)])),
        EvalError::WorkLimit(_)
    ));
}

#[test]
fn try_does_not_catch_the_work_limit() {
    // try: while 1: c ← 1; on error: r ← 5  -- the on-error block must not run.
    let body = prog(vec![Stmt::Try {
        body: vec![while_(n(1.0), vec![local("c", n(1.0))])],
        on_error: vec![line(n(5.0))],
    }]);
    assert!(matches!(
        error(run_last(&[shown(body)])),
        EvalError::WorkLimit(_)
    ));
}

#[test]
fn loop_variable_is_gone_after_its_loop() {
    // for idx ∈ 0..2: x ← idx; idx   → idx is not defined after the loop.
    let body = prog(vec![
        for_in("idx", upto(0.0, 2.0), vec![local("x", v("idx"))]),
        line(v("idx")),
    ]);
    assert!(matches!(
        error(run_last(&[shown(body)])),
        EvalError::Undefined(name) if name == "idx"
    ));
}

#[test]
fn a_loop_body_local_stays_after_the_loop() {
    // for k ∈ 0..2: x ← k·3  →  x = 6 after the loop (a loop is not a scope).
    let body = prog(vec![
        for_in(
            "k",
            upto(0.0, 2.0),
            vec![local("x", bin(BinaryOp::Mul, v("k"), n(3.0)))],
        ),
        line(v("x")),
    ]);
    close(real(run_last(&[shown(body)])), 6.0, 0.0);
}

// --- Functions and recursion (example_recursive_functions) ---------------------------------

#[test]
fn recursive_factorial_by_return() {
    // Fac(n) := if n > 1: return n·Fac(n−1); else: return 1
    let fac = prog(vec![
        iff(
            bin(BinaryOp::Gt, v("n"), n(1.0)),
            vec![ret(bin(
                BinaryOp::Mul,
                v("n"),
                call("Fac", vec![bin(BinaryOp::Sub, v("n"), n(1.0))]),
            ))],
        ),
        else_(vec![ret(n(1.0))]),
    ]);
    let out = run_last(&[def_fn("Fac", &["n"], fac), shown(call("Fac", vec![n(6.0)]))]);
    close(real(out), 720.0, 0.0);
}

#[test]
fn recursive_factorial_of_forty_fits_a_small_stack() {
    // Each level of a program call adds a few node levels; forty of them stay under the limit and
    // the native stack of a 2 MiB thread (the debug-build default for tests).
    let fac = prog(vec![
        iff(
            bin(BinaryOp::Gt, v("n"), n(1.0)),
            vec![ret(bin(
                BinaryOp::Mul,
                v("n"),
                call("Fac", vec![bin(BinaryOp::Sub, v("n"), n(1.0))]),
            ))],
        ),
        else_(vec![ret(n(1.0))]),
    ]);
    let out = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let mut s = Session::new();
            s.run(&def_fn("Fac", &["n"], fac));
            s.run(&shown(call("Fac", vec![n(40.0)])))
        })
        .expect("spawn")
        .join()
        .expect("no panic, no stack overflow");
    let expected = (1..=40).fold(1.0f64, |acc, k| acc * k as f64);
    close(real(out), expected, expected * 1e-12);
}

#[test]
fn recursive_gcd_by_return() {
    // GCD(x, y) := if x = 0: return y; else: return GCD(mod(y, x), x)
    let gcd = prog(vec![
        iff(bin(BinaryOp::Eq, v("x"), n(0.0)), vec![ret(v("y"))]),
        else_(vec![ret(call("GCD", vec![modulo(v("y"), v("x")), v("x")]))]),
    ]);
    let out = run_last(&[
        def_fn("GCD", &["x", "y"], gcd),
        shown(call("GCD", vec![n(945.0), n(90.0)])),
    ]);
    close(real(out), 45.0, 0.0);
}

#[test]
fn fibonacci_by_two_recursive_calls() {
    // fib(n) := if n > 1: return fib(n−1) + fib(n−2); else: return 1  →  fib(4) = 5
    let fib = prog(vec![
        iff(
            bin(BinaryOp::Gt, v("n"), n(1.0)),
            vec![ret(bin(
                BinaryOp::Add,
                call("fib", vec![bin(BinaryOp::Sub, v("n"), n(1.0))]),
                call("fib", vec![bin(BinaryOp::Sub, v("n"), n(2.0))]),
            ))],
        ),
        else_(vec![ret(n(1.0))]),
    ]);
    let out = run_last(&[def_fn("fib", &["n"], fib), shown(call("fib", vec![n(4.0)]))]);
    close(real(out), 5.0, 0.0);
}

#[test]
fn function_program_reads_its_parameter_and_locals() {
    // f(x) := s ← x·2; s   →  f(3) = 6, and the local s is not a worksheet name.
    let f = prog(vec![
        local("s", bin(BinaryOp::Mul, v("x"), n(2.0))),
        line(v("s")),
    ]);
    let out = run_last(&[def_fn("f", &["x"], f), shown(call("f", vec![n(3.0)]))]);
    close(real(out), 6.0, 0.0);
}

#[test]
fn function_locals_do_not_leak_to_the_worksheet() {
    let f = prog(vec![local("s", n(7.0)), line(v("s"))]);
    let mut session = Session::new();
    assert!(matches!(
        session.run(&def_fn("f", &["x"], f)),
        Outcome::Defined { .. }
    ));
    close(real(session.run(&shown(call("f", vec![n(0.0)])))), 7.0, 0.0);
    assert!(matches!(
        error(session.run(&shown(v("s")))),
        EvalError::Undefined(_)
    ));
}

#[test]
fn definition_with_loop_variable_is_not_an_undefined_name() {
    // The loop variable k is bound by the program, so the definition itself is fine.
    let f = prog(vec![
        for_in("k", upto(0.0, 1.0), vec![local("s", v("k"))]),
        line(v("s")),
    ]);
    let mut session = Session::new();
    assert!(matches!(
        session.run(&def_fn("g", &["x"], f)),
        Outcome::Defined { .. }
    ));
    close(real(session.run(&shown(call("g", vec![n(0.0)])))), 1.0, 0.0);
}

#[test]
fn deep_recursion_is_an_error_not_a_crash() {
    // f(x) := return f(x) never ends; the call limits stop it on a small stack.
    let f = prog(vec![ret(call("f", vec![v("x")]))]);
    let out = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let mut s = Session::new();
            s.run(&def_fn("f", &["x"], f));
            s.run(&shown(call("f", vec![n(1.0)])))
        })
        .expect("spawn")
        .join()
        .expect("no panic, no stack overflow");
    assert!(matches!(
        error(out),
        EvalError::CallLimit(_) | EvalError::DepthLimit(_) | EvalError::WorkLimit(_)
    ));
}

// --- try / on error (example_using_the_try_on_error_operator) -------------------------------

/// `Y ← Z; for x ∈ 0..1: for y ∈ 0..1: try: Y[x,y] ← (x+y)/(x−y); on error: <handler>; Y`, with
/// `Z = [1, 2; 3, 4]` on the worksheet. The element (x,x) is a division by zero.
fn divide_matrix_with(handler: Vec<Stmt>) -> Expr {
    let quotient = bin(
        BinaryOp::Div,
        bin(BinaryOp::Add, v("x"), v("y")),
        bin(BinaryOp::Sub, v("x"), v("y")),
    );
    prog(vec![
        local("Y", v("Z")),
        for_in(
            "x",
            upto(0.0, 1.0),
            vec![for_in(
                "y",
                upto(0.0, 1.0),
                vec![Stmt::Try {
                    body: vec![Stmt::Local {
                        target: element("Y", vec![v("x"), v("y")]),
                        expr: quotient,
                    }],
                    on_error: handler,
                }],
            )],
        ),
        line(v("Y")),
    ])
}

fn z_definition() -> Expr {
    def("Z", mat(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)]))
}

fn minus_one() -> Stmt {
    Stmt::Local {
        target: element("Y", vec![v("x"), v("y")]),
        expr: n(-1.0),
    }
}

#[test]
fn try_on_error_sets_the_element_and_the_program_goes_on() {
    // Continue-like behaviour: every element is visited. Y = [-1, -1; 1, -1]
    let body = divide_matrix_with(vec![minus_one()]);
    let out = with_defs(vec![z_definition()], shown(body));
    assert_eq!(elements(out), vec![-1.0, -1.0, 1.0, -1.0]);
}

#[test]
fn break_in_on_error_leaves_only_the_inner_loop() {
    // Y1: break after the handler. Row 0 keeps its second element (2), row 1 is filled.
    let body = divide_matrix_with(vec![minus_one(), Stmt::Break]);
    let out = with_defs(vec![z_definition()], shown(body));
    assert_eq!(elements(out), vec![-1.0, 2.0, 1.0, -1.0]);
}

#[test]
fn return_in_on_error_ends_the_program() {
    // Y3: return Y from the handler. Only the first element was set.
    let body = divide_matrix_with(vec![minus_one(), ret(v("Y"))]);
    let out = with_defs(vec![z_definition()], shown(body));
    assert_eq!(elements(out), vec![-1.0, 2.0, 3.0, 4.0]);
}

// --- Assignments and scope (local_assignment_operator) ----------------------------------

#[test]
fn local_assignment_does_not_change_the_worksheet() {
    // a := 2; prog: a ← 5; a  → 5, and the worksheet a is still 2.
    let body = prog(vec![local("a", n(5.0)), line(v("a"))]);
    let mut session = Session::new();
    session.run(&Expr::Define {
        target: Box::new(v("a")),
        expr: Box::new(n(2.0)),
    });
    close(real(session.run(&shown(body))), 5.0, 0.0);
    close(real(session.run(&shown(v("a")))), 2.0, 0.0);
}

#[test]
fn a_local_starts_from_the_worksheet_value() {
    // b ← a reads the worksheet a (2) before any local a exists.
    let body = prog(vec![local("b", v("a")), line(v("b"))]);
    let mut session = Session::new();
    session.run(&Expr::Define {
        target: Box::new(v("a")),
        expr: Box::new(n(2.0)),
    });
    close(real(session.run(&shown(body))), 2.0, 0.0);
}

#[test]
fn nested_program_names_end_with_the_nested_program() {
    // x ← prog{ y ← 2; y·3 }; x + 1  →  7, and y is gone.
    let inner = prog(vec![
        local("y", n(2.0)),
        line(bin(BinaryOp::Mul, v("y"), n(3.0))),
    ]);
    let body = prog(vec![
        local("x", inner),
        line(bin(BinaryOp::Add, v("x"), n(1.0))),
    ]);
    close(real(run_last(&[shown(body)])), 7.0, 0.0);
    let leaked = prog(vec![line(prog(vec![local("y", n(2.0))])), line(v("y"))]);
    assert!(matches!(
        error(run_last(&[shown(leaked)])),
        EvalError::Undefined(_)
    ));
}

#[test]
fn indexed_assignment_builds_a_local_copy() {
    // W is on the worksheet; the program's w ← [0;0;0] and w[k] ← k+1 leave W alone.
    let body = prog(vec![
        local("w", mat(3, 1, vec![n(0.0), n(0.0), n(0.0)])),
        for_in(
            "k",
            upto(0.0, 2.0),
            vec![Stmt::Local {
                target: element("w", vec![v("k")]),
                expr: bin(BinaryOp::Add, v("k"), n(1.0)),
            }],
        ),
        line(v("w")),
    ]);
    let out = with_defs(
        vec![def("W", mat(3, 1, vec![n(9.0), n(9.0), n(9.0)]))],
        shown(body),
    );
    assert_eq!(elements(out), vec![1.0, 2.0, 3.0]);
}

#[test]
fn matrix_element_assignment_with_two_indices_on_a_worksheet_array() {
    // M := [1, 2; 3, 4]; prog: M[1,0] ← 9 (a local copy); M[1,0] (the copy's element).
    let body = prog(vec![
        Stmt::Local {
            target: element("M", vec![n(1.0), n(0.0)]),
            expr: n(9.0),
        },
        line(element("M", vec![n(1.0), n(0.0)])),
    ]);
    let mut session = Session::new();
    session.run(&Expr::Define {
        target: Box::new(v("M")),
        expr: Box::new(mat(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)])),
    });
    close(real(session.run(&shown(body))), 9.0, 0.0);
    close(
        real(session.run(&shown(element("M", vec![n(1.0), n(0.0)])))),
        3.0,
        0.0,
    );
}

#[test]
fn element_read_with_a_local_index() {
    // w := [10; 20; 30]; for k ∈ 0..2: s ← s + w_k  →  60; w_k reads the element named by k.
    let body = prog(vec![
        local("s", n(0.0)),
        for_in(
            "k",
            upto(0.0, 2.0),
            vec![local(
                "s",
                bin(BinaryOp::Add, v("s"), Expr::subscripted("w", v("k"))),
            )],
        ),
        line(v("s")),
    ]);
    let mut session = Session::new();
    session.run(&Expr::Define {
        target: Box::new(v("w")),
        expr: Box::new(mat(3, 1, vec![n(10.0), n(20.0), n(30.0)])),
    });
    close(real(session.run(&shown(body))), 60.0, 0.0);
}

#[test]
fn index_out_of_range_is_an_error() {
    // A read past the end is an error (a write grows the vector instead, see the tests below).
    let body = prog(vec![
        local("w", mat(2, 1, vec![n(0.0), n(0.0)])),
        line(element("w", vec![n(5.0)])),
    ]);
    assert!(matches!(
        error(run_last(&[shown(body)])),
        EvalError::IndexOutOfRange { .. }
    ));
}

#[test]
fn break_outside_a_loop_is_an_error() {
    let body = prog(vec![Stmt::Break]);
    assert!(matches!(
        error(run_last(&[shown(body)])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn printed_program_has_one_line_per_statement() {
    let body = prog(vec![
        local("s", n(0.0)),
        for_in(
            "i",
            upto(0.0, 7.0),
            vec![iff(
                bin(BinaryOp::Eq, modulo(v("i"), n(2.0)), n(0.0)),
                vec![Stmt::Continue],
            )],
        ),
        line(v("s")),
    ]);
    assert_eq!(
        body.to_string(),
        "s ← 0\nfor i ∈ 0 .. 7\n    if i-2*floor(i/2)==0\n        continue\ns"
    );
}

// --- Nesting depth (native stack on a 2 MiB thread) ----------------------------------------

#[derive(Clone, Copy, Debug)]
enum Nest {
    For,
    While,
    Try,
    If,
}

/// `depth` statements of one kind, nested, around a one-line innermost body. Each `while` ends
/// after one pass (its body ends with `break`), and each `for` runs over `0..0`.
fn nested(kind: Nest, depth: usize) -> Vec<Stmt> {
    let mut body = vec![line(n(1.0))];
    for _ in 0..depth {
        body = vec![match kind {
            Nest::For => for_in("k", upto(0.0, 0.0), body),
            Nest::While => {
                let mut inner = body;
                inner.push(Stmt::Break);
                while_(n(1.0), inner)
            }
            Nest::Try => Stmt::Try {
                body,
                on_error: vec![line(n(0.0))],
            },
            Nest::If => iff(n(1.0), body),
        }];
    }
    body
}

/// The outcome of `depth` nested statements of `kind`, followed by `line(7)`, run on a 2 MiB thread.
fn run_nested_on_small_stack(kind: Nest, depth: usize) -> Outcome {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let mut stmts = nested(kind, depth);
            stmts.push(line(n(7.0)));
            let mut s = Session::new();
            s.run(&shown(prog(stmts)))
        })
        .expect("spawn")
        .join()
        .expect("no panic, no stack overflow")
}

#[test]
fn nested_statements_stop_cleanly_on_a_small_stack() {
    // Every statement kind, shallow and deep: shallow nests give the value, deep ones stop at the
    // depth limit. A native stack overflow would abort the test process.
    for kind in [Nest::For, Nest::While, Nest::Try, Nest::If] {
        for depth in [1, 10, 50, 100, 130] {
            assert!(
                matches!(run_nested_on_small_stack(kind, depth), Outcome::Value(_)),
                "{kind:?} nested {depth} deep should give the value"
            );
        }
        for depth in [400, 1000] {
            assert!(
                matches!(
                    error(run_nested_on_small_stack(kind, depth)),
                    EvalError::DepthLimit(_)
                ),
                "{kind:?} nested {depth} deep should stop at the depth limit"
            );
        }
    }
}

#[test]
fn nested_ifs_of_three_hundred_ninety_levels_still_run() {
    assert!(matches!(
        run_nested_on_small_stack(Nest::If, 390),
        Outcome::Value(_)
    ));
}

// --- Element writes: units, growth and budget -------------------------------------------------

/// `m`, the metre, as an expression.
fn metre() -> Expr {
    Expr::Id(Id::new("m", IdKind::Unit))
}

#[test]
fn element_write_with_other_units_is_refused() {
    // v := [1; 2]; v_0 ← 3 m  → the element would be a length in a dimensionless array.
    let body = prog(vec![
        Stmt::Local {
            target: element("v", vec![n(0.0)]),
            expr: Expr::Scale {
                value: Box::new(n(3.0)),
                unit: Box::new(metre()),
            },
        },
        line(v("v")),
    ]);
    let out = with_defs(vec![def("v", mat(2, 1, vec![n(1.0), n(2.0)]))], shown(body));
    assert!(matches!(
        error(out),
        EvalError::Unsupported(msg) if msg == "units differ from the array"
    ));
}

#[test]
fn element_write_with_the_same_units_is_kept() {
    // v := [1; 2]; v_0 ← 3  → [3; 2] (both dimensionless).
    let body = prog(vec![
        Stmt::Local {
            target: element("v", vec![n(0.0)]),
            expr: n(3.0),
        },
        line(v("v")),
    ]);
    let out = with_defs(vec![def("v", mat(2, 1, vec![n(1.0), n(2.0)]))], shown(body));
    assert_eq!(elements(out), vec![3.0, 2.0]);
}

#[test]
fn vector_write_past_the_end_grows_it() {
    // Help, task 3-1: vec has two rows; for i ∈ 0..2: vec_i ← i leaves three rows, [0; 1; 2].
    let body = prog(vec![
        local("w", mat(2, 1, vec![n(9.0), n(9.0)])),
        for_in(
            "i",
            upto(0.0, 2.0),
            vec![Stmt::Local {
                target: element("w", vec![v("i")]),
                expr: v("i"),
            }],
        ),
        line(v("w")),
    ]);
    assert_eq!(elements(run_last(&[shown(body)])), vec![0.0, 1.0, 2.0]);
}

#[test]
fn vector_write_far_past_the_end_grows_with_zeros() {
    // w := [5; 5]; w_4 ← 9  → [5; 5; 0; 0; 9].
    let body = prog(vec![
        local("w", mat(2, 1, vec![n(5.0), n(5.0)])),
        Stmt::Local {
            target: element("w", vec![n(4.0)]),
            expr: n(9.0),
        },
        line(v("w")),
    ]);
    assert_eq!(
        elements(run_last(&[shown(body)])),
        vec![5.0, 5.0, 0.0, 0.0, 9.0]
    );
}

#[test]
fn matrix_write_past_the_end_is_refused() {
    let body = prog(vec![
        local("M", mat(2, 2, vec![n(0.0), n(0.0), n(0.0), n(0.0)])),
        Stmt::Local {
            target: element("M", vec![n(2.0), n(0.0)]),
            expr: n(1.0),
        },
    ]);
    assert!(matches!(
        error(run_last(&[shown(body)])),
        EvalError::IndexOutOfRange { .. }
    ));
}

#[test]
fn a_hundred_thousand_element_reads_and_writes_fit_the_budget() {
    // W is a worksheet column of 100000 zeros. A local copy is written, then read, element by
    // element: each access costs a constant, so both loops finish. The sum is 0 + 1 + ... + 99999.
    let size = 100_000;
    let zeros: Vec<Expr> = vec![n(0.0); size];
    let body = prog(vec![
        local("w", v("W")),
        local("s", n(0.0)),
        for_in(
            "i",
            upto(0.0, (size - 1) as f64),
            vec![Stmt::Local {
                target: element("w", vec![v("i")]),
                expr: v("i"),
            }],
        ),
        for_in(
            "i",
            upto(0.0, (size - 1) as f64),
            vec![local(
                "s",
                bin(BinaryOp::Add, v("s"), Expr::subscripted("w", v("i"))),
            )],
        ),
        line(v("s")),
    ]);
    let out = with_defs(vec![def("W", mat(size, 1, zeros))], shown(body));
    close(real(out), 4_999_950_000.0, 0.0);
}

#[test]
fn assignment_to_a_function_is_refused_before_its_value_is_evaluated() {
    // f(x) ← x + 1 at worksheet level: the message names the function, not the undefined x.
    let body = prog(vec![
        Stmt::Local {
            target: call("f", vec![v("x")]),
            expr: bin(BinaryOp::Add, v("x"), n(1.0)),
        },
        line(n(0.0)),
    ]);
    assert!(matches!(
        error(run_last(&[shown(body)])),
        EvalError::Unsupported(msg) if msg.contains("function")
    ));
}

// --- Builtins used by programs (conditional_branching, gcd_lcm_and_mod, combinations) --------

#[test]
fn if_evaluates_only_the_branch_it_takes() {
    // if(1, 2, 1/0) = 2 and if(0, 1/0, 5) = 5: the untaken division by zero is never evaluated.
    let div_zero = bin(BinaryOp::Div, n(1.0), n(0.0));
    close(
        real(run_last(&[shown(call(
            "if",
            vec![n(1.0), n(2.0), div_zero.clone()],
        ))])),
        2.0,
        0.0,
    );
    close(
        real(run_last(&[shown(call(
            "if",
            vec![n(0.0), div_zero, n(5.0)],
        ))])),
        5.0,
        0.0,
    );
}

#[test]
fn integer_builtins_from_the_help_examples() {
    let value = |name: &str, args: Vec<Expr>| real(run_last(&[shown(call(name, args))]));
    close(value("gcd", vec![n(945.0), n(90.0)]), 45.0, 0.0);
    close(value("lcm", vec![n(4.0), n(6.0)]), 12.0, 0.0);
    close(value("mod", vec![n(-7.0), n(3.0)]), -1.0, 0.0);
    close(value("permut", vec![n(5.0), n(2.0)]), 20.0, 0.0);
    close(value("combin", vec![n(5.0), n(2.0)]), 10.0, 0.0);
    close(value("Re", vec![n(3.0)]), 3.0, 0.0);
    close(value("Im", vec![n(3.0)]), 0.0, 0.0);
}
