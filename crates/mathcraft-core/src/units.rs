//! Units: dimensional analysis, the unit table, and SI-base storage.
//!
//! Values are stored in SI base units (`Quantity::value_si`), whatever unit is displayed
//! (`documentation/MCDX_FORMAT.md` section 3). A dimension is a vector of exponents over the
//! seven SI base dimensions. Prime also has a money base dimension; it is not modelled here.

use std::f64::consts::{PI, TAU};
use std::fmt;
use std::sync::OnceLock;

use num_rational::Ratio;
use thiserror::Error;

/// Exponent of one base dimension (rational, so `sqrt(m^2)` keeps `m^1`).
pub type Exp = Ratio<i32>;

/// Symbols of the seven base dimensions, in storage order.
pub const BASE_SYMBOLS: [&str; 7] = ["m", "kg", "s", "A", "K", "mol", "cd"];

/// Exponents over (length, mass, time, current, temperature, amount, luminous intensity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dimension {
    exps: [Exp; 7],
}

/// Errors from unit arithmetic, conversion and parsing.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum UnitsError {
    #[error("dimension mismatch: {left} and {right}")]
    DimensionMismatch { left: Dimension, right: Dimension },
    #[error("unknown unit `{0}`")]
    UnknownUnit(String),
    #[error("cannot parse unit expression `{0}`")]
    Parse(String),
    #[error("affine unit `{0}` cannot be used in a compound unit")]
    AffineInCompound(String),
    #[error("result is not a real number")]
    NotReal,
    #[error("result is not finite (division by zero or overflow)")]
    NotFinite,
    #[error("exponent overflow in unit arithmetic")]
    Overflow,
}

/// Exact `a * b` for exponents, reduced; fails when the reduced result leaves `i32`.
fn exp_mul(a: Exp, b: Exp) -> Result<Exp, UnitsError> {
    exp_from_wide(
        i128::from(*a.numer()) * i128::from(*b.numer()),
        i128::from(*a.denom()) * i128::from(*b.denom()),
    )
}

/// Exact `a + b` for exponents, reduced; fails when the reduced result leaves `i32`.
fn exp_add(a: Exp, b: Exp) -> Result<Exp, UnitsError> {
    exp_from_wide(
        i128::from(*a.numer()) * i128::from(*b.denom())
            + i128::from(*b.numer()) * i128::from(*a.denom()),
        i128::from(*a.denom()) * i128::from(*b.denom()),
    )
}

fn exp_from_wide(num: i128, den: i128) -> Result<Exp, UnitsError> {
    fn gcd(a: i128, b: i128) -> i128 {
        if b == 0 {
            a
        } else {
            gcd(b, a % b)
        }
    }
    // `den` is never zero here: every operand has a non-zero denominator.
    let g = gcd(num.abs(), den.abs()).max(1);
    let (mut num, mut den) = (num / g, den / g);
    if den < 0 {
        num = -num;
        den = -den;
    }
    let num = i32::try_from(num).map_err(|_| UnitsError::Overflow)?;
    let den = i32::try_from(den).map_err(|_| UnitsError::Overflow)?;
    Ok(Ratio::new(num, den))
}

impl Dimension {
    pub fn dimensionless() -> Self {
        Self::from_ints([0; 7])
    }

    pub fn from_ints(exps: [i32; 7]) -> Self {
        Self {
            exps: exps.map(Ratio::from_integer),
        }
    }

    /// The dimension of base quantity `index` (0 = length ... 6 = luminous intensity).
    pub fn base(index: usize) -> Self {
        let mut exps = [0; 7];
        exps[index] = 1;
        Self::from_ints(exps)
    }

    pub fn exponents(&self) -> &[Exp; 7] {
        &self.exps
    }

    pub fn is_dimensionless(&self) -> bool {
        self.exps.iter().all(|e| *e.numer() == 0)
    }

    /// Raises every exponent to `p`. Fails with `Overflow` when an exponent leaves `i32`.
    pub fn pow(self, p: Exp) -> Result<Self, UnitsError> {
        let mut exps = [Ratio::from_integer(0); 7];
        for (out, e) in exps.iter_mut().zip(self.exps) {
            *out = exp_mul(e, p)?;
        }
        Ok(Self { exps })
    }

    pub fn inverse(self) -> Result<Self, UnitsError> {
        self.pow(Ratio::from_integer(-1))
    }

    /// Product of two dimensions (exponents add). Fails with `Overflow` on `i32` overflow.
    pub fn checked_mul(self, rhs: Self) -> Result<Self, UnitsError> {
        let mut exps = [Ratio::from_integer(0); 7];
        for (i, out) in exps.iter_mut().enumerate() {
            *out = exp_add(self.exps[i], rhs.exps[i])?;
        }
        Ok(Self { exps })
    }

    /// Quotient of two dimensions. Fails with `Overflow` on `i32` overflow.
    pub fn checked_div(self, rhs: Self) -> Result<Self, UnitsError> {
        self.checked_mul(rhs.inverse()?)
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let items: Vec<(&str, Exp)> = BASE_SYMBOLS
            .iter()
            .copied()
            .zip(self.exps)
            .filter(|(_, e)| *e.numer() != 0)
            .collect();
        f.write_str(&fmt_product(&items))
    }
}

/// Renders `sym^e` factors as `a*b/(c*d)`; `1` when empty.
fn fmt_product(items: &[(&str, Exp)]) -> String {
    let mut num = Vec::new();
    let mut den = Vec::new();
    for &(sym, e) in items {
        // unsigned_abs: `-e` would overflow for i32::MIN.
        let (n, d) = (e.numer().unsigned_abs(), e.denom().unsigned_abs());
        if *e.numer() > 0 {
            num.push(power(sym, n, d));
        } else if *e.numer() < 0 {
            den.push(power(sym, n, d));
        }
    }
    let numerator = if num.is_empty() {
        "1".to_string()
    } else {
        num.join("*")
    };
    match den.len() {
        0 => numerator,
        1 => format!("{numerator}/{}", den[0]),
        _ => format!("{numerator}/({})", den.join("*")),
    }
}

/// `sym^(n/d)` for a magnitude `n/d` (`d >= 1`); the sign is carried by the caller.
fn power(sym: &str, n: u32, d: u32) -> String {
    match d {
        1 if n == 1 => sym.to_string(),
        1 => format!("{sym}^{n}"),
        _ => format!("{sym}^({n}/{d})"),
    }
}

/// `value_si - offset` for an offset temperature unit. A result within a few ulps (8 epsilon) of
/// the SI magnitude of an exact zero is float noise from the scale change (`491.67 °R` is 273.15 K, which is
/// 0 °C): it snaps to 0. Units without an offset are untouched.
pub(crate) fn shift_to_unit(value_si: f64, offset: f64) -> f64 {
    let shifted = value_si - offset;
    if offset != 0.0 && shifted.abs() <= 8.0 * f64::EPSILON * value_si.abs().max(offset.abs()) {
        0.0
    } else {
        shifted
    }
}

/// A named unit. `value_si = value * factor_to_si + offset_to_si.unwrap_or(0)`.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitDef {
    pub name: &'static str,
    pub symbol: &'static str,
    pub dimension: Dimension,
    pub factor_to_si: f64,
    /// Set only for absolute temperature scales (°C, °F). Differences (Δ°C, Δ°F) have none.
    pub offset_to_si: Option<f64>,
}

static TABLE: OnceLock<Vec<UnitDef>> = OnceLock::new();

/// The whole unit table: SI base and derived, non-SI, USCS/CGS and prefixed variants.
pub fn units() -> &'static [UnitDef] {
    TABLE.get_or_init(build_table)
}

/// Looks a unit up by symbol (case-sensitive), then by name (case-insensitive).
///
/// Symbols are case-sensitive, so `PA` is petaampere (prefix `P` + `A`), `Pa` is pascal and
/// `pa` is unknown. Prime's gram symbol is `gm` (`g` is standard gravity), so milligram is `mgm`.
pub fn unit(key: &str) -> Option<&'static UnitDef> {
    let table = units();
    table
        .iter()
        .find(|u| u.symbol == key)
        .or_else(|| table.iter().find(|u| u.name.eq_ignore_ascii_case(key)))
}

/// The unit system of a worksheet (`currentUnitSystem/@name`). Prime shows the variables and
/// results of a worksheet in its unit system (help: `about_units`). A result that has no unit of
/// its own is shown in the preferred unit of its dimension (USCS: feet, pounds; CGS: centimetres,
/// grams). A custom system has no table here and shows SI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnitSystem {
    /// International System: results keep their SI base units.
    #[default]
    Si,
    /// United States Customary System (`us`).
    Us,
    /// Centimetre-gram-second (`cgs`).
    Cgs,
}

impl UnitSystem {
    /// The built-in system named by `currentUnitSystem/@name`, `None` for a custom system.
    #[must_use]
    pub fn from_attr(value: &str) -> Option<Self> {
        match value {
            "si" => Some(Self::Si),
            "us" => Some(Self::Us),
            "cgs" => Some(Self::Cgs),
            _ => None,
        }
    }

    /// The `name` spelling of this system in `calculation.xml`.
    #[must_use]
    pub fn attr(self) -> &'static str {
        match self {
            Self::Si => "si",
            Self::Us => "us",
            Self::Cgs => "cgs",
        }
    }

    /// The preferred unit symbols, looked up in this order. SI has none: values keep SI units.
    fn preferred(self) -> &'static [&'static str] {
        match self {
            Self::Si => &[],
            Self::Us => &["ft", "lb", "lbf", "psi"],
            Self::Cgs => &["cm", "gm", "dyn", "erg", "Ba"],
        }
    }
}

/// The unit a value of dimension `dim` is shown in under `system`, when the system prefers one for
/// that dimension (feet for a length in USCS). `None` means the value keeps its SI units. Only the
/// dimensions in [`UnitSystem::preferred`] change: derived ones such as area keep SI.
#[must_use]
pub fn display_unit(dim: &Dimension, system: UnitSystem) -> Option<&'static UnitDef> {
    system
        .preferred()
        .iter()
        .filter_map(|symbol| unit(symbol))
        .find(|u| u.dimension == *dim && u.offset_to_si.is_none())
}

const LB: f64 = 0.45359237;
const IN: f64 = 0.0254;
const LBF: f64 = LB * 9.80665;

const PREFIXES: &[(&str, &str, i32)] = &[
    ("Y", "yotta", 24),
    ("Z", "zetta", 21),
    ("E", "exa", 18),
    ("P", "peta", 15),
    ("T", "tera", 12),
    ("G", "giga", 9),
    ("M", "mega", 6),
    ("k", "kilo", 3),
    ("h", "hecto", 2),
    ("da", "deka", 1),
    ("d", "deci", -1),
    ("c", "centi", -2),
    ("m", "milli", -3),
    ("µ", "micro", -6),
    ("μ", "micro", -6),
    ("u", "micro", -6),
    ("n", "nano", -9),
    ("p", "pico", -12),
    ("f", "femto", -15),
    ("a", "atto", -18),
    ("z", "zepto", -21),
    ("y", "yocto", -24),
];

/// Units that get every prefix in `PREFIXES` (by symbol). `g` (standard gravity) is not
/// prefixable: `mgm`, `µgm` and `kgm` are the prefixed grams.
const PREFIXABLE: &[&str] = &[
    "m", "gm", "s", "A", "mol", "cd", "Hz", "N", "Pa", "J", "W", "C", "V", "Ω", "F", "S", "T",
    "Wb", "H", "L", "bar",
];

fn def(name: &'static str, symbol: &'static str, dimension: Dimension, factor: f64) -> UnitDef {
    UnitDef {
        name,
        symbol,
        dimension,
        factor_to_si: factor,
        offset_to_si: None,
    }
}

fn affine(
    name: &'static str,
    symbol: &'static str,
    dimension: Dimension,
    factor: f64,
    offset: f64,
) -> UnitDef {
    UnitDef {
        offset_to_si: Some(offset),
        ..def(name, symbol, dimension, factor)
    }
}

fn alias(v: &mut Vec<UnitDef>, of: &str, symbol: &'static str) {
    let mut a = v
        .iter()
        .find(|u| u.symbol == of)
        .expect("alias target is in the table")
        .clone();
    a.symbol = symbol;
    v.push(a);
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Table-only dimension arithmetic: the table's exponents are tiny, so overflow is a bug here.
fn power_of(d: Dimension, n: i32) -> Dimension {
    d.pow(Ratio::from_integer(n))
        .expect("table exponents are small")
}

fn build_table() -> Vec<UnitDef> {
    let mul = |a: Dimension, b: Dimension| a.checked_mul(b).expect("table exponents are small");
    let div = |a: Dimension, b: Dimension| a.checked_div(b).expect("table exponents are small");
    let inv = |a: Dimension| a.inverse().expect("table exponents are small");

    let len = Dimension::base(0);
    let mass = Dimension::base(1);
    let time = Dimension::base(2);
    let current = Dimension::base(3);
    let temp = Dimension::base(4);
    let amount = Dimension::base(5);
    let lum = Dimension::base(6);
    let none = Dimension::dimensionless();

    let area = power_of(len, 2);
    let volume = power_of(len, 3);
    let velocity = div(len, time);
    let acceleration = div(len, power_of(time, 2));
    let force = div(mul(mass, len), power_of(time, 2));
    let pressure = div(force, area);
    let energy = mul(force, len);
    let power = div(energy, time);
    let charge = mul(current, time);
    let voltage = div(power, current);
    let resistance = div(voltage, current);
    let capacitance = div(charge, voltage);
    let conductance = div(current, voltage);
    let flux = mul(voltage, time);
    let henry = div(flux, current);
    let frequency = inv(time);
    let inverse_length = inv(len);

    #[rustfmt::skip]
    let mut v = vec![
        // SI base
        def("meter", "m", len, 1.0),
        def("kilogram", "kg", mass, 1.0),
        def("second", "s", time, 1.0),
        def("ampere", "A", current, 1.0),
        def("kelvin", "K", temp, 1.0),
        def("mole", "mol", amount, 1.0),
        def("candela", "cd", lum, 1.0),
        // SI derived and dimensionless
        def("radian", "rad", none, 1.0),
        def("steradian", "sr", none, 1.0),
        def("degree", "°", none, PI / 180.0),
        def("revolution", "rev", none, TAU),
        def("arcminute", "arcmin", none, PI / 10800.0),
        def("arcsecond", "arcsec", none, PI / 648_000.0),
        def("hertz", "Hz", frequency, 1.0),
        def("becquerel", "Bq", frequency, 1.0),
        def("newton", "N", force, 1.0),
        def("pascal", "Pa", pressure, 1.0),
        def("joule", "J", energy, 1.0),
        def("watt", "W", power, 1.0),
        def("coulomb", "C", charge, 1.0),
        def("volt", "V", voltage, 1.0),
        def("ohm", "Ω", resistance, 1.0),
        def("farad", "F", capacitance, 1.0),
        def("siemens", "S", conductance, 1.0),
        def("weber", "Wb", flux, 1.0),
        def("tesla", "T", div(flux, area), 1.0),
        def("henry", "H", henry, 1.0),
        def("lumen", "lm", lum, 1.0),
        def("lux", "lx", div(lum, area), 1.0),
        def("gray", "Gy", div(energy, mass), 1.0),
        def("sievert", "Sv", div(energy, mass), 1.0),
        def("katal", "kat", div(amount, time), 1.0),
        // time, mass, volume, pressure
        def("minute", "min", time, 60.0),
        def("hour", "h", time, 3600.0),
        def("day", "d", time, 86_400.0),
        def("week", "wk", time, 604_800.0),
        def("gram", "gm", mass, 1e-3),
        // Standard gravity: Prime's `g`. Value is the standard 9.80665 m/s^2 (the help shows
        // a rounded 9.807 m/s^2 only).
        def("standard gravity", "g", acceleration, 9.80665),
        def("tonne", "t", mass, 1000.0),
        def("liter", "L", volume, 1e-3),
        def("bar", "bar", pressure, 1e5),
        def("standard atmosphere", "atm", pressure, 101_325.0),
        def("barye", "Ba", pressure, 0.1),
        def("revolution per minute", "rpm", frequency, TAU / 60.0),
        // Not in the help: standard definitions. torr = 101325/760 Pa exactly; inch of mercury
        // at 0 degC = 3386.389 Pa (NIST); psf = lbf/ft^2; ksf = 1000 psf.
        def("torr", "torr", pressure, 101_325.0 / 760.0),
        def("inch of mercury", "in_Hg", pressure, 3_386.389),
        def("pound-force per square foot", "psf", pressure, LBF / (0.3048 * 0.3048)),
        def("kip per square foot", "ksf", pressure, 1000.0 * LBF / (0.3048 * 0.3048)),
        // Not in the help: barn = 1e-28 m^2 (cross-section).
        def("barn", "barn", area, 1e-28),
        // Not in the help: dpi = 1 / inch.
        def("dots per inch", "dpi", inverse_length, 1.0 / IN),
        // Statcoulomb (symbol statC, cut off in the CGS dialog screenshot): exactly 0.1/c C.
        def("Statcoulomb", "statC", charge, 0.1 / 299_792_458.0),
        // Hza: help image reads "1 Hza = 60 rpm" (the md alt text wrongly says "1 Hz"). rpm is
        // TAU/60 s^-1 here, so 1 Hza = TAU s^-1.
        def("Hz (angular)", "Hza", frequency, TAU),
        // length, velocity
        def("foot", "ft", len, 0.3048),
        def("inch", "in", len, IN),
        def("yard", "yd", len, 0.9144),
        def("mile", "mi", len, 1609.344),
        def("nautical mile", "nmi", len, 1852.0),
        def("knot", "knot", velocity, 1852.0 / 3600.0),
        def("mile per hour", "mph", velocity, 1609.344 / 3600.0),
        // USCS mass, force, pressure, volume, area
        def("pound", "lb", mass, LB),
        def("ounce", "oz", mass, LB / 16.0),
        def("short ton", "ton", mass, 2000.0 * LB),
        def("pound-force", "lbf", force, LBF),
        def("kip", "kip", force, 1000.0 * LBF),
        def(
            "pound-force per square inch",
            "psi",
            pressure,
            LBF / (IN * IN),
        ),
        def(
            "kip per square inch",
            "ksi",
            pressure,
            1000.0 * LBF / (IN * IN),
        ),
        def("US gallon", "gal", volume, 0.003_785_411_784),
        def("acre", "acre", area, 4046.8564224),
        def("hectare", "ha", area, 1e4),
        // energy, power, CGS
        def("horsepower", "hp", power, 745.699_871_582_270_2),
        def("calorie", "cal", energy, 4.184),
        // Not in the help: standard definitions. cal15/cal20 are the 15 and 20 degC calories
        // (NIST); dcal is the dietary Calorie (kilocalorie); Btu is the thermochemical BTU
        // (NIST); IBTU is the International Table BTU (same value as BTU); CBTU is the
        // centigrade BTU = 1.8 IBTU.
        // BTU15 definition used: 252 * cal15 = 1054.746 J. Some references give 1054.68 J;
        // that value is not used here.
        def("calorie at 15 degC", "cal15", energy, 4.1855),
        def("calorie at 20 degC", "cal20", energy, 4.1819),
        def("dietary calorie", "dcal", energy, 4_184.0),
        def("thermochemical British thermal unit", "Btu", energy, 1_054.350_264_488),
        def("British thermal unit at 15 degC", "BTU15", energy, 252.0 * 4.1855),
        def("International Table British thermal unit", "IBTU", energy, 1_055.055_852_62),
        def("centigrade British thermal unit", "CBTU", energy, 1.8 * 1_055.055_852_62),
        def("British thermal unit", "BTU", energy, 1_055.055_852_62),
        def("kilowatt-hour", "kWh", energy, 3.6e6),
        def("electronvolt", "eV", energy, 1.602_176_634e-19),
        def("dyne", "dyn", force, 1e-5),
        def("erg", "erg", energy, 1e-7),
        // temperature: absolute scales carry an offset, differences do not
        affine("degree Celsius", "°C", temp, 1.0, 273.15),
        affine(
            "degree Fahrenheit",
            "°F",
            temp,
            5.0 / 9.0,
            459.67 * 5.0 / 9.0,
        ),
        def("degree Rankine", "°R", temp, 5.0 / 9.0),
        def("delta degree Celsius", "Δ°C", temp, 1.0),
        def("delta degree Fahrenheit", "Δ°F", temp, 5.0 / 9.0),
    ];

    alias(&mut v, "°C", "degC");
    alias(&mut v, "°F", "degF");
    alias(&mut v, "°R", "degR");
    alias(&mut v, "Δ°C", "ΔC");
    alias(&mut v, "Δ°F", "ΔF");
    alias(&mut v, "h", "hr");
    alias(&mut v, "°", "deg");
    alias(&mut v, "Ω", "ohm");
    alias(&mut v, "L", "l");
    alias(&mut v, "ft", "feet");
    // `sec` is second: help input_regions_good4.png shows `Z := (1; 3) sec`, and the samples use
    // `(m/sec)`. Prefixes are not generated for aliases (`msec` is not added).
    alias(&mut v, "s", "sec");

    let mut prefixed = Vec::new();
    for base_symbol in PREFIXABLE {
        let base = v
            .iter()
            .find(|u| u.symbol == *base_symbol)
            .expect("prefixable unit is in the table")
            .clone();
        for &(psym, pname, exp) in PREFIXES {
            let symbol = format!("{psym}{}", base.symbol);
            let taken = v
                .iter()
                .chain(prefixed.iter())
                .any(|u: &UnitDef| u.symbol == symbol);
            if taken {
                continue;
            }
            prefixed.push(UnitDef {
                name: leak(format!("{pname}{}", base.name)),
                symbol: leak(symbol),
                dimension: base.dimension,
                factor_to_si: base.factor_to_si * 10f64.powi(exp),
                offset_to_si: None,
            });
        }
    }
    v.extend(prefixed);
    // Milligram / microgram spellings. Prime's `mgm`/`µgm` are the prefixed-gram symbols; `mg`,
    // `µg` and `μg` are accepted as aliases. Both micro signs are written as escapes:
    // U+00B5 (micro sign) and U+03BC (Greek mu).
    // spelling unverified against live Prime; both accepted when reading files
    alias(&mut v, "mgm", "mg");
    // Megagram (1000 kg). spelling unverified against live Prime; both accepted
    alias(&mut v, "Mgm", "Mg");
    alias(&mut v, "\u{00B5}gm", "\u{00B5}g");
    alias(&mut v, "\u{03BC}gm", "\u{03BC}g");
    v
}

/// A value stored in SI base units together with its dimension.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantity {
    pub value_si: f64,
    pub dim: Dimension,
}

impl Quantity {
    pub fn dimensionless(value: f64) -> Self {
        Self {
            value_si: value,
            dim: Dimension::dimensionless(),
        }
    }

    /// A quantity given in SI base units.
    pub fn from_si(value_si: f64, dim: Dimension) -> Self {
        Self { value_si, dim }
    }

    /// Converts a value expressed in `unit` into SI base units.
    pub fn from_unit(value: f64, unit: &UnitDef) -> Self {
        Self {
            value_si: value * unit.factor_to_si + unit.offset_to_si.unwrap_or(0.0),
            dim: unit.dimension,
        }
    }

    /// Expresses the quantity in `unit`. Dimensions must match.
    pub fn to_unit(&self, unit: &UnitDef) -> Result<f64, UnitsError> {
        if self.dim != unit.dimension {
            return Err(UnitsError::DimensionMismatch {
                left: self.dim,
                right: unit.dimension,
            });
        }
        finite(shift_to_unit(self.value_si, unit.offset_to_si.unwrap_or(0.0)) / unit.factor_to_si)
    }

    /// Expresses the quantity in a compound unit such as `km/h`. Dimensions must match.
    pub fn to_compound(&self, unit: &CompoundUnit) -> Result<f64, UnitsError> {
        if self.dim != unit.dimension() {
            return Err(UnitsError::DimensionMismatch {
                left: self.dim,
                right: unit.dimension(),
            });
        }
        finite(shift_to_unit(self.value_si, unit.offset_to_si()) / unit.factor_to_si())
    }

    /// Addition; both operands must have the same dimension.
    pub fn checked_add(self, rhs: Self) -> Result<Self, UnitsError> {
        self.same_dimension(&rhs)?;
        let (a, b) = (finite(self.value_si)?, finite(rhs.value_si)?);
        Ok(Self {
            value_si: finite(a + b)?,
            dim: self.dim,
        })
    }

    /// Subtraction; both operands must have the same dimension.
    pub fn checked_sub(self, rhs: Self) -> Result<Self, UnitsError> {
        self.same_dimension(&rhs)?;
        let (a, b) = (finite(self.value_si)?, finite(rhs.value_si)?);
        Ok(Self {
            value_si: finite(a - b)?,
            dim: self.dim,
        })
    }

    /// Raises the quantity to a rational power. Fails with `NotFinite` for a non-finite input,
    /// when the result is infinite (e.g. `0 m^-1`), and with `NotReal` when the real result
    /// does not exist.
    pub fn pow(self, p: Exp) -> Result<Self, UnitsError> {
        let value = real_power(finite(self.value_si)?, p)?;
        Ok(Self {
            value_si: value,
            dim: self.dim.pow(p)?,
        })
    }

    /// Multiplication; exponents of the dimensions must not overflow and the value must stay
    /// finite (`NotFinite` on overflow or non-finite input).
    pub fn checked_mul(self, rhs: Self) -> Result<Self, UnitsError> {
        let (a, b) = (finite(self.value_si)?, finite(rhs.value_si)?);
        Ok(Self {
            value_si: finite(a * b)?,
            dim: self.dim.checked_mul(rhs.dim)?,
        })
    }

    /// Division; exponents of the dimensions must not overflow and the value must stay finite
    /// (`NotFinite` for division by zero, e.g. `1 m / 0 s`, and for non-finite operands such as
    /// an infinite divisor).
    pub fn checked_div(self, rhs: Self) -> Result<Self, UnitsError> {
        let (a, b) = (finite(self.value_si)?, finite(rhs.value_si)?);
        Ok(Self {
            value_si: finite(a / b)?,
            dim: self.dim.checked_div(rhs.dim)?,
        })
    }

    fn same_dimension(&self, rhs: &Self) -> Result<(), UnitsError> {
        if self.dim == rhs.dim {
            Ok(())
        } else {
            Err(UnitsError::DimensionMismatch {
                left: self.dim,
                right: rhs.dim,
            })
        }
    }
}

/// `Ok(v)` when `v` is finite; NaN and infinities are `NotFinite`.
fn finite(v: f64) -> Result<f64, UnitsError> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(UnitsError::NotFinite)
    }
}

/// Real power `x^p` of an SI value. An odd root of a negative base is real: `(-8)^(1/3) = -2`.
fn real_power(x: f64, p: Exp) -> Result<f64, UnitsError> {
    let exponent = *p.numer() as f64 / *p.denom() as f64;
    let value = if x < 0.0 && p.denom() % 2 == 1 {
        // x^(n/d) = (x^(1/d))^n with x^(1/d) = -|x|^(1/d) for odd d.
        let magnitude = (-x).powf(exponent);
        if p.numer() % 2 != 0 {
            -magnitude
        } else {
            magnitude
        }
    } else {
        x.powf(exponent)
    };
    if value.is_nan() {
        Err(UnitsError::NotReal)
    } else if !value.is_finite() {
        Err(UnitsError::NotFinite)
    } else {
        Ok(value)
    }
}

/// A product of unit powers, such as `kg*m/s^2` or `km/h`.
#[derive(Debug, Clone, PartialEq)]
pub struct CompoundUnit {
    terms: Vec<(&'static UnitDef, Exp)>,
    /// Cached product dimension, computed once with checked arithmetic in `new`.
    dim: Dimension,
    /// The SI offset of a single absolute temperature unit (`°C`, `°F`); 0 for every other unit.
    offset: f64,
}

impl CompoundUnit {
    /// Builds a compound unit. Affine units (°C, °F) are refused in a product, but a lone one
    /// (`°C`, to the first power) is a valid display unit: it converts with its offset.
    /// Fails with `Overflow` when the combined dimension's exponents leave `i32`.
    pub fn new(terms: Vec<(&'static UnitDef, Exp)>) -> Result<Self, UnitsError> {
        let mut offset = 0.0;
        if let Some((u, _)) = terms.iter().find(|(u, _)| u.offset_to_si.is_some()) {
            match terms.as_slice() {
                [(only, p)] if *p == Exp::from_integer(1) => {
                    offset = only.offset_to_si.unwrap_or(0.0);
                }
                _ => return Err(UnitsError::AffineInCompound(u.symbol.to_string())),
            }
        }
        let dim = terms
            .iter()
            .try_fold(Dimension::dimensionless(), |acc, (u, p)| {
                acc.checked_mul(u.dimension.pow(*p)?)
            })?;
        Ok(Self { terms, dim, offset })
    }

    /// Parses `a*b/c^2` style expressions: `*` or `·` separate factors, one `/` splits the
    /// denominator, and `^n` takes an integer power. `1/s` is accepted.
    pub fn parse(src: &str) -> Result<Self, UnitsError> {
        let parse_err = || UnitsError::Parse(src.to_string());
        let (num, den) = match src.split_once('/') {
            Some((n, d)) if !d.contains('/') => (n.trim(), Some(d)),
            Some(_) => return Err(parse_err()),
            None => (src.trim(), None),
        };
        let mut terms = Vec::new();
        if num != "1" {
            push_terms(num, 1, src, &mut terms)?;
        }
        if let Some(d) = den {
            push_terms(d, -1, src, &mut terms)?;
        }
        if terms.is_empty() {
            return Err(parse_err());
        }
        Self::new(terms)
    }

    pub fn terms(&self) -> &[(&'static UnitDef, Exp)] {
        &self.terms
    }

    pub fn factor_to_si(&self) -> f64 {
        self.terms
            .iter()
            .map(|(u, p)| u.factor_to_si.powf(*p.numer() as f64 / *p.denom() as f64))
            .product()
    }

    pub fn dimension(&self) -> Dimension {
        self.dim
    }

    /// The SI offset of the unit: nonzero only for a lone `°C` or `°F`.
    pub fn offset_to_si(&self) -> f64 {
        self.offset
    }

    /// The unit as text for display: `m/s`, `kg*m/s^2` with a middle dot, parentheses for several
    /// denominator factors.
    pub fn display_text(&self) -> String {
        let text = self.to_string().replace('*', "·");
        // The aliases of the temperature units are shown with the degree sign (`degF` as `°F`).
        match text.as_str() {
            "degC" | "degF" | "degR" => text.replacen("deg", "°", 1),
            _ => text,
        }
    }

    /// A quantity of `value` in this compound unit.
    pub fn quantity(&self, value: f64) -> Quantity {
        Quantity {
            value_si: value * self.factor_to_si() + self.offset,
            dim: self.dimension(),
        }
    }
}

impl fmt::Display for CompoundUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let items: Vec<(&str, Exp)> = self.terms.iter().map(|(u, p)| (u.symbol, *p)).collect();
        f.write_str(&fmt_product(&items))
    }
}

fn push_terms(
    part: &str,
    sign: i32,
    src: &str,
    terms: &mut Vec<(&'static UnitDef, Exp)>,
) -> Result<(), UnitsError> {
    for tok in part.split(['*', '·']) {
        let tok = tok.trim();
        if tok.is_empty() {
            return Err(UnitsError::Parse(src.to_string()));
        }
        let (sym, power) = match tok.split_once('^') {
            Some((s, p)) => {
                let p = p
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| UnitsError::Parse(src.to_string()))?;
                (s.trim(), p)
            }
            None => (tok, 1),
        };
        let u = unit(sym).ok_or_else(|| UnitsError::UnknownUnit(sym.to_string()))?;
        // `-1 * i32::MIN` overflows; such an exponent is not a valid unit power.
        let power = sign
            .checked_mul(power)
            .ok_or_else(|| UnitsError::Parse(src.to_string()))?;
        terms.push((u, Ratio::from_integer(power)));
    }
    Ok(())
}

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*;

    fn u(key: &str) -> &'static UnitDef { unit(key).unwrap_or_else(|| panic!("unit {key} missing")) }
    fn cu(src: &str) -> CompoundUnit { CompoundUnit::parse(src).unwrap() }
    fn close(a: f64, b: f64) { let s = a.abs().max(b.abs()).max(1.0); assert!((a - b).abs() <= 1e-9 * s, "{a} != {b}"); }
    fn vel() -> Dimension { dim_div(Dimension::base(0), Dimension::base(2)) }
    fn conv(v: f64, from: &str, to: &str) -> f64 { Quantity::from_unit(v, u(from)).to_unit(u(to)).unwrap() }
    fn is_mismatch<T>(r: Result<T, UnitsError>) -> bool { matches!(r, Err(UnitsError::DimensionMismatch { .. })) }

    #[test] fn newton_is_kg_m_per_s_squared() {
        assert_eq!(u("N").dimension, cu("kg*m/s^2").dimension());
        assert_eq!(u("N").dimension.to_string(), "m*kg/s^2");
    }
    #[test] fn metre_plus_second_is_a_dimension_error() {
        let m = Quantity::from_unit(2.0, u("m"));
        let s = Quantity::from_unit(3.0, u("s"));
        assert!(is_mismatch(m.checked_add(s)));
        assert!(is_mismatch(m.checked_sub(s)));
    }
    #[test] fn same_dimension_adds_in_si() {
        let a = Quantity::from_unit(1.0, u("km"));
        let b = Quantity::from_unit(500.0, u("m"));
        close(a.checked_add(b).unwrap().value_si, 1500.0);
        close(a.checked_sub(b).unwrap().value_si, 500.0);
    }
    #[test] fn foot_to_metre() {
        close(conv(1.0, "ft", "m"), 0.3048);
        close(conv(10.0, "ft", "in"), 120.0);
    }
    #[test] fn hour_is_3600_seconds() {
        close(conv(1.0, "h", "s"), 3600.0);
        close(conv(1.0, "hr", "s"), 3600.0);
        close(conv(2.0, "min", "s"), 120.0);
        close(conv(1.0, "d", "s"), 86_400.0);
    }
    #[test] fn degrees_to_radians() {
        close(conv(180.0, "°", "rad"), PI);
        close(conv(90.0, "deg", "rad"), PI / 2.0);
        close(conv(1.0, "rad", "°"), 180.0 / PI);
        assert!(u("°").dimension.is_dimensionless());
    }
    #[test] fn kilopascal_and_pascal() {
        close(conv(2.0, "kPa", "Pa"), 2000.0);
        close(conv(5000.0, "Pa", "kPa"), 5.0);
        close(conv(1.0, "MPa", "Pa"), 1e6);
        close(conv(1.0, "mbar", "Pa"), 100.0);
    }
    #[test] fn psi_and_pascal() {
        close(conv(1.0, "psi", "Pa"), 6_894.757_293_168_36);
        close(conv(14.695_948_775_5, "psi", "Pa"), 101_325.0);
        close(conv(6_894.757_293_168_36, "Pa", "psi"), 1.0);
    }
    #[test] fn pound_force_to_newton() {
        close(conv(1.0, "lbf", "N"), 4.448_221_615_260_5);
        close(conv(1.0, "lb", "kg"), 0.45359237);
    }
    #[test] fn kilometre_per_hour_to_metre_per_second() {
        let kmh = cu("km/h");
        close(kmh.quantity(36.0).to_compound(&cu("m/s")).unwrap(), 10.0);
        close(Quantity::from_si(10.0, vel()).to_compound(&kmh).unwrap(), 36.0);
    }
    #[test] fn miles_per_hour() {
        close(Quantity::from_unit(60.0, u("mph")).to_compound(&cu("m/s")).unwrap(), 26.8224);
        close(conv(1.0, "mph", "knot"), 0.868_976_241_9);
    }
    #[test] fn compound_factor_and_dimension() {
        close(cu("km/h").factor_to_si(), 1000.0 / 3600.0);
        assert_eq!(cu("km/h").dimension(), vel());
        assert_eq!(cu("kg*m/s^2").to_string(), "kg*m/s^2");
        assert_eq!(cu("km/h").to_string(), "km/h");
        close(cu("kg*m/s^2").factor_to_si(), 1.0);
    }
    #[test] fn compound_mismatch_is_an_error() {
        assert!(is_mismatch(Quantity::from_unit(1.0, u("m")).to_compound(&cu("m/s"))));
    }
    #[test] fn multiplication_and_division() {
        let p = Quantity::from_unit(2.0, u("m")).checked_mul(Quantity::from_unit(3.0, u("kg"))).unwrap();
        assert_eq!(p.dim, cu("kg*m").dimension());
        close(p.value_si, 6.0);
        let v = Quantity::from_unit(10.0, u("m")).checked_div(Quantity::from_unit(2.0, u("s"))).unwrap();
        assert_eq!(v.dim, vel());
        close(v.to_compound(&cu("m/s")).unwrap(), 5.0);
        assert!(Quantity::from_unit(3.0, u("m")).checked_div(Quantity::from_unit(1.0, u("m"))).unwrap().dim.is_dimensionless());
    }
    #[test] fn force_from_mass_and_acceleration() {
        let accel = Quantity::from_unit(3.0, u("m")).checked_div(Quantity::from_unit(1.0, u("s")).pow(Ratio::from_integer(2)).unwrap()).unwrap();
        let f = Quantity::from_unit(2.0, u("kg")).checked_mul(accel).unwrap();
        assert_eq!(f.dim, u("N").dimension);
        close(f.to_unit(u("N")).unwrap(), 6.0);
    }
    #[test] fn powers_and_roots_track_dimensions() {
        let area = Quantity::from_unit(4.0, u("m")).pow(Ratio::from_integer(2)).unwrap();
        assert_eq!(area.dim, Dimension::base(0).pow(Ratio::from_integer(2)).unwrap());
        close(area.value_si, 16.0);
        let side = area.pow(Ratio::new(1, 2)).unwrap();
        assert_eq!(side.dim, Dimension::base(0));
        close(side.value_si, 4.0);
        let cube = Quantity::from_unit(8.0, u("m")).pow(Ratio::from_integer(3)).unwrap();
        close(cube.pow(Ratio::new(1, 3)).unwrap().value_si, 8.0);
    }
    #[test] fn root_of_negative_is_not_real() {
        assert_eq!(Quantity::dimensionless(-4.0).pow(Ratio::new(1, 2)), Err(UnitsError::NotReal));
    }
    #[test] fn odd_root_of_negative_is_real() {
        // (-8 m)^(1/3) = -2 m; (-8)^(2/3) = 4; (-8)^(-1/3) = -1/2.
        let r = Quantity::from_unit(-8.0, u("m")).pow(Ratio::new(1, 3)).unwrap();
        close(r.value_si, -2.0);
        assert_eq!(r.dim, Dimension::base(0).pow(Ratio::new(1, 3)).unwrap());
        close(Quantity::dimensionless(-8.0).pow(Ratio::new(2, 3)).unwrap().value_si, 4.0);
        close(Quantity::dimensionless(-8.0).pow(Ratio::new(-1, 3)).unwrap().value_si, -0.5);
        // Even denominators stay non-real for negative bases.
        assert_eq!(Quantity::dimensionless(-8.0).pow(Ratio::new(1, 4)), Err(UnitsError::NotReal));
    }
    #[test] fn non_finite_powers_are_errors() {
        // 0 m^-1 is infinite.
        assert_eq!(Quantity::from_unit(0.0, u("m")).pow(Ratio::from_integer(-1)), Err(UnitsError::NotFinite));
        // Infinite input is rejected by pow.
        let inf = Quantity::from_si(f64::INFINITY, Dimension::base(0));
        assert_eq!(inf.pow(Ratio::from_integer(1)), Err(UnitsError::NotFinite));
        // Overflow of the value itself is also not finite.
        assert_eq!(Quantity::from_unit(2.0, u("m")).pow(Ratio::from_integer(i32::MAX)), Err(UnitsError::NotFinite));
    }
    #[test] fn checked_div_by_zero_is_not_finite() {
        // 1 m / 0 s is infinite.
        assert_eq!(
            Quantity::from_unit(1.0, u("m")).checked_div(Quantity::from_unit(0.0, u("s"))),
            Err(UnitsError::NotFinite)
        );
        // 0 m / 0 s is NaN.
        assert_eq!(
            Quantity::from_unit(0.0, u("m")).checked_div(Quantity::from_unit(0.0, u("s"))),
            Err(UnitsError::NotFinite)
        );
    }
    #[test] fn checked_mul_overflow_is_not_finite() {
        // 1e200 m * 1e200 m = 1e400 m^2 overflows f64.
        assert_eq!(
            Quantity::from_unit(1e200, u("m")).checked_mul(Quantity::from_unit(1e200, u("m"))),
            Err(UnitsError::NotFinite)
        );
    }
    #[test] fn non_finite_inputs_are_rejected_by_arithmetic() {
        let inf = Quantity::from_si(f64::INFINITY, Dimension::base(0));
        let nan = Quantity::from_si(f64::NAN, Dimension::base(0));
        let one = Quantity::from_unit(1.0, u("m"));
        assert_eq!(inf.checked_mul(one), Err(UnitsError::NotFinite));
        assert_eq!(nan.checked_mul(one), Err(UnitsError::NotFinite));
        assert_eq!(one.checked_div(nan), Err(UnitsError::NotFinite));
        assert_eq!(inf.checked_div(one), Err(UnitsError::NotFinite));
        assert_eq!(inf.checked_add(one), Err(UnitsError::NotFinite));
        assert_eq!(nan.checked_add(one), Err(UnitsError::NotFinite));
        assert_eq!(one.checked_sub(inf), Err(UnitsError::NotFinite));
        assert_eq!(nan.checked_sub(one), Err(UnitsError::NotFinite));
    }
    #[test] fn infinite_or_nan_divisor_is_not_finite() {
        let one = Quantity::from_unit(1.0, u("m"));
        let inf = Quantity::from_si(f64::INFINITY, Dimension::base(2));
        let neg_inf = Quantity::from_si(f64::NEG_INFINITY, Dimension::base(2));
        let nan = Quantity::from_si(f64::NAN, Dimension::base(2));
        // 1 / inf would be 0 and 1 / -inf would be -0; both must be rejected as inputs.
        assert_eq!(one.checked_div(inf), Err(UnitsError::NotFinite));
        assert_eq!(one.checked_div(neg_inf), Err(UnitsError::NotFinite));
        assert_eq!(one.checked_div(nan), Err(UnitsError::NotFinite));
        assert_eq!(nan.checked_div(one), Err(UnitsError::NotFinite));
        assert_eq!(neg_inf.checked_div(one), Err(UnitsError::NotFinite));
    }
    #[test] fn infinite_or_nan_operands_are_rejected_by_mul_add_sub() {
        let one = Quantity::from_unit(1.0, u("m"));
        let neg_inf = Quantity::from_si(f64::NEG_INFINITY, Dimension::base(0));
        let nan = Quantity::from_si(f64::NAN, Dimension::base(0));
        assert_eq!(one.checked_mul(neg_inf), Err(UnitsError::NotFinite));
        assert_eq!(one.checked_mul(nan), Err(UnitsError::NotFinite));
        assert_eq!(one.checked_add(neg_inf), Err(UnitsError::NotFinite));
        assert_eq!(one.checked_add(nan), Err(UnitsError::NotFinite));
        assert_eq!(one.checked_sub(nan), Err(UnitsError::NotFinite));
        assert_eq!(neg_inf.checked_sub(one), Err(UnitsError::NotFinite));
    }
    #[test] fn non_finite_inputs_are_rejected_by_pow() {
        let inf = Quantity::from_si(f64::INFINITY, Dimension::base(0));
        let neg_inf = Quantity::from_si(f64::NEG_INFINITY, Dimension::base(0));
        let nan = Quantity::from_si(f64::NAN, Dimension::base(0));
        // Input check runs first, so these are NotFinite rather than Ok(0.0) or NotReal.
        assert_eq!(inf.pow(Ratio::from_integer(-1)), Err(UnitsError::NotFinite));
        assert_eq!(neg_inf.pow(Ratio::from_integer(-1)), Err(UnitsError::NotFinite));
        assert_eq!(nan.pow(Ratio::from_integer(1)), Err(UnitsError::NotFinite));
    }
    #[test] fn non_finite_results_are_rejected_by_add_and_sub() {
        let big = Quantity::from_si(f64::MAX, Dimension::base(0));
        assert_eq!(big.checked_add(big), Err(UnitsError::NotFinite));
        assert_eq!(big.checked_sub(Quantity::from_si(-f64::MAX, Dimension::base(0))), Err(UnitsError::NotFinite));
    }
    #[test] fn non_finite_conversions_are_errors() {
        let inf = Quantity::from_si(f64::INFINITY, Dimension::base(0));
        let nan = Quantity::from_si(f64::NAN, Dimension::base(0));
        assert_eq!(inf.to_unit(u("m")), Err(UnitsError::NotFinite));
        assert_eq!(nan.to_unit(u("m")), Err(UnitsError::NotFinite));
        // Finite SI value that overflows when expressed in nm.
        assert_eq!(Quantity::from_si(f64::MAX, Dimension::base(0)).to_unit(u("nm")), Err(UnitsError::NotFinite));
        let vel_nan = Quantity::from_si(f64::NAN, vel());
        assert_eq!(vel_nan.to_compound(&cu("m/s")), Err(UnitsError::NotFinite));
        assert_eq!(Quantity::from_si(f64::MAX, vel()).to_compound(&cu("nm/s")), Err(UnitsError::NotFinite));
    }
    #[test] fn milligram_spellings_resolve_to_milligram() {
        // Both `mg` and `mgm` are milligram (1e-6 kg); both micro signs are accepted.
        close(u("mg").factor_to_si, 1e-6);
        close(u("mgm").factor_to_si, 1e-6);
        close(u("\u{00B5}g").factor_to_si, 1e-9);
        close(u("\u{03BC}g").factor_to_si, 1e-9);
        close(u("\u{00B5}gm").factor_to_si, 1e-9);
        close(u("\u{03BC}gm").factor_to_si, 1e-9);
        assert_eq!(u("mg").dimension, u("kg").dimension);
        close(conv(1.0, "mg", "gm"), 1e-3);
        close(conv(1.0, "kgm", "kg"), 1.0);
        close(u("kg").factor_to_si, 1.0);
        // `g` stays standard gravity (acceleration), never a mass.
        assert_eq!(u("g").dimension, dim_div(u("m").dimension, power_of(u("s").dimension, 2)));
        assert!(u("g").dimension != u("kg").dimension);
    }
    #[test] fn megagram_spellings_resolve_to_megagram() {
        // `Mg` and `Mgm` are both megagram (1e3 kg); spelling unverified against live Prime.
        close(u("Mg").factor_to_si, 1e3);
        close(u("Mgm").factor_to_si, 1e3);
        assert_eq!(u("Mg").dimension, u("kg").dimension);
        close(conv(1.0, "Mg", "kg"), 1000.0);
        close(conv(1.0, "Mgm", "kg"), 1000.0);
        // `kg` stays the base and `g` stays standard gravity.
        close(u("kg").factor_to_si, 1.0);
        assert_eq!(u("g").dimension, dim_div(u("m").dimension, power_of(u("s").dimension, 2)));
    }
    #[test] fn fractional_dimension_display() {
        assert_eq!(Dimension::base(0).pow(Ratio::new(1, 2)).unwrap().to_string(), "m^(1/2)");
        assert_eq!(Dimension::dimensionless().to_string(), "1");
    }
    #[test] fn inverse_turns_hertz_into_seconds_and_back() {
        assert_eq!(u("Hz").dimension.inverse().unwrap(), Dimension::base(2));
        assert_eq!(u("s").dimension.inverse().unwrap(), u("Hz").dimension);
    }
    #[test] fn exponent_overflow_is_an_error_not_a_panic() {
        // Dimension::base(0).pow(i32::MAX).pow(2) used to panic.
        assert_eq!(
            Dimension::base(0).pow(Ratio::from_integer(i32::MAX)).unwrap().pow(Ratio::from_integer(2)),
            Err(UnitsError::Overflow)
        );
        // Summing exponents past i32::MAX.
        let big = Dimension::base(0).pow(Ratio::from_integer(i32::MAX)).unwrap();
        assert_eq!(big.checked_mul(big), Err(UnitsError::Overflow));
        assert_eq!(Dimension::base(0).pow(Ratio::from_integer(i32::MIN)).unwrap().inverse(), Err(UnitsError::Overflow));
        // Quantity arithmetic reports the same error.
        let q = Quantity::from_unit(1.0, u("m")).pow(Ratio::from_integer(i32::MAX)).unwrap();
        assert_eq!(q.checked_mul(q), Err(UnitsError::Overflow));
    }
    #[test] fn compound_exponent_overflow_is_an_error_not_a_panic() {
        // `-1 * i32::MIN` in the parser used to panic.
        assert!(matches!(CompoundUnit::parse("m/s^-2147483648"), Err(UnitsError::Parse(_))));
        // Summing the dimension exponents past i32::MAX used to panic in num-rational.
        assert_eq!(CompoundUnit::parse("m^2147483647*m^2147483647"), Err(UnitsError::Overflow));
    }
    #[test] fn minimum_exponent_displays_without_panic() {
        // fmt_product used `-e`, which overflows for i32::MIN.
        assert_eq!(CompoundUnit::parse("m^-2147483648").unwrap().to_string(), "1/m^2147483648");
        assert_eq!(Dimension::base(0).pow(Ratio::from_integer(i32::MIN)).unwrap().to_string(), "1/m^2147483648");
    }
    #[test] fn celsius_kelvin_fahrenheit_offsets() {
        close(Quantity::from_unit(0.0, u("°C")).value_si, 273.15);
        close(conv(273.15, "K", "°C"), 0.0);
        close(conv(100.0, "°C", "°F"), 212.0);
        close(conv(212.0, "°F", "K"), 373.15);
        close(conv(32.0, "°F", "°C"), 0.0);
        close(conv(-40.0, "°C", "°F"), -40.0);
        close(conv(0.0, "°R", "K"), 0.0);
    }
    #[test] fn offset_round_trips_snap_float_noise_to_zero() {
        // 491.67 degR * 5/9 is not exactly 273.15 in binary floating point
        assert_eq!(conv(491.67, "°R", "°C"), 0.0);
        assert_eq!(conv(32.0, "°F", "°C"), 0.0);
        assert_eq!(conv(459.67, "°R", "°F"), 0.0);
        // real small values survive, and units without an offset are never snapped
        close(conv(273.150_001, "K", "°C"), 1e-6);
        // 1e-10 degC is a real value, not noise (the snap is a few ulps, about 5e-13 near 0 degC)
        assert_ne!(conv(273.150_000_000_1, "K", "°C"), 0.0);
        assert_ne!(conv(5e-14, "m", "m"), 0.0);
    }
    #[test] fn temperature_differences_have_no_offset() {
        close(conv(9.0, "Δ°F", "Δ°C"), 5.0);
        close(conv(5.0, "Δ°C", "K"), 5.0);
        assert!(u("Δ°C").offset_to_si.is_none() && u("°C").offset_to_si.is_some());
    }
    #[test] fn affine_unit_refused_in_compound() {
        assert!(matches!(CompoundUnit::parse("°C*m"), Err(UnitsError::AffineInCompound(_))));
    }
    #[test] fn prefixed_lookup() {
        close(u("kN").factor_to_si, 1000.0);
        close(u("mm").factor_to_si, 1e-3);
        close(u("µm").factor_to_si, 1e-6);
        close(u("kg").factor_to_si, 1.0);
        // Prefixed grams go through the `gm` symbol (`g` is standard gravity).
        close(u("mgm").factor_to_si, 1e-6);
        close(u("µgm").factor_to_si, 1e-9);
        close(u("kgm").factor_to_si, 1.0);
        assert_eq!(u("kN").dimension, u("N").dimension);
        assert_eq!(unit("kilonewton").unwrap().symbol, "kN");
    }
    #[test] fn gram_is_gm_and_g_is_standard_gravity() {
        assert_eq!(u("gm").name, "gram");
        close(conv(150.0, "gm", "kg"), 0.15);
        close(conv(1.0, "kg", "gm"), 1000.0);
        let g = u("g");
        assert_eq!(g.name, "standard gravity");
        assert_eq!(g.dimension, cu("m/s^2").dimension());
        close(Quantity::from_unit(1.0, g).value_si, 9.80665);
        assert_eq!(unit("standard gravity").unwrap().symbol, "g");
    }
    #[test] fn pa_is_petaampere_and_pascal_is_pa_symbol() {
        // Symbols are case-sensitive; this pins the current resolution.
        assert_eq!(u("PA").name, "petaampere");
        assert_eq!(u("Pa").name, "pascal");
        assert!(unit("pa").is_none());
    }
    #[test] fn added_palette_units_convert() {
        close(conv(1.0, "torr", "Pa"), 101_325.0 / 760.0);
        close(conv(760.0, "torr", "Pa"), 101_325.0);
        close(conv(1.0, "in_Hg", "Pa"), 3_386.389);
        close(conv(1.0, "psf", "Pa"), 47.880_258_98);
        close(conv(1.0, "ksf", "psf"), 1000.0);
        close(Quantity::from_unit(1.0, u("barn")).value_si, 1e-28);
        assert_eq!(u("dpi").dimension, Dimension::base(0).inverse().unwrap());
        close(Quantity::from_unit(1.0, u("dpi")).value_si, 1.0 / IN);
        close(conv(1.0, "Btu", "J"), 1_054.350_264_488);
        close(conv(1.0, "IBTU", "J"), 1_055.055_852_62);
        close(conv(1.0, "BTU", "IBTU"), 1.0);
        close(conv(1.0, "BTU15", "J"), 252.0 * 4.1855);
        close(conv(1.0, "CBTU", "IBTU"), 1.8);
        close(conv(1.0, "cal15", "J"), 4.1855);
        close(conv(1.0, "cal20", "J"), 4.1819);
        close(conv(1.0, "dcal", "J"), 4_184.0);
        close(conv(1.0, "C", "statC"), 2_997_924_580.0);
        assert_eq!(u("statC").dimension, u("C").dimension);
    }
    #[test] fn hza_is_sixty_rpm() {
        close(conv(1.0, "Hza", "rpm"), 60.0);
        assert_eq!(u("Hza").dimension, u("Hz").dimension);
    }
    #[test] fn name_and_symbol_lookup() {
        assert_eq!(unit("kilogram").unwrap().symbol, "kg");
        assert_eq!(unit("Meter").unwrap().symbol, "m");
        assert!(unit("zorblax").is_none());
    }
    #[test] fn electrical_and_energy_units() {
        close(conv(1.0, "kWh", "J"), 3.6e6);
        close(conv(1.0, "erg", "J"), 1e-7);
        close(conv(1.0, "dyn", "N"), 1e-5);
        assert_eq!(u("V").dimension, dim_div(u("W").dimension, u("A").dimension));
        assert_eq!(u("C").dimension, dim_mul(u("A").dimension, u("s").dimension));
        assert_eq!(u("ohm").dimension, dim_div(u("V").dimension, u("A").dimension));
        assert_eq!(u("T").dimension, dim_div(u("kg").dimension, dim_mul(u("A").dimension, power_of(u("s").dimension, 2))));
    }
    fn dim_mul(a: Dimension, b: Dimension) -> Dimension { a.checked_mul(b).unwrap() }
    fn dim_div(a: Dimension, b: Dimension) -> Dimension { a.checked_div(b).unwrap() }
    #[test] fn parse_errors() {
        assert!(matches!(CompoundUnit::parse("m//s"), Err(UnitsError::Parse(_))));
        assert!(matches!(CompoundUnit::parse("foo/s"), Err(UnitsError::UnknownUnit(_))));
        assert!(matches!(CompoundUnit::parse("m*"), Err(UnitsError::Parse(_))));
        assert!(matches!(CompoundUnit::parse("m^x"), Err(UnitsError::Parse(_))));
        assert_eq!(cu("1/s").dimension(), u("Hz").dimension);
    }
    #[test] fn pascal_is_newton_per_square_metre() {
        assert_eq!(u("Pa").dimension, dim_div(u("N").dimension, power_of(u("m").dimension, 2)));
        assert_eq!(u("Pa").dimension.to_string(), "kg/(m*s^2)");
        close(conv(1.0, "bar", "Pa"), 1e5);
    }
    #[test] fn wrong_target_unit_is_rejected() {
        assert!(is_mismatch(Quantity::from_unit(1.0, u("m")).to_unit(u("s"))));
        assert!(is_mismatch(Quantity::from_unit(1.0, u("°C")).to_unit(u("m"))));
    }
    #[test] fn revolutions_per_minute_follow_prime_definition() {
        // Prime: 1 rpm = 2*pi rad/min with rad dimensionless, so 1 rpm = 0.105 Hz (not 1/60 Hz).
        close(conv(1.0, "rpm", "Hz"), TAU / 60.0);
        close(conv(1.0, "rev", "rad"), TAU);
    }
    #[test] fn unit_table_symbols_are_unique() {
        let mut symbols: Vec<&str> = units().iter().map(|u| u.symbol).collect();
        symbols.sort_unstable();
        let before = symbols.len();
        symbols.dedup();
        assert_eq!(before, symbols.len());
    }
    #[test] fn compound_terms_expose_their_powers() {
        let c = cu("kg*m/s^2");
        assert_eq!(c.terms().len(), 3);
        assert_eq!(c.terms()[2].1, Ratio::from_integer(-2));
        assert_eq!(c.quantity(2.0).dim, u("N").dimension);
    }
}
