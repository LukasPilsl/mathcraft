//! Evaluation of 2D plots on synthetic models: series over a domain, parametric traces, two traces,
//! gaps (non-finite values), errors, empty and huge ranges, log scales and polar plots.

use std::f64::consts::PI;

use mathcraft_core::eval::Session;
use mathcraft_core::expr::Expr;
use mathcraft_core::parser::{parse, DefaultIdContext};
use mathcraft_import::evaluate_plots::{evaluate_plot, PlotData, MAX_PLOT_POINTS};
use mathcraft_import::regions::{Axis2d, Equation, Plot2d, PlotTrace, TraceStyle};

fn expr(src: &str) -> Expr {
    parse(src, &DefaultIdContext).unwrap_or_else(|e| panic!("{src}: {e:?}"))
}

/// A session where each line is run in order (definitions for the plot).
fn session(lines: &[&str]) -> Session {
    let mut s = Session::new();
    for line in lines {
        s.run(&expr(line));
    }
    s
}

fn axis(start: Option<f64>, end: Option<f64>, equations: &[&str]) -> Axis2d {
    Axis2d {
        start,
        end,
        domain_start: None,
        domain_end: None,
        log: false,
        grid_lines: true,
        grid_frequency: Some(10),
        look: Default::default(),
        equations: equations
            .iter()
            .map(|src| match *src {
                "" => Equation::Empty,
                other => Equation::Expr(expr(other)),
            })
            .collect(),
    }
}

fn trace(points: Option<u32>) -> PlotTrace {
    PlotTrace {
        result_ref: None,
        num_of_points: points,
        style: TraceStyle {
            color: Some([255, 0, 0, 255]),
            line_weight: Some(1.0),
            line_style: Some("Solid".into()),
            symbol: Some("none".into()),
            mode: Some("lines".into()),
            automatic: false,
        },
        name: None,
        x: Equation::Empty,
        y: Equation::Empty,
    }
}

/// A plot with one trace per `(x, y)` pair of equation sources, and the given axes.
fn plot(pairs: &[(&str, &str, Option<u32>)], x: Axis2d, y: Axis2d) -> Plot2d {
    let traces = pairs
        .iter()
        .map(|(xs, ys, n)| {
            let mut t = trace(*n);
            t.x = if xs.is_empty() {
                Equation::Empty
            } else {
                Equation::Expr(expr(xs))
            };
            t.y = if ys.is_empty() {
                Equation::Empty
            } else {
                Equation::Expr(expr(ys))
            };
            t
        })
        .collect();
    Plot2d {
        polar: false,
        legend: false,
        title: None,
        x_axis: x,
        y_axis: y,
        traces,
        markers: Vec::new(),
    }
}

fn run(model: &Plot2d, s: &Session) -> PlotData {
    evaluate_plot(model, s.env())
}

fn xy(axis_x: (f64, f64), axis_y: (f64, f64)) -> (Axis2d, Axis2d) {
    (
        axis(Some(axis_x.0), Some(axis_x.1), &["x"]),
        axis(Some(axis_y.0), Some(axis_y.1), &["y"]),
    )
}

#[test]
fn sin_over_the_x_domain() {
    let (x, y) = xy((0.0, 2.0 * PI), (-1.0, 1.0));
    let m = plot(&[("x", "sin(x)", Some(6))], x, y);
    let data = run(&m, &Session::new());
    let trace = &data.traces[0];
    assert!(trace.error.is_none(), "{:?}", trace.error);
    assert_eq!(trace.segments.len(), 1);
    let pts = &trace.segments[0];
    assert_eq!(pts.len(), 5);
    assert_eq!(trace.gaps, 0);
    assert!((pts[0].0).abs() < 1e-15 && (pts[0].1).abs() < 1e-15);
    assert!(
        (pts[4].0 - 2.0 * PI).abs() < 1e-12,
        "last x is the domain end"
    );
    for (px, py) in pts {
        assert!((py - px.sin()).abs() < 1e-12, "sin({px}) = {py}");
    }
    assert_eq!(data.x_range, (0.0, 2.0 * PI));
    assert_eq!(data.y_range, (-1.0, 1.0));
}

#[test]
fn parametric_trace_uses_the_worksheet_range() {
    // t := 0 .. 4 has five values; with no num-of-points the trace takes them.
    let s = session(&["t := 0 .. 4"]);
    let (x, y) = xy((0.0, 1.0), (0.0, 1.0));
    let m = plot(&[("2 t", "t^2", None)], x, y);
    let data = run(&m, &s);
    let trace = &data.traces[0];
    assert!(trace.error.is_none(), "{:?}", trace.error);
    let pts = &trace.segments[0];
    assert_eq!(pts.len(), 5);
    for (i, (px, py)) in pts.iter().enumerate() {
        let t = i as f64;
        assert!((px - 2.0 * t).abs() < 1e-12, "x at {i}: {px}");
        assert!((py - t * t).abs() < 1e-12, "y at {i}: {py}");
    }
}

#[test]
fn two_traces_are_both_evaluated() {
    let (x, y) = xy((-1.0, 1.0), (-2.0, 2.0));
    let m = plot(&[("x", "x^2", Some(4)), ("x", "x^3", Some(8))], x, y);
    let data = run(&m, &Session::new());
    assert_eq!(data.traces.len(), 2);
    assert_eq!(data.traces[0].segments[0].len(), 3);
    assert_eq!(data.traces[1].segments[0].len(), 7);
    assert_eq!(data.point_count(), 10);
}

#[test]
fn a_constant_trace_is_one_point() {
    let (x, y) = xy((0.0, 10.0), (0.0, 10.0));
    let m = plot(&[("2", "1", None)], x, y);
    let data = run(&m, &Session::new());
    assert_eq!(data.traces[0].segments, vec![vec![(2.0, 1.0)]]);
}

#[test]
fn non_finite_values_are_gaps_and_split_the_polyline() {
    // 1/x at x = 0 divides by zero: the middle value is a gap.
    let (x, y) = xy((-1.0, 1.0), (-100.0, 100.0));
    let m = plot(&[("x", "1/x", Some(6))], x, y);
    let data = run(&m, &Session::new());
    let trace = &data.traces[0];
    assert!(trace.error.is_none(), "{:?}", trace.error);
    assert_eq!(trace.gaps, 1);
    assert_eq!(
        trace.segments.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![2, 2]
    );
}

#[test]
fn all_values_non_finite_give_no_points() {
    let (x, y) = xy((0.0, 1.0), (0.0, 1.0));
    let m = plot(&[("x", "0/0", Some(5))], x, y);
    let data = run(&m, &Session::new());
    let trace = &data.traces[0];
    assert!(trace.error.is_none(), "{:?}", trace.error);
    assert!(trace.segments.is_empty());
    assert_eq!(trace.gaps, 4);
    assert_eq!(data.point_count(), 0);
}

#[test]
fn an_undefined_name_ends_only_its_trace() {
    let (x, y) = xy((0.0, 1.0), (0.0, 1.0));
    let m = plot(&[("x", "nosuch(x)", Some(4)), ("x", "x", Some(4))], x, y);
    let data = run(&m, &Session::new());
    let bad = data.traces[0].error.as_ref().expect("first trace fails");
    assert!(!bad.unsupported, "undefined names are engine errors");
    assert!(data.traces[0].segments.is_empty());
    assert!(data.traces[1].error.is_none());
    assert_eq!(data.traces[1].segments[0].len(), 3);
}

#[test]
fn an_unsupported_equation_is_an_unsupported_trace() {
    let (x, y) = xy((0.0, 1.0), (0.0, 1.0));
    let mut m = plot(&[("x", "x", Some(3))], x, y);
    m.traces[0].y = Equation::Unsupported("cannot convert plot equation".into());
    let data = run(&m, &Session::new());
    let error = data.traces[0].error.as_ref().expect("error");
    assert!(error.unsupported);
    assert_eq!(error.message, "cannot convert plot equation");
}

#[test]
fn an_empty_equation_is_an_empty_trace() {
    let (x, y) = xy((0.0, 1.0), (0.0, 1.0));
    let m = plot(&[("", "", Some(3))], x, y);
    let data = run(&m, &Session::new());
    assert!(data.traces[0].error.is_none());
    assert!(data.traces[0].segments.is_empty());
}

#[test]
fn a_huge_point_count_is_capped() {
    let (x, y) = xy((0.0, 1.0), (0.0, 1.0));
    let m = plot(&[("x", "x", Some(5_000_000))], x, y);
    let data = run(&m, &Session::new());
    assert_eq!(data.traces[0].segments[0].len(), MAX_PLOT_POINTS);
    assert!(
        data.notes.iter().any(|n| n.contains("capped")),
        "{:?}",
        data.notes
    );
}

#[test]
fn a_single_point_count_is_raised_to_two() {
    let (x, y) = xy((0.0, 1.0), (0.0, 1.0));
    let m = plot(&[("x", "x", Some(1))], x, y);
    let data = run(&m, &Session::new());
    assert_eq!(data.traces[0].segments[0].len(), 2);
}

#[test]
fn an_undefined_parameter_without_points_uses_the_x_domain() {
    // No range variable t is defined, so the trace samples the x domain (500 values, inferred).
    let (x, y) = xy((0.0, 1.0), (0.0, 1.0));
    let m = plot(&[("t", "t", None)], x, y);
    let data = run(&m, &Session::new());
    let trace = &data.traces[0];
    assert!(trace.error.is_none(), "{:?}", trace.error);
    assert_eq!(trace.segments.len(), 1);
    assert_eq!(trace.segments[0].len(), 500);
    assert_eq!(trace.segments[0][0], (0.0, 0.0));
    assert_eq!(trace.segments[0][499], (1.0, 1.0));
}

#[test]
fn a_reversed_or_missing_axis_falls_back_with_a_note() {
    let x = axis(Some(5.0), Some(1.0), &["x"]);
    let y = axis(None, None, &["y"]);
    let m = plot(&[("x", "x", Some(3))], x, y);
    let data = run(&m, &Session::new());
    assert_eq!(data.x_range, (0.0, 1.0));
    assert!(data.notes.iter().any(|n| n.contains("x axis")));
    // The y range comes from the points when the axis states none.
    assert_eq!(data.y_range, (0.0, 1.0));
}

#[test]
fn log_scale_needs_a_positive_range() {
    let mut x = axis(Some(0.1), Some(100.0), &["x"]);
    x.log = true;
    let mut y = axis(Some(-1.0), Some(10.0), &["y"]);
    y.log = true;
    let m = plot(&[("x", "x", Some(3))], x, y);
    let data = run(&m, &Session::new());
    assert!(data.log_x, "positive range is logarithmic");
    assert!(
        !data.log_y,
        "a range that includes zero or below stays linear"
    );
}

#[test]
fn polar_angles_in_degrees_are_converted() {
    // Angular axis 0..360 degrees, radius 2 everywhere: every point lies on the circle of radius 2.
    let angle = axis(Some(0.0), Some(360.0), &["phi"]);
    let radius = axis(Some(1.0), Some(2.0), &["r"]);
    let mut m = plot(&[("phi", "2", Some(41))], angle.clone(), radius);
    m.polar = true;
    let data = run(&m, &session(&[]));
    assert!(data.polar);
    let pts = &data.traces[0].segments[0];
    assert_eq!(pts.len(), 40);
    for (px, py) in pts {
        assert!(
            (px.hypot(*py) - 2.0).abs() < 1e-12,
            "radius of ({px}, {py})"
        );
    }
    assert_eq!(data.x_range, (-2.0, 2.0));
    assert_eq!(data.y_range, (-2.0, 2.0));
    // The last angle is 2 pi, so the last point is back on the positive x axis.
    let last = pts.last().unwrap();
    assert!((last.0 - 2.0).abs() < 1e-9 && last.1.abs() < 1e-9);
}

#[test]
fn num_of_points_gives_one_value_fewer() {
    // Prime stores 499 values for num-of-points 500 (every sample trace): the grid is spread
    // evenly over the x domain, first to last.
    let (x, y) = xy((0.0, 1.0), (0.0, 1.0));
    let m = plot(&[("x", "x", Some(500))], x, y);
    let data = run(&m, &Session::new());
    let trace = &data.traces[0];
    assert!(trace.error.is_none(), "{:?}", trace.error);
    assert_eq!(trace.samples.len(), 499);
    assert_eq!(trace.segments[0].len(), 499);
    assert_eq!(trace.samples[0], Some((0.0, 0.0)));
    assert_eq!(trace.samples[498], Some((1.0, 1.0)));
}

#[test]
fn a_complex_value_is_a_gap_as_in_prime() {
    // sqrt(x) is complex for x < 0. Prime stores NaN there, so the value is a gap, not an error.
    let (x, y) = xy((-1.0, 1.0), (-2.0, 2.0));
    let m = plot(&[("x", "sqrt(x)", Some(6))], x, y);
    let data = run(&m, &Session::new());
    let trace = &data.traces[0];
    assert!(trace.error.is_none(), "{:?}", trace.error);
    assert_eq!(trace.gaps, 2);
    assert_eq!(trace.samples[0], None);
    assert_eq!(trace.samples[1], None);
    assert!(trace.samples[2].is_some());
    assert_eq!(
        trace.segments.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![3]
    );
}

#[test]
fn a_subscripted_name_is_the_range_variable_of_that_name() {
    // x_1 := 0 .. 2 is a range variable named x_1 (a literal subscript), plotted on x; the y axis
    // uses c_1, which Mathcad reads as c(x_1) when c is a function.
    let s = session(&["x_1 := 0 .. 2", "c(v) := 2 v"]);
    let (x, y) = xy((0.0, 2.0), (0.0, 4.0));
    let m = plot(&[("x_1", "c_1", None)], x, y);
    let data = run(&m, &s);
    let trace = &data.traces[0];
    assert!(trace.error.is_none(), "{:?}", trace.error);
    assert_eq!(
        trace.samples,
        vec![Some((0.0, 0.0)), Some((1.0, 2.0)), Some((2.0, 4.0))]
    );
}

#[test]
fn sampling_starts_at_the_stored_domain_not_the_displayed_range() {
    // Sample 6 region 7: auto-scale displays the x axis from -5.1 to 3.4, but Prime samples x over
    // the domain's own values, -5 to 10/3 (the xyDomain `startValue` and `endValue`, evaluated in
    // the worksheet). The stored trace has 499 values from -5 to 10/3.
    let (mut x, y) = xy((-5.1, 3.4), (-1.0, 6.0));
    x.domain_start = Some(Equation::Expr(expr("-5")));
    x.domain_end = Some(Equation::Expr(expr("10/3")));
    let m = plot(&[("x", "x", Some(500))], x, y);
    let data = run(&m, &Session::new());
    let trace = &data.traces[0];
    assert!(trace.error.is_none(), "{:?}", trace.error);
    assert_eq!(trace.samples.len(), 499);
    assert_eq!(trace.samples[0], Some((-5.0, -5.0)));
    let last = trace.samples[498].expect("a value");
    assert!((last.0 - 10.0 / 3.0).abs() < 1e-12, "{last:?}");
    // The displayed range is still the axis as shown.
    assert_eq!(data.x_range, (-5.1, 3.4));
}

#[test]
fn a_domain_bound_that_is_a_placeholder_or_absent_falls_back_to_the_display() {
    let (mut x, y) = xy((-2.0, 2.0), (-1.0, 1.0));
    x.domain_start = Some(Equation::Empty);
    x.domain_end = None;
    // num-of-points 3 samples two values (one fewer), across the displayed -2 .. 2.
    let m = plot(&[("x", "x", Some(3))], x, y);
    let data = run(&m, &Session::new());
    let trace = &data.traces[0];
    assert_eq!(trace.samples, vec![Some((-2.0, -2.0)), Some((2.0, 2.0))]);
}
