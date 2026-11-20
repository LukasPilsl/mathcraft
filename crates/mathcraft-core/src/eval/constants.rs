//! Built-in constants, from the help page `built-in_constants_and_variables`.
//!
//! - Math: `π`, `e`, `i` and `j`. `∞` and `NaN` are refused: the value model holds finite
//!   numbers only.
//! - Physical constants: the value and unit printed in the help images, in SI base units. The
//!   dimension is written as exponents over (length, mass, time, current, temperature, amount,
//!   luminous intensity).
//! - `g` is standard gravity, 9.80665 m/s^2, the same value as the unit `g`. The help does not
//!   list `g` as a constant (its keystroke `g` inserts `γ`), but Prime's sample worksheets label
//!   `g` as a constant and use it as gravity.
//!
//! Not implemented: `Catalan`. The help prints only `Catalan = 0.916`, not a value with enough
//! digits to use.
//!
//! A user variable with the same name shadows a constant (see `expr_eval::lookup`).

use std::f64::consts::{E, PI};

use num_complex::Complex64;

use super::error::EvalError;
use super::value::{Scalar, Value};
use crate::units::Dimension;

/// A physical constant: its value in SI base units and its dimension exponents.
struct Physical {
    key: &'static str,
    value: f64,
    /// Exponents over (length, mass, time, current, temperature, amount, luminous intensity).
    dim: [i32; 7],
}

/// The physical constants of the help table, values exactly as printed.
const PHYSICAL: &[Physical] = &[
    // standard gravity; same value as the unit `g`
    Physical {
        key: "g",
        value: 9.80665,
        dim: [1, 0, -2, 0, 0, 0, 0],
    },
    // c = 2.99792458·10^8 m/s
    Physical {
        key: "c",
        value: 2.997_924_58e8,
        dim: [1, 0, -1, 0, 0, 0, 0],
    },
    // e_c = 1.602176634·10^-19 C
    Physical {
        key: "e_c",
        value: 1.602_176_634e-19,
        dim: [0, 0, 1, 1, 0, 0, 0],
    },
    // h = 6.62607015·10^-34 kg·m²/s
    Physical {
        key: "h",
        value: 6.626_070_15e-34,
        dim: [2, 1, -1, 0, 0, 0, 0],
    },
    // ħ = 1.054571817·10^-34 kg·m²/s
    Physical {
        key: "ħ",
        value: 1.054_571_817e-34,
        dim: [2, 1, -1, 0, 0, 0, 0],
    },
    // k = 1.380649·10^-23 kg·m²/(s²·K)
    Physical {
        key: "k",
        value: 1.380_649e-23,
        dim: [2, 1, -2, 0, -1, 0, 0],
    },
    // m_u = 1.66053906892·10^-27 kg
    Physical {
        key: "m_u",
        value: 1.660_539_068_92e-27,
        dim: [0, 1, 0, 0, 0, 0, 0],
    },
    // N_A = 6.02214076·10^23 1/mol
    Physical {
        key: "N_A",
        value: 6.022_140_76e23,
        dim: [0, 0, 0, 0, 0, -1, 0],
    },
    // R = 8.314462618 kg·m²/(s²·K·mol)
    Physical {
        key: "R",
        value: 8.314_462_618,
        dim: [2, 1, -2, 0, -1, -1, 0],
    },
    // R_∞ = 1.0973731568157·10^7 1/m
    Physical {
        key: "R_∞",
        value: 1.097_373_156_815_7e7,
        dim: [-1, 0, 0, 0, 0, 0, 0],
    },
    // α = 0.0072973525643 (dimensionless)
    Physical {
        key: "α",
        value: 0.007_297_352_564_3,
        dim: [0, 0, 0, 0, 0, 0, 0],
    },
    // γ = 0.577215664901533 (dimensionless)
    Physical {
        key: "γ",
        value: 0.577_215_664_901_533,
        dim: [0, 0, 0, 0, 0, 0, 0],
    },
    // ε_0 = 8.8541878188·10^-12 s^4·A²/(kg·m³)
    Physical {
        key: "ε_0",
        value: 8.854_187_818_8e-12,
        dim: [-3, -1, 4, 2, 0, 0, 0],
    },
    // μ_0 = 1.25663706127·10^-6 kg·m/(s²·A²)
    Physical {
        key: "μ_0",
        value: 1.256_637_061_27e-6,
        dim: [1, 1, -2, -2, 0, 0, 0],
    },
    // σ = 5.670374419·10^-8 kg/(s³·K⁴)
    Physical {
        key: "σ",
        value: 5.670_374_419e-8,
        dim: [0, 1, -3, 0, -4, 0, 0],
    },
    // Φ_0 = 2.067833848·10^-15 Wb (weber = kg·m²/(s²·A))
    Physical {
        key: "Φ_0",
        value: 2.067_833_848e-15,
        dim: [2, 1, -2, -1, 0, 0, 0],
    },
];

/// One built-in constant, for listings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConstantInfo {
    pub name: &'static str,
    /// The value in SI base units (the real part).
    pub value: f64,
    /// Dimension exponents over (length, mass, time, current, temperature, amount, luminous
    /// intensity).
    pub dim: [i32; 7],
}

/// The built-in constants with a finite real value: `π`, `e` and the physical constants.
pub fn all_constants() -> Vec<ConstantInfo> {
    let mut list = vec![
        ConstantInfo {
            name: "π",
            value: PI,
            dim: [0; 7],
        },
        ConstantInfo {
            name: "e",
            value: E,
            dim: [0; 7],
        },
    ];
    list.extend(PHYSICAL.iter().map(|p| ConstantInfo {
        name: p.key,
        value: p.value,
        dim: p.dim,
    }));
    list
}

/// The built-in constant `key`. `None` when `key` is not one of them.
///
/// `∞` and `NaN` are refused with `Unsupported`: infinity and NaN have no finite representation.
pub(super) fn constant(key: &str) -> Option<Result<Value, EvalError>> {
    let scalar =
        |z: f64, dim: Dimension| Ok(Value::Scalar(Scalar::new(Complex64::new(z, 0.0), dim)));
    match key {
        "π" => Some(scalar(PI, Dimension::dimensionless())),
        "e" => Some(scalar(E, Dimension::dimensionless())),
        "i" | "j" => Some(Ok(Value::Scalar(Scalar::new(
            Complex64::new(0.0, 1.0),
            Dimension::dimensionless(),
        )))),
        "∞" => Some(Err(EvalError::Unsupported("infinity".to_string()))),
        "NaN" => Some(Err(EvalError::Unsupported("NaN".to_string()))),
        _ => PHYSICAL
            .iter()
            .find(|p| p.key == key)
            .map(|p| scalar(p.value, Dimension::from_ints(p.dim))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scalar value of a constant, with its dimension. Panics on anything else.
    fn value_of(key: &str) -> (f64, Dimension) {
        match constant(key) {
            Some(Ok(Value::Scalar(s))) => {
                assert_eq!(s.value.im, 0.0, "{key} is real");
                (s.value.re, s.dim)
            }
            other => panic!("{key}: unexpected {other:?}"),
        }
    }

    fn dim(exps: [i32; 7]) -> Dimension {
        Dimension::from_ints(exps)
    }

    #[test]
    fn math_constants_have_the_help_values() {
        assert_eq!(value_of("π").0, std::f64::consts::PI);
        assert_eq!(value_of("e").0, std::f64::consts::E);
        assert_eq!(value_of("π").1, Dimension::dimensionless());
        let i = constant("i").unwrap().unwrap();
        assert_eq!(i, constant("j").unwrap().unwrap());
        assert!(matches!(i, Value::Scalar(s) if s.value == Complex64::new(0.0, 1.0)));
    }

    #[test]
    fn infinity_and_nan_are_unsupported() {
        for key in ["∞", "NaN"] {
            assert!(matches!(
                constant(key),
                Some(Err(EvalError::Unsupported(_)))
            ));
        }
        assert!(constant("zz").is_none());
    }

    #[test]
    fn standard_gravity_is_9_80665_m_per_s2() {
        assert_eq!(value_of("g"), (9.80665, dim([1, 0, -2, 0, 0, 0, 0])));
    }

    #[test]
    fn physical_constants_match_the_help_table() {
        let cases: [(&str, f64, [i32; 7]); 16] = [
            ("c", 2.99792458e8, [1, 0, -1, 0, 0, 0, 0]),
            ("e_c", 1.602176634e-19, [0, 0, 1, 1, 0, 0, 0]),
            ("h", 6.62607015e-34, [2, 1, -1, 0, 0, 0, 0]),
            ("ħ", 1.054571817e-34, [2, 1, -1, 0, 0, 0, 0]),
            ("k", 1.380649e-23, [2, 1, -2, 0, -1, 0, 0]),
            ("m_u", 1.66053906892e-27, [0, 1, 0, 0, 0, 0, 0]),
            ("N_A", 6.02214076e23, [0, 0, 0, 0, 0, -1, 0]),
            ("R", 8.314462618, [2, 1, -2, 0, -1, -1, 0]),
            ("R_∞", 1.0973731568157e7, [-1, 0, 0, 0, 0, 0, 0]),
            ("α", 0.0072973525643, [0, 0, 0, 0, 0, 0, 0]),
            ("γ", 0.577215664901533, [0, 0, 0, 0, 0, 0, 0]),
            ("ε_0", 8.8541878188e-12, [-3, -1, 4, 2, 0, 0, 0]),
            ("μ_0", 1.25663706127e-6, [1, 1, -2, -2, 0, 0, 0]),
            ("σ", 5.670374419e-8, [0, 1, -3, 0, -4, 0, 0]),
            ("Φ_0", 2.067833848e-15, [2, 1, -2, -1, 0, 0, 0]),
            ("g", 9.80665, [1, 0, -2, 0, 0, 0, 0]),
        ];
        for (key, value, exps) in cases {
            assert_eq!(value_of(key), (value, dim(exps)), "constant {key}");
        }
    }
}
