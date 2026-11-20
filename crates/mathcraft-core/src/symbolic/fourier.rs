//! Fourier and inverse Fourier transforms (Prime's `fourier` and `invfourier` keywords) for the
//! transform pairs that need no distribution.
//!
//! Convention (fixed by the help examples, `Delta(t)` gives `1` and `1` gives `2 pi Delta(w)`):
//! `F(w) = Int f(t) exp(-i w t) dt` and `f(t) = 1/(2 pi) Int F(w) exp(i w t) dw`.
//!
//! The symbolic tree has no Dirac delta and no Heaviside step, so every pair whose answer needs
//! one (constants, steps, `1/t`, sines, `sin(c w)/w`) is refused. The pairs kept, with `a > 0`:
//! - Gaussian: `exp(-a t^2)` and `sqrt(pi/a) exp(-w^2/(4 a))`;
//! - Lorentzian: `1/(t^2 + k)` and `pi/sqrt(k) exp(-sqrt(k) |w|)`, `k > 0`.
//!
//! Linearity over sums and constant factors. A coefficient must be a number (or a constant such as
//! `sqrt(2)`) that evaluates positive. Every answer is checked by a numeric quadrature of the
//! defining integral at two frequencies (the inverse is checked as the forward integral of the
//! result against the input); a failed check refuses.

use num_complex::Complex64;

use super::budget;
use super::calc::coefficients;
use super::num::{half, int};
use super::numcheck;
use super::ratfunc::simplify;
use super::sym::Sym;
use super::SymError;

fn unsupported(text: &str) -> SymError {
    SymError::Unsupported(text.into())
}

/// Fourier transform of `f` in `t`, with the new variable named `w`.
pub fn fourier(f: &Sym, t: &str, w: &str) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    let out = map_terms(f, t, &|c, dep| forward_term(c, dep, t, w))?;
    check(f, &out, t, w)?;
    Ok(out)
}

/// Inverse Fourier transform of `f` in `w`, with the new variable named `t`.
pub fn inverse_fourier(f: &Sym, w: &str, t: &str) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    let out = map_terms(f, w, &|c, dep| inverse_term(c, dep, w, t))?;
    check(&out, f, t, w)?;
    Ok(out)
}

/// Applies `one(constant, dependent factor)` to every summand and adds the results.
fn map_terms(
    f: &Sym,
    var: &str,
    one: &dyn Fn(Sym, &Sym) -> Result<Sym, SymError>,
) -> Result<Sym, SymError> {
    let names = [var.to_string()];
    let terms = match f {
        Sym::Add(ts) => ts.clone(),
        other => vec![other.clone()],
    };
    let mut parts = Vec::with_capacity(terms.len());
    for term in &terms {
        budget::spend(1)?;
        if !term.mentions_any(&names) {
            return Err(unsupported("transform of a constant (Dirac delta)"));
        }
        let (mut consts, mut deps) = (Vec::new(), Vec::new());
        match term {
            Sym::Mul(fs) => {
                for x in fs {
                    if x.mentions_any(&names) {
                        deps.push(x.clone());
                    } else {
                        consts.push(x.clone());
                    }
                }
            }
            other => deps.push(other.clone()),
        }
        let [dep] = deps.as_slice() else {
            return Err(unsupported("transform of a product of t-dependent factors"));
        };
        parts.push(one(Sym::product(consts)?, dep)?);
    }
    simplify(&Sym::sum(parts)?)
}

/// The value of a symbol-free constant (numbers, `pi`, roots) when it is a real positive number.
fn positive(s: &Sym) -> bool {
    let point = |n: &str| (n == "pi").then(|| Complex64::new(std::f64::consts::PI, 0.0));
    match numcheck::eval(s, &point) {
        Some(v) => v.im == 0.0 && v.re.is_finite() && v.re > 0.0,
        None => false,
    }
}

/// Coefficients of `p` as a polynomial of degree at most 2 in `var`: `(c0, c1, c2)`.
fn quadratic(p: &Sym, var: &str) -> Result<(Sym, Sym, Sym), SymError> {
    let cs = coefficients(p, var)?;
    if cs.len() > 3 {
        return Err(unsupported("polynomial of degree above 2"));
    }
    let get = |i: usize| cs.get(i).cloned().unwrap_or_else(|| Sym::int(0));
    Ok((get(0), get(1), get(2)))
}

fn pi() -> Sym {
    Sym::symbol("pi")
}

fn forward_term(c: Sym, dep: &Sym, t: &str, w: &str) -> Result<Sym, SymError> {
    let wv = Sym::symbol(w);
    match dep {
        // exp(-a t^2)
        Sym::Func(n, args) if n == "exp" && args.len() == 1 => {
            let (c0, c1, c2) = quadratic(&args[0], t)?;
            let a = c2.negate()?;
            if !c0.is_zero() || !c1.is_zero() || !positive(&a) {
                return Err(unsupported("fourier of this exponential"));
            }
            let gauss = Sym::func(
                "exp",
                vec![Sym::product(vec![
                    Sym::int(-1),
                    Sym::pow(Sym::int(4), int(-1))?,
                    Sym::pow(a.clone(), int(-1))?,
                    Sym::pow(wv, int(2))?,
                ])?],
            )?;
            Sym::product(vec![
                c,
                Sym::pow(pi(), half())?,
                Sym::pow(a, -half())?,
                gauss,
            ])
        }
        // 1/(m t^2 + k)
        Sym::Pow(base, e) if e.is_integer() && *e == int(-1) => {
            let (k, c1, m) = quadratic(base, t)?;
            if !c1.is_zero() || !positive(&k) || !positive(&m) {
                return Err(unsupported("fourier of this fraction"));
            }
            // 1/(m t^2 + k) = (1/m) / (t^2 + q), q = k/m, with the transform pi/sqrt(q) e^(-sqrt(q)|w|).
            let q = k.times(Sym::pow(m.clone(), int(-1))?)?;
            let root = Sym::pow(q, half())?;
            let decay = Sym::func(
                "exp",
                vec![Sym::product(vec![
                    Sym::int(-1),
                    root.clone(),
                    Sym::func("abs", vec![wv])?,
                ])?],
            )?;
            Sym::product(vec![
                c,
                Sym::pow(m, int(-1))?,
                pi(),
                Sym::pow(root, int(-1))?,
                decay,
            ])
        }
        _ => Err(unsupported("fourier of this function")),
    }
}

fn inverse_term(c: Sym, dep: &Sym, w: &str, t: &str) -> Result<Sym, SymError> {
    let tv = Sym::symbol(t);
    let Sym::Func(n, args) = dep else {
        return Err(unsupported("invfourier of this function"));
    };
    if n != "exp" || args.len() != 1 {
        return Err(unsupported("invfourier of this function"));
    }
    let arg = &args[0];
    // exp(-b w^2)
    if let Ok((c0, c1, c2)) = quadratic(arg, w) {
        let b = c2.negate()?;
        if c0.is_zero() && c1.is_zero() && positive(&b) {
            let bell = Sym::func(
                "exp",
                vec![Sym::product(vec![
                    Sym::int(-1),
                    Sym::pow(Sym::int(4), int(-1))?,
                    Sym::pow(b.clone(), int(-1))?,
                    Sym::pow(tv, int(2))?,
                ])?],
            )?;
            return Sym::product(vec![
                c,
                Sym::pow(Sym::int(2), int(-1))?,
                Sym::pow(pi(), -half())?,
                Sym::pow(b, -half())?,
                bell,
            ]);
        }
    }
    // exp(-a |w|): a/(pi (t^2 + a^2))
    let abs_w = Sym::func("abs", vec![Sym::symbol(w)])?;
    let a = arg.clone().negate()?.times(Sym::pow(abs_w, int(-1))?)?;
    let a = simplify(&a)?;
    if !a.mentions_any(&[w.to_string()]) && positive(&a) {
        let denom = Sym::sum(vec![Sym::pow(tv, int(2))?, Sym::pow(a.clone(), int(2))?])?;
        return Sym::product(vec![
            c,
            a,
            Sym::pow(pi(), int(-1))?,
            Sym::pow(denom, int(-1))?,
        ]);
    }
    Err(unsupported("invfourier of this exponential"))
}

/// Quadrature half-width, step and frequencies of the check.
const HALF_WIDTH: f64 = 200.0;
const STEP: f64 = 0.01;
const FREQS: [f64; 2] = [0.7, 1.9];

/// Whether `Int time(t) exp(-i w t) dt` equals `freq(w)` at the check frequencies (Simpson rule).
fn check(time: &Sym, freq: &Sym, t: &str, w: &str) -> Result<(), SymError> {
    let steps = (2.0 * HALF_WIDTH / STEP) as usize; // even
    let fail = || unsupported("transform failed the numeric check");
    for wv in FREQS {
        let mut sum = Complex64::new(0.0, 0.0);
        for k in 0..=steps {
            let tv = -HALF_WIDTH + (k as f64) * STEP;
            let point = |n: &str| match n {
                n if n == t => Some(Complex64::new(tv, 0.0)),
                "pi" => Some(Complex64::new(std::f64::consts::PI, 0.0)),
                _ => None,
            };
            let Some(f) = numcheck::eval(time, &point) else {
                return Err(fail());
            };
            let weight = if k == 0 || k == steps {
                1.0
            } else if k % 2 == 1 {
                4.0
            } else {
                2.0
            };
            sum += f * Complex64::from_polar(1.0, -wv * tv) * weight;
        }
        let integral = sum * (STEP / 3.0);
        let wpoint = |n: &str| match n {
            n if n == w => Some(Complex64::new(wv, 0.0)),
            "pi" => Some(Complex64::new(std::f64::consts::PI, 0.0)),
            _ => None,
        };
        let Some(expected) = numcheck::eval(freq, &wpoint) else {
            return Err(fail());
        };
        if !integral.is_finite()
            || !expected.is_finite()
            || (integral - expected).norm() > 1e-3 * (1.0 + expected.norm())
        {
            return Err(fail());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolic::tests::sy;

    fn same(a: &Sym, b: &Sym) -> bool {
        match a.clone().minus(b.clone()).and_then(|d| simplify(&d)) {
            Ok(d) => d.is_zero() || numcheck::agree(a, b),
            Err(_) => false,
        }
    }

    #[test]
    fn gaussian_pair() {
        let f = fourier(&sy("exp(-t^2)"), "t", "w").expect("transform");
        assert!(same(&f, &sy("pi^(1/2)*exp(-w^2/4)")), "{f}");
        let back = inverse_fourier(&f, "w", "t").expect("inverse");
        assert!(same(&back, &sy("exp(-t^2)")), "{back}");
        let g = fourier(&sy("3*exp(-4*t^2)"), "t", "w").expect("transform");
        assert!(same(&g, &sy("3*pi^(1/2)/2*exp(-w^2/16)")), "{g}");
    }

    #[test]
    fn lorentzian_pair() {
        let f = fourier(&sy("1/(t^2+4)"), "t", "w").expect("transform");
        assert!(same(&f, &sy("pi/2*exp(-2*abs(w))")), "{f}");
        let back = inverse_fourier(&sy("exp(-3*abs(w))"), "w", "t").expect("inverse");
        assert!(same(&back, &sy("3/(pi*(t^2+9))")), "{back}");
        let inv = inverse_fourier(&f, "w", "t").expect("round trip");
        assert!(same(&inv, &sy("1/(t^2+4)")), "{inv}");
    }

    #[test]
    fn sums_are_linear() {
        let f = fourier(&sy("exp(-t^2)+2/(t^2+1)"), "t", "w").expect("transform");
        assert!(
            same(&f, &sy("pi^(1/2)*exp(-w^2/4)+2*pi*exp(-abs(w))")),
            "{f}"
        );
    }

    #[test]
    fn distributions_and_unknown_pairs_are_refused() {
        for src in [
            "1",
            "t",
            "sin(t)",
            "1/t",
            "exp(t)",
            "exp(-t^2+t)",
            "exp(a*t^2)",
        ] {
            assert!(
                matches!(fourier(&sy(src), "t", "w"), Err(SymError::Unsupported(_))),
                "{src}"
            );
        }
        assert!(matches!(
            inverse_fourier(&sy("1/w"), "w", "t"),
            Err(SymError::Unsupported(_))
        ));
        assert!(matches!(
            inverse_fourier(&sy("exp(w)"), "w", "t"),
            Err(SymError::Unsupported(_))
        ));
    }
}
