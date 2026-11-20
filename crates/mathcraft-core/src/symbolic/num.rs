//! Exact rational helpers over [`BigRational`].
//!
//! The integer type of [`Q`] comes from `num-rational`; it is never named here, only reached
//! through `numer()` / `denom()` and the `num_traits` conversions, so no second `num-bigint`
//! version can leak in.

use num_rational::BigRational;
use num_traits::{FromPrimitive, One, Signed, ToPrimitive, Zero};

use super::{SymError, MAX_COEFF_BITS, MAX_DEGREE};

/// The exact rational number type of the symbolic core.
pub type Q = BigRational;

/// Exact integer `n` as a rational.
pub fn int(n: i64) -> Q {
    Q::from_i64(n).unwrap_or_else(Q::zero)
}

/// Exact integer from an `i128`.
pub fn from_i128(v: i128) -> Q {
    format!("{v}").parse().unwrap_or_else(|_| Q::zero())
}

/// One half, the exponent of a square root.
pub fn half() -> Q {
    int(1) / int(2)
}

/// Exact zero.
pub fn zero() -> Q {
    Q::zero()
}

/// Exact one.
pub fn one() -> Q {
    Q::one()
}

/// Bit length of the larger of numerator and denominator.
pub fn bits(q: &Q) -> u64 {
    q.numer().bits().max(q.denom().bits())
}

/// Fails with `TooLarge` when a coefficient exceeds the bit cap.
pub fn check_bits(q: &Q) -> Result<(), SymError> {
    if bits(q) > MAX_COEFF_BITS {
        Err(SymError::TooLarge("coefficient bit length"))
    } else {
        Ok(())
    }
}

/// `acc *= x`, then `TooLarge` when the result exceeds the bit cap. Every coefficient
/// product goes through here so growth stops at the first overflow instead of after a long
/// run of multiplications.
pub fn mul_into(acc: &mut Q, x: &Q) -> Result<(), SymError> {
    *acc *= x;
    check_bits(acc)
}

/// `acc += x`, then `TooLarge` when the result exceeds the bit cap.
pub fn add_into(acc: &mut Q, x: &Q) -> Result<(), SymError> {
    *acc += x;
    check_bits(acc)
}

/// `acc -= x * y`, with the same bit check.
pub fn submul_into(acc: &mut Q, x: &Q, y: &Q) -> Result<(), SymError> {
    let mut p = x.clone();
    mul_into(&mut p, y)?;
    *acc -= p;
    check_bits(acc)
}

/// Parses a decimal literal exactly: `0.5` gives `1/2`, `2.5e-3` gives `1/400`.
///
/// Returns `None` for anything that is not `[+-]digits[.digits][e[+-]digits]` or whose
/// exponent exceeds 400 in magnitude.
pub fn parse_decimal(text: &str) -> Option<Q> {
    let t = text.trim();
    let (neg, body) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let (mantissa, exp) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], body[i + 1..].parse::<i32>().ok()?),
        None => (body, 0),
    };
    if !(-400..=400).contains(&exp) {
        return None;
    }
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((a, b)) => (a, b),
        None => (mantissa, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part
        .bytes()
        .chain(frac_part.bytes())
        .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{int_part}{frac_part}");
    if digits.len() as u64 > MAX_COEFF_BITS / 3 + 2 {
        return None;
    }
    let mut n: Q = digits.parse().ok()?;
    let scale = exp - i32::try_from(frac_part.len()).ok()?;
    let ten = int(10);
    let mut factor = one();
    for _ in 0..scale.unsigned_abs() {
        mul_into(&mut factor, &ten).ok()?;
    }
    n = if scale >= 0 { n * factor } else { n / factor };
    if bits(&n) > MAX_COEFF_BITS {
        return None;
    }
    Some(if neg { -n } else { n })
}

/// Integer `n`-th power with the size guard: refuses results above the bit cap.
pub fn checked_pow(base: &Q, n: i64) -> Result<Q, SymError> {
    if n == 0 {
        return Ok(one());
    }
    if base.is_zero() {
        return if n > 0 {
            Ok(zero())
        } else {
            Err(SymError::Undefined("zero to a negative power"))
        };
    }
    if n.unsigned_abs() > MAX_DEGREE as u64 {
        return Err(SymError::TooLarge("exponent"));
    }
    let e = n.unsigned_abs();
    if !base.is_one() && !(-base.clone()).is_one() {
        let est = bits(base).saturating_mul(e);
        if est > MAX_COEFF_BITS {
            return Err(SymError::TooLarge("coefficient bit length"));
        }
    }
    let e_i32 = i32::try_from(e).map_err(|_| SymError::TooLarge("exponent"))?;
    let p = base.pow(e_i32);
    Ok(if n < 0 { p.recip() } else { p })
}

/// Floor of the `k`-th root of a non-negative integer-valued rational (Newton iteration).
fn int_root_floor(n: &Q, k: u32) -> Q {
    if k == 1 || n.is_zero() {
        return n.clone();
    }
    let kk = i32::try_from(k).unwrap_or(i32::MAX);
    let shift = n.numer().bits() / u64::from(k) + 1;
    let shift_i32 = i32::try_from(shift).unwrap_or(i32::MAX);
    let mut x = int(2).pow(shift_i32);
    loop {
        let xk1 = x.pow(kk - 1);
        let next = ((int(i64::from(kk) - 1) * x.clone() + n.clone() / xk1).floor()
            / int(i64::from(kk)))
        .floor();
        if next >= x {
            return x;
        }
        x = next;
    }
}

/// Exact `k`-th root of a non-negative rational, or `None` when it is not rational.
pub fn exact_root(q: &Q, k: u32) -> Option<Q> {
    if k == 0 || q.is_negative() {
        return None;
    }
    if q.is_zero() {
        return Some(zero());
    }
    let num = Q::from_integer(q.numer().clone());
    let den = Q::from_integer(q.denom().clone());
    let rn = int_root_floor(&num, k);
    let rd = int_root_floor(&den, k);
    let kk = i32::try_from(k).ok()?;
    if rn.pow(kk) == num && rd.pow(kk) == den && !rd.is_zero() {
        Some(rn / rd)
    } else {
        None
    }
}

/// The primes up to 10 000, computed once.
fn small_primes() -> &'static [i64] {
    static PRIMES: std::sync::OnceLock<Vec<i64>> = std::sync::OnceLock::new();
    PRIMES.get_or_init(|| {
        (2..=10_000i64)
            .filter(|d| (2..).take_while(|p| p * p <= *d).all(|p| d % p != 0))
            .collect()
    })
}

/// Splits a positive rational `q` as `q = outside^2 * inside` with `inside` an integer
/// free of square factors (`8 -> (2, 2)`, `1/8 -> (1/4, 2)`, `1/4 -> (1/2, 1)`).
///
/// Uses `sqrt(p/q) = sqrt(p*q) / q`: the radicand is `p*q` and the denominator `q` moves
/// out. Square factors are removed up to 10 000; a leftover perfect square (the square of a
/// larger prime) is removed too. Larger square factors of a cofactor with several primes
/// above 10 000 are not detected.
pub fn sqrt_split(q: &Q) -> (Q, Q) {
    let denom = Q::from_integer(q.denom().clone());
    let mut n = Q::from_integer(q.numer().clone() * q.denom().clone());
    let mut k = one();
    // Primes only: a square of a composite is already removed by the square of its prime factors,
    // so the result is the same and the bignum divisions run 1229 times, not 9999.
    for &d in small_primes() {
        let sq = int(d * d);
        while (n.clone() % sq.clone()).is_zero() {
            n /= sq.clone();
            k *= int(d);
        }
    }
    if let Some(root) = exact_root(&n, 2) {
        if !root.is_one() {
            k *= root.clone();
            n = one();
        }
    }
    (k / denom, n)
}

/// Least common multiple of two positive integer-valued rationals.
pub fn lcm_int(a: &Q, b: &Q) -> Q {
    let g = gcd_int(a.clone(), b.clone());
    if g.is_zero() {
        zero()
    } else {
        (a.clone() * b.clone()) / g
    }
}

pub(crate) fn gcd_int(mut a: Q, mut b: Q) -> Q {
    a = a.abs();
    b = b.abs();
    while !b.is_zero() {
        let r = a.clone() % b.clone();
        a = b;
        b = r;
    }
    a
}

/// Exact decimal rendering used by [`super::Sym`]'s `Display`: integers plain, others `n/d`.
pub fn render(q: &Q) -> String {
    if q.is_integer() {
        format!("{}", q.numer())
    } else {
        format!("{}/{}", q.numer(), q.denom())
    }
}

/// Approximate `f64` value (used only to order roots for display).
pub fn to_f64(q: &Q) -> f64 {
    q.to_f64().unwrap_or(f64::NAN)
}

/// `q` rounded to `digits` significant figures, as an exact decimal (`float, n` in Prime).
///
/// Rounding is exact on the rational value, half away from zero: `7.5675` to 4 figures is
/// `7.568`, `-7.5675` is `-7.568`, `2.5` to 1 figure is `3`. The decimal exponent `e` with
/// `10^e <= |q| < 10^(e+1)` is found by exact comparison. Refused below 1e-60 and above 1e300.
pub fn round_significant(q: &Q, digits: u32) -> Result<Q, SymError> {
    if q.is_zero() {
        return Ok(zero());
    }
    let a = q.abs();
    // Estimate from the bit lengths (within one of the true exponent), then correct exactly.
    let estimate = ((a.numer().bits() as f64 - a.denom().bits() as f64) * std::f64::consts::LOG10_2)
        .floor() as i64;
    if !(-400..=400).contains(&estimate) {
        return Err(SymError::Unsupported(if estimate > 0 {
            "float of a value above 1e300".into()
        } else {
            "float of a value below 1e-60".into()
        }));
    }
    let pow10 = |e: i64| checked_pow(&int(10), e);
    let mut e = estimate;
    while a < pow10(e)? {
        e -= 1;
    }
    while a >= pow10(e + 1)? {
        e += 1;
    }
    let d = i64::from(digits) - 1 - e;
    if d > 60 {
        return Err(SymError::Unsupported("float of a value below 1e-60".into()));
    }
    if d < -300 {
        return Err(SymError::Unsupported("float of a value above 1e300".into()));
    }
    let scale = pow10(d)?;
    Ok((q.clone() * scale.clone()).round() / scale)
}

/// The exact rational value of a finite `f64` (its binary fraction `m * 2^e`). `None` for NaN
/// and infinities.
pub fn from_f64_exact(x: f64) -> Option<Q> {
    if !x.is_finite() {
        return None;
    }
    if x == 0.0 {
        return Some(zero());
    }
    let bits = x.to_bits();
    let negative = bits >> 63 == 1;
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    let (mantissa, exp) = if biased == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1u64 << 52), biased - 1075)
    };
    let mut q = from_i128(i128::from(mantissa));
    if exp >= 0 {
        q *= int(2).pow(exp);
    } else {
        q /= int(2).pow(-exp);
    }
    Some(if negative { -q } else { q })
}
