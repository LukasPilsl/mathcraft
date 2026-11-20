//! Text of computed outcomes and stored results, as the worksheet shows them after `=`. Numbers
//! follow the result format of the worksheet (or of the region), see `mathcraft_core::eval`, and
//! the separators of the display language (`NumberSeparators`). A result without a unit of its own
//! is shown in the worksheet's unit system (`UnitSystem`).

use mathcraft_core::eval::{
    format_complex_with, format_real_with, Matrix, MatrixDisplay, NumberSeparators, Outcome,
    ResultFormat, Value,
};
use mathcraft_core::units::{self, display_unit, CompoundUnit, Dimension, UnitSystem};
use mathcraft_import::results::{ResultValue, UnitRef};

use crate::i18n::{tr, trf};

/// What the text of a result depends on besides its value: the separators of the display language,
/// the worksheet's unit system, and its array origin (the first index a matrix shows).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextContext {
    /// Decimal point and digit group of the display language.
    pub seps: NumberSeparators,
    /// The worksheet's unit system.
    pub units: UnitSystem,
    /// `ORIGIN`: the index of the first row and column.
    pub origin: u32,
}

/// What a computed outcome shows after `=`.
#[derive(Debug, Clone, PartialEq)]
pub enum Shown {
    /// A value, as text.
    Value(String),
    /// The engine failed, with its message.
    Error(String),
    /// Nothing to show (a plain definition).
    Nothing,
}

/// The text of a computed outcome. A definition shows its display when it has one.
pub fn shown_outcome(outcome: &Outcome, format: &ResultFormat, ctx: &TextContext) -> Shown {
    match outcome {
        Outcome::Value(v) => Shown::Value(value_text(v, format, ctx)),
        Outcome::Converted { unit, shown, .. } => match shown {
            Value::Scalar(s) => Shown::Value(with_unit(
                &format_complex_with(s.value.re, s.value.im, format, &ctx.seps),
                &unit_text(unit),
            )),
            other => Shown::Value(value_text(other, format, ctx)),
        },
        Outcome::Defined {
            display: Some(inner),
            ..
        } => shown_outcome(inner, format, ctx),
        Outcome::Defined { display: None, .. } => Shown::Nothing,
        Outcome::Error(e) => Shown::Error(crate::messages::localize_error(&e.to_string())),
        Outcome::Accepted => Shown::Nothing,
    }
}

fn value_text(v: &Value, format: &ResultFormat, ctx: &TextContext) -> String {
    match v {
        Value::Scalar(s) => {
            let (re, im, unit) = scalar_in_system(s.value.re, s.value.im, &s.dim, ctx.units);
            with_unit(&format_complex_with(re, im, format, &ctx.seps), &unit)
        }
        Value::Matrix(m) => {
            let (r, c) = m.shape();
            shape_text("val.matrix_shape", r, c)
        }
        Value::Str(s) => format!("\"{s}\""),
        Value::List(items) => list_text(items, format, ctx),
        Value::Symbolic(s) => s.shown.to_string(),
        // A function value has no number to show; `ƒ` marks that it is a function.
        Value::Func(_) => "ƒ".to_string(),
    }
}

/// A matrix result as the grid the worksheet shows: row-major cells as text, the grid's size, and
/// the unit text the shown cells share (empty when they are dimensionless or do not share one).
#[derive(Debug, Clone, PartialEq)]
pub struct MatrixText {
    /// Grid rows, including the index row and the ellipsis row when they show.
    pub rows: usize,
    /// Grid columns, including the index column and the ellipsis column when they show.
    pub cols: usize,
    /// The cells, row-major, `rows * cols` of them. Unreadable cells are empty.
    pub cells: Vec<String>,
    /// The unit text of the shown cells, in the worksheet's unit system.
    pub unit: String,
}

/// The grid of a matrix result under `display`: the window of visible rows and columns (see
/// [`MatrixDisplay::window`]), with the row and column indices from `ctx.origin` when they are
/// shown, and `⋮`, `⋯` and `⋱` where the matrix continues past the window.
pub fn matrix_text(
    m: &Matrix,
    display: &MatrixDisplay,
    format: &ResultFormat,
    ctx: &TextContext,
) -> MatrixText {
    let (rows, cols) = m.shape();
    let (shown_rows, shown_cols) = display.window(rows, cols);
    let header = usize::from(display.show_indices);
    let cut_rows = shown_rows.end < rows;
    let cut_cols = shown_cols.end < cols;
    let grid_rows = header + shown_rows.len() + usize::from(cut_rows);
    let grid_cols = header + shown_cols.len() + usize::from(cut_cols);
    let mut cells = vec![String::new(); grid_rows * grid_cols];

    // The shown scalars, row-major, and whether they share one unit.
    let mut shown = Vec::with_capacity(shown_rows.len() * shown_cols.len());
    for r in shown_rows.clone() {
        for c in shown_cols.clone() {
            shown.push(m.get(r, c).ok().map(|s| (s.value.re, s.value.im, s.dim)));
        }
    }
    let first = shown.first().copied().flatten().map(|(_, _, d)| d);
    let shared = first.filter(|d| !d.is_dimensionless());
    let uniform = shown
        .iter()
        .all(|cell| cell.map(|(_, _, d)| Some(d)) == Some(first));
    let unit = match shared {
        Some(dim) if uniform => unit_in_system(&dim, ctx.units),
        _ => String::new(),
    };

    let mut shown_cells = shown.into_iter();
    for r in shown_rows.clone() {
        for c in shown_cols.clone() {
            let text = match shown_cells.next().flatten() {
                Some((re, im, dim)) => {
                    let (re, im, _) = scalar_in_system(re, im, &dim, ctx.units);
                    pow_text(&format_complex_with(re, im, format, &ctx.seps))
                }
                None => String::new(),
            };
            let (gr, gc) = (
                header + (r - shown_rows.start),
                header + (c - shown_cols.start),
            );
            cells[gr * grid_cols + gc] = text;
        }
    }
    if display.show_indices {
        let origin = ctx.origin as usize;
        for c in shown_cols.clone() {
            cells[header + (c - shown_cols.start)] = (origin + c).to_string();
        }
        for r in shown_rows.clone() {
            cells[(header + (r - shown_rows.start)) * grid_cols] = (origin + r).to_string();
        }
    }
    let data_cols = grid_cols - usize::from(cut_cols);
    let data_rows = grid_rows - usize::from(cut_rows);
    if cut_rows {
        for gc in header..data_cols {
            cells[(grid_rows - 1) * grid_cols + gc] = "⋮".to_string();
        }
    }
    if cut_cols {
        for gr in header..data_rows {
            cells[gr * grid_cols + grid_cols - 1] = "⋯".to_string();
        }
    }
    if cut_rows && cut_cols {
        cells[grid_rows * grid_cols - 1] = "⋱".to_string();
    }
    MatrixText {
        rows: grid_rows,
        cols: grid_cols,
        cells,
        unit,
    }
}

/// A matrix's shape under the key `key` (`matrix {}×{}`).
fn shape_text(key: &'static str, rows: usize, cols: usize) -> String {
    trf(key, &[&rows.to_string(), &cols.to_string()])
}

/// A nested array as a bracketed summary of its elements, for example `[3 matrices: 4x4, 4x4,
/// 4x4]`. The noun follows the elements (matrices, or items when they are mixed), so a list is
/// never shown as a matrix.
fn list_text(items: &[Value], format: &ResultFormat, ctx: &TextContext) -> String {
    if items.is_empty() {
        return tr("val.list_empty").to_string();
    }
    let all_matrices = items.iter().all(|v| matches!(v, Value::Matrix(_)));
    let noun = match (all_matrices, items.len()) {
        (true, 1) => tr("val.matrix"),
        (true, _) => tr("val.matrices"),
        (false, 1) => tr("val.item"),
        (false, _) => tr("val.items"),
    };
    let parts: Vec<String> = items.iter().map(|v| element_text(v, format, ctx)).collect();
    trf(
        "val.list",
        &[&items.len().to_string(), noun, &parts.join(", ")],
    )
}

/// One element of a nested array: a matrix as its shape (`4x4`), a nested array summarized again,
/// anything else as its value.
fn element_text(v: &Value, format: &ResultFormat, ctx: &TextContext) -> String {
    match v {
        Value::Matrix(m) => {
            let (r, c) = m.shape();
            format!("{r}x{c}")
        }
        Value::List(items) => list_text(items, format, ctx),
        other => value_text(other, format, ctx),
    }
}

fn with_unit(number: &str, unit: &str) -> String {
    if unit.is_empty() {
        pow_text(number)
    } else {
        format!("{} {unit}", pow_text(number))
    }
}

/// Coherent SI derived units with their own names. A dimension that matches one of them is shown
/// with its name (`m·kg/s²` as `N`). Display only: the value is the same, since these units are
/// coherent (factor 1 to SI).
const NAMED_DERIVED: [&str; 12] = ["N", "Pa", "J", "W", "C", "V", "Ω", "F", "S", "T", "Wb", "H"];

/// The unit text of a dimension-only value: a named derived unit when one matches, else the base
/// symbols (`m·kg/s²`).
pub fn dimension_text(dim: &Dimension) -> String {
    let named = units::units().iter().find(|u| {
        u.factor_to_si == 1.0
            && u.offset_to_si.is_none()
            && NAMED_DERIVED.contains(&u.symbol)
            && u.dimension == *dim
    });
    match named {
        Some(unit) => unit.symbol.to_string(),
        None => pow_text(&dim.to_string().replace('*', "·")),
    }
}

/// The unit text of a dimension in the worksheet's unit system: the system's preferred unit for it
/// (feet for a length in USCS), else the SI text of [`dimension_text`].
fn unit_in_system(dim: &Dimension, system: UnitSystem) -> String {
    match display_unit(dim, system) {
        Some(unit) => unit.symbol.to_string(),
        None => dimension_text(dim),
    }
}

/// A scalar's shown number and unit text in the worksheet's unit system: divided by the preferred
/// unit's factor when the system has one for its dimension, else the SI number. A dimensionless
/// scalar has no unit.
fn scalar_in_system(re: f64, im: f64, dim: &Dimension, system: UnitSystem) -> (f64, f64, String) {
    if dim.is_dimensionless() {
        return (re, im, String::new());
    }
    match display_unit(dim, system) {
        Some(unit) => (
            re / unit.factor_to_si,
            im / unit.factor_to_si,
            unit.symbol.to_string(),
        ),
        None => (re, im, dimension_text(dim)),
    }
}

/// Exponents written as superscript characters: `s^2` becomes `s²`, `1.5×10^-3` becomes
/// `1.5×10⁻³`. A caret without a number after it is left as it is.
pub fn pow_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '^' {
            out.push(c);
            continue;
        }
        let mut exp = String::new();
        if chars.peek() == Some(&'-') {
            exp.push('-');
            chars.next();
        }
        while let Some(&d) = chars.peek() {
            if !d.is_ascii_digit() {
                break;
            }
            exp.push(d);
            chars.next();
        }
        if exp.chars().any(|d| d.is_ascii_digit()) {
            out.extend(exp.chars().map(superscript_char));
        } else {
            out.push('^');
            out.push_str(&exp);
        }
    }
    out
}

/// The ASCII digit or sign of a superscript character (the inverse of `superscript_char`).
#[cfg(test)]
fn unsuperscript(c: char) -> char {
    match c {
        '⁰' => '0',
        '¹' => '1',
        '²' => '2',
        '³' => '3',
        '⁴' => '4',
        '⁵' => '5',
        '⁶' => '6',
        '⁷' => '7',
        '⁸' => '8',
        '⁹' => '9',
        '⁻' => '-',
        other => other,
    }
}

fn superscript_char(c: char) -> char {
    match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '-' => '⁻',
        other => other,
    }
}

/// Symbols of a compound unit with powers, for example `N·m`.
fn unit_text(unit: &CompoundUnit) -> String {
    pow_text(&unit.display_text())
}

/// The text of a stored result, as Prime shows it in its stored form. Values are SI, so a united
/// value shows its SI number with the unit symbols of its unit references.
pub fn stored_text(value: &ResultValue, format: &ResultFormat, ctx: &TextContext) -> String {
    match value {
        ResultValue::Empty => tr("res.na").to_string(),
        ResultValue::Real(x) => pow_text(&format_real_with(*x, format, &ctx.seps)),
        ResultValue::United { value, units } => with_unit(
            &format_real_with(*value, format, &ctx.seps),
            &stored_units_text(units),
        ),
        ResultValue::Matrix { rows, cols, .. } => shape_text("val.matrix_shape", *rows, *cols),
        ResultValue::Error { code, .. } => trf("val.error", &[&code.to_string()]),
        ResultValue::Trace2d(t) => trf("val.plot_points", &[&t.range_points.len().to_string()]),
        ResultValue::Symbolic(name) => trf("val.symbolic", &[name]),
        ResultValue::Unknown(name) => format!("<{name}>"),
    }
}

fn stored_units_text(units_ref: &[UnitRef]) -> String {
    let joined = units_ref
        .iter()
        .map(|u| {
            let symbol = units::unit(&u.unit).map_or(u.unit.as_str(), |d| d.symbol);
            let mut s = symbol.to_string();
            if u.num != 1 || u.den != 1 {
                s.push_str(&format!("^{}", u.num));
                if u.den != 1 {
                    s.push_str(&format!("/{}", u.den));
                }
            }
            s
        })
        .collect::<Vec<_>>()
        .join("·");
    pow_text(&joined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathcraft_core::eval::{
        AngleUnit, ComplexForm, Matrix, NumberFormat, NumberSeparators, Radix, Scalar,
    };
    use mathcraft_core::units::Dimension;

    fn square(n: usize) -> Value {
        let data = vec![Scalar::real(0.0, Dimension::dimensionless()); n * n];
        Value::Matrix(Matrix::new(n, n, data).expect("matrix"))
    }

    fn general() -> ResultFormat {
        ResultFormat::default()
    }

    /// The display of the English language on a worksheet in SI with origin 0.
    fn ctx() -> TextContext {
        TextContext {
            seps: NumberSeparators::INVARIANT,
            units: UnitSystem::Si,
            origin: 0,
        }
    }

    fn german() -> TextContext {
        TextContext {
            seps: NumberSeparators::GERMAN,
            ..ctx()
        }
    }

    fn value_text(v: &Value, format: &ResultFormat) -> String {
        super::value_text(v, format, &ctx())
    }

    fn real(x: f64) -> String {
        value_text(
            &Value::Scalar(Scalar::real(x, Dimension::dimensionless())),
            &general(),
        )
    }

    #[test]
    fn german_numbers_in_results_use_the_decimal_comma() {
        let d = general();
        let scalar = |x: f64| Value::Scalar(Scalar::real(x, Dimension::dimensionless()));
        assert_eq!(super::value_text(&scalar(0.0), &d, &german()), "0");
        assert_eq!(super::value_text(&scalar(-0.5), &d, &german()), "-0,5");
        assert_eq!(super::value_text(&scalar(1e-12), &d, &german()), "1×10⁻¹²");
        assert_eq!(
            super::value_text(&scalar(123456.789), &d, &german()),
            "1,235×10⁵"
        );
        let list = Value::List(vec![scalar(0.5), scalar(2.0)]);
        assert_eq!(super::value_text(&list, &d, &german()), "[2 items: 0,5, 2]");
        assert_eq!(super::value_text(&list, &d, &ctx()), "[2 items: 0.5, 2]");
    }

    #[test]
    fn stored_results_follow_the_separators_too() {
        let d = general();
        assert_eq!(stored_text(&ResultValue::Real(0.25), &d, &german()), "0,25");
        assert_eq!(stored_text(&ResultValue::Real(0.25), &d, &ctx()), "0.25");
    }

    #[test]
    fn a_matrix_is_shown_by_its_shape() {
        assert_eq!(value_text(&square(4), &general()), "matrix 4×4");
    }

    #[test]
    fn matrix_cells_are_formatted_row_major_with_their_shared_unit() {
        let metre = Dimension::base(0);
        let data = vec![
            Scalar::real(1.0, metre),
            Scalar::real(2000.0, metre),
            Scalar::real(-0.5, metre),
            Scalar::real(4.0, metre),
        ];
        let m = Matrix::new(2, 2, data).expect("matrix");
        let grid = matrix_text(&m, &MatrixDisplay::default(), &general(), &ctx());
        assert_eq!(grid.cells, vec!["1", "2×10³", "-0.5", "4"]);
        assert_eq!((grid.rows, grid.cols), (2, 2));
        assert_eq!(grid.unit, "m");
    }

    #[test]
    fn matrix_cells_have_no_unit_when_dimensionless_or_mixed() {
        let plain = Matrix::new(
            1,
            2,
            vec![
                Scalar::real(1.0, Dimension::dimensionless()),
                Scalar::real(2.0, Dimension::dimensionless()),
            ],
        )
        .expect("matrix");
        let display = MatrixDisplay::default();
        assert_eq!(matrix_text(&plain, &display, &general(), &ctx()).unit, "");
        let mixed = Matrix::new(
            1,
            2,
            vec![
                Scalar::real(1.0, Dimension::base(0)),
                Scalar::real(2.0, Dimension::base(1)),
            ],
        )
        .expect("matrix");
        assert_eq!(matrix_text(&mixed, &display, &general(), &ctx()).unit, "");
    }

    #[test]
    fn a_matrix_shows_its_window_with_indices_and_ellipses() {
        let data: Vec<Scalar> = (1..=9)
            .map(|k| Scalar::real(f64::from(k), Dimension::dimensionless()))
            .collect();
        let m = Matrix::new(3, 3, data).expect("matrix");
        let window = MatrixDisplay {
            rows: 2,
            cols: 2,
            show_indices: true,
            ..MatrixDisplay::default()
        };
        let grid = matrix_text(&m, &window, &general(), &ctx());
        assert_eq!((grid.rows, grid.cols), (4, 4));
        assert_eq!(
            grid.cells,
            vec![
                "", "0", "1", "", //
                "0", "1", "2", "⋯", //
                "1", "4", "5", "⋯", //
                "", "⋮", "⋮", "⋱",
            ]
        );
        // The indices start at ORIGIN.
        let from_one = TextContext { origin: 1, ..ctx() };
        assert_eq!(
            matrix_text(&m, &window, &general(), &from_one).cells[1],
            "1"
        );
        // The whole matrix fits the default window with no indices and no ellipses.
        let all = matrix_text(&m, &MatrixDisplay::default(), &general(), &ctx());
        assert_eq!((all.rows, all.cols, all.cells.len()), (3, 3, 9));
        assert!(!all.cells.iter().any(|c| c == "⋮"));
    }

    #[test]
    fn a_scalar_is_shown_in_the_unit_system_of_the_worksheet() {
        let metre = Dimension::base(0);
        let us = TextContext {
            units: UnitSystem::Us,
            ..ctx()
        };
        let cgs = TextContext {
            units: UnitSystem::Cgs,
            ..ctx()
        };
        let length = Value::Scalar(Scalar::real(0.3048, metre));
        assert_eq!(value_text_in(&length, &us), "1 ft");
        assert_eq!(value_text_in(&length, &cgs), "30.48 cm");
        assert_eq!(value_text_in(&length, &ctx()), "0.305 m");
        let force = Value::Scalar(Scalar::real(1.0, units::unit("N").unwrap().dimension));
        assert_eq!(value_text_in(&force, &cgs), "1×10⁵ dyn");
        // A dimensionless value has no unit in any system.
        let plain = Value::Scalar(Scalar::real(2.0, Dimension::dimensionless()));
        assert_eq!(value_text_in(&plain, &us), "2");
    }

    fn value_text_in(v: &Value, ctx: &TextContext) -> String {
        super::value_text(v, &general(), ctx)
    }

    #[test]
    fn a_list_of_matrices_is_a_bracketed_list_of_shapes() {
        let list = Value::List(vec![square(4), square(4), square(4)]);
        assert_eq!(value_text(&list, &general()), "[3 matrices: 4x4, 4x4, 4x4]");
        assert_eq!(
            shown_outcome(&Outcome::Value(list), &general(), &ctx()),
            Shown::Value("[3 matrices: 4x4, 4x4, 4x4]".to_string())
        );
        assert_eq!(
            value_text(&Value::List(vec![square(2)]), &general()),
            "[1 matrix: 2x2]"
        );
    }

    #[test]
    fn a_nested_list_is_summarized_again_and_never_says_matrix_for_a_list() {
        let nested = Value::List(vec![square(2), Value::List(vec![square(3)])]);
        assert_eq!(
            value_text(&nested, &general()),
            "[2 items: 2x2, [1 matrix: 3x3]]"
        );
        assert_eq!(value_text(&Value::List(vec![]), &general()), "[0 items]");
    }

    #[test]
    fn real_results_follow_the_prime_default_format() {
        // Prime default: General, precision 3, no trailing zeros, exponential threshold 3.
        assert_eq!(real(0.0), "0");
        assert_eq!(real(2.0), "2");
        assert_eq!(real(-3.25), "-3.25");
        assert_eq!(real(12461.7556), "1.246×10⁴");
        assert_eq!(real(999.9996), "1000");
        assert_eq!(real(36.8698976458), "36.87");
        assert_eq!(real(0.001), "1×10⁻³");
        // Three decimal places: 0.0123 shows as 0.012 (help: display precision 3).
        assert_eq!(real(0.0123), "0.012");
        assert_eq!(real(f64::NAN), "NaN");
    }

    #[test]
    fn the_worksheet_format_changes_the_shown_text() {
        let decimal = ResultFormat {
            number: NumberFormat::Decimal,
            precision: 3,
            ..general()
        };
        assert_eq!(
            value_text(
                &Value::Scalar(Scalar::real(12461.7556, Dimension::dimensionless())),
                &decimal
            ),
            "12461.756"
        );
        let hex = ResultFormat {
            radix: Radix::Hexadecimal,
            ..general()
        };
        assert_eq!(
            value_text(
                &Value::Scalar(Scalar::real(255.0, Dimension::dimensionless())),
                &hex
            ),
            "ffh"
        );
        let padded = ResultFormat {
            precision: 15,
            trailing_zeros: true,
            number: NumberFormat::Decimal,
            ..general()
        };
        assert_eq!(
            value_text(
                &Value::Scalar(Scalar::real(12345.6789, Dimension::dimensionless())),
                &padded
            ),
            "12345.678900000000000"
        );
    }

    #[test]
    fn the_display_options_reach_the_shown_text() {
        let scalar = |x: f64| Value::Scalar(Scalar::real(x, Dimension::dimensionless()));
        let grouped = ResultFormat {
            number: NumberFormat::Decimal,
            precision: 2,
            digit_grouping: true,
            ..general()
        };
        assert_eq!(value_text(&scalar(1234567.5), &grouped), "1,234,567.5");
        assert_eq!(
            super::value_text(&scalar(1234567.5), &grouped, &german()),
            "1.234.567,5"
        );
        let polar = ResultFormat {
            complex_form: ComplexForm::Polar(AngleUnit::Degrees),
            ..general()
        };
        // A real negative number is a complex number with a 180 degree angle.
        let negative = Value::Scalar(Scalar::real(-2.0, Dimension::dimensionless()));
        assert_eq!(value_text(&negative, &polar), "2∠180°");
    }

    /// Golden-style check on the 24 samples. Prime's stored results are numbers only (no display
    /// strings), so the check is that each stored number, shown in the format its region has,
    /// reads back within the display rounding: half a unit of the last shown decimal, relative
    /// in exponential notation and absolute in plain notation.
    #[test]
    fn stored_sample_results_read_back_within_their_display_rounding() {
        require_samples!();
        use crate::testdata::examples_dir;
        use mathcraft_import::evaluate::load_results;
        use mathcraft_import::package::Package;
        use mathcraft_import::regions::Worksheet;

        let mut checked = 0usize;
        let mut paths: Vec<_> = std::fs::read_dir(examples_dir())
            .expect("documentation/examples exists")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "mcdx"))
            .collect();
        paths.sort();
        for path in paths {
            let pkg = Package::open(&path).expect("sample opens");
            let ws = Worksheet::from_package(&pkg).expect("sample models");
            let table = load_results(&pkg).expect("sample results load");
            let global = ws.settings.result_format.clone().unwrap_or_default();
            let formats: std::collections::HashMap<u32, ResultFormat> = ws
                .all_regions()
                .into_iter()
                .filter_map(|r| r.result_format.clone().map(|f| (r.id, f)))
                .collect();
            for (id, data) in ws.attach_stored_results(&table) {
                // A united result shows its number, then its unit symbols.
                let x = match &data.value {
                    ResultValue::Real(x) => *x,
                    ResultValue::United { value, .. } => *value,
                    _ => continue,
                };
                let format = formats.get(&id).unwrap_or(&global);
                let shown_full = stored_text(&ResultValue::Real(x), format, &ctx());
                // Exponents are superscript characters on screen; read them back as ASCII.
                let shown = shown_full.as_str();
                let ascii: String = shown.chars().map(unsuperscript).collect();
                let parsed: f64 = ascii
                    .replace("×10", "e")
                    .parse()
                    .unwrap_or_else(|_| panic!("{}: `{shown}` is not a number", path.display()));
                let half_unit = 0.5 * 10f64.powi(-i32::from(format.precision));
                let tolerance = if shown.contains("×10") {
                    half_unit * x.abs()
                } else {
                    half_unit
                } + 1e-12 * x.abs().max(1.0);
                // Below the zero threshold a number shows as 0.
                let tolerance = tolerance.max(10f64.powi(-i32::from(format.zero_threshold)));
                assert!(
                    (parsed - x).abs() <= tolerance,
                    "{}: region {id} stored {x}, shown `{shown}`",
                    path.display()
                );
                checked += 1;
            }
        }
        // 67 real and united numbers in the samples at the time of writing; matrices, complex and
        // other stored kinds are not checked here.
        assert!(checked >= 60, "only {checked} stored numbers checked");
    }

    #[test]
    fn stored_results_use_the_same_format() {
        assert_eq!(
            stored_text(&ResultValue::Real(50_000.0), &general(), &ctx()),
            "5×10⁴"
        );
        assert_eq!(stored_text(&ResultValue::Empty, &general(), &ctx()), "n/a");
    }
}

#[cfg(test)]
mod display_units_tests {
    use super::*;
    use mathcraft_core::units::{self, CompoundUnit};

    #[test]
    fn exponents_become_superscript_characters() {
        assert_eq!(pow_text("1.015×10^3 m*kg/s^2"), "1.015×10³ m*kg/s²");
        assert_eq!(pow_text("1×10^-12"), "1×10⁻¹²");
        assert_eq!(pow_text("a^b"), "a^b");
        assert_eq!(pow_text("x^"), "x^");
    }

    #[test]
    fn superscript_characters_have_glyphs_in_the_roman_face() {
        for c in "⁰¹²³⁴⁵⁶⁷⁸⁹⁻".chars() {
            assert!(
                crate::fonts::covers(crate::fonts::Face::Roman, c),
                "no glyph for {c}"
            );
        }
    }

    #[test]
    fn a_force_dimension_shows_newton() {
        let kg_m_s2 = CompoundUnit::parse("kg*m/s^2").expect("parses").dimension();
        assert_eq!(dimension_text(&kg_m_s2), "N");
        assert_eq!(dimension_text(&units::unit("J").unwrap().dimension), "J");
    }

    #[test]
    fn a_dimension_without_a_named_unit_keeps_its_base_symbols() {
        assert_eq!(dimension_text(&Dimension::base(0)), "m");
        let accel = CompoundUnit::parse("m/s^2").expect("parses").dimension();
        assert_eq!(dimension_text(&accel), "m/s²");
    }

    #[test]
    fn a_named_unit_is_shown_only_for_the_coherent_one() {
        // kN has the same dimension as N but another factor, so the base name is kept for N only.
        let newton = units::unit("N").unwrap().dimension;
        assert_eq!(dimension_text(&newton), "N");
    }
}
