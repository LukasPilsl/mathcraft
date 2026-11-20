//! Exact complex numbers `re + im*i` with canonical symbolic parts (rationals, surds and
//! symbols). Every operation refuses (`Unsupported`) what it cannot give exactly; a wrong
//! value is never returned.
//!
//! Exact support:
//! - `+`, `-`, `*`, conjugate, `|z|^2` for any parts; division when `|w|^2` is a nonzero
//!   number.
//! - Integer powers (`i^n`, `(1+i)^2 = 2i`), with `i^-n = 1/i^n`.
//! - Modulus `|z|` (numeric parts only; a surd when `|z|^2` is not a square).
//! - Argument `arg z` and principal rational powers `z^r` for numeric `z` whose argument is
//!   a multiple of 15 degrees after multiplying by `r` (so `(-1)^(1/2) = i`,
//!   `(-1)^(1/3) = 1/2 + sqrt(3)/2 i` as the help shows, `sqrt(-4) = 2i`). Other arguments
//!   such as `(1+i)^(1/2)` are `Unsupported`.
//! - Euler forms `e^(i*q*pi)` for rational `q` with `180 q` a multiple of 15 degrees.
//!
//! Not supported: complex roots of polynomials, complex parts that depend on symbols in
//! division or modulus, and principal values outside the table.

use num_traits::{Signed, Zero};

use super::num::{self, Q};
use super::ratfunc::simplify;
use super::sym::Sym;
use super::{budget, SymError, MAX_DEGREE};

/// A complex number `re + im*i`. Both parts are canonical and simplified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cplx {
    pub re: Sym,
    pub im: Sym,
}

fn q(n: i64, d: i64) -> Q {
    num::int(n) / num::int(d)
}

/// Simplifies both parts, keeping the value.
fn finish(re: Sym, im: Sym) -> Result<Cplx, SymError> {
    Ok(Cplx {
        re: simplify(&re)?,
        im: simplify(&im)?,
    })
}

fn is_numeric(s: &Sym) -> bool {
    matches!(s, Sym::Num(_))
}

impl Cplx {
    /// `re + im*i` from parts (simplified).
    pub fn new(re: Sym, im: Sym) -> Result<Cplx, SymError> {
        let _scope = budget::Scope::enter();
        finish(re, im)
    }

    /// A real number.
    pub fn real(re: Sym) -> Cplx {
        Cplx {
            re,
            im: Sym::int(0),
        }
    }

    /// The imaginary unit `i`.
    pub fn i() -> Cplx {
        Cplx {
            re: Sym::int(0),
            im: Sym::int(1),
        }
    }

    /// `self + other`.
    pub fn add(&self, o: &Cplx) -> Result<Cplx, SymError> {
        let _scope = budget::Scope::enter();
        finish(
            self.re.clone().plus(o.re.clone())?,
            self.im.clone().plus(o.im.clone())?,
        )
    }

    /// `self - other`.
    pub fn sub(&self, o: &Cplx) -> Result<Cplx, SymError> {
        let _scope = budget::Scope::enter();
        finish(
            self.re.clone().minus(o.re.clone())?,
            self.im.clone().minus(o.im.clone())?,
        )
    }

    /// `self * other`.
    pub fn mul(&self, o: &Cplx) -> Result<Cplx, SymError> {
        let _scope = budget::Scope::enter();
        let re = self
            .re
            .clone()
            .times(o.re.clone())?
            .minus(self.im.clone().times(o.im.clone())?)?;
        let im = self
            .re
            .clone()
            .times(o.im.clone())?
            .plus(self.im.clone().times(o.re.clone())?)?;
        finish(re, im)
    }

    /// Complex conjugate `re - im*i`.
    pub fn conj(&self) -> Result<Cplx, SymError> {
        let _scope = budget::Scope::enter();
        finish(self.re.clone(), self.im.clone().negate()?)
    }

    /// `|z|^2 = re^2 + im^2`, simplified.
    pub fn norm_sq(&self) -> Result<Sym, SymError> {
        let _scope = budget::Scope::enter();
        let s = self
            .re
            .clone()
            .times(self.re.clone())?
            .plus(self.im.clone().times(self.im.clone())?)?;
        simplify(&s)
    }

    /// `self / other`. Refused (`Unsupported`) unless `|other|^2` is a nonzero number, which
    /// makes the rationalised form exact.
    pub fn div(&self, o: &Cplx) -> Result<Cplx, SymError> {
        let _scope = budget::Scope::enter();
        let n = o.norm_sq()?;
        let Sym::Num(nq) = &n else {
            return Err(SymError::Unsupported(
                "complex division by a symbolic modulus".into(),
            ));
        };
        if nq.is_zero() {
            return Err(SymError::Undefined("division by zero"));
        }
        let num = self.mul(&o.conj()?)?;
        finish(num.re.over(n.clone())?, num.im.over(n)?)
    }

    /// Integer power `self^n`; negative `n` gives `1 / self^|n|`.
    pub fn pow_int(&self, n: i64) -> Result<Cplx, SymError> {
        let _scope = budget::Scope::enter();
        if n.unsigned_abs() > u64::from(MAX_DEGREE) {
            return Err(SymError::TooLarge("complex power"));
        }
        let mut result = Cplx::real(Sym::int(1));
        let mut sq = self.clone();
        let mut e = n.unsigned_abs();
        while e > 0 {
            if e & 1 == 1 {
                result = result.mul(&sq)?;
            }
            e >>= 1;
            if e > 0 {
                sq = sq.mul(&sq)?;
            }
        }
        if n < 0 {
            Cplx::real(Sym::int(1)).div(&result)
        } else {
            Ok(result)
        }
    }

    /// `|z|`, for numeric parts only: `|z|^2 ^ (1/2)`.
    pub fn modulus(&self) -> Result<Sym, SymError> {
        let _scope = budget::Scope::enter();
        if !is_numeric(&self.re) || !is_numeric(&self.im) {
            return Err(SymError::Unsupported("modulus of symbolic complex".into()));
        }
        Sym::pow(self.norm_sq()?, q(1, 2))
    }

    /// Argument in degrees, principal value in (-180, 180], for numeric parts on the axes or
    /// the diagonals. `None` for other values. `z = 0` is `Undefined`.
    fn arg_degrees(&self) -> Result<Option<i64>, SymError> {
        let (Sym::Num(a), Sym::Num(b)) = (&self.re, &self.im) else {
            return Err(SymError::Unsupported("argument of symbolic complex".into()));
        };
        if a.is_zero() && b.is_zero() {
            return Err(SymError::Undefined("argument of zero"));
        }
        let deg = if b.is_zero() {
            if a.is_positive() {
                0
            } else {
                180
            }
        } else if a.is_zero() {
            if b.is_positive() {
                90
            } else {
                -90
            }
        } else if a.abs() == b.abs() {
            match (a.is_positive(), b.is_positive()) {
                (true, true) => 45,
                (false, true) => 135,
                (false, false) => -135,
                (true, false) => -45,
            }
        } else {
            return Ok(None);
        };
        Ok(Some(deg))
    }

    /// Principal argument `arg(z)` in radians as `d/180 * pi`, for the supported cases.
    pub fn arg(&self) -> Result<Sym, SymError> {
        let _scope = budget::Scope::enter();
        match self.arg_degrees()? {
            None => Err(SymError::Unsupported(
                "argument not in the exact table".into(),
            )),
            Some(0) => Ok(Sym::int(0)),
            Some(d) => Sym::product(vec![Sym::Num(q(d, 180)), Sym::symbol("pi")]),
        }
    }

    /// Principal power `z^r` for a rational exponent `r`, for the supported arguments.
    pub fn pow_rational(&self, r: &Q) -> Result<Cplx, SymError> {
        let _scope = budget::Scope::enter();
        if r.is_zero() {
            return Ok(Cplx::real(Sym::int(1)));
        }
        if r.is_integer() {
            let n = r.to_integer();
            let n = i64::try_from(n).map_err(|_| SymError::TooLarge("complex power"))?;
            return self.pow_int(n);
        }
        let deg = match self.arg_degrees()? {
            Some(d) => d,
            None => {
                return Err(SymError::Unsupported(
                    "principal power with an argument outside the exact table".into(),
                ))
            }
        };
        let phase = num::int(deg) * r.clone();
        if !phase.is_integer() {
            return Err(SymError::Unsupported(
                "principal power with an argument outside the exact table".into(),
            ));
        }
        // |z|^r = (|z|^2)^(r/2)
        let mag = Sym::pow(self.norm_sq()?, r.clone() / num::int(2))?;
        let unit = exp_i_degrees(&phase)?;
        finish(mag.clone().times(unit.re)?, mag.times(unit.im)?)
    }
}

/// `e^(i*q*pi)` for rational `q`, exact when `180 q` is a multiple of 15 degrees.
pub fn euler(q_pi: &Q) -> Result<Cplx, SymError> {
    let _scope = budget::Scope::enter();
    exp_i_degrees(&(q_pi * num::int(180)))
}

/// `cos(d) + i sin(d)` for a rational number of degrees `d` that is a multiple of 15.
fn exp_i_degrees(d: &Q) -> Result<Cplx, SymError> {
    let _scope = budget::Scope::enter();
    let ang = d.clone() / num::int(15);
    if !ang.is_integer() {
        return Err(SymError::Unsupported(
            "angle outside the exact table (multiples of 15 degrees)".into(),
        ));
    }
    let steps = i64::try_from(ang.to_integer()).map_err(|_| SymError::TooLarge("angle"))?;
    let deg = (steps * 15).rem_euclid(360);
    let (c, s) = trig(deg)?;
    finish(c, s)
}

/// Exact `(cos, sin)` for an integer number of degrees in [0, 360) on the table.
fn trig(deg: i64) -> Result<(Sym, Sym), SymError> {
    let half = Sym::rational(1, 2)?;
    let r2 = Sym::product(vec![Sym::rational(1, 2)?, Sym::pow(Sym::int(2), q(1, 2))?])?;
    let r3 = Sym::product(vec![Sym::rational(1, 2)?, Sym::pow(Sym::int(3), q(1, 2))?])?;
    let neg = |s: &Sym| s.clone().negate();
    let out = match deg {
        0 => (Sym::int(1), Sym::int(0)),
        30 => (r3.clone(), half.clone()),
        45 => (r2.clone(), r2.clone()),
        60 => (half.clone(), r3.clone()),
        90 => (Sym::int(0), Sym::int(1)),
        120 => (neg(&half)?, r3.clone()),
        135 => (neg(&r2)?, r2.clone()),
        150 => (neg(&r3)?, half.clone()),
        180 => (Sym::int(-1), Sym::int(0)),
        210 => (neg(&r3)?, neg(&half)?),
        225 => (neg(&r2)?, neg(&r2)?),
        240 => (neg(&half)?, neg(&r3)?),
        270 => (Sym::int(0), Sym::int(-1)),
        300 => (half.clone(), neg(&r3)?),
        315 => (r2.clone(), neg(&r2)?),
        330 => (r3.clone(), neg(&half)?),
        _ => {
            return Err(SymError::Unsupported(
                "angle outside the exact table".into(),
            ))
        }
    };
    Ok(out)
}
