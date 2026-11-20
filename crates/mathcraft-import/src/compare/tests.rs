//! Unit tests of the value comparison and of the region classification.

use super::*;
use crate::evaluate::StepKind;
use crate::results::Status;

fn metre() -> Dimension {
    Dimension::base(0)
}

fn scalar(re: f64, im: f64, dim: Dimension) -> Scalar {
    let mut s = Scalar::real(re, dim);
    s.value.im = im;
    s
}

fn sc(re: f64, dim: Dimension) -> Expected {
    Expected::Scalar { re, im: 0.0, dim }
}

fn stored(value: ResultValue) -> ResultData {
    ResultData {
        status: Status::Synchronized,
        value,
    }
}

/// Classifies an engine outcome against an optional stored result.
fn judge(stored: Option<&ResultData>, outcome: &Outcome) -> Category {
    classify(&RegionEvaluation {
        region_id: 1,
        expr_text: "x".to_string(),
        outcome: EvalOutcome::Engine(Box::new(outcome.clone())),
        stored: stored.cloned(),
        step_kind: StepKind::Normal,
    })
}

#[test]
fn a_computed_nested_array_is_unsupported_not_a_mismatch() {
    // The stored format of nested arrays is unverified, so a nested result is never compared.
    let nested = Value::List(vec![Value::Scalar(Scalar::real(1.0, metre()))]);
    let stored_scalar = stored(ResultValue::Real(1.0));
    assert!(matches!(
        judge(Some(&stored_scalar), &Outcome::Value(nested.clone())),
        Category::Unsupported(msg) if msg.contains("nested array")
    ));
    // A stored nested matrix is refused by the reader's comparison rules too.
    let stored_nested = stored(ResultValue::Matrix {
        rows: 1,
        cols: 1,
        elements: vec![ResultValue::Matrix {
            rows: 1,
            cols: 1,
            elements: vec![ResultValue::Real(1.0)],
        }],
    });
    assert!(matches!(
        judge(Some(&stored_nested), &Outcome::Value(nested)),
        Category::Unsupported(_)
    ));
}

#[test]
fn tolerance_is_relative_with_absolute_floor() {
    assert!(close(1.0, 1.0 + 0.5e-12));
    assert!(!close(1.0, 1.0 + 5e-12));
    // Near zero the absolute tolerance 1e-12 applies.
    assert!(close(0.0, 0.5e-12));
    assert!(!close(0.0, 2e-12));
    // A value of about 37 has an ulp of about 7e-15, far inside the tolerance.
    assert!(close(-36.869897645844, -36.869897645844 - 1e-12));
    assert!(!close(-36.869897645844, -36.869897645844 - 1e-6));
}

#[test]
fn nan_never_matches_and_infinity_matches_only_itself() {
    assert!(!close(f64::NAN, f64::NAN));
    assert!(!close(1.0, f64::NAN));
    assert!(close(f64::INFINITY, f64::INFINITY));
    assert!(!close(f64::INFINITY, f64::NEG_INFINITY));
    assert!(!close(1e300, f64::INFINITY));
}

#[test]
fn scalar_dimension_mismatch_is_reported_before_value() {
    let actual = Value::Scalar(scalar(1.0, 0.0, metre()));
    let err = compare_value(&sc(1.0, Dimension::dimensionless()), &actual).unwrap_err();
    assert!(err.starts_with("dimension mismatch"), "{err}");
}

#[test]
fn scalar_value_mismatch_names_both_values() {
    let actual = Value::Scalar(scalar(2.0, 0.0, metre()));
    let err = compare_value(&sc(1.0, metre()), &actual).unwrap_err();
    assert!(
        err.contains("expected 1.0") && err.contains("got 2.0"),
        "{err}"
    );
}

#[test]
fn complex_values_compare_both_parts() {
    let d = Dimension::dimensionless();
    let expected = Expected::Scalar {
        re: 1.0,
        im: 2.0,
        dim: d,
    };
    let good = Value::Scalar(scalar(1.0, 2.0, d));
    assert!(compare_value(&expected, &good).is_ok());
    let bad_im = Value::Scalar(scalar(1.0, -2.0, d));
    assert!(compare_value(&expected, &bad_im).is_err());
    let real_only = Value::Scalar(scalar(1.0, 0.0, d));
    assert!(compare_value(&expected, &real_only).is_err());
}

#[test]
fn matrices_compare_shape_then_elements_row_major() {
    let d = Dimension::dimensionless();
    let expected = Expected::Matrix {
        rows: 1,
        cols: 2,
        elements: vec![sc(0.8, d), sc(1.4, d)],
    };
    let ok =
        Value::Matrix(Matrix::new(1, 2, vec![Scalar::real(0.8, d), Scalar::real(1.4, d)]).unwrap());
    assert!(compare_value(&expected, &ok).is_ok());

    let swapped =
        Value::Matrix(Matrix::new(1, 2, vec![Scalar::real(1.4, d), Scalar::real(0.8, d)]).unwrap());
    let err = compare_value(&expected, &swapped).unwrap_err();
    assert!(err.starts_with("element 0"), "{err}");

    let transposed =
        Value::Matrix(Matrix::new(2, 1, vec![Scalar::real(0.8, d), Scalar::real(1.4, d)]).unwrap());
    let err = compare_value(&expected, &transposed).unwrap_err();
    assert!(err.starts_with("shape mismatch"), "{err}");
}

#[test]
fn scalar_against_matrix_and_text_are_mismatches() {
    let d = Dimension::dimensionless();
    let matrix = Value::Matrix(Matrix::new(1, 1, vec![Scalar::real(1.0, d)]).unwrap());
    assert!(compare_value(&sc(1.0, d), &matrix).is_err());
    let scalar_value = Value::Scalar(Scalar::real(1.0, d));
    let matrix_expected = Expected::Matrix {
        rows: 1,
        cols: 1,
        elements: vec![sc(1.0, d)],
    };
    assert!(compare_value(&matrix_expected, &scalar_value).is_err());
    assert!(compare_value(&sc(1.0, d), &Value::Str("x".into())).is_err());
}

#[test]
fn stored_united_value_uses_the_units_table_for_dimension() {
    let newton = ResultValue::United {
        value: 1015.1544629760358,
        units: vec![UnitRef {
            unit: "newton".to_string(),
            num: 1,
            den: 1,
        }],
    };
    let expected = expected_from_stored(&newton).unwrap();
    let force = Value::Scalar(Scalar::real(
        1015.1544629760358,
        Dimension::from_ints([1, 1, -2, 0, 0, 0, 0]),
    ));
    assert!(compare_value(&expected, &force).is_ok());
}

#[test]
fn stored_unit_powers_and_unknown_units() {
    let area = dimension_of(&[UnitRef {
        unit: "meter".to_string(),
        num: 2,
        den: 1,
    }])
    .unwrap();
    assert_eq!(area, Dimension::from_ints([2, 0, 0, 0, 0, 0, 0]));
    let root = dimension_of(&[UnitRef {
        unit: "meter".to_string(),
        num: 1,
        den: 2,
    }])
    .unwrap();
    assert_eq!(root.exponents()[0], units::Exp::new(1, 2));
    assert!(dimension_of(&[UnitRef {
        unit: "no-such-unit".to_string(),
        num: 1,
        den: 1,
    }])
    .is_err());
    assert!(dimension_of(&[UnitRef {
        unit: "meter".to_string(),
        num: 1,
        den: 0,
    }])
    .is_err());
}

#[test]
fn matrix_expected_from_stored_rejects_nested_matrices() {
    let nested = ResultValue::Matrix {
        rows: 1,
        cols: 1,
        elements: vec![ResultValue::Matrix {
            rows: 1,
            cols: 1,
            elements: vec![ResultValue::Real(1.0)],
        }],
    };
    assert!(expected_from_stored(&nested).is_err());
}

#[test]
fn dimension_mismatch_between_stored_and_computed_is_a_mismatch() {
    let stored_length = stored(ResultValue::United {
        value: 2.0,
        units: vec![UnitRef {
            unit: "meter".to_string(),
            num: 1,
            den: 1,
        }],
    });
    let outcome = Outcome::Value(Value::Scalar(Scalar::real(2.0, Dimension::dimensionless())));
    assert!(matches!(
        judge(Some(&stored_length), &outcome),
        Category::Mismatch(_)
    ));
}

#[test]
fn stored_error_needs_an_engine_error_not_a_value() {
    let stored_error = stored(ResultValue::Error {
        code: "bad%_variable".to_string(),
        message: "not defined".to_string(),
    });
    let error = Outcome::Error(EvalError::Undefined("a".to_string()));
    assert_eq!(judge(Some(&stored_error), &error), Category::Pass);
    let value = Outcome::Value(Value::Scalar(Scalar::real(1.0, Dimension::dimensionless())));
    assert!(matches!(
        judge(Some(&stored_error), &value),
        Category::Mismatch(_)
    ));
}

#[test]
fn stored_value_with_engine_error_is_an_engine_error() {
    let stored_real = stored(ResultValue::Real(1.0));
    let error = Outcome::Error(EvalError::DivisionByZero);
    assert!(matches!(
        judge(Some(&stored_real), &error),
        Category::EngineError(_)
    ));
}

#[test]
fn unsupported_error_wins_over_everything() {
    let stored_real = stored(ResultValue::Real(1.0));
    let unsupported = Outcome::Error(EvalError::Unsupported("range variables".into()));
    assert_eq!(
        judge(Some(&stored_real), &unsupported),
        Category::Unsupported("range variables".to_string())
    );
    let stored_error = stored(ResultValue::Error {
        code: "x".to_string(),
        message: "y".to_string(),
    });
    assert_eq!(
        judge(Some(&stored_error), &unsupported),
        Category::Unsupported("range variables".to_string())
    );
}

#[test]
fn nothing_stored_or_empty_is_no_reference() {
    let outcome = Outcome::Defined {
        name: "x".to_string(),
        display: None,
    };
    assert_eq!(judge(None, &outcome), Category::NoReference);
    let empty = stored(ResultValue::Empty);
    assert_eq!(judge(Some(&empty), &outcome), Category::NoReference);
}

#[test]
fn engine_error_without_reference_is_its_own_category() {
    let error = Outcome::Error(EvalError::DivisionByZero);
    assert!(matches!(judge(None, &error), Category::EngineErrorNoRef(_)));
    let empty = stored(ResultValue::Empty);
    assert!(matches!(
        judge(Some(&empty), &error),
        Category::EngineErrorNoRef(_)
    ));
    // Unsupported still wins, even without a reference.
    let unsupported = Outcome::Error(EvalError::Unsupported("plot".into()));
    assert_eq!(
        judge(None, &unsupported),
        Category::Unsupported("plot".to_string())
    );
}

#[test]
fn stored_undefined_error_needs_an_undefined_name_error() {
    // bad%_variable (and codes starting with it) passes on Undefined or UndefinedFunction only.
    let stored_error = stored(ResultValue::Error {
        code: "bad%_variable".to_string(),
        message: "not defined".to_string(),
    });
    let undefined = Outcome::Error(EvalError::Undefined("a".to_string()));
    let function = Outcome::Error(EvalError::UndefinedFunction("f".to_string()));
    let divide = Outcome::Error(EvalError::DivisionByZero);
    assert_eq!(judge(Some(&stored_error), &undefined), Category::Pass);
    assert_eq!(judge(Some(&stored_error), &function), Category::Pass);
    assert!(matches!(
        judge(Some(&stored_error), &divide),
        Category::Mismatch(_)
    ));
}

#[test]
fn stored_undefined_error_code_is_matched_by_prefix() {
    let stored_error = stored(ResultValue::Error {
        code: "bad%_variable_2".to_string(),
        message: "not defined".to_string(),
    });
    let undefined = Outcome::Error(EvalError::Undefined("a".to_string()));
    assert_eq!(judge(Some(&stored_error), &undefined), Category::Pass);
    let divide = Outcome::Error(EvalError::DivisionByZero);
    assert!(matches!(
        judge(Some(&stored_error), &divide),
        Category::Mismatch(_)
    ));
}

#[test]
fn other_stored_error_codes_accept_any_engine_error_as_weak_pass() {
    let stored_error = stored(ResultValue::Error {
        code: "bad%_divide".to_string(),
        message: "division".to_string(),
    });
    let divide = Outcome::Error(EvalError::DivisionByZero);
    assert_eq!(
        judge(Some(&stored_error), &divide),
        Category::WeakPass("bad%_divide".to_string())
    );
}

#[test]
fn override_on_a_united_stored_value_is_refused() {
    // Untested case: Prime's stored value of an override result has unit references. The
    // harness must not guess which number to compare.
    let united = stored(ResultValue::United {
        value: 1.315e-4,
        units: vec![UnitRef {
            unit: "meter".to_string(),
            num: 1,
            den: 1,
        }],
    });
    let converted = Outcome::Converted {
        value: Value::Scalar(Scalar::real(1.315e-4, Dimension::base(0))),
        unit: units::CompoundUnit::parse("mm").unwrap(),
        shown: Value::Scalar(Scalar::real(0.1315, Dimension::base(0))),
    };
    assert_eq!(
        judge(Some(&united), &converted),
        Category::Unsupported("override with united stored value".to_string())
    );
}

#[test]
fn converted_outcome_compares_shown_number_for_plain_stored_values() {
    // Prime stores a unit-override result as the number shown, without unit references.
    let stored_plain = stored(ResultValue::Real(76.6075022462489));
    let converted = Outcome::Converted {
        value: Value::Scalar(Scalar::real(1.3370531459259951, Dimension::dimensionless())),
        unit: units::CompoundUnit::parse("°").unwrap(),
        shown: Value::Scalar(Scalar::real(76.6075022462489, Dimension::dimensionless())),
    };
    assert_eq!(judge(Some(&stored_plain), &converted), Category::Pass);
}

#[test]
fn definition_with_an_evaluation_is_classified_by_its_display() {
    // `lb := (|C-A| =)`: Prime stores the displayed value of the evaluation.
    let stored_real = stored(ResultValue::Real(6.708203932499369));
    let shown = Outcome::Value(Value::Scalar(Scalar::real(
        6.708203932499369,
        Dimension::dimensionless(),
    )));
    let defined = Outcome::Defined {
        name: "lb".to_string(),
        display: Some(Box::new(shown)),
    };
    assert_eq!(judge(Some(&stored_real), &defined), Category::Pass);
    let wrong = Outcome::Defined {
        name: "lb".to_string(),
        display: Some(Box::new(Outcome::Value(Value::Scalar(Scalar::real(
            1.0,
            Dimension::dimensionless(),
        ))))),
    };
    assert!(matches!(
        judge(Some(&stored_real), &wrong),
        Category::Mismatch(_)
    ));
}

#[test]
fn plain_definition_with_a_stored_value_is_still_a_mismatch() {
    let stored_real = stored(ResultValue::Real(1.0));
    let plain = Outcome::Defined {
        name: "x".to_string(),
        display: None,
    };
    assert!(matches!(
        judge(Some(&stored_real), &plain),
        Category::Mismatch(_)
    ));
}

#[test]
fn skipped_region_is_unsupported_whatever_is_stored() {
    let stored_real = stored(ResultValue::Real(1.0));
    let row = RegionEvaluation {
        region_id: 1,
        expr_text: "plot (xyPlot)".to_string(),
        outcome: EvalOutcome::Skipped("plot".to_string()),
        stored: Some(stored_real),
        step_kind: StepKind::Normal,
    };
    assert_eq!(classify(&row), Category::Unsupported("plot".to_string()));
}

#[test]
fn panic_is_an_engine_error_with_or_without_a_reference() {
    let panic = |stored: Option<ResultData>| {
        classify(&RegionEvaluation {
            region_id: 1,
            expr_text: "x".to_string(),
            outcome: EvalOutcome::Panic("boom".to_string()),
            stored,
            step_kind: StepKind::Normal,
        })
    };
    assert_eq!(
        panic(Some(stored(ResultValue::Real(1.0)))),
        Category::EngineError("Panic: boom".to_string())
    );
    assert_eq!(
        panic(Some(stored(ResultValue::Empty))),
        Category::EngineErrorNoRef("Panic: boom".to_string())
    );
    assert_eq!(
        panic(None),
        Category::EngineErrorNoRef("Panic: boom".to_string())
    );
}

mod plot_series {
    use super::*;
    use crate::evaluate_plots::{TraceData, TraceError};
    use crate::regions::TraceStyle;
    use crate::results::Trace2d;

    fn stored_trace(range: Vec<f64>, result: Vec<f64>) -> Trace2d {
        Trace2d {
            trace_type: "Parametric".to_string(),
            range_min: 0.0,
            range_max: 1.0,
            range_units: Vec::new(),
            result_min: 0.0,
            result_max: 1.0,
            range_points: range,
            result_points: result,
        }
    }

    /// A computed trace with the given samples (`None` is a gap).
    fn computed(samples: Vec<Option<(f64, f64)>>) -> TraceData {
        TraceData {
            name: None,
            style: TraceStyle {
                color: None,
                line_weight: None,
                line_style: None,
                symbol: None,
                mode: None,
                automatic: false,
            },
            segments: Vec::new(),
            gaps: samples.iter().filter(|s| s.is_none()).count(),
            error: None::<TraceError>,
            samples,
            stored: None,
        }
    }

    #[test]
    fn equal_series_pass() {
        let stored = stored_trace(vec![0.0, 1.0], vec![2.0, 3.0]);
        let trace = computed(vec![Some((0.0, 2.0)), Some((1.0, 3.0))]);
        assert_eq!(compare_trace(&stored, &trace), Ok(()));
    }

    #[test]
    fn the_tolerance_is_1e_9_absolute_and_relative() {
        // Relative: 1e-10 passes, 1e-8 does not. Absolute near zero: the same bounds.
        assert!(close_within(1.0e6, 1.0e6 * (1.0 + 1e-10), PLOT_TOL));
        assert!(!close_within(1.0e6, 1.0e6 * (1.0 + 1e-8), PLOT_TOL));
        assert!(close_within(0.0, 1e-10, PLOT_TOL));
        assert!(!close_within(0.0, 1e-8, PLOT_TOL));
        assert!(!close_within(f64::NAN, f64::NAN, PLOT_TOL));
        assert!(close_within(f64::INFINITY, f64::INFINITY, PLOT_TOL));
        assert!(!close_within(f64::INFINITY, 1.0, PLOT_TOL));
    }

    #[test]
    fn a_point_outside_the_tolerance_is_reported_by_index() {
        let stored = stored_trace(vec![0.0, 1.0, 2.0], vec![2.0, 3.0, 4.0]);
        let trace = computed(vec![
            Some((0.0, 2.0)),
            Some((1.0, 3.0 + 1e-6)),
            Some((2.0, 4.0)),
        ]);
        let err = compare_trace(&stored, &trace).unwrap_err();
        assert!(
            err.starts_with("point 1: expected (1, 3), got (1, 3.000001"),
            "{err}"
        );
    }

    #[test]
    fn differing_lengths_are_a_mismatch() {
        let stored = stored_trace(vec![0.0, 1.0], vec![2.0, 3.0]);
        let trace = computed(vec![Some((0.0, 2.0))]);
        assert_eq!(
            compare_trace(&stored, &trace),
            Err("point count: stored 2, computed 1".to_string())
        );
    }

    #[test]
    fn stored_vectors_of_different_lengths_are_refused() {
        let stored = stored_trace(vec![0.0, 1.0], vec![2.0]);
        let trace = computed(vec![Some((0.0, 2.0))]);
        assert!(compare_trace(&stored, &trace)
            .unwrap_err()
            .contains("differ in length"));
    }

    #[test]
    fn nan_in_the_stored_series_is_a_gap_that_must_be_a_gap() {
        let stored = stored_trace(vec![0.0, 1.0, 2.0], vec![2.0, f64::NAN, 4.0]);
        assert_eq!(
            stored.points().unwrap(),
            vec![Some((0.0, 2.0)), None, Some((2.0, 4.0))]
        );
        let gap_too = computed(vec![Some((0.0, 2.0)), None, Some((2.0, 4.0))]);
        assert_eq!(compare_trace(&stored, &gap_too), Ok(()));
        let value_instead = computed(vec![Some((0.0, 2.0)), Some((1.0, 3.0)), Some((2.0, 4.0))]);
        assert_eq!(
            compare_trace(&stored, &value_instead),
            Err("point 1: expected a gap, got (1, 3)".to_string())
        );
        let missing_gap = computed(vec![Some((0.0, 2.0)), Some((1.0, 3.0)), None]);
        assert!(compare_trace(&stored, &missing_gap)
            .unwrap_err()
            .starts_with("point 1: expected a gap"));
    }

    #[test]
    fn a_stored_vector_is_one_point() {
        let stored = Trace2d {
            trace_type: "Vector".to_string(),
            ..stored_trace(vec![0.0625], vec![1.875])
        };
        assert_eq!(
            compare_trace(&stored, &computed(vec![Some((0.0625, 1.875))])),
            Ok(())
        );
    }

    #[test]
    fn a_value_where_prime_has_a_gap_is_refused() {
        let stored = stored_trace(vec![0.0], vec![f64::NAN]);
        assert!(compare_trace(&stored, &computed(vec![Some((0.0, 1.0))])).is_err());
    }
}
