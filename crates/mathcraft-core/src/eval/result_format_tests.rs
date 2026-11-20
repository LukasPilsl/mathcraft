use super::*;

fn fmt(x: f64, number: NumberFormat, precision: u8) -> String {
    let format = ResultFormat {
        number,
        precision,
        ..ResultFormat::default()
    };
    format_real(x, &format)
}

fn general(x: f64) -> String {
    format_real(x, &ResultFormat::default())
}

fn decimal(x: f64, precision: u8) -> String {
    fmt(x, NumberFormat::Decimal, precision)
}

#[test]
fn default_format_is_prime_default() {
    let d = ResultFormat::default();
    assert_eq!(d.number, NumberFormat::General);
    assert_eq!(d.precision, 3);
    assert!(!d.trailing_zeros);
    assert_eq!(d.exponential_threshold, 3);
    assert_eq!(d.zero_threshold, 15);
    assert_eq!(d.complex_threshold, 10);
    assert_eq!(d.radix, Radix::Decimal);
    assert!(d.checked().is_ok());
}

#[test]
fn out_of_range_settings_are_refused() {
    let base = ResultFormat::default();
    let precision = ResultFormat {
        precision: 17,
        ..base.clone()
    };
    assert!(matches!(
        precision.checked(),
        Err(FormatError::OutOfRange {
            field: "precision",
            value: 17,
            max: 16
        })
    ));
    let exponential = ResultFormat {
        exponential_threshold: 16,
        ..base.clone()
    };
    assert!(exponential.checked().is_err());
    let zero = ResultFormat {
        zero_threshold: 308,
        ..base.clone()
    };
    assert!(zero.checked().is_err());
    let complex = ResultFormat {
        complex_threshold: 308,
        ..base
    };
    assert!(complex.checked().is_err());
}

#[test]
fn zero_and_signed_zero_show_as_zero() {
    assert_eq!(general(0.0), "0");
    assert_eq!(general(-0.0), "0");
    assert_eq!(decimal(-0.0004, 3), "0");
    assert_eq!(general(1e-16), "0");
    // Below the default zero threshold (10^-15), even 1e-300 is zero.
    assert_eq!(general(1e-300), "0");
}

#[test]
fn nan_and_infinities_keep_short_forms() {
    assert_eq!(general(f64::NAN), "NaN");
    assert_eq!(general(f64::INFINITY), "inf");
    assert_eq!(general(f64::NEG_INFINITY), "-inf");
    assert_eq!(fmt(f64::NEG_INFINITY, NumberFormat::Percent, 3), "-inf");
}

#[test]
fn tricky_magnitudes_in_general_format() {
    assert_eq!(general(1e-12), "1×10^-12");
    assert_eq!(general(123_456_789.0), "1.235×10^8");
    assert_eq!(general(0.000_123), "1.23×10^-4");
    assert_eq!(general(-12461.7556), "-1.246×10^4");
    assert_eq!(general(1e300), "1×10^300");
    assert_eq!(general(f64::MAX), "1.798×10^308");
    // With zero threshold 307 the tiny magnitudes are shown.
    let deep = ResultFormat {
        zero_threshold: 307,
        ..ResultFormat::default()
    };
    assert_eq!(format_real(1e-300, &deep), "1×10^-300");
    assert_eq!(format_real(-1.5e-300, &deep), "-1.5×10^-300");
    // 2.2e-308 is below 10^-307, the largest zero threshold, so it is zero.
    assert_eq!(format_real(-f64::MIN_POSITIVE, &deep), "0");
}

#[test]
fn general_uses_the_exponential_threshold() {
    // Threshold 3 (default): 999 is plain, 1000 is exponential, 0.001 is exponential.
    assert_eq!(general(999.0), "999");
    assert_eq!(general(1000.0), "1×10^3");
    assert_eq!(general(0.01), "0.01");
    assert_eq!(general(0.001), "1×10^-3");
}

#[test]
fn help_exponential_threshold_table() {
    // Help table, display precision 6 (the table's digits are 7 significant).
    let at = |x: f64, t: u8| {
        let format = ResultFormat {
            precision: 6,
            exponential_threshold: t,
            ..ResultFormat::default()
        };
        format_real(x, &format)
    };
    assert_eq!(at(1_123_581.0, 5), "1.123581×10^6");
    assert_eq!(at(1_123_581.0, 6), "1.123581×10^6");
    assert_eq!(at(1_123_581.0, 7), "1123581");
    assert_eq!(at(0.123_581_3, 2), "0.123581");
    assert_eq!(at(0.123_581_3, 1), "1.235813×10^-1");
    assert_eq!(at(0.123_581_3, 0), "1.235813×10^-1");
}

#[test]
fn help_general_and_decimal_examples() {
    // Help: 12461.7556 = 1.246×10^4 (General), = 12461.756 (Decimal, precision 3).
    assert_eq!(general(12461.7556), "1.246×10^4");
    assert_eq!(decimal(12461.7556, 3), "12461.756");
    // Help: 12345678901234567890 = 12345678901234600000 (Decimal, 15 accurate digits).
    assert_eq!(
        decimal(12_345_678_901_234_567_890.0, 3),
        "12345678901234600000"
    );
}

#[test]
fn help_scientific_engineering_examples() {
    // Help: 12461.7556 = 1.246 × 10^4 (Scientific).
    assert_eq!(fmt(12461.7556, NumberFormat::Scientific, 3), "1.246×10^4");
    // Help: 12461.7556 = 12.462 × 10^3 (Engineering).
    assert_eq!(fmt(12461.7556, NumberFormat::Engineering, 3), "12.462×10^3");
    // Help: 304.8 m shown as (3.05 · 10^2) m with two decimal places (Scientific).
    assert_eq!(fmt(304.8, NumberFormat::Scientific, 2), "3.05×10^2");
}

#[test]
fn help_percent_example_with_its_arithmetic() {
    // Help prints 12462175.56% for 12461.7556; the arithmetic gives 1246175.56%. The help figure
    // looks like a typo, so the arithmetic is pinned here.
    assert_eq!(fmt(12461.7556, NumberFormat::Percent, 2), "1246175.56%");
    assert_eq!(fmt(0.5, NumberFormat::Percent, 3), "50%");
    assert_eq!(fmt(-0.5, NumberFormat::Percent, 3), "-50%");
}

#[test]
fn trailing_zeros_follow_the_display_precision() {
    let shown = ResultFormat {
        precision: 3,
        trailing_zeros: true,
        ..ResultFormat::default()
    };
    let hidden = ResultFormat {
        precision: 3,
        ..ResultFormat::default()
    };
    // Help: with precision 3, 1.5 = 1.500 with trailing zeros, 1.5 without.
    assert_eq!(format_real(1.5, &shown), "1.500");
    assert_eq!(format_real(1.5, &hidden), "1.5");
    assert_eq!(
        format_real(
            1000.0,
            &ResultFormat {
                number: NumberFormat::Decimal,
                ..shown
            }
        ),
        "1000.000"
    );
    assert_eq!(format_real(1000.0, &hidden), "1×10^3");
    assert_eq!(decimal(2.0, 3), "2");
}

#[test]
fn decimal_places_are_not_capped_by_the_integer_digits() {
    // The help text says at most 16 - N decimals, but its rendered images show 15 decimals for a
    // 5-digit integer part (`12345.6789` at precision 15 is `12345.678900000000000`). The images
    // are followed: only the 15 significant digits are limited.
    let padded = |x: f64, p: u8| {
        format_real(
            x,
            &ResultFormat {
                number: NumberFormat::Decimal,
                precision: p,
                trailing_zeros: true,
                ..ResultFormat::default()
            },
        )
    };
    assert_eq!(padded(12345.6789, 15), "12345.678900000000000");
    assert_eq!(
        padded(1_234_567_890.123_456_7, 15),
        "1234567890.123460000000000"
    );
    assert_eq!(decimal(123_456_789_012_345.13, 3), "123456789012345");
    assert_eq!(decimal(1_234_567.123_456_789, 16), "1234567.12345679");
    assert_eq!(decimal(0.5, 16), "0.5");
}

#[test]
fn help_images_of_non_integer_decimal_results() {
    // Images example_display_of_non-int_decimal_results0-7 (precision 3, trailing zeros off).
    assert_eq!(decimal(12345.6789, 3), "12345.679");
    assert_eq!(decimal(1_234_567_890.123_456_7, 3), "1234567890.123");
    assert_eq!(decimal(123_456_789_012_345.13, 3), "123456789012345");
    let zeros = |x: f64| {
        format_real(
            x,
            &ResultFormat {
                number: NumberFormat::Decimal,
                precision: 3,
                trailing_zeros: true,
                ..ResultFormat::default()
            },
        )
    };
    assert_eq!(zeros(12345.6789), "12345.679");
    assert_eq!(zeros(123_456_789_012_345.13), "123456789012345.000");
}

#[test]
fn help_images_of_integer_decimal_results() {
    // Images example_display_of_int_decimal_results0-8.
    assert_eq!(decimal(1_234_567.0, 3), "1234567");
    assert_eq!(decimal(123_456_789_012_345.0, 3), "123456789012345");
    assert_eq!(
        decimal(12_345_678_901_234_567_890.0, 3),
        "12345678901234600000"
    );
    assert_eq!(
        decimal(12_345_678_901_234_567_890.0, 15),
        "12345678901234600000"
    );
    let zeros = |x: f64, p: u8| {
        format_real(
            x,
            &ResultFormat {
                number: NumberFormat::Decimal,
                precision: p,
                trailing_zeros: true,
                ..ResultFormat::default()
            },
        )
    };
    assert_eq!(zeros(1_234_567.0, 3), "1234567.000");
    assert_eq!(zeros(123_456_789_012_345.0, 3), "123456789012345.000");
    assert_eq!(zeros(1_234_567.0, 15), "1234567.000000000000000");
    assert_eq!(
        zeros(12_345_678_901_234_567_890.0, 3),
        "12345678901234600000.000"
    );
}

#[test]
fn screenshot_result_format_list_for_50000() {
    // Screenshot 0127 (Ergebnisformat list): 50000 is 5*10^4 (Allgemein), 50000 (Dezimal),
    // 5*10^4 (Wissenschaftlich), 50*10^3 (Ingenieurtechnisch), 5000000% (Prozent).
    assert_eq!(general(50_000.0), "5×10^4");
    assert_eq!(decimal(50_000.0, 3), "50000");
    assert_eq!(fmt(50_000.0, NumberFormat::Scientific, 3), "5×10^4");
    assert_eq!(fmt(50_000.0, NumberFormat::Engineering, 3), "50×10^3");
    assert_eq!(fmt(50_000.0, NumberFormat::Percent, 3), "5000000%");
}

#[test]
fn rounding_carry_moves_the_exponent() {
    assert_eq!(general(9.9996), "10");
    assert_eq!(fmt(999.9996, NumberFormat::Engineering, 3), "1×10^3");
    assert_eq!(fmt(9.9996, NumberFormat::Scientific, 3), "1×10^1");
}

#[test]
fn zero_threshold_help_example() {
    // Help: 0.000125 is 0 at zero thresholds 2 and 3, and 0.000125 at 4 and 5 (Decimal, 6 places).
    let at = |t: u16| {
        format_real(
            0.000_125,
            &ResultFormat {
                number: NumberFormat::Decimal,
                precision: 6,
                zero_threshold: t,
                ..ResultFormat::default()
            },
        )
    };
    assert_eq!(at(2), "0");
    assert_eq!(at(3), "0");
    assert_eq!(at(4), "0.000125");
    assert_eq!(at(5), "0.000125");
}

#[test]
fn radix_of_integers() {
    let radix = |r: Radix| {
        format_real(
            8.0,
            &ResultFormat {
                radix: r,
                ..ResultFormat::default()
            },
        )
    };
    assert_eq!(radix(Radix::Decimal), "8");
    assert_eq!(radix(Radix::Binary), "1000b");
    assert_eq!(radix(Radix::Octal), "10o");
    assert_eq!(radix(Radix::Hexadecimal), "8h");
    // A non-integer is always decimal.
    let hex = ResultFormat {
        radix: Radix::Hexadecimal,
        ..ResultFormat::default()
    };
    assert_eq!(format_real(0.5, &hex), "0.5");
    assert_eq!(format_real(-255.0, &hex), "-ffh");
}

#[test]
fn integers_beyond_2_pow_53_are_exact_in_radix_and_padded_in_decimal() {
    // 2^60 is exact as a double. Decimal shows 15 accurate digits, padded with zeros.
    let two_60 = 2f64.powi(60);
    assert_eq!(decimal(two_60, 3), "1152921504606850000");
    // Radix applies to the decimal form only, so the format is Decimal here.
    let hex = ResultFormat {
        number: NumberFormat::Decimal,
        radix: Radix::Hexadecimal,
        ..ResultFormat::default()
    };
    assert_eq!(format_real(two_60, &hex), "1000000000000000h");
}

#[test]
fn engineering_exponents_are_multiples_of_three() {
    assert_eq!(fmt(0.5, NumberFormat::Engineering, 3), "500×10^-3");
    assert_eq!(fmt(1500.0, NumberFormat::Engineering, 3), "1.5×10^3");
    assert_eq!(fmt(-2.5e-9, NumberFormat::Engineering, 3), "-2.5×10^-9");
}

#[test]
fn a_matrix_style_mixed_magnitude_list_formats_per_element() {
    // Each element has its own exponent: the column is not forced into one format.
    let column = [1e-12, 123_456_789.0, 0.000_123, 0.0, -2.0];
    let text: Vec<String> = column.iter().map(|x| general(*x)).collect();
    assert_eq!(text, ["1×10^-12", "1.235×10^8", "1.23×10^-4", "0", "-2"]);
}

#[test]
fn complex_cartesian_with_negative_imaginary_part() {
    let d = ResultFormat::default();
    assert_eq!(format_complex(1.0, -2.0, &d), "1 - 2i");
    assert_eq!(format_complex(1.0, 2.0, &d), "1 + 2i");
    let j = ResultFormat {
        imaginary_j: true,
        ..ResultFormat::default()
    };
    assert_eq!(format_complex(1.0, -2.0, &j), "1 - 2j");
}

#[test]
fn complex_pure_imaginary_and_pure_real() {
    let d = ResultFormat::default();
    // Help: √-3 = 1.732i.
    assert_eq!(format_complex(0.0, 3f64.sqrt(), &d), "1.732i");
    assert_eq!(format_complex(0.0, -3f64.sqrt(), &d), "-1.732i");
    assert_eq!(format_complex(0.0, 1.0, &d), "i");
    assert_eq!(format_complex(0.0, -1.0, &d), "-i");
    assert_eq!(format_complex(2.5, 0.0, &d), "2.5");
    assert_eq!(format_complex(0.0, 0.0, &d), "0");
    // A tiny imaginary part below the zero threshold is dropped.
    assert_eq!(format_complex(2.5, 1e-16, &d), "2.5");
}

#[test]
fn complex_threshold_help_example() {
    // Help: 1+0.002i is 1 at complex threshold 2, and 1+0.002i at 3 and 4 (Decimal, 3 places).
    let at = |t: u16| {
        format_complex(
            1.0,
            0.002,
            &ResultFormat {
                number: NumberFormat::Decimal,
                complex_threshold: t,
                ..ResultFormat::default()
            },
        )
    };
    assert_eq!(at(2), "1");
    assert_eq!(at(3), "1 + 0.002i");
    assert_eq!(at(4), "1 + 0.002i");
}

#[test]
fn complex_with_nan_part_is_nan_and_never_panics() {
    let d = ResultFormat::default();
    assert_eq!(format_complex(f64::NAN, 1.0, &d), "NaN");
    assert!(!format_complex(1.0, f64::INFINITY, &d).is_empty());
}

#[test]
fn german_separators_on_tricky_reals() {
    let d = ResultFormat::default();
    let de = &NumberSeparators::GERMAN;
    assert_eq!(format_real_with(0.0, &d, de), "0");
    assert_eq!(format_real_with(-0.5, &d, de), "-0,5");
    assert_eq!(format_real_with(1e-12, &d, de), "1×10^-12");
    assert_eq!(format_real_with(-2.5e-7, &d, de), "-2,5×10^-7");
    assert_eq!(format_real_with(123456.789, &d, de), "1,235×10^5");
    let decimal = ResultFormat {
        number: NumberFormat::Decimal,
        precision: 3,
        ..ResultFormat::default()
    };
    assert_eq!(format_real_with(123456.789, &decimal, de), "123456,789");
    assert_eq!(format_real_with(-0.25, &decimal, de), "-0,25");
    assert_eq!(format_real_with(50000.0, &decimal, de), "50000");
    assert_eq!(format_real_with(f64::NAN, &d, de), "NaN");
    assert_eq!(format_real_with(f64::NEG_INFINITY, &d, de), "-inf");
}

#[test]
fn invariant_separators_are_the_default_output() {
    let d = ResultFormat::default();
    for x in [0.0, -0.5, 1e-12, 123456.789, 3.25, -7.0] {
        assert_eq!(
            format_real_with(x, &d, &NumberSeparators::INVARIANT),
            format_real(x, &d)
        );
    }
    assert_eq!(
        format_complex_with(1.0, -2.5, &d, &NumberSeparators::INVARIANT),
        format_complex(1.0, -2.5, &d)
    );
}

#[test]
fn german_separators_on_complex_and_radix() {
    let d = ResultFormat::default();
    let de = &NumberSeparators::GERMAN;
    assert_eq!(format_complex_with(1.5, -0.5, &d, de), "1,5 - 0,5i");
    assert_eq!(format_complex_with(0.0, 3f64.sqrt(), &d, de), "1,732i");
    assert_eq!(format_complex_with(4.0, 1.0, &d, de), "4 + i");
    let hex = ResultFormat {
        radix: Radix::Hexadecimal,
        ..ResultFormat::default()
    };
    assert_eq!(format_real_with(255.0, &hex, de), "ffh");
}

#[test]
fn thousands_separator_groups_the_integer_digits_only() {
    let decimal = ResultFormat {
        number: NumberFormat::Decimal,
        precision: 2,
        ..ResultFormat::default()
    };
    let grouped = NumberSeparators {
        decimal: ',',
        thousands: Some('.'),
    };
    assert_eq!(
        format_real_with(1234567.5, &decimal, &grouped),
        "1.234.567,5"
    );
    assert_eq!(format_real_with(-1234.0, &decimal, &grouped), "-1.234");
    assert_eq!(format_real_with(999.0, &decimal, &grouped), "999");
    assert_eq!(format_real_with(0.25, &decimal, &grouped), "0,25");
}

#[test]
fn digit_grouping_uses_the_locale_group_character() {
    let decimal = ResultFormat {
        number: NumberFormat::Decimal,
        precision: 2,
        digit_grouping: true,
        ..ResultFormat::default()
    };
    let de = &NumberSeparators::GERMAN;
    assert_eq!(
        format_real_with(1234567.5, &decimal, &NumberSeparators::INVARIANT),
        "1,234,567.5"
    );
    assert_eq!(format_real_with(1234567.5, &decimal, de), "1.234.567,5");
    assert_eq!(format_real_with(-1234.0, &decimal, de), "-1.234");
    assert_eq!(format_real_with(999.0, &decimal, de), "999");
    // Fractions, exponents and radix integers keep their digits.
    assert_eq!(format_real_with(0.123456, &decimal, de), "0,12");
    let binary = ResultFormat {
        radix: Radix::Binary,
        digit_grouping: true,
        ..decimal.clone()
    };
    assert_eq!(format_real_with(1000.0, &binary, de), "1111101000b");
    let scientific = ResultFormat {
        number: NumberFormat::Scientific,
        digit_grouping: true,
        ..decimal.clone()
    };
    assert_eq!(format_real_with(12461.7556, &scientific, de), "1,25×10^4");
    let ungrouped = ResultFormat {
        digit_grouping: false,
        ..decimal.clone()
    };
    assert_eq!(
        format_complex_with(1234.0, 5000.0, &ungrouped, &NumberSeparators::INVARIANT),
        "1234 + 5000i"
    );
    assert_eq!(
        format_complex_with(1234.0, 5000.0, &decimal, &NumberSeparators::INVARIANT),
        "1,234 + 5,000i"
    );
}

#[test]
fn polar_form_writes_magnitude_and_angle() {
    let radians = ResultFormat {
        complex_form: ComplexForm::Polar(AngleUnit::Radians),
        ..ResultFormat::default()
    };
    let degrees = ResultFormat {
        complex_form: ComplexForm::Polar(AngleUnit::Degrees),
        ..ResultFormat::default()
    };
    assert_eq!(format_complex(1.0, 1.0, &radians), "1.414∠0.785");
    assert_eq!(format_complex(1.0, 1.0, &degrees), "1.414∠45°");
    assert_eq!(format_complex(-2.0, 0.0, &degrees), "2∠180°");
    assert_eq!(format_complex(0.0, 0.0, &degrees), "0");
    assert_eq!(
        format_complex_with(1.0, 1.0, &degrees, &NumberSeparators::GERMAN),
        "1,414∠45°"
    );
}

#[test]
fn matrix_window_is_the_visible_size_from_the_offset() {
    let default = MatrixDisplay::default();
    assert_eq!(default.window(5, 20), (0..5, 0..12));
    let small = MatrixDisplay {
        rows: 4,
        cols: 4,
        offset_row: 2,
        offset_col: 3,
        ..MatrixDisplay::default()
    };
    assert_eq!(small.window(10, 10), (2..6, 3..7));
    // The window is cut at the matrix's last row.
    assert_eq!(small.window(3, 10), (2..3, 3..7));
}

#[test]
fn unit_system_shows_the_preferred_unit_of_each_dimension() {
    use crate::units::{display_unit, unit, UnitSystem};
    let length = unit("m").expect("m").dimension;
    let force = unit("N").expect("N").dimension;
    assert_eq!(
        display_unit(&length, UnitSystem::Us).map(|u| u.symbol),
        Some("ft")
    );
    assert_eq!(
        display_unit(&length, UnitSystem::Cgs).map(|u| u.symbol),
        Some("cm")
    );
    assert_eq!(
        display_unit(&force, UnitSystem::Cgs).map(|u| u.symbol),
        Some("dyn")
    );
    assert_eq!(
        display_unit(&force, UnitSystem::Us).map(|u| u.symbol),
        Some("lbf")
    );
    assert_eq!(display_unit(&length, UnitSystem::Si), None);
    assert_eq!(UnitSystem::from_attr("us"), Some(UnitSystem::Us));
    assert_eq!(UnitSystem::from_attr("custom"), None);
    assert_eq!(UnitSystem::Cgs.attr(), "cgs");
}
