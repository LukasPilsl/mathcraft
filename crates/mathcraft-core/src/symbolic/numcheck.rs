//! Independent numeric spot checks for symbolic results.
//!
//! A new symbolic route (trigonometric expansion, rewriting, transforms, trigonometric solving)
//! never trusts itself: its result is compared with the input at fixed sample points in double
//! precision (complex arithmetic, so `i` and complex exponentials work). The check is
//! conservative. [`agree`] is `true` only when the two values are evaluated at no fewer than
//! [`MIN_POINTS`] points, are finite there and match to a relative `1e-8`; a value it cannot
//! evaluate (a function it does not know, a free name it has no value for) makes the answer
//! `false`, so the caller refuses.

use num_complex::Complex64;
use num_traits::ToPrimitive;

use super::num::to_f64;
use super::sym::Sym;

/// Fewest sample points that must evaluate for a check to count.
pub(crate) const MIN_POINTS: usize = 3;

/// Values given to the free names, one set per sample point. The names rotate over these
/// numbers so that different names get different values at every point.
const SAMPLES: [f64; 7] = [0.37, 1.13, -0.71, 2.29, 0.83, -1.57, 1.91];

/// Whether `a` and `b` are numerically equal at enough sample points. The names in `a` and `b`
/// are valued automatically; `i`, `j` and `pi` have their own values.
pub(crate) fn agree(a: &Sym, b: &Sym) -> bool {
    let mut names = super::calc::symbols(a);
    names.extend(super::calc::symbols(b));
    names.sort();
    names.dedup();
    let names = &names;
    let mut good = 0;
    for shift in 0..SAMPLES.len() {
        let point = |name: &str| -> Option<Complex64> {
            if name == "i" || name == "j" {
                return Some(Complex64::new(0.0, 1.0));
            }
            if name == "pi" {
                return Some(Complex64::new(std::f64::consts::PI, 0.0));
            }
            let k = names.iter().position(|n| n == name)?;
            Some(Complex64::new(
                SAMPLES[(k * 3 + shift) % SAMPLES.len()],
                0.0,
            ))
        };
        match (eval(a, &point), eval(b, &point)) {
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => {
                if (x - y).norm() > 1e-8 * (1.0 + x.norm().max(y.norm())) {
                    return false;
                }
                good += 1;
            }
            // A pole or an undefined value at this point: skip the point.
            _ => {}
        }
    }
    good >= MIN_POINTS
}

/// The value of `s` at a point, or `None` for a function or name that is not known.
pub(crate) fn eval(s: &Sym, point: &dyn Fn(&str) -> Option<Complex64>) -> Option<Complex64> {
    match s {
        Sym::Num(q) => Some(Complex64::new(to_f64(q), 0.0)),
        Sym::Symbol(n) => point(n),
        Sym::Unit(_) => None,
        Sym::Add(ts) => {
            let mut acc = Complex64::new(0.0, 0.0);
            for t in ts {
                acc += eval(t, point)?;
            }
            Some(acc)
        }
        Sym::Mul(fs) => {
            let mut acc = Complex64::new(1.0, 0.0);
            for f in fs {
                acc *= eval(f, point)?;
            }
            Some(acc)
        }
        Sym::Pow(b, e) => {
            let base = eval(b, point)?;
            if e.is_integer() {
                let n = e.to_integer().to_i32()?;
                if base.norm() == 0.0 && n < 0 {
                    return None;
                }
                return Some(base.powi(n));
            }
            // A rational power is the principal root, as the evaluator reads it.
            Some(base.powf(to_f64(e)))
        }
        Sym::Func(name, args) => {
            if name == "combin" && args.len() == 2 {
                return None;
            }
            if args.len() != 1 {
                return None;
            }
            let v = eval(&args[0], point)?;
            Some(match name.as_str() {
                "sin" => v.sin(),
                "cos" => v.cos(),
                "tan" => v.tan(),
                "sinh" => v.sinh(),
                "cosh" => v.cosh(),
                "tanh" => v.tanh(),
                "exp" => v.exp(),
                "ln" => v.ln(),
                "log" => v.ln() / Complex64::new(10.0_f64.ln(), 0.0),
                "asin" => v.asin(),
                "acos" => v.acos(),
                "atan" => v.atan(),
                "asinh" => v.asinh(),
                "acosh" => v.acosh(),
                "atanh" => v.atanh(),
                "abs" => Complex64::new(v.norm(), 0.0),
                _ => return None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_values_agree_and_different_ones_do_not() {
        let x = Sym::symbol("x");
        let sin = |u: Sym| Sym::func("sin", vec![u]).expect("sin");
        let cos = |u: Sym| Sym::func("cos", vec![u]).expect("cos");
        let double = Sym::product(vec![Sym::int(2), sin(x.clone()), cos(x.clone())]).expect("p");
        let sin2x = sin(Sym::product(vec![Sym::int(2), x.clone()]).expect("2x"));
        assert!(agree(&double, &sin2x));
        let wrong = cos(Sym::product(vec![Sym::int(2), x]).expect("2x"));
        assert!(!agree(&double, &wrong));
    }

    #[test]
    fn an_unknown_function_is_not_agreement() {
        let f = Sym::func("zzz", vec![Sym::symbol("x")]).expect("f");
        assert!(!agree(&f, &f));
    }
}
