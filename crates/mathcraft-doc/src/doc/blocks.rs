//! Solve blocks and programs, for callers without a mouse (the MCP server). The GUI has no tool
//! for either yet, so these build the same region structures the importer reads from a file:
//! a [`RegionKind::SolveBlock`] whose members are its `children` (guess values, constraints and
//! one solver call, in that order), and a function definition whose body is an [`Expr::Program`].

use mathcraft_core::expr::{BinaryOp, Expr};
use mathcraft_import::regions::{SolveBlock, MAX_REGIONS};

use super::*;
use crate::program_text;

/// Left margin and row height of the members inside a block, in page points.
const MEMBER_LEFT: f64 = 18.9;
const MEMBER_ROW: f64 = 18.9;
/// Width of a new block, in page points.
const BLOCK_WIDTH: f64 = 360.0;

/// The solver calls a solve block can end with. The first word of the call is the keyword the
/// file stores for the block.
pub const SOLVER_NAMES: [&str; 6] = [
    "find", "minerr", "minimize", "maximize", "odesolve", "pdesolve",
];

/// What to put in a solve block. All members are text in Mathcraft's notation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolveBlockSpec {
    /// Guess definitions, such as `x:=1`.
    pub guesses: Vec<String>,
    /// Equations and inequalities, such as `x^2+y^2==28` or `y<0`. A single `=` is read as `==`.
    pub constraints: Vec<String>,
    /// The solver call, such as `find(x, y)` or `minimize(f, x)`.
    pub solver: String,
    /// A name or vector of names the result is assigned to, such as `sol` or `[a, b]`.
    pub target: Option<String>,
}

/// The ids of the regions a solve block made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolveBlockIds {
    pub block: u32,
    pub guesses: Vec<u32>,
    pub constraints: Vec<u32>,
    pub solver: u32,
}

fn total_regions(regions: &[RegionModel]) -> usize {
    regions.iter().map(|r| 1 + total_regions(&r.children)).sum()
}

/// The names before `:=` of each guess, so a guess for `L` or `m` defines a variable.
fn declare_guess_names(ctx: &mut EditContext, guesses: &[String]) {
    for g in guesses {
        if let Some((target, _)) = g.split_once(":=") {
            let t = target.trim();
            if !t.is_empty() && !t.starts_with('[') {
                ctx.declare_variable(t.split('(').next().unwrap_or(t).trim());
            }
        }
    }
}

/// The plain names in the arguments of the solver call (`x`, `y` of `find(x, y)`, `t` of
/// `odesolve(y(t), ...)`) are variables, even when they spell a unit (`t` is a tonne).
fn declare_solver_names(ctx: &mut EditContext, solver: &str) {
    fn walk(e: &Expr, depth: usize, out: &mut Vec<String>) {
        if depth > 3 {
            return;
        }
        match e {
            Expr::Id(id) if id.subscript.is_none() => out.push(id.name.clone()),
            Expr::Call { args, .. } => args.iter().for_each(|a| walk(a, depth + 1, out)),
            _ => {}
        }
    }
    let mut names = Vec::new();
    if let Ok(Expr::Call { args, .. }) = parse_math_in(solver, ctx) {
        args.iter().for_each(|a| walk(a, 0, &mut names));
    }
    for n in names {
        ctx.declare_variable(&n);
    }
}

/// Turns a lone `=` of an equation into `==`; text that already has a comparison is unchanged.
/// Mathcraft's equation sign inside a solve block or after `-> solve` is `==`.
///
/// A `=` inside a double-quoted string is text and is left alone. A `=` inside call arguments
/// (`f(x=1)`) is still turned into `==`: the comparison is the usual meaning there (`if(a=b, ...)`),
/// and leaving it would read as an evaluation, which is not a comparison. Grouping parentheses
/// (`(a=b)`) are treated like any other text.
pub fn normalise_equation(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 1);
    let mut in_string = false;
    for (i, &c) in chars.iter().enumerate() {
        if c == '"' {
            in_string = !in_string;
            out.push(c);
            continue;
        }
        if in_string {
            out.push(c);
            continue;
        }
        let prev = i.checked_sub(1).map(|j| chars[j]);
        let next = chars.get(i + 1).copied();
        let lone = c == '='
            && !matches!(prev, Some(':' | '<' | '>' | '=' | '!' | '≠'))
            && next != Some('=');
        if lone {
            out.push_str("==");
        } else {
            out.push(c);
        }
    }
    out
}

impl Doc {
    /// Inserts a solve block as the GUI's file format holds it: a block region with its guess
    /// definitions, constraints and solver call as members, in that order. One undo step.
    ///
    /// # Errors
    /// [`DocError::Invalid`] when a member does not read or is not the kind of member it is given
    /// as, [`DocError::TooManyRegions`], or [`DocError::NotFound`] for an `After` id that does not
    /// exist.
    pub fn insert_solve_block(
        &mut self,
        spec: &SolveBlockSpec,
        at: Placement,
    ) -> Result<SolveBlockIds, DocError> {
        let needed = 2 + spec.guesses.len() + spec.constraints.len();
        if total_regions(self.document.regions()) + needed > MAX_REGIONS {
            return Err(DocError::TooManyRegions);
        }
        let mut ctx = EditContext::for_regions(self.document.regions());
        declare_guess_names(&mut ctx, &spec.guesses);
        declare_solver_names(&mut ctx, &spec.solver);
        let ctx = ctx;
        let bad = |what: &str, m: String| DocError::Invalid(format!("{what}: {m}"));
        let read = |text: &str, what: String| -> Result<Expr, DocError> {
            parse_math_in(text, &ctx)
                .map_err(|e| bad(&what, message("edit.cannot_read", &[&e.to_string()])))
        };

        let mut members: Vec<(Expr, &'static str)> = Vec::new();
        for (i, g) in spec.guesses.iter().enumerate() {
            let e = read(g, format!("guess {} `{g}`", i + 1))?;
            if !matches!(e, Expr::Define { .. }) {
                return Err(bad(
                    &format!("guess {} `{g}`", i + 1),
                    "a guess is a definition such as `x:=1`".into(),
                ));
            }
            members.push((e, "guess-value"));
        }
        for (i, c) in spec.constraints.iter().enumerate() {
            let what = format!("constraint {} `{c}`", i + 1);
            let e = read(&normalise_equation(c), what.clone())?;
            let comparison = matches!(
                &e,
                Expr::Binary {
                    op: BinaryOp::Eq | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge,
                    ..
                }
            );
            if !comparison {
                return Err(bad(
                    &what,
                    "a constraint is an equation or an inequality (`==`, `<`, `>`, `<=`, `>=`)"
                        .into(),
                ));
            }
            members.push((e, "constraint"));
        }
        let call = read(&spec.solver, format!("solver `{}`", spec.solver))?;
        let keyword = match &call {
            Expr::Call { func, .. } if SOLVER_NAMES.contains(&func.name.as_str()) => {
                func.name.clone()
            }
            _ => {
                return Err(bad(
                    &format!("solver `{}`", spec.solver),
                    format!(
                        "the solver is one call of {}, such as `find(x, y)`",
                        SOLVER_NAMES.join(", ")
                    ),
                ))
            }
        };
        // odesolve and pdesolve define functions: no `=`, and a target is needed to name them.
        let defines_functions = matches!(keyword.as_str(), "odesolve" | "pdesolve");
        if defines_functions && spec.target.is_none() {
            return Err(bad(
                &format!("solver `{}`", spec.solver),
                format!("{keyword} defines a function: give a `target` name for it"),
            ));
        }
        let eval = if defines_functions {
            call
        } else {
            Expr::Eval {
                expr: Box::new(call),
                unit_override: None,
            }
        };
        let solver = match &spec.target {
            None => eval,
            Some(t) => {
                let target = read(t, format!("target `{t}`"))?;
                if !matches!(target, Expr::Id(_) | Expr::Matrix { .. }) {
                    return Err(bad(
                        &format!("target `{t}`"),
                        "the result goes to a name or a vector of names such as `[a, b]`".into(),
                    ));
                }
                Expr::Define {
                    target: Box::new(target),
                    expr: Box::new(eval),
                }
            }
        };
        members.push((solver, "solver"));

        let (pos, index) = self.place(at)?;
        let block_id = self.document.allocate_id();
        let mut ids = SolveBlockIds {
            block: block_id,
            guesses: Vec::new(),
            constraints: Vec::new(),
            solver: 0,
        };
        let count = members.len();
        let mut children = Vec::with_capacity(count);
        for (i, (expr, category)) in members.into_iter().enumerate() {
            let id = self.document.allocate_id();
            match category {
                "guess-value" => ids.guesses.push(id),
                "constraint" => ids.constraints.push(id),
                _ => ids.solver = id,
            }
            // The solver sits one row below the others, as in a hand-made block.
            let gap = if category == "solver" {
                MEMBER_ROW
            } else {
                0.0
            };
            children.push(RegionModel {
                id,
                pos: Some((MEMBER_LEFT, MEMBER_LEFT + MEMBER_ROW * i as f64 + gap)),
                size: None,
                result_ref: None,
                kind: RegionKind::Math(expr),
                solve_category: Some(category.to_owned()),
                children: Vec::new(),
                result_format: None,
                matrix: Default::default(),
            });
        }
        let height = MEMBER_LEFT * 2.0 + MEMBER_ROW * (count as f64 + 1.0);
        let region = RegionModel {
            id: block_id,
            pos: Some((pos[0], pos[1])),
            size: Some((BLOCK_WIDTH, height)),
            result_ref: None,
            kind: RegionKind::SolveBlock(SolveBlock {
                keyword: Some(keyword),
            }),
            solve_category: None,
            children,
            result_format: None,
            matrix: Default::default(),
        };
        self.execute(Box::new(InsertRegion { index, region }))?;
        Ok(ids)
    }

    /// Inserts the definition `name(params) := <program>`; `lines` are the program's statements
    /// in the line form of [`crate::program_text`]. Without `params` it defines `name := <program>`.
    /// One undo step.
    ///
    /// # Errors
    /// [`DocError::Invalid`] when the header or a line does not read, plus the errors of
    /// [`Doc::insert_math`].
    pub fn insert_program(
        &mut self,
        name: &str,
        params: &[String],
        lines: &[String],
        at: Placement,
    ) -> Result<u32, DocError> {
        let mut ctx = EditContext::for_regions(self.document.regions());
        for p in params {
            ctx.declare_variable(p);
        }
        let header = if params.is_empty() {
            name.to_owned()
        } else {
            format!("{name}({})", params.join(", "))
        };
        let target = parse_math_in(&header, &ctx).map_err(|e| {
            DocError::Invalid(format!(
                "header `{header}`: {}",
                message("edit.cannot_read", &[&e.to_string()])
            ))
        })?;
        let header_ok = match (&target, params.len()) {
            (Expr::Id(_), 0) => true,
            (Expr::Call { args, .. }, n) => {
                args.len() == n && args.iter().all(|a| matches!(a, Expr::Id(_)))
            }
            _ => false,
        };
        if !header_ok {
            return Err(DocError::Invalid(format!(
                "header `{header}`: the name and the parameters must be plain names"
            )));
        }
        let stmts = program_text::parse_program(lines, &mut ctx).map_err(DocError::Invalid)?;
        let expr = Expr::Define {
            target: Box::new(target),
            expr: Box::new(Expr::Program(stmts)),
        };
        self.insert_region(RegionKind::Math(expr), None, at)
    }
}
