//! Exact calculus and assumptions on canonical values, all bounded by the symbolic work budget.
//!
//! - [`differentiate`]: derivative with respect to one symbol of sums, products, powers with a
//!   rational exponent, and the elementary functions `sin`, `cos`, `exp`, `ln` (chain rule).
//!   Any other function is refused.
//! - [`integrate_polynomial`]: indefinite integral of a polynomial in one symbol, with symbolic
//!   coefficients. No constant of integration is added. Non-polynomial integrands are refused
//!   (`1/x` would need `ln`, and transcendental cases are not implemented).
//! - [`coefficients`]: the coefficient of each power of one symbol (`coeffs`).
//! - [`assume_positive`]: `assume, ALL>0` on every symbol: `(a^n)^p` becomes `a^(n p)`, which
//!   makes `sqrt(a^2)` equal to `a`.

use num_traits::{Signed, ToPrimitive};

use super::budget;
use super::num::{self, int, one};
use super::poly::{expand, substitute, Poly};
use super::poly_solve::approximate;
use super::ratfunc::simplify;
use super::sym::Sym;
use super::SymError;

/// Derivative of `s` with respect to the symbol `var` (see the module docs).
pub fn differentiate(s: &Sym, var: &str) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    diff_rec(s, var)
}

fn diff_rec(s: &Sym, var: &str) -> Result<Sym, SymError> {
    budget::spend(1)?;
    // An `exp(u)` is `e^u` whose base is the constant `e`; when the variable is named `e` the base
    // varies too, so such a term is not constant even though it does not mention the name.
    let base_varies = var == "e" && has_exp(s);
    if !base_varies && !s.mentions_any(&[var.to_string()]) {
        return Ok(Sym::int(0));
    }
    match s {
        Sym::Symbol(_) => Ok(Sym::int(1)),
        Sym::Add(ts) => Sym::sum(
            ts.iter()
                .map(|t| diff_rec(t, var))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Sym::Mul(fs) => {
            let mut terms = Vec::with_capacity(fs.len());
            for i in 0..fs.len() {
                let mut prod = fs.clone();
                prod[i] = diff_rec(&fs[i], var)?;
                terms.push(Sym::product(prod)?);
            }
            Sym::sum(terms)
        }
        Sym::Pow(b, e) => {
            // d(b^e) = e * b^(e-1) * b'
            let inner = diff_rec(b, var)?;
            Sym::product(vec![
                Sym::Num(e.clone()),
                Sym::pow((**b).clone(), e.clone() - one())?,
                inner,
            ])
        }
        Sym::Func(name, args) if args.len() == 1 => {
            let u = &args[0];
            if name == "exp" && var == "e" {
                // d/de e^u = u e^(u-1) + e^u u' (the base varies, and ln e = 1).
                let shifted = Sym::sum(vec![u.clone(), Sym::int(-1)])?;
                let base_term = Sym::product(vec![u.clone(), Sym::func("exp", vec![shifted])?])?;
                let chain =
                    Sym::product(vec![Sym::func("exp", vec![u.clone()])?, diff_rec(u, var)?])?;
                return Sym::sum(vec![base_term, chain]);
            }
            let outer = match name.as_str() {
                "sin" => Sym::func("cos", vec![u.clone()])?,
                "cos" => Sym::product(vec![Sym::int(-1), Sym::func("sin", vec![u.clone()])?])?,
                "exp" => Sym::func("exp", vec![u.clone()])?,
                "ln" => Sym::pow(u.clone(), int(-1))?,
                "tan" => Sym::pow(Sym::func("cos", vec![u.clone()])?, int(-2))?,
                "sinh" => Sym::func("cosh", vec![u.clone()])?,
                "cosh" => Sym::func("sinh", vec![u.clone()])?,
                "tanh" => Sym::pow(Sym::func("cosh", vec![u.clone()])?, int(-2))?,
                "asin" => Sym::pow(one_plus_k_square(u, -1)?, minus_half())?,
                "acos" => Sym::product(vec![
                    Sym::int(-1),
                    Sym::pow(one_plus_k_square(u, -1)?, minus_half())?,
                ])?,
                "atan" => Sym::pow(one_plus_k_square(u, 1)?, int(-1))?,
                other => {
                    return Err(SymError::Unsupported(format!(
                        "derivative of the function {other}"
                    )))
                }
            };
            Sym::product(vec![outer, diff_rec(u, var)?])
        }
        _ => Err(SymError::Unsupported("derivative of this form".to_string())),
    }
}

/// Whether `s` contains an `exp` (the constant `e` to a power, in the canonical form).
fn has_exp(s: &Sym) -> bool {
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => false,
        Sym::Func(name, args) => name == "exp" || args.iter().any(has_exp),
        Sym::Add(ts) | Sym::Mul(ts) => ts.iter().any(has_exp),
        Sym::Pow(b, _) => has_exp(b),
    }
}

/// Indefinite integral of a polynomial in `var`, without a constant of integration.
pub fn integrate_polynomial(s: &Sym, var: &str) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    let parts = coefficients_of(s, var)?;
    let x = Sym::symbol(var);
    let mut terms = Vec::with_capacity(parts.len());
    for (k, c) in parts.iter().enumerate() {
        let power_plus_one = num::int(k as i64 + 1);
        let scale = Sym::Num(one() / power_plus_one.clone());
        let power = Sym::pow(x.clone(), power_plus_one)?;
        terms.push(Sym::product(vec![c.clone(), scale, power])?);
    }
    Sym::sum(terms)
}

/// Indefinite integral of `s` in `var`, without a constant of integration.
///
/// Covered: polynomials (see [`integrate_polynomial`]), sums, a constant factor times one
/// `var`-dependent factor, powers `b^e` of a linear `b = a var + c` (`ln` for `e = -1`), and
/// `exp`, `sin`, `cos`, `sinh`, `cosh`, `ln` of a linear argument. The logarithm takes the linear
/// argument as written: `c/(a x + b)` integrates to `c ln(b' )/a` with `b' = (a x + b)/c`
/// (`1/(2x+1)` gives `ln(2x+1)/2`, `1/x` gives `ln(x)`). Anything else (integration by parts,
/// `1/(x^2+1)`, `x sin x`) is refused. Every result is checked: its derivative must equal the
/// integrand, either exactly after normalisation or at sample points; otherwise it is refused.
pub fn integrate(s: &Sym, var: &str) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    let antiderivative = simplify(&integrate_rec(&simplify(s)?, var)?)?;
    let want = simplify(s)?;
    let derivative = differentiate(&antiderivative, var)?;
    // Exact comparison first. The simplifier does not always cancel a linear factor against a
    // power of the same linear form (2x-1 against (x-1/2)^-1), so the derivative and the integrand
    // are also compared at sample points; a wrong antiderivative differs there.
    let exact = normal_form(&derivative)? == normal_form(&want)?;
    if !exact && !agree_numerically(&derivative, &want, var) {
        return Err(SymError::Unsupported(
            "integral could not be verified by differentiation".into(),
        ));
    }
    Ok(antiderivative)
}

/// Whether `a` and `b` agree (in floating point) at sample points of `var` where both are defined.
/// At least two points must be defined and all of them must agree. Any other symbol makes the
/// answer no.
fn agree_numerically(a: &Sym, b: &Sym, var: &str) -> bool {
    let mut checked = 0;
    for x in [0.37, 0.83, 1.61, 2.73, 3.31, 4.19] {
        if let (Some(p), Some(q)) = (approx_at(a, var, x), approx_at(b, var, x)) {
            if (p - q).abs() > 1e-9 * (1.0 + p.abs().max(q.abs())) {
                return false;
            }
            checked += 1;
        }
    }
    checked >= 2
}

/// The value of `s` at `var = x` in floating point, or `None` when it is not a real number there
/// (a logarithm of a non-positive number, a fractional power of a negative one) or depends on
/// another symbol.
pub(crate) fn approx_at(s: &Sym, var: &str, x: f64) -> Option<f64> {
    let v = match s {
        Sym::Num(q) => num::to_f64(q),
        Sym::Symbol(n) if n == var => x,
        Sym::Symbol(_) | Sym::Unit(_) => return None,
        Sym::Add(ts) => ts
            .iter()
            .map(|t| approx_at(t, var, x))
            .sum::<Option<f64>>()?,
        Sym::Mul(fs) => fs
            .iter()
            .map(|t| approx_at(t, var, x))
            .product::<Option<f64>>()?,
        Sym::Pow(b, e) => approx_at(b, var, x)?.powf(num::to_f64(e)),
        Sym::Func(name, args) if args.len() == 1 => {
            let u = approx_at(&args[0], var, x)?;
            match name.as_str() {
                "sin" => u.sin(),
                "cos" => u.cos(),
                "tan" => u.tan(),
                "exp" => u.exp(),
                "ln" => u.ln(),
                "sinh" => u.sinh(),
                "cosh" => u.cosh(),
                "tanh" => u.tanh(),
                "asin" => u.asin(),
                "acos" => u.acos(),
                "atan" => u.atan(),
                _ => return None,
            }
        }
        Sym::Func(..) => return None,
    };
    v.is_finite().then_some(v)
}

/// The definite integral of `s` in `var` from `lower` to `upper`: the antiderivative at the upper
/// end minus the one at the lower end (see [`integrate`]). A logarithm of a zero or negative
/// number, or a fractional power of a negative number, at an end is refused (the integral diverges
/// or is complex there). The bounds must not contain `var`.
pub fn definite_integral(s: &Sym, var: &str, lower: &Sym, upper: &Sym) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    if lower.mentions_any(&[var.to_string()]) || upper.mentions_any(&[var.to_string()]) {
        return Err(SymError::Invalid(
            "a bound of the integral uses its variable".into(),
        ));
    }
    let antiderivative = integrate(s, var)?;
    let at_upper = simplify(&fold_exact(&substitute(
        &antiderivative,
        &[(var, upper.clone())],
    )?)?)?;
    let at_lower = simplify(&fold_exact(&substitute(
        &antiderivative,
        &[(var, lower.clone())],
    )?)?)?;
    if has_nonpositive_log(&at_upper) || has_nonpositive_log(&at_lower) {
        return Err(SymError::Unsupported(
            "definite integral diverges or is complex at a bound".into(),
        ));
    }
    // F(upper) - F(lower) is the integral only when F is continuous between the bounds: a
    // singularity of F inside the interval (1/x^2 over -1..1) makes the difference wrong.
    if singular_between(&antiderivative, var, lower, upper) {
        return Err(SymError::Unsupported(
            "definite integral crosses a singularity of the antiderivative".into(),
        ));
    }
    simplify(&fold_exact(&at_upper.minus(at_lower)?)?)
}

/// Exact values of elementary functions at simple arguments: `ln(1) = 0`, `ln(e) = 1`,
/// `exp(0) = 1`, and `sin`, `cos`, `tan` at 0, pi/6, pi/4, pi/3, pi/2 and pi, as exact surds where
/// they are needed. Other functions and arguments are left as they are.
pub(crate) fn fold_exact(s: &Sym) -> Result<Sym, SymError> {
    budget::spend(1)?;
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => Ok(s.clone()),
        Sym::Add(ts) => Sym::sum(ts.iter().map(fold_exact).collect::<Result<Vec<_>, _>>()?),
        Sym::Mul(fs) => Sym::product(fs.iter().map(fold_exact).collect::<Result<Vec<_>, _>>()?),
        Sym::Pow(b, e) => Sym::pow(fold_exact(b)?, e.clone()),
        Sym::Func(name, args) => {
            let args = args.iter().map(fold_exact).collect::<Result<Vec<_>, _>>()?;
            match args.as_slice() {
                [arg] => match exact_value(name, arg)? {
                    Some(v) => Ok(v),
                    None => Sym::func(name, args),
                },
                _ => Sym::func(name, args),
            }
        }
    }
}

/// The exact value of `name(arg)` at a simple argument (see [`fold_exact`]), if there is one.
fn exact_value(name: &str, arg: &Sym) -> Result<Option<Sym>, SymError> {
    let is_e = matches!(arg, Sym::Symbol(n) if n == "e");
    match name {
        "ln" if arg == &Sym::int(1) => return Ok(Some(Sym::int(0))),
        "ln" if is_e => return Ok(Some(Sym::int(1))),
        "exp" if arg.is_zero() => return Ok(Some(Sym::int(1))),
        "sinh" | "tanh" | "asin" | "atan" if arg.is_zero() => return Ok(Some(Sym::int(0))),
        "cosh" if arg.is_zero() => return Ok(Some(Sym::int(1))),
        "acos" if arg == &Sym::int(1) => return Ok(Some(Sym::int(0))),
        "sin" | "cos" | "tan" => {}
        _ => return Ok(None),
    }
    // The angle as a reduced fraction k of pi: 0, pi, or q pi.
    let k = match arg {
        Sym::Num(q) if *q == num::zero() => (0, 1),
        Sym::Symbol(n) if n == "pi" => (1, 1),
        Sym::Mul(fs) => match fs.as_slice() {
            [Sym::Num(q), Sym::Symbol(n)] if n == "pi" => {
                match (q.numer().to_i64(), q.denom().to_i64()) {
                    (Some(a), Some(b)) => (a, b),
                    _ => return Ok(None),
                }
            }
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    // `sqrt(n) * c / d` as an exact surd.
    let surd = |c: i64, n: i64, d: i64| -> Result<Sym, SymError> {
        Sym::product(vec![
            Sym::rational(c, d)?,
            Sym::pow(Sym::int(n), num::half())?,
        ])
    };
    let value = match (name, k) {
        ("sin", (0, 1)) | ("sin", (1, 1)) => Sym::int(0),
        ("sin", (1, 6)) => Sym::rational(1, 2)?,
        ("sin", (1, 4)) => surd(1, 2, 2)?,
        ("sin", (1, 3)) => surd(1, 3, 2)?,
        ("sin", (1, 2)) => Sym::int(1),
        ("cos", (0, 1)) => Sym::int(1),
        ("cos", (1, 6)) => surd(1, 3, 2)?,
        ("cos", (1, 4)) => surd(1, 2, 2)?,
        ("cos", (1, 3)) => Sym::rational(1, 2)?,
        ("cos", (1, 2)) => Sym::int(0),
        ("cos", (1, 1)) => Sym::int(-1),
        ("tan", (0, 1)) | ("tan", (1, 1)) => Sym::int(0),
        ("tan", (1, 6)) => surd(1, 3, 3)?,
        ("tan", (1, 4)) => Sym::int(1),
        ("tan", (1, 3)) => surd(1, 3, 1)?,
        _ => return Ok(None),
    };
    Ok(Some(value))
}

/// Whether `s` has a singularity (a negative or fractional power, or a logarithm, of a base that
/// vanishes) at a point of the closed interval between the bounds. Without numeric bounds, or for
/// a base that is not linear in `var`, the answer is yes: the check is conservative.
fn singular_between(s: &Sym, var: &str, lower: &Sym, upper: &Sym) -> bool {
    let interval = match (approximate(lower), approximate(upper)) {
        (Some(a), Some(b)) => Some((a.min(b), a.max(b))),
        _ => None,
    };
    walk_singular(s, var, interval)
}

fn walk_singular(s: &Sym, var: &str, interval: Option<(f64, f64)>) -> bool {
    let names = [var.to_string()];
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => false,
        Sym::Func(name, args) => {
            let log_of_var = name == "ln" && args.iter().any(|a| a.mentions_any(&names));
            (log_of_var && base_vanishes_in(&args[0], var, interval))
                || args.iter().any(|a| walk_singular(a, var, interval))
        }
        Sym::Add(ts) | Sym::Mul(ts) => ts.iter().any(|t| walk_singular(t, var, interval)),
        Sym::Pow(b, e) => {
            // A negative power is singular at its base's zero; a positive fractional one is not.
            let singular_power = e.is_negative() && b.mentions_any(&names);
            (singular_power && base_vanishes_in(b, var, interval))
                || walk_singular(b, var, interval)
        }
    }
}

/// Whether the linear base `a var + c` vanishes inside `interval` (`None`: unknown, so yes).
fn base_vanishes_in(base: &Sym, var: &str, interval: Option<(f64, f64)>) -> bool {
    let Some((lo, hi)) = interval else {
        return true;
    };
    let Ok(Some(slope)) = linear_slope(base, var) else {
        return true;
    };
    let Ok(offset) = substitute(base, &[(var, Sym::int(0))]) else {
        return true;
    };
    match (approximate(&slope), approximate(&offset)) {
        (Some(a), Some(c)) if a != 0.0 => {
            let root = -c / a;
            lo <= root && root <= hi
        }
        _ => true,
    }
}

/// `expand(simplify(s))`: the form in which two equal expressions are compared.
fn normal_form(s: &Sym) -> Result<Sym, SymError> {
    expand(&simplify(s)?)
}

fn integrate_rec(s: &Sym, var: &str) -> Result<Sym, SymError> {
    budget::spend(1)?;
    if !s.mentions_any(&[var.to_string()]) {
        return Sym::product(vec![s.clone(), Sym::symbol(var)]);
    }
    if coefficients_of(s, var).is_ok() {
        return integrate_polynomial(s, var);
    }
    match s {
        Sym::Add(ts) => Sym::sum(
            ts.iter()
                .map(|t| integrate_rec(t, var))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Sym::Mul(fs) => {
            let (dependent, mut rest): (Vec<Sym>, Vec<Sym>) = fs
                .iter()
                .cloned()
                .partition(|f| f.mentions_any(&[var.to_string()]));
            let [single] = dependent.as_slice() else {
                return Err(SymError::Unsupported(
                    "integral of a product of several factors in the variable".into(),
                ));
            };
            // c / b with a rational constant c and a linear b: the antiderivative is
            // c ln(b/c) / a, so the argument of ln is the denominator as written (1/(2x+1) gives
            // ln(2x+1)/2, 1/(1-2x) gives -ln(1-2x)/2), not the monic form canonical values keep.
            if let Sym::Pow(b, e) = single {
                if *e == int(-1) && rest.iter().all(|f| matches!(f, Sym::Num(_))) {
                    if let Some(a) = linear_slope(b, var)? {
                        let c = rest.iter().fold(one(), |acc, f| match f {
                            Sym::Num(q) => acc * q.clone(),
                            _ => acc,
                        });
                        let scaled =
                            Sym::product(vec![(**b).clone(), Sym::Num(one() / c.clone())])?;
                        return Sym::product(vec![
                            Sym::Num(c),
                            Sym::func("ln", vec![scaled])?,
                            Sym::pow(a, int(-1))?,
                        ]);
                    }
                }
            }
            rest.push(integrate_rec(single, var)?);
            Sym::product(rest)
        }
        Sym::Pow(b, e) => {
            let slope = linear_slope(b, var)?
                .ok_or_else(|| SymError::Unsupported("integral of this power".into()))?;
            if *e == int(-1) {
                return Sym::product(vec![
                    Sym::func("ln", vec![(**b).clone()])?,
                    Sym::pow(slope, int(-1))?,
                ]);
            }
            let next = e.clone() + one();
            Sym::product(vec![
                Sym::pow((**b).clone(), next.clone())?,
                Sym::Num(one() / next),
                Sym::pow(slope, int(-1))?,
            ])
        }
        Sym::Func(name, args) if args.len() == 1 => {
            let u = &args[0];
            let slope = linear_slope(u, var)?
                .ok_or_else(|| SymError::Unsupported(format!("integral of {name}")))?;
            let inverse = Sym::pow(slope, int(-1))?;
            let antiderivative = match name.as_str() {
                "exp" => Sym::func("exp", vec![u.clone()])?,
                "sin" => Sym::product(vec![Sym::int(-1), Sym::func("cos", vec![u.clone()])?])?,
                "cos" => Sym::func("sin", vec![u.clone()])?,
                "sinh" => Sym::func("cosh", vec![u.clone()])?,
                "cosh" => Sym::func("sinh", vec![u.clone()])?,
                "ln" => {
                    // int ln(u) = (u ln(u) - u) / a
                    let u_ln_u = Sym::product(vec![u.clone(), Sym::func("ln", vec![u.clone()])?])?;
                    let minus_u = Sym::product(vec![Sym::int(-1), u.clone()])?;
                    Sym::sum(vec![u_ln_u, minus_u])?
                }
                other => {
                    return Err(SymError::Unsupported(format!(
                        "integral of the function {other}"
                    )))
                }
            };
            Sym::product(vec![antiderivative, inverse])
        }
        _ => Err(SymError::Unsupported("integral of this form".into())),
    }
}

/// The slope `a` of `u = a var + c` when `u` is linear in `var` with a nonzero constant slope.
fn linear_slope(u: &Sym, var: &str) -> Result<Option<Sym>, SymError> {
    let slope = differentiate(u, var)?;
    if slope.is_zero() || slope.mentions_any(&[var.to_string()]) {
        return Ok(None);
    }
    Ok(Some(slope))
}

/// Whether a value is not real or not finite: a logarithm of a number that is zero or negative,
/// or a fractional power of a negative number (`(-1)^(3/2)`) occurs in `s`.
fn has_nonpositive_log(s: &Sym) -> bool {
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => false,
        Sym::Func(name, args) => {
            let bad = name == "ln" && matches!(args.as_slice(), [Sym::Num(q)] if *q <= num::zero());
            bad || args.iter().any(has_nonpositive_log)
        }
        Sym::Add(ts) | Sym::Mul(ts) => ts.iter().any(has_nonpositive_log),
        Sym::Pow(b, e) => {
            (!e.is_integer() && matches!(&**b, Sym::Num(q) if q.is_negative()))
                || has_nonpositive_log(b)
        }
    }
}

/// `1 + k u^2`, the radicand of the inverse trigonometric derivatives.
fn one_plus_k_square(u: &Sym, k: i64) -> Result<Sym, SymError> {
    Sym::sum(vec![
        Sym::int(1),
        Sym::product(vec![Sym::int(k), Sym::pow(u.clone(), int(2))?])?,
    ])
}

/// The exponent -1/2.
fn minus_half() -> num::Q {
    int(-1) / int(2)
}

/// The coefficient of each power of `var`, lowest power first (`coeffs`). Coefficients are free
/// of `var`; a polynomial with no `var` gives one coefficient.
pub fn coefficients(s: &Sym, var: &str) -> Result<Vec<Sym>, SymError> {
    let _scope = budget::Scope::enter();
    coefficients_of(s, var)
}

fn coefficients_of(s: &Sym, var: &str) -> Result<Vec<Sym>, SymError> {
    let poly = Poly::from_sym(s)?;
    let x = Sym::symbol(var);
    let parts = poly.coeffs_in(&x)?;
    let mut out = Vec::with_capacity(parts.len());
    for p in &parts {
        let c = p.to_sym()?;
        if c.mentions_any(&[var.to_string()]) {
            return Err(SymError::Unsupported(
                "the variable appears inside a function or a power".into(),
            ));
        }
        out.push(c);
    }
    if out.is_empty() {
        out.push(Sym::Num(num::zero()));
    }
    Ok(out)
}

/// `collect, x`: the polynomial in `var` grouped by powers, `c0 + x*c1 + x^2*c2 + ...`, with each
/// coefficient (free of `var`) as a factor. Zero coefficients are dropped. The result is canonical,
/// so its terms print in the canonical order (descending powers of the last variable in order).
pub fn collect(s: &Sym, var: &str) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    let parts = coefficients_of(s, var)?;
    let x = Sym::symbol(var);
    let mut terms = Vec::with_capacity(parts.len());
    for (k, c) in parts.into_iter().enumerate() {
        if c.is_zero() {
            continue;
        }
        let power = if k == 0 {
            Sym::int(1)
        } else {
            Sym::pow(x.clone(), num::int(k as i64))?
        };
        terms.push(Sym::product(vec![c, power])?);
    }
    Sym::sum(terms)
}

/// The symbol names that occur in `s` (sorted, without repeats).
pub fn symbols(s: &Sym) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    collect_symbols(s, &mut out);
    out.into_iter().collect()
}

fn collect_symbols(s: &Sym, out: &mut std::collections::BTreeSet<String>) {
    match s {
        Sym::Num(_) | Sym::Unit(_) => {}
        Sym::Symbol(n) => {
            out.insert(n.clone());
        }
        Sym::Func(_, args) | Sym::Add(args) | Sym::Mul(args) => {
            for a in args {
                collect_symbols(a, out);
            }
        }
        Sym::Pow(b, _) => collect_symbols(b, out),
    }
}

/// `assume, ALL>0`: every symbol is positive. `(a^n)^p` with an integer `n` becomes `a^(n p)`.
pub fn assume_positive(s: &Sym) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    positive_rec(s)
}

fn positive_rec(s: &Sym) -> Result<Sym, SymError> {
    budget::spend(1)?;
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => Ok(s.clone()),
        Sym::Func(name, args) => Sym::func(
            name,
            args.iter()
                .map(positive_rec)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Sym::Add(ts) => Sym::sum(ts.iter().map(positive_rec).collect::<Result<Vec<_>, _>>()?),
        Sym::Mul(fs) => Sym::product(fs.iter().map(positive_rec).collect::<Result<Vec<_>, _>>()?),
        Sym::Pow(b, e) => {
            let base = positive_rec(b)?;
            if let Sym::Pow(inner, n) = &base {
                if matches!(**inner, Sym::Symbol(_)) && n.is_integer() {
                    return Sym::pow((**inner).clone(), n.clone() * e.clone());
                }
            }
            Sym::pow(base, e.clone())
        }
    }
}
