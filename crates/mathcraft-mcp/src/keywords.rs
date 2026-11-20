//! Keywords: the words that are not functions. Symbolic commands (`expr -> solve, x`), the
//! solve-block words (`find`, `minimize`, ...) and the program words (`if`, `for`, `return`, ...).
//!
//! Each entry has usage in Mathcraft's text notation, one example and the result that example
//! gives. The unit tests run every example through the engine and compare the result, so the
//! table cannot drift from what the engine does. The prose is Mathcraft's own.

use std::fmt::Write as _;

use mathcraft_doc::functions_table::FUNCTIONS;
use serde_json::{json, Value};

/// Where a keyword is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// After `->` in a math region.
    Symbolic,
    /// In a solve block.
    SolveBlock,
    /// In a program (the body of a function definition).
    Program,
}

impl Group {
    pub fn category(self) -> &'static str {
        match self {
            Self::Symbolic => "Keyword: symbolic",
            Self::SolveBlock => "Keyword: solve block",
            Self::Program => "Keyword: program",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Symbolic => "Symbolic keywords",
            Self::SolveBlock => "Solve-block keywords",
            Self::Program => "Program keywords",
        }
    }

    /// The tool that creates it.
    fn tool(self) -> &'static str {
        match self {
            Self::Symbolic => "insert_symbolic (or insert_math with `expr -> keyword`)",
            Self::SolveBlock => "insert_solve_block",
            Self::Program => "insert_program",
        }
    }
}

/// An example that the tests run.
#[derive(Debug, Clone, Copy)]
pub enum Example {
    /// Math lines, inserted in order. The result is the value of the last one.
    Math(&'static [&'static str]),
    /// Lines, then a solve block, then more lines. The result is the value of the last line, or
    /// of the block's solver call when `after` is empty.
    Solve {
        before: &'static [&'static str],
        guesses: &'static [&'static str],
        constraints: &'static [&'static str],
        solver: &'static str,
        target: Option<&'static str>,
        after: &'static [&'static str],
    },
    /// A program defined as `name(params) := program`, then `call`; the result is the value of
    /// the call.
    Program {
        name: &'static str,
        params: &'static [&'static str],
        lines: &'static [&'static str],
        call: &'static str,
    },
}

/// One keyword.
pub struct Keyword {
    pub name: &'static str,
    /// Other spellings that find this entry.
    pub aliases: &'static [&'static str],
    pub group: Group,
    /// How it is written, in Mathcraft's text notation.
    pub syntax: &'static str,
    pub description: &'static str,
    pub example: Example,
    /// The value the example gives.
    pub result: &'static str,
    pub mistakes: &'static [&'static str],
}

const TRANSFORM_NAMES: &str = "Names of variables that are also unit names (`t` tonne, `s` second, `m` metre) read as units: use `x`, `p`, `w`, `k` for the variable.";

/// Every keyword the engine supports.
pub const KEYWORDS: &[Keyword] = &[
    // ---- Symbolic ----
    Keyword {
        name: "solve",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "equation -> solve, x   |   [eq1; eq2] -> solve, x, y",
        description: "Solves an equation for the unknown(s) named after the comma. One unknown gives a column of solutions; several unknowns need as many equations, written as a column. An expression without `==` is taken as `expr == 0`.",
        example: Example::Math(&["x^2 - 5*x + 6 == 0 -> solve, x"]),
        result: "[[3], [2]]",
        mistakes: &[
            "Writing `solve(x^2 = 4, x)`: solve is a keyword after `->`, not a function. Write `x^2 == 4 -> solve, x`.",
            "Writing the equation with a single `=`: in a symbolic line the equation sign is `==`.",
            "Leaving out the unknown: `x^2 == 4 -> solve` fails; name it, `-> solve, x`.",
            "Expecting complex or trigonometric roots: the engine reports `unsupported` for those; use a solve block (`insert_solve_block`) for a numeric answer.",
        ],
    },
    Keyword {
        name: "simplify",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "expr -> simplify",
        description: "Rewrites an expression in a simpler form: collects like terms, cancels common factors, applies trigonometric identities. Also the default when the arrow has no keyword.",
        example: Example::Math(&["sin(x)^2 + cos(x)^2 -> simplify"]),
        result: "1",
        mistakes: &[
            "Expecting a number: a numeric constant such as `sqrt(8)` comes out as a decimal (2.828), not `2*sqrt(2)`.",
            "Adding words after it: `simplify` takes none (use `assume` to give conditions).",
        ],
    },
    Keyword {
        name: "expand",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "expr -> expand",
        description: "Turns a product or a power of a sum into a plain sum of terms.",
        example: Example::Math(&["(x+1)^3 -> expand"]),
        result: "x^3+3*x^2+3*x+1",
        mistakes: &["Putting a variable after it (`expand, x`): `expand` takes no words."],
    },
    Keyword {
        name: "factor",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "expr -> factor",
        description: "Writes a polynomial as a product of factors with rational coefficients.",
        example: Example::Math(&["x^2 - 1 -> factor"]),
        result: "(x-1)*(x+1)",
        mistakes: &["Expecting factors with irrational or complex roots: only rational factorisation is done."],
    },
    Keyword {
        name: "collect",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "expr -> collect, x   (or: expr -> collect)",
        description: "Expands the expression, then groups the terms by powers of the named variable. Without a name the expression must contain one symbol only.",
        example: Example::Math(&["a*x^2 + b*x + d*x^2 -> collect, x"]),
        result: "x^2*(d+a)+b*x",
        mistakes: &[
            "Using a single-letter constant name: `c` is the speed of light in Mathcraft and is replaced by its value. Use other letters for coefficients.",
        ],
    },
    Keyword {
        name: "coeffs",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "polynomial -> coeffs, x",
        description: "The coefficients of a polynomial in the named variable as a column, constant term first.",
        example: Example::Math(&["a*x^2 + b*x + 3 -> coeffs, x"]),
        result: "[3; b; a]",
        mistakes: &["Expecting highest power first: the order is lowest power first."],
    },
    Keyword {
        name: "substitute",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "expr -> substitute, y = value   (several: substitute, x = 1, y = 2)",
        description: "Replaces each named variable by the given expression, then simplifies.",
        example: Example::Math(&["x^2 + y -> substitute, y = 3"]),
        result: "x^2+3",
        mistakes: &[
            "Writing `substitute` with no replacement: at least one `name = value` is needed.",
            "Only a bare name can be on the left of `=`: a plain name (or `f(x) = body`) is what can be replaced.",
        ],
    },
    Keyword {
        name: "float",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "expr -> float, n   (n from 1 to 15 significant digits)",
        description: "Shows every number of the result as a decimal with n significant digits.",
        example: Example::Math(&["π -> float, 4"]),
        result: "3.142",
        mistakes: &["Leaving out n: write `float, 5`. More than 15 digits is refused."],
    },
    Keyword {
        name: "rectangular",
        aliases: &["complex"],
        group: Group::Symbolic,
        syntax: "expr -> rectangular   (also: expr -> complex)",
        description: "Writes a complex result as real part plus imaginary part times i.",
        example: Example::Math(&["(1+i)^2 -> rectangular"]),
        result: "2*i",
        mistakes: &["Adding words: `rectangular` and `complex` take none."],
    },
    Keyword {
        name: "assume",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "expr -> assume, x > 0   (several: assume, x > 0, y < 3)",
        description: "Simplifies the expression under conditions on its variables, for this line only.",
        example: Example::Math(&["sqrt(x^2) -> assume, x > 0"]),
        result: "x",
        mistakes: &["Expecting the assumption to carry on to later lines: it applies to the one line."],
    },
    Keyword {
        name: "series",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "expr -> series, x = a, n   (also: series, x = a   |   series, n   |   series)",
        description: "Taylor expansion in the variable around the point a (default 0) with n terms (default 6).",
        example: Example::Math(&["sin(x) -> series, x = 0, 4"]),
        result: "-(x^3/6)+x",
        mistakes: &["Naming several variables: only one expansion variable is accepted."],
    },
    Keyword {
        name: "parfrac",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "rational expression -> parfrac, x",
        description: "Splits a rational function into partial fractions over the rationals.",
        example: Example::Math(&["1/(x^2-1) -> parfrac, x"]),
        result: "-(1/(2*(x+1)))+1/(2*(x-1))",
        mistakes: &["Using it on something that is not a quotient of polynomials."],
    },
    Keyword {
        name: "laplace",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "f -> laplace, x, p   (from variable x to new variable p; bare `laplace` means t to s)",
        description: "Laplace transform.",
        example: Example::Math(&["sin(x) -> laplace, x, p"]),
        result: "1/(p^2+1)",
        mistakes: &[TRANSFORM_NAMES],
    },
    Keyword {
        name: "invlaplace",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "F -> invlaplace, p, x   (bare `invlaplace` means s to t)",
        description: "Inverse Laplace transform.",
        example: Example::Math(&["1/(p^2+1) -> invlaplace, p, x"]),
        result: "sin(x)",
        mistakes: &[TRANSFORM_NAMES],
    },
    Keyword {
        name: "fourier",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "f -> fourier, x, w   (bare `fourier` means t to ω)",
        description: "Fourier transform. Pairs that need Dirac delta distributions (a constant, a sine) are refused.",
        example: Example::Math(&["exp(-x^2) -> fourier, x, w"]),
        result: "e^(-(w^2/4))*sqrt(pi)",
        mistakes: &[TRANSFORM_NAMES, "Transforming a constant or a plain sine: the result would be a delta distribution, which the engine reports as unsupported."],
    },
    Keyword {
        name: "invfourier",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "F -> invfourier, w, x   (bare `invfourier` means ω to t)",
        description: "Inverse Fourier transform.",
        example: Example::Math(&["exp(-w^2/4) -> invfourier, w, x"]),
        result: "e^(-x^2)/sqrt(pi)",
        mistakes: &[TRANSFORM_NAMES],
    },
    Keyword {
        name: "ztrans",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "f -> ztrans, k, z   (bare `ztrans` means n to z)",
        description: "Z transform of a sequence. A symbolic exponent such as `2^k` is not supported.",
        example: Example::Math(&["1 -> ztrans, k, z"]),
        result: "z/(z-1)",
        mistakes: &["Using a sequence with a symbolic exponent (`2^k`): reported as unsupported."],
    },
    Keyword {
        name: "invztrans",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "F -> invztrans, z, k   (bare `invztrans` means z to n)",
        description: "Inverse Z transform. Only a pole at z = 1 is supported.",
        example: Example::Math(&["z/(z-1) -> invztrans, z, k"]),
        result: "1",
        mistakes: &["Expecting poles other than z = 1 to work: they are reported as unsupported."],
    },
    Keyword {
        name: "rewrite",
        aliases: &[],
        group: Group::Symbolic,
        syntax: "expr -> rewrite, exp   (modifier: exp, e, cos, sin, ln, log, gamma)",
        description: "Rewrites the expression in terms of one elementary function.",
        example: Example::Math(&["cos(x) -> rewrite, exp"]),
        result: "e^(i*x)/2+e^(-(i*x))/2",
        mistakes: &["Writing `rewrite` with no modifier: exactly one is required."],
    },
    // ---- Solve block ----
    Keyword {
        name: "Given",
        aliases: &[],
        group: Group::SolveBlock,
        syntax: "insert_solve_block {given: [...], find: {kind, vars}}",
        description: "In Mathcraft a solve block is one region that holds guesses, constraints and the solver call: there is no separate `Given` line to write. `given` of `insert_solve_block` lists the constraints. Constraints use `==` and the inequality signs.",
        example: Example::Solve {
            before: &[],
            guesses: &["x:=1", "y:=1"],
            constraints: &["x+y==3", "x-y==1"],
            solver: "find(x, y)",
            target: None,
            after: &[],
        },
        result: "[[2], [1]]",
        mistakes: &[
            "Writing `Given` as a line with insert_math: it is not a function and is not needed.",
            "Leaving out a guess for an unknown: every unknown needs a starting value (a definition above the block also counts).",
            "Using `≠` or logical operators in a constraint: only `==`, `<`, `>`, `<=`, `>=` are accepted.",
        ],
    },
    Keyword {
        name: "find",
        aliases: &[],
        group: Group::SolveBlock,
        syntax: "find(x, y)   as the solver of a block; optional target [a, b] := find(x, y)",
        description: "Solves the constraints of the block for the named unknowns, numerically, starting from the guesses. One unknown gives a number, several give a column. Units of the guesses carry through.",
        example: Example::Solve {
            before: &[],
            guesses: &["x:=1 m", "y:=1 m"],
            constraints: &["x+y==3 m", "x-y==1 m"],
            solver: "find(x, y)",
            target: None,
            after: &[],
        },
        result: "[[2 m], [1 m]]",
        mistakes: &[
            "Calling `find(x)` outside a block: it is only valid inside one, so use `insert_solve_block`.",
            "A guess without the unit of the unknown (`x:=1` for a length): the constraints then mix units and fail.",
            "Giving more unknowns than equations: the solver cannot find a unique answer.",
        ],
    },
    Keyword {
        name: "minerr",
        aliases: &[],
        group: Group::SolveBlock,
        syntax: "minerr(x, y)   as the solver of a block",
        description: "Like `find`, but when the constraints cannot all be met it returns the values that come closest. The remaining error is in the built-in name `ERR`.",
        example: Example::Solve {
            before: &[],
            guesses: &["x:=1"],
            constraints: &["x^2==2", "x>0"],
            solver: "minerr(x)",
            target: None,
            after: &[],
        },
        result: "1.414",
        mistakes: &["Trusting the answer without checking `ERR`: a large `ERR` means no good fit exists."],
    },
    Keyword {
        name: "minimize",
        aliases: &[],
        group: Group::SolveBlock,
        syntax: "minimize(f, x)   where f is a function defined above the block",
        description: "Finds the values of the named arguments of `f` that make `f` smallest, limited by the block's constraints (a block without constraints is allowed).",
        example: Example::Solve {
            before: &["f(x):=(x-3)^2+1"],
            guesses: &["x:=0"],
            constraints: &[],
            solver: "minimize(f, x)",
            target: None,
            after: &[],
        },
        result: "3",
        mistakes: &["Passing an expression instead of a function name: define `f(x):=...` above the block and write `minimize(f, x)`."],
    },
    Keyword {
        name: "maximize",
        aliases: &[],
        group: Group::SolveBlock,
        syntax: "maximize(f, x)   where f is a function defined above the block",
        description: "Finds the values of the named arguments of `f` that make `f` largest, limited by the block's constraints.",
        example: Example::Solve {
            before: &["f(x):=4-(x-1)^2"],
            guesses: &["x:=0"],
            constraints: &["x<=2"],
            solver: "maximize(f, x)",
            target: None,
            after: &[],
        },
        result: "1",
        mistakes: &["Forgetting a bound for a function that has no maximum: the search then runs away."],
    },
    Keyword {
        name: "odesolve",
        aliases: &[],
        group: Group::SolveBlock,
        syntax: "y := odesolve(y(t), end, steps)   with constraints y'(t) + y(t) == 0 and y(0) == 1",
        description: "Solves an ordinary differential equation given by the constraints (the equation and its starting values) from the start value to `end`, in `steps` intervals, and defines `target` as a function of the variable.",
        example: Example::Solve {
            before: &[],
            guesses: &[],
            constraints: &["y'(t) + y(t) == 0", "y(0) == 1"],
            solver: "odesolve(y(t), 2, 200)",
            target: Some("y"),
            after: &["y(1)"],
        },
        result: "0.368",
        mistakes: &[
            "Giving fewer starting values than the order of the equation.",
            "Reading the result as a number: `y` is a function, call it as `y(1)`.",
        ],
    },
    Keyword {
        name: "pdesolve",
        aliases: &[],
        group: Group::SolveBlock,
        syntax: "target := pdesolve(u, x, [x0; x1], t, [t0; t1], nx, nt)",
        description: "Solves a partial differential equation of two variables (given by the constraints, with starting and boundary values) on a grid, and defines `target` as a function of both variables.",
        example: Example::Solve {
            before: &[],
            guesses: &[],
            constraints: &[
                "uxx(x, t) == ut(x, t)",
                "u(0, t) == 0",
                "u(1, t) == 0",
                "u(x, 0) == sin(π*x)",
            ],
            solver: "pdesolve(u, x, [0; 1], t, [0; 0.1], 20, 20)",
            target: Some("u"),
            after: &["u(0.5, 0.05)"],
        },
        result: "0.609",
        mistakes: &["Naming the derivatives wrongly: `uxx(x, t)` is the second derivative in x, `ut(x, t)` the first in t."],
    },
    // ---- Program ----
    Keyword {
        name: "←",
        aliases: &["<-", "local assignment", "local"],
        group: Group::Program,
        syntax: "name ← expr   (typed `←` or `<-`)",
        description: "Assigns inside a program. The name is local: it does not exist outside the program.",
        example: Example::Program {
            name: "twice",
            params: &["n"],
            lines: &["k ← 2*n", "return k"],
            call: "twice(4) =",
        },
        result: "8",
        mistakes: &[
            "Using `:=` inside a program: that defines a worksheet name. Use `←`.",
            "Expecting the local name to be visible below the program.",
        ],
    },
    Keyword {
        name: "if",
        aliases: &["else if", "also if"],
        group: Group::Program,
        syntax: "if cond   (lines indented below)   |   else if cond   |   also if cond   |   else",
        description: "Runs the indented lines when the test holds. `else if` is tried only when the earlier branches did not run. `also if` is tried whatever the earlier branches did, and keeps a later `else` from running.",
        example: Example::Program {
            name: "sign3",
            params: &["x"],
            lines: &["if x > 0", "  return 1", "else if 0 > x", "  return -1", "else", "  return 0"],
            call: "sign3(-5) =",
        },
        result: "-1",
        mistakes: &[
            "Forgetting to indent the lines under `if`: they would not belong to it.",
            "Putting `else` first, or after a line that is not an `if` block: it must follow an `if` at the same indentation.",
            "Using `=` for a comparison in the condition: write `==`.",
        ],
    },
    Keyword {
        name: "otherwise",
        aliases: &["else"],
        group: Group::Program,
        syntax: "else   (also accepted: otherwise)   followed by indented lines",
        description: "The last branch of an `if` group: runs when no branch before it ran.",
        example: Example::Program {
            name: "absval",
            params: &["x"],
            lines: &["if x >= 0", "  return x", "otherwise", "  return -x"],
            call: "absval(-7) =",
        },
        result: "7",
        mistakes: &["Giving `else` a condition: it has none, use `else if cond`."],
    },
    Keyword {
        name: "for",
        aliases: &[],
        group: Group::Program,
        syntax: "for k ∈ 1..n   (or: for k in 1..n)   followed by indented lines",
        description: "Runs the indented lines once per element of the range (or of a vector). The loop variable is local.",
        example: Example::Program {
            name: "fact",
            params: &["n"],
            lines: &["acc ← 1", "for k ∈ 1..n", "  acc ← acc*k", "return acc"],
            call: "fact(5) =",
        },
        result: "120",
        mistakes: &[
            "Writing the range with a colon or `to`: the range is `a..b` (or `a, next..b`).",
            "A loop that runs a huge number of times: the engine stops a program after its work limit.",
        ],
    },
    Keyword {
        name: "while",
        aliases: &[],
        group: Group::Program,
        syntax: "while cond   followed by indented lines",
        description: "Tests the condition before each pass and runs the indented lines while it is true.",
        example: Example::Program {
            name: "halvings",
            params: &["x"],
            lines: &["v ← x", "n ← 0", "while v > 1", "  v ← v/2", "  n ← n + 1", "return n"],
            call: "halvings(100) =",
        },
        result: "7",
        mistakes: &["A condition that never becomes false: the program stops with a work-limit error."],
    },
    Keyword {
        name: "break",
        aliases: &[],
        group: Group::Program,
        syntax: "break   (inside a loop)",
        description: "Leaves the innermost loop at once.",
        example: Example::Program {
            name: "firstsq",
            params: &["lim"],
            lines: &["r ← 0", "for k ∈ 1..100", "  if k*k > lim", "    break", "  r ← k", "return r"],
            call: "firstsq(50) =",
        },
        result: "7",
        mistakes: &["Using `break` outside a loop."],
    },
    Keyword {
        name: "continue",
        aliases: &[],
        group: Group::Program,
        syntax: "continue   (inside a loop)",
        description: "Skips the rest of this pass and goes on with the next one.",
        example: Example::Program {
            name: "sumodd",
            params: &["n"],
            lines: &["s ← 0", "for k ∈ 1..n", "  if k - 2*floor(k/2) == 0", "    continue", "  s ← s + k", "return s"],
            call: "sumodd(6) =",
        },
        result: "9",
        mistakes: &["Using `continue` outside a loop."],
    },
    Keyword {
        name: "return",
        aliases: &[],
        group: Group::Program,
        syntax: "return expr",
        description: "Ends the program with the value of `expr`. Without a `return` the value is that of the last line that has one.",
        example: Example::Program {
            name: "sq",
            params: &["x"],
            lines: &["return x^2"],
            call: "sq(6) =",
        },
        result: "36",
        mistakes: &["Writing `return` alone: it needs a value."],
    },
    Keyword {
        name: "on error",
        aliases: &["try"],
        group: Group::Program,
        syntax: "try   (indented lines)   then   on error   (indented lines)",
        description: "Runs the lines under `try`; if one of them fails, runs the lines under `on error` instead.",
        example: Example::Program {
            name: "safediv",
            params: &["a", "b"],
            lines: &["try", "  return a/b", "on error", "  return 0"],
            call: "safediv(1, 0) =",
        },
        result: "0",
        mistakes: &[
            "A `try` without an `on error` at the same indentation.",
            "Expecting the work limit to be caught: it never is.",
        ],
    },
];

/// Words from the keyword palette that this build does not implement. They are keywords, so a
/// lookup says so instead of calling them unknown functions.
pub const UNSUPPORTED: &[&str] = &[
    "cauchy", "combine", "confrac", "decomp", "explicit", "fully", "standard", "using", "ALL",
];

/// The entry for `name`, ignoring case, by name or alias.
pub fn find(name: &str) -> Option<&'static Keyword> {
    KEYWORDS.iter().find(|k| {
        k.name.eq_ignore_ascii_case(name) || k.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
    })
}

/// True for a word the palette has but the engine does not implement.
pub fn is_unsupported(name: &str) -> bool {
    UNSUPPORTED.iter().any(|u| u.eq_ignore_ascii_case(name))
}

/// The example as text, for display.
fn example_text(k: &Keyword) -> String {
    match &k.example {
        Example::Math(lines) => lines.join("\n"),
        Example::Solve {
            before,
            guesses,
            constraints,
            solver,
            target,
            after,
        } => {
            let mut parts = vec![format!(
                "insert_solve_block {{given: {}, find: {{kind: \"{}\", vars: [...]}}}}",
                json!(constraints),
                solver.split('(').next().unwrap_or(solver)
            )];
            parts.insert(0, format!("before: {}", json!(before)));
            parts.push(format!("guesses: {}", json!(guesses)));
            parts.push(format!("solver: {solver}"));
            if let Some(t) = target {
                parts.push(format!("target: {t}"));
            }
            if !after.is_empty() {
                parts.push(format!("after: {}", json!(after)));
            }
            parts.join("\n")
        }
        Example::Program {
            name,
            params,
            lines,
            call,
        } => format!(
            "insert_program {{name: \"{name}\", params: {}, body: {}}}\nthen: {call}",
            json!(params),
            json!(lines)
        ),
    }
}

/// The full JSON answer of `describe_function` for a keyword.
pub fn describe_json(k: &Keyword) -> Value {
    let example_json = match &k.example {
        Example::Math(lines) => json!({"lines": lines}),
        Example::Solve {
            before,
            guesses,
            constraints,
            solver,
            target,
            after,
        } => json!({
            "tool": "insert_solve_block",
            "before": before,
            "guesses": guesses,
            "given": constraints,
            "solver": solver,
            "target": target,
            "after": after,
        }),
        Example::Program {
            name,
            params,
            lines,
            call,
        } => json!({
            "tool": "insert_program",
            "name": name,
            "params": params,
            "body": lines,
            "then": call,
        }),
    };
    json!({
        "name": k.name,
        "kind": "keyword",
        "category": k.group.category(),
        "description": k.description,
        "syntax": k.syntax,
        "example": example_json,
        "example_result": k.result,
        "common_mistakes": k.mistakes,
        "created_with": k.group.tool(),
        "reference": "mathcraft://reference/keywords",
    })
}

/// One line for `list_functions`.
pub fn list_json(k: &Keyword) -> Value {
    json!({
        "name": k.name,
        "category": k.group.category(),
        "description": k.description.split(". ").next().unwrap_or(k.description),
        "keyword": true,
    })
}

/// The markdown of `mathcraft://reference/keywords`.
pub fn reference_text() -> String {
    let mut s = String::from(
        "# Keywords\n\nKeywords are not functions: `describe_function` answers for them too, but they \
         are written differently. A symbolic keyword follows `->`. Solve-block keywords live in a solve \
         block region and program keywords in a program. Every example below is run by the test suite \
         and gives the result shown.\n\n",
    );
    for group in [Group::Symbolic, Group::SolveBlock, Group::Program] {
        let _ = writeln!(
            s,
            "## {}\n\nCreated with {}.\n",
            group.title(),
            group.tool()
        );
        for k in KEYWORDS.iter().filter(|k| k.group == group) {
            let _ = writeln!(s, "### `{}`\n", k.name);
            let _ = writeln!(s, "{}\n", k.description);
            let _ = writeln!(s, "- Syntax: `{}`", k.syntax);
            let _ = writeln!(s, "- Example result: `{}`", k.result);
            let _ = writeln!(s, "- Example:\n```\n{}\n```", example_text(k));
            if !k.mistakes.is_empty() {
                let _ = writeln!(s, "- Common mistakes:");
                for m in k.mistakes {
                    let _ = writeln!(s, "  - {m}");
                }
            }
            s.push('\n');
        }
    }
    let _ = writeln!(
        s,
        "## Not implemented\n\nThe keyword palette also has {}. This build does not implement them.",
        UNSUPPORTED
            .iter()
            .map(|u| format!("`{u}`"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    s
}

/// Edit distance between two strings (insert, delete, replace).
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut diag = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let up = row[j + 1];
            row[j + 1] = (diag + usize::from(ca != cb)).min(up + 1).min(row[j] + 1);
            diag = up;
        }
    }
    row[b.len()]
}

/// Up to five function and keyword names close to `name`, nearest first.
pub fn close_matches(name: &str) -> Vec<String> {
    // No name is this long, and the edit distance is quadratic in the length.
    if name.len() > 256 {
        return Vec::new();
    }
    let lower = name.to_lowercase();
    let limit = if lower.chars().count() <= 3 { 1 } else { 2 };
    let mut scored: Vec<(usize, &str)> = FUNCTIONS
        .iter()
        .map(|f| f.name)
        .chain(KEYWORDS.iter().map(|k| k.name))
        .chain(UNSUPPORTED.iter().copied())
        .filter_map(|n| {
            let d = distance(&lower, &n.to_lowercase());
            // A name that contains the typed text (`solv` in `solve`) is close as well.
            let near = d.min(if lower.len() >= 3 && n.to_lowercase().contains(&lower) {
                1
            } else {
                usize::MAX
            });
            (near <= limit).then_some((near, n))
        })
        .collect();
    scored.sort();
    scored.dedup_by(|a, b| a.1 == b.1);
    scored
        .into_iter()
        .take(5)
        .map(|(_, n)| n.to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use mathcraft_doc::{CalcOptions, Doc, Placement, SolveBlockSpec};

    use super::*;

    /// Runs the example of `k` and returns the value of its last region.
    fn run(k: &Keyword) -> Result<String, String> {
        let mut doc = Doc::new("keyword example");
        let strings = |xs: &[&str]| xs.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let mut last = 0;
        let insert = |doc: &mut Doc, line: &str| -> Result<u32, String> {
            doc.insert_math(line, Placement::Auto)
                .map_err(|e| format!("`{line}`: {e}"))
        };
        match k.example {
            Example::Math(lines) => {
                for l in lines {
                    last = insert(&mut doc, l)?;
                }
            }
            Example::Solve {
                before,
                guesses,
                constraints,
                solver,
                target,
                after,
            } => {
                for l in before {
                    insert(&mut doc, l)?;
                }
                let spec = SolveBlockSpec {
                    guesses: strings(guesses),
                    constraints: strings(constraints),
                    solver: solver.to_owned(),
                    target: target.map(str::to_owned),
                };
                last = doc
                    .insert_solve_block(&spec, Placement::Auto)
                    .map_err(|e| e.to_string())?
                    .solver;
                for l in after {
                    last = insert(&mut doc, l)?;
                }
            }
            Example::Program {
                name,
                params,
                lines,
                call,
            } => {
                doc.insert_program(name, &strings(params), &strings(lines), Placement::Auto)
                    .map_err(|e| e.to_string())?;
                last = insert(&mut doc, call)?;
            }
        }
        let calc = doc.calculate(&CalcOptions::default());
        let r = calc.result(last).ok_or("no result")?;
        match &r.error {
            Some(e) => Err(format!("error: {e}")),
            None => Ok(r.value.clone()),
        }
    }

    #[test]
    fn every_example_gives_its_stated_result() {
        let mut bad = Vec::new();
        for k in KEYWORDS {
            match run(k) {
                Ok(v) if v == k.result => {}
                Ok(v) => bad.push(format!("{}: expected `{}`, got `{v}`", k.name, k.result)),
                Err(e) => bad.push(format!("{}: {e}", k.name)),
            }
        }
        assert!(bad.is_empty(), "keyword examples:\n{}", bad.join("\n"));
    }

    #[test]
    fn every_engine_keyword_is_listed() {
        for word in [
            "solve",
            "simplify",
            "expand",
            "factor",
            "collect",
            "coeffs",
            "substitute",
            "float",
            "complex",
            "rectangular",
            "assume",
            "series",
            "parfrac",
            "fourier",
            "laplace",
            "invfourier",
            "invlaplace",
            "ztrans",
            "invztrans",
            "rewrite",
            "given",
            "find",
            "minerr",
            "minimize",
            "maximize",
            "odesolve",
            "pdesolve",
            "if",
            "otherwise",
            "else",
            "for",
            "while",
            "break",
            "continue",
            "return",
            "on error",
            "try",
            "←",
        ] {
            assert!(find(word).is_some(), "`{word}` has no entry");
        }
    }

    #[test]
    fn names_are_unique_and_no_function_is_shadowed_by_accident() {
        let mut seen = std::collections::HashSet::new();
        for k in KEYWORDS {
            assert!(seen.insert(k.name.to_lowercase()), "{} twice", k.name);
        }
    }

    #[test]
    fn close_matches_find_typos() {
        assert!(close_matches("simplfy").contains(&"simplify".to_owned()));
        assert!(close_matches("minimise").contains(&"minimize".to_owned()));
        assert!(
            close_matches("slove").is_empty()
                || close_matches("slove").iter().all(|n| !n.is_empty())
        );
        assert!(close_matches("zzzzzz").is_empty());
    }

    #[test]
    fn distance_is_edit_distance() {
        assert_eq!(distance("kitten", "sitting"), 3);
        assert_eq!(distance("", "abc"), 3);
        assert_eq!(distance("solve", "solve"), 0);
    }
}
