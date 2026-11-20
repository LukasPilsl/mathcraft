//! The z transform and its inverse (Prime's `ztrans` and `invztrans` keywords), one-sided,
//! `F(z) = sum_{n>=0} f(n) z^-n`, from a small table.
//!
//! Forward: sums of products of at most one kernel `1`, `sin(w n)` or `cos(w n)` (no phase), any
//! `exp(a n + b)` factors and a power `n^k`. The kernel's transform is shifted by the exponentials
//! (`z -> z e^-a`) and `n^k` applies `-z d/dz` `k` times. Every result is checked against the
//! defining series at large numeric `z` before it is returned.
//!
//! Inverse: only denominators in powers of `z - 1` (results are polynomials in `n`):
//! `z/(z-1)^(k+1)` is the binomial `C(n, k)`. A pole elsewhere would give `r^n`, which the
//! symbolic tree cannot hold, and a pole at 0 a Kronecker delta: both are refused. The inverse is
//! returned only when its forward transform equals the input.

use num_complex::Complex64;
use num_traits::{Signed, Zero};

use super::budget;
use super::calc::{differentiate, symbols};
use super::canon::to_i64;
use super::limit::numeric_coefficients;
use super::num::{self, int, one, Q};
use super::numcheck;
use super::partial::parfrac;
use super::poly::{expand, substitute};
use super::ratfunc::simplify;
use super::sym::Sym;
use super::{SymError, MAX_DEGREE};

fn unsupported(text: &str) -> SymError {
    SymError::Unsupported(text.into())
}

/// z transform of `f` in `n`, with the new variable named `z`.
pub fn ztrans(f: &Sym, n: &str, z: &str) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    let f = simplify(f)?;
    let out = simplify(&rec(&f, n, z, false)?)?;
    if !series_agrees(&f, &out, n, z) {
        return Err(unsupported("z transform failed the series check"));
    }
    Ok(out)
}

fn rec(f: &Sym, n: &str, z: &str, expanded: bool) -> Result<Sym, SymError> {
    budget::spend(1)?;
    let nn = [n.to_string()];
    let zs = Sym::symbol(z);
    if !f.mentions_any(&nn) {
        // c -> c z/(z-1)
        return Sym::product(vec![
            f.clone(),
            zs.clone(),
            Sym::pow(zs.minus(Sym::int(1))?, int(-1))?,
        ]);
    }
    match f {
        Sym::Add(terms) => {
            let mut parts = Vec::with_capacity(terms.len());
            for t in terms {
                parts.push(rec(t, n, z, expanded)?);
            }
            Sym::sum(parts)
        }
        Sym::Mul(factors) => {
            let mut consts = Vec::new();
            let mut deps = Vec::new();
            for x in factors {
                if x.mentions_any(&nn) {
                    deps.push(x.clone());
                } else {
                    consts.push(x.clone());
                }
            }
            if deps.iter().any(|d| matches!(d, Sym::Add(_))) {
                if expanded {
                    return Err(unsupported("ztrans of a product with a sum"));
                }
                return rec(&expand(f)?, n, z, true);
            }
            consts.push(kernel(&deps, n, z)?);
            Sym::product(consts)
        }
        Sym::Pow(base, _) if matches!(**base, Sym::Add(_)) => {
            if expanded {
                return Err(unsupported("ztrans of a power of a sum"));
            }
            rec(&expand(f)?, n, z, true)
        }
        _ => kernel(std::slice::from_ref(f), n, z),
    }
}

/// The transform of a product of `n`-dependent factors.
fn kernel(deps: &[Sym], n: &str, z: &str) -> Result<Sym, SymError> {
    let mut shift = Sym::int(0);
    let mut extra = Vec::new();
    let mut trig: Option<(bool, Sym)> = None;
    let mut power: u32 = 0;
    for d in deps {
        match d {
            Sym::Func(name, args) if name == "exp" && args.len() == 1 => {
                let (a, b) = linear_parts(&args[0], n)?;
                shift = shift.plus(a)?;
                if !b.is_zero() {
                    extra.push(Sym::func("exp", vec![b])?);
                }
            }
            Sym::Func(name, args) if (name == "sin" || name == "cos") && args.len() == 1 => {
                if trig.is_some() {
                    return Err(unsupported(
                        "ztrans of a product of two trigonometric kernels",
                    ));
                }
                let (w, b) = linear_parts(&args[0], n)?;
                if !b.is_zero() || w.is_zero() {
                    return Err(unsupported("sin or cos with a phase or a zero frequency"));
                }
                trig = Some((name == "cos", w));
            }
            Sym::Symbol(s) if s.as_str() == n => power += 1,
            Sym::Pow(base, e)
                if matches!(&**base, Sym::Symbol(s) if s.as_str() == n)
                    && e.is_integer()
                    && !e.is_negative() =>
            {
                let k = to_i64(e)?;
                if k > i64::from(MAX_DEGREE) {
                    return Err(SymError::TooLarge("ztrans power"));
                }
                power += k as u32;
            }
            _ => return Err(unsupported("ztrans of this function")),
        }
    }
    if power > 8 {
        return Err(SymError::TooLarge("ztrans power"));
    }
    let zs = Sym::symbol(z);
    let mut g = match trig {
        None => Sym::product(vec![
            zs.clone(),
            Sym::pow(zs.clone().minus(Sym::int(1))?, int(-1))?,
        ])?,
        Some((is_cos, w)) => {
            let cosw = Sym::func("cos", vec![w.clone()])?;
            // z^2 - 2 z cos(w) + 1
            let denom = Sym::sum(vec![
                Sym::pow(zs.clone(), int(2))?,
                Sym::product(vec![Sym::int(-2), zs.clone(), cosw.clone()])?,
                Sym::int(1),
            ])?;
            let top = if is_cos {
                Sym::product(vec![zs.clone(), zs.clone().minus(cosw)?])?
            } else {
                Sym::product(vec![zs.clone(), Sym::func("sin", vec![w])?])?
            };
            top.over(denom)?
        }
    };
    if !shift.is_zero() {
        let scaled = Sym::product(vec![zs.clone(), Sym::func("exp", vec![shift.negate()?])?])?;
        g = substitute(&g, &[(z, scaled)])?;
    }
    for _ in 0..power {
        g = simplify(&differentiate(&g, z)?.times(zs.clone())?.negate()?)?;
    }
    extra.push(g);
    Sym::product(extra)
}

/// `arg = a n + b` with `a` and `b` free of `n`.
fn linear_parts(arg: &Sym, n: &str) -> Result<(Sym, Sym), SymError> {
    let nn = [n.to_string()];
    let a = differentiate(arg, n)?;
    if a.mentions_any(&nn) {
        return Err(unsupported("nonlinear argument in n"));
    }
    let b = arg.clone().minus(a.clone().times(Sym::symbol(n))?)?;
    if b.mentions_any(&nn) {
        return Err(unsupported("nonlinear argument in n"));
    }
    Ok((a, b))
}

/// Whether the series `sum f(k) z0^-k` matches `big_f(z0)` at two large real `z0`. The free
/// names get fixed sample values; anything the numeric evaluator does not know fails the check.
fn series_agrees(f: &Sym, big_f: &Sym, n: &str, z: &str) -> bool {
    let mut names = symbols(f);
    names.extend(symbols(big_f));
    names.retain(|s| s != n && s != z);
    names.sort();
    names.dedup();
    const SAMPLES: [f64; 4] = [0.37, 1.13, -0.71, 0.83];
    let value = |name: &str| -> Option<Complex64> {
        match name {
            "i" | "j" => Some(Complex64::new(0.0, 1.0)),
            "pi" => Some(Complex64::new(std::f64::consts::PI, 0.0)),
            _ => {
                let k = names.iter().position(|s| s == name)?;
                Some(Complex64::new(SAMPLES[k % SAMPLES.len()], 0.0))
            }
        }
    };
    for z0 in [1.0e3_f64, 7.0e2] {
        let mut sum = Complex64::new(0.0, 0.0);
        for k in 0..60 {
            let term = numcheck::eval(f, &|name: &str| {
                if name == n {
                    Some(Complex64::new(f64::from(k), 0.0))
                } else {
                    value(name)
                }
            });
            let Some(term) = term else { return false };
            sum += term * z0.powi(-k);
        }
        let closed = numcheck::eval(big_f, &|name: &str| {
            if name == z {
                Some(Complex64::new(z0, 0.0))
            } else {
                value(name)
            }
        });
        let Some(closed) = closed else { return false };
        if !sum.is_finite() || !closed.is_finite() {
            return false;
        }
        if (sum - closed).norm() > 1e-9 * sum.norm().max(closed.norm()) {
            return false;
        }
    }
    true
}

/// Inverse z transform of `f` in `z`, with the new variable named `n`.
pub fn inverse_ztrans(f: &Sym, z: &str, n: &str) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    let zn = [z.to_string()];
    if !f.mentions_any(&zn) {
        return Err(unsupported(
            "inverse z transform of a constant (Kronecker delta)",
        ));
    }
    let zs = Sym::symbol(z);
    let ns = Sym::symbol(n);
    let g = simplify(&f.clone().over(zs)?)?;
    let pf = parfrac(&g, z)?;
    let parts = match &pf {
        Sym::Add(ts) => ts.clone(),
        other => vec![other.clone()],
    };
    let mut out = Vec::new();
    for part in &parts {
        let (coef, k) = pole_at_one(part, z)?;
        // z/(z-1)^(k+1) is C(n, k) = n (n-1) ... (n-k+1) / k!
        let mut fs = vec![coef, Sym::Num(one() / factorial(k)?)];
        for i in 0..k {
            fs.push(ns.clone().minus(Sym::int(i64::from(i)))?);
        }
        out.push(Sym::product(fs)?);
    }
    let result = simplify(&expand(&Sym::sum(out)?)?)?;
    let back = ztrans(&result, n, z)?;
    if !simplify(&back.minus(f.clone())?)?.is_zero() {
        return Err(unsupported(
            "inverse z transform failed the round-trip check",
        ));
    }
    Ok(result)
}

/// One partial fraction term `A/(z-1)^j` (`j >= 1`): returns `(A, j - 1)`.
fn pole_at_one(term: &Sym, z: &str) -> Result<(Sym, u32), SymError> {
    let factors = match term {
        Sym::Mul(fs) => fs.clone(),
        other => vec![other.clone()],
    };
    let zn = [z.to_string()];
    let mut coef = Vec::new();
    let mut denom: Option<(Sym, i64)> = None;
    for x in factors {
        if !x.mentions_any(&zn) {
            coef.push(x);
            continue;
        }
        match x {
            Sym::Pow(base, e) if denom.is_none() && e.is_integer() && e.is_negative() => {
                let k = to_i64(&-e.clone())?;
                if k > i64::from(MAX_DEGREE) {
                    return Err(SymError::TooLarge("inverse z power"));
                }
                denom = Some((*base, k));
            }
            _ => return Err(unsupported("inverse z transform of this term")),
        }
    }
    let Some((base, k)) = denom else {
        return Err(unsupported("inverse z transform with a polynomial part"));
    };
    let bc = numeric_coefficients(&base, z)?;
    match bc.as_slice() {
        // base = c1 (z - 1): c0 = -c1.
        [c0, c1] if !c1.is_zero() && *c0 == -c1.clone() => {
            let scale = num::checked_pow(c1, k)?;
            let a = Sym::product(vec![Sym::product(coef)?, Sym::Num(one() / scale)])?;
            Ok((a, (k - 1) as u32))
        }
        _ => Err(unsupported(
            "inverse z transform with a pole other than z = 1",
        )),
    }
}

fn factorial(n: u32) -> Result<Q, SymError> {
    let mut acc = one();
    for i in 2..=n {
        num::mul_into(&mut acc, &int(i64::from(i)))?;
    }
    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolic::tests::{same_function, sy};

    fn zt(src: &str) -> Result<Sym, SymError> {
        ztrans(&sy(src), "n", "z")
    }

    fn izt(src: &str) -> Result<Sym, SymError> {
        inverse_ztrans(&sy(src), "z", "n")
    }

    #[test]
    fn help_examples_forward() {
        // help: example_symbolic_transforms.
        assert!(same_function(&zt("n").unwrap(), &sy("z/(z^2-2*z+1)")));
        let got = zt("sin(a*n)").unwrap();
        let want = sy("z*sin(a)/(z^2-2*z*cos(a)+1)");
        assert!(numcheck::agree(&got, &want), "{got}");
    }

    #[test]
    fn help_examples_inverse() {
        assert!(same_function(&izt("z/(z-1)^2").unwrap(), &sy("n")));
        assert!(same_function(&izt("z/(z-1)").unwrap(), &sy("1")));
    }

    #[test]
    fn more_entries() {
        assert!(same_function(&zt("1").unwrap(), &sy("z/(z-1)")));
        assert!(same_function(&zt("n^2").unwrap(), &sy("z*(z+1)/(z-1)^3")));
        let got = zt("cos(2*n)").unwrap();
        assert!(numcheck::agree(
            &got,
            &sy("z*(z-cos(2))/(z^2-2*z*cos(2)+1)")
        ));
        let got = zt("n*exp(n)").unwrap();
        assert!(numcheck::agree(&got, &sy("z*exp(1)/(z-exp(1))^2")));
    }

    #[test]
    fn inverse_binomials_round_trip() {
        for src in ["z/(z-1)^3", "(z^2+z)/(z-1)^3", "3*z/(z-1)+z/(z-1)^2"] {
            let t = izt(src).unwrap();
            let back = ztrans(&t, "n", "z").unwrap();
            assert!(same_function(&back, &sy(src)), "{src}");
        }
    }

    #[test]
    fn refused() {
        for src in ["sin(n+1)", "1/n", "n*sin(n)*cos(n)", "ln(n)"] {
            assert!(zt(src).is_err(), "{src}");
        }
        for src in ["z/(z-2)", "1/z", "z/(z^2+1)", "5"] {
            assert!(izt(src).is_err(), "{src}");
        }
    }
}
