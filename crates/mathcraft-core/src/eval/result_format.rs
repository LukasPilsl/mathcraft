//! Display of numeric results in Prime's result formats (help: `to_format_the_result_display`):
//! General, Decimal, Scientific, Engineering and Percent, with display precision, trailing zeros,
//! the exponential, zero and complex thresholds, and the radix of integers.
//!
//! Display only: a value keeps its full precision, and the text is cut here.

use std::ops::Range;

use thiserror::Error;

/// Largest display precision (decimal places) the help allows.
pub const MAX_PRECISION: u8 = 16;
/// Largest exponential threshold the help allows.
pub const MAX_EXPONENTIAL_THRESHOLD: u8 = 15;
/// Largest zero or complex threshold the help allows.
pub const MAX_ZERO_THRESHOLD: u16 = 307;
/// Most significant digits of a long integer part. Prime shows 15 accurate digits and pads the
/// rest with zeros (help: `12345678901234567890` shows as `12345678901234600000`).
const ACCURATE_DIGITS: usize = 15;

/// The number format of a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NumberFormat {
    /// Exponential notation past the exponential threshold, decimal otherwise.
    #[default]
    General,
    /// Never exponential notation.
    Decimal,
    /// Always exponential notation, mantissa in `[1, 10)`.
    Scientific,
    /// Always exponential notation, exponent a multiple of three.
    Engineering,
    /// Multiplied by 100 and shown with a percent sign.
    Percent,
}

/// The radix of an integer result. Other results are always decimal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Radix {
    /// Decimal.
    #[default]
    Decimal,
    /// Binary, written with a `b` suffix (`1000b`).
    Binary,
    /// Octal, written with an `o` suffix (`10o`).
    Octal,
    /// Hexadecimal, written with an `h` suffix (`8h`).
    Hexadecimal,
}

impl Radix {
    /// The `radix` attribute of `presentation.xml`. Only `dec` is observed in the samples; the
    /// other spellings are unverified against Prime.
    #[must_use]
    pub fn from_attr(value: &str) -> Option<Self> {
        match value {
            "dec" => Some(Self::Decimal),
            "bin" => Some(Self::Binary),
            "oct" => Some(Self::Octal),
            "hex" => Some(Self::Hexadecimal),
            _ => None,
        }
    }

    /// The `radix` attribute spelling of this radix.
    #[must_use]
    pub fn attr(self) -> &'static str {
        match self {
            Self::Decimal => "dec",
            Self::Binary => "bin",
            Self::Octal => "oct",
            Self::Hexadecimal => "hex",
        }
    }
}

/// The unit of a polar angle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AngleUnit {
    /// Radians (`∠0.785`).
    #[default]
    Radians,
    /// Degrees (`∠45°`).
    Degrees,
}

/// How a complex result is written (help: Complex Values, Cartesian or Polar form).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComplexForm {
    /// `re + im·i` (or `j`), the default.
    #[default]
    Cartesian,
    /// `magnitude∠angle`.
    Polar(AngleUnit),
}

/// The display of a matrix result: how many rows and columns show, where the window starts, the
/// row and column indices, and whether nested matrices show their elements. Stored per region
/// (`resultFormat/matrix`). Display only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatrixDisplay {
    /// Rows shown before the ellipsis (help: 12 by default).
    pub rows: u32,
    /// Columns shown before the ellipsis.
    pub cols: u32,
    /// The first row shown.
    pub offset_row: u32,
    /// The first column shown.
    pub offset_col: u32,
    /// Whether the row and column indices are shown (help: Show Indices).
    pub show_indices: bool,
    /// Whether nested matrices show their elements (help: Collapse Nested Matrices is off).
    pub expand_nested: bool,
}

impl Default for MatrixDisplay {
    fn default() -> Self {
        Self {
            rows: 12,
            cols: 12,
            offset_row: 0,
            offset_col: 0,
            show_indices: false,
            expand_nested: false,
        }
    }
}

impl MatrixDisplay {
    /// The rows and columns of a `rows` by `cols` matrix that the display shows: the window that
    /// starts at the offset and is at most the visible size.
    #[must_use]
    pub fn window(&self, rows: usize, cols: usize) -> (Range<usize>, Range<usize>) {
        let r0 = (self.offset_row as usize).min(rows);
        let c0 = (self.offset_col as usize).min(cols);
        let r1 = r0.saturating_add(self.rows as usize).min(rows);
        let c1 = c0.saturating_add(self.cols as usize).min(cols);
        (r0..r1, c0..c1)
    }
}

/// A complete result format. The default is Prime's default: General, display precision 3, no
/// trailing zeros, exponential threshold 3, zero threshold 15, complex threshold 10, decimal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultFormat {
    /// The number format.
    pub number: NumberFormat,
    /// Digits right of the decimal point (0 to 16). For Scientific and Engineering, the digits of
    /// the mantissa.
    pub precision: u8,
    /// Whether trailing zeros fill the display precision (`1.500` rather than `1.5`).
    pub trailing_zeros: bool,
    /// General format: numbers whose power of ten is at least this, or at most its negative, show
    /// in exponential notation (0 to 15).
    pub exponential_threshold: u8,
    /// Numbers below `10^-zero_threshold` in magnitude show as zero (0 to 307).
    pub zero_threshold: u16,
    /// A part of a complex number is dropped when the other part is more than `10^complex_threshold`
    /// times larger (0 to 307).
    pub complex_threshold: u16,
    /// The radix of integer results.
    pub radix: Radix,
    /// Whether the imaginary unit is written `j` rather than `i`.
    pub imaginary_j: bool,
    /// Whether the integer digits are grouped in threes with the locale's group separator
    /// (`1,234,567` in English, `1.234.567` in German). Display only.
    pub digit_grouping: bool,
    /// Cartesian or polar form of complex results.
    pub complex_form: ComplexForm,
}

impl Default for ResultFormat {
    fn default() -> Self {
        Self {
            number: NumberFormat::General,
            precision: 3,
            trailing_zeros: false,
            exponential_threshold: 3,
            zero_threshold: 15,
            complex_threshold: 10,
            radix: Radix::Decimal,
            imaginary_j: false,
            digit_grouping: false,
            complex_form: ComplexForm::Cartesian,
        }
    }
}

/// A result format setting outside the range the help allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FormatError {
    /// A setting above its largest allowed value.
    #[error("{field} must be at most {max}, got {value}")]
    OutOfRange {
        /// The setting's name.
        field: &'static str,
        /// The given value.
        value: u32,
        /// The largest allowed value.
        max: u32,
    },
}

impl ResultFormat {
    /// The format with every setting checked against the help's ranges.
    ///
    /// # Errors
    /// [`FormatError::OutOfRange`] for a precision, threshold above its maximum.
    pub fn checked(self) -> Result<Self, FormatError> {
        let checks = [
            (
                "precision",
                u32::from(self.precision),
                u32::from(MAX_PRECISION),
            ),
            (
                "exponential threshold",
                u32::from(self.exponential_threshold),
                u32::from(MAX_EXPONENTIAL_THRESHOLD),
            ),
            (
                "zero threshold",
                u32::from(self.zero_threshold),
                u32::from(MAX_ZERO_THRESHOLD),
            ),
            (
                "complex threshold",
                u32::from(self.complex_threshold),
                u32::from(MAX_ZERO_THRESHOLD),
            ),
        ];
        for (field, value, max) in checks {
            if value > max {
                return Err(FormatError::OutOfRange { field, value, max });
            }
        }
        Ok(self)
    }
}

/// How a number is written for a locale: the decimal point and an optional digit-group separator.
///
/// Display only. The worksheet file always stores invariant numbers (`.` and no grouping).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberSeparators {
    /// The character written for the decimal point (`.` or `,`).
    pub decimal: char,
    /// The character that groups the integer digits in threes, when there is one.
    pub thousands: Option<char>,
}

impl NumberSeparators {
    /// English and invariant numbers: `1234.5`, no grouping. The default output.
    pub const INVARIANT: Self = Self {
        decimal: '.',
        thousands: None,
    };
    /// German numbers: `1234,5`. Prime's German results show no digit grouping.
    pub const GERMAN: Self = Self {
        decimal: ',',
        thousands: None,
    };
}

/// The text of a real result in `format`, with invariant separators.
///
/// NaN and the infinities keep their short forms (`NaN`, `inf`, `-inf`). A value below the zero
/// threshold, and zero itself, show as `0`.
#[must_use]
pub fn format_real(x: f64, format: &ResultFormat) -> String {
    format_real_with(x, format, &NumberSeparators::INVARIANT)
}

/// The text of a real result in `format`, written with the separators `seps`.
#[must_use]
pub fn format_real_with(x: f64, format: &ResultFormat, seps: &NumberSeparators) -> String {
    localize(format_real_plain(x, format), seps, format.digit_grouping)
}

/// Applies a locale's separators to text written with `.` as the decimal point. Only the decimal
/// points and the integer digits are touched: the radix suffixes, `%`, `×10^` and `NaN` have no
/// digit group. The integer digits are grouped when the separators name a group character, or
/// when `grouping` is set: then the group is `,` after a `.` decimal and `.` after a `,` decimal.
fn localize(text: String, seps: &NumberSeparators, grouping: bool) -> String {
    // Every `.` is a decimal point (a complex result has two). The point is swapped first, so a
    // digit group separator that is also `.` cannot be mistaken for it.
    let text = if seps.decimal == '.' {
        text
    } else {
        text.replace('.', &seps.decimal.to_string())
    };
    let group = seps
        .thousands
        .or_else(|| grouping.then_some(if seps.decimal == '.' { ',' } else { '.' }));
    match group {
        Some(sep) => group_integer_digits(&text, sep, seps.decimal),
        None => text,
    }
}

/// Inserts `sep` every three digits in each integer of `text`. An integer is a run of digits
/// that does not follow a digit, a point, the decimal character, a letter or `^` (so exponents,
/// fractions and `1.5` are left alone), and that is not a radix integer (`1010b`, `12o`, `8h`).
/// A run of three digits or fewer is not grouped.
fn group_integer_digits(text: &str, sep: char, decimal: char) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + text.len() / 3);
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let run = &chars[start..i];
        let before = start.checked_sub(1).map(|p| chars[p]);
        let after = chars.get(i).copied();
        let follows_number = matches!(
            before,
            Some(c) if c.is_ascii_alphanumeric() || c == '.' || c == ',' || c == decimal || c == '^'
        );
        let radix_suffix = matches!(after, Some('b' | 'o' | 'h'));
        if run.len() > 3 && !follows_number && !radix_suffix {
            for (k, c) in run.iter().enumerate() {
                if k > 0 && (run.len() - k).is_multiple_of(3) {
                    out.push(sep);
                }
                out.push(*c);
            }
        } else {
            out.extend(run.iter());
        }
    }
    out
}

/// The text of a real result with `.` as the decimal point.
fn format_real_plain(x: f64, format: &ResultFormat) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let magnitude = x.abs();
    if x == 0.0 || magnitude < pow10(-i32::from(format.zero_threshold)) {
        return "0".to_string();
    }
    let body = match format.number {
        NumberFormat::Decimal => plain_or_radix(magnitude, format),
        NumberFormat::Scientific => scientific(magnitude, format),
        NumberFormat::Engineering => engineering(magnitude, format),
        NumberFormat::Percent => {
            let scaled = magnitude * 100.0;
            if scaled.is_finite() {
                format!("{}%", plain_digits(scaled, format))
            } else {
                "inf%".to_string()
            }
        }
        NumberFormat::General => {
            let power = magnitude.log10().floor() as i32;
            let threshold = i32::from(format.exponential_threshold);
            if power >= threshold || power <= -threshold {
                scientific(magnitude, format)
            } else {
                plain_or_radix(magnitude, format)
            }
        }
    };
    if x < 0.0 && !body.bytes().all(|b| b == b'0' || b == b'.') {
        format!("-{body}")
    } else {
        body
    }
}

/// The text of a complex result `re + im·i` (or `j`) in `format`.
///
/// A part below the zero threshold is dropped, and so is a part more than `10^complex_threshold`
/// times smaller than the other (help: `1+0.002i` is `1` at threshold 2 and `1+0.002i` at 3).
/// A pure imaginary part shows as `1.732i`, and a unit imaginary part as `i`.
#[must_use]
pub fn format_complex(re: f64, im: f64, format: &ResultFormat) -> String {
    format_complex_with(re, im, format, &NumberSeparators::INVARIANT)
}

/// The text of a complex result written with the separators `seps` (see [`format_complex`]).
#[must_use]
pub fn format_complex_with(
    re: f64,
    im: f64,
    format: &ResultFormat,
    seps: &NumberSeparators,
) -> String {
    localize(
        format_complex_plain(re, im, format),
        seps,
        format.digit_grouping,
    )
}

/// The text of a complex result with `.` as the decimal point.
fn format_complex_plain(re: f64, im: f64, format: &ResultFormat) -> String {
    if re.is_nan() || im.is_nan() {
        return "NaN".to_string();
    }
    if let ComplexForm::Polar(angle) = format.complex_form {
        return format_polar(re, im, format, angle);
    }
    let unit = if format.imaginary_j { "j" } else { "i" };
    let re_text = format_real_plain(re, format);
    let im_magnitude_text = format_real_plain(im.abs(), format);
    let im_negative = im < 0.0 && im_magnitude_text != "0";
    let (ar, ai) = (re.abs(), im.abs());
    let threshold = pow10(i32::from(format.complex_threshold));

    if im_magnitude_text == "0" {
        return re_text;
    }
    let imaginary_only = |negative: bool| {
        let sign = if negative { "-" } else { "" };
        if im_magnitude_text == "1" {
            format!("{sign}{unit}")
        } else {
            format!("{sign}{im_magnitude_text}{unit}")
        }
    };
    if re_text == "0" || ai > threshold * ar {
        return imaginary_only(im_negative);
    }
    if ar > threshold * ai {
        return re_text;
    }
    let sign = if im_negative { "-" } else { "+" };
    format!("{re_text} {sign} {}", imaginary_only(false))
}

/// A complex result in polar form, `magnitude∠angle` (help: `1.141∠45°`). The magnitude and the
/// angle use the display precision and the zero threshold like a real result, and the complex
/// threshold does not apply. The radian form is written without a unit symbol, which the samples
/// do not show: unverified against Prime.
fn format_polar(re: f64, im: f64, format: &ResultFormat, angle: AngleUnit) -> String {
    let magnitude = format_real_plain(re.hypot(im), format);
    if magnitude == "0" {
        return magnitude;
    }
    let radians = im.atan2(re);
    let (value, suffix) = match angle {
        AngleUnit::Radians => (radians, ""),
        AngleUnit::Degrees => (radians.to_degrees(), "°"),
    };
    format!("{magnitude}∠{}{suffix}", format_real_plain(value, format))
}

fn pow10(exponent: i32) -> f64 {
    10f64.powi(exponent)
}

/// The decimal form, or its radix form for an integer in a radix other than decimal.
fn plain_or_radix(magnitude: f64, format: &ResultFormat) -> String {
    radix_digits(magnitude, format.radix).unwrap_or_else(|| plain_digits(magnitude, format))
}

/// The integer in its radix, when `magnitude` is an integer that fits in 128 bits.
fn radix_digits(magnitude: f64, radix: Radix) -> Option<String> {
    if radix == Radix::Decimal || magnitude.fract() != 0.0 || magnitude >= 2f64.powi(128) {
        return None;
    }
    let n = magnitude as u128;
    Some(match radix {
        Radix::Decimal => return None,
        Radix::Binary => format!("{n:b}b"),
        Radix::Octal => format!("{n:o}o"),
        Radix::Hexadecimal => format!("{n:x}h"),
    })
}

/// The decimal form of a non-negative magnitude with the display precision.
///
/// Prime shows at most 15 significant digits. The digits beyond them are zeros, and the decimals
/// are padded with zeros up to the display precision (help images: `12345.6789` at precision 15
/// is `12345.678900000000000`, `1234567890.1234567890` is `1234567890.123460000000000`).
fn plain_digits(magnitude: f64, format: &ResultFormat) -> String {
    let decimals = usize::from(format.precision.min(MAX_PRECISION));
    let text = fixed_digits(magnitude, decimals);
    if format.trailing_zeros {
        text
    } else {
        trim_zeros(&text)
    }
}

/// `magnitude` with `decimals` places, from its 15 significant digits. When the 15 digits have
/// more places than `decimals`, the value is rounded directly.
fn fixed_digits(magnitude: f64, decimals: usize) -> String {
    let sci = format!("{magnitude:.prec$e}", prec = ACCURATE_DIGITS - 1);
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exponent: i64 = exponent.parse().unwrap_or(0);
    let digits: Vec<char> = mantissa.chars().filter(char::is_ascii_digit).collect();
    // Digits left of the point: `point` of the 15 digits (zeros past them), or none below zero.
    let point = exponent + 1;
    let frac_len = if point < 0 {
        ACCURATE_DIGITS as i64 - point
    } else {
        (ACCURATE_DIGITS as i64 - point).max(0)
    } as usize;
    if frac_len > decimals {
        return format!("{magnitude:.decimals$}");
    }
    let digit = |i: i64| -> char {
        usize::try_from(i)
            .ok()
            .and_then(|i| digits.get(i).copied())
            .unwrap_or('0')
    };
    let mut out = String::new();
    if point <= 0 {
        out.push('0');
    } else {
        for i in 0..point {
            out.push(digit(i));
        }
    }
    if decimals > 0 {
        out.push('.');
        let mut frac = String::new();
        if point < 0 {
            frac.extend(std::iter::repeat_n('0', (-point) as usize));
            frac.extend(digits.iter());
        } else {
            for i in point..ACCURATE_DIGITS as i64 {
                frac.push(digit(i));
            }
        }
        out.push_str(&frac);
        while out.len() < out.find('.').map_or(0, |p| p + 1) + decimals {
            out.push('0');
        }
    }
    out
}

/// Removes trailing zeros after a decimal point, and the point when nothing is left.
fn trim_zeros(text: &str) -> String {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text.to_string()
    }
}

/// The mantissa (with `decimals` digits after its point) and the exponent of a magnitude.
fn scientific_parts(magnitude: f64, decimals: usize) -> (String, i32) {
    let text = format!("{magnitude:.decimals$e}");
    match text.split_once('e') {
        Some((mantissa, exponent)) => (mantissa.to_string(), exponent.parse().unwrap_or(0)),
        None => (text, 0),
    }
}

fn scientific(magnitude: f64, format: &ResultFormat) -> String {
    let (mantissa, exponent) = scientific_parts(magnitude, usize::from(format.precision));
    let mantissa = if format.trailing_zeros {
        mantissa
    } else {
        trim_zeros(&mantissa)
    };
    format!("{mantissa}×10^{exponent}")
}

fn engineering(magnitude: f64, format: &ResultFormat) -> String {
    let precision = usize::from(format.precision);
    let mut exponent = scientific_parts(magnitude, precision).1;
    // A round-up to the next power of ten can move the exponent: settle it in at most three steps.
    for _ in 0..3 {
        let engineering_exponent = exponent.div_euclid(3) * 3;
        let shift = (exponent - engineering_exponent) as usize;
        let (mantissa, rounded) = scientific_parts(magnitude, precision + shift);
        if rounded != exponent {
            exponent = rounded;
            continue;
        }
        let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
        let int_len = 1 + shift;
        let mut digits = digits;
        while digits.len() < int_len {
            digits.push('0');
        }
        let (int_part, frac) = digits.split_at(int_len);
        let mut shown = int_part.to_string();
        if !frac.is_empty() {
            shown.push('.');
            shown.push_str(frac);
        }
        let shown = if format.trailing_zeros {
            shown
        } else {
            trim_zeros(&shown)
        };
        return format!("{shown}×10^{engineering_exponent}");
    }
    scientific(magnitude, format)
}
