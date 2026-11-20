//! Evaluation of 3D plots on synthetic models: curves, surfaces over two ranges, matrix surfaces,
//! parametric surfaces, gaps (`NaN` holes), huge grids (capped) and undefined variables.

use mathcraft_core::eval::Session;
use mathcraft_core::expr::Expr;
use mathcraft_core::parser::{parse, DefaultIdContext};
use mathcraft_import::evaluate_plots3d::{evaluate_plot3d, Plot3dData, MAX_SURFACE_SIDE};
use mathcraft_import::regions::{Axis3d, Equation, Plot3d, Trace3d, TraceStyle};

fn expr(src: &str) -> Expr {
    parse(src, &DefaultIdContext).unwrap_or_else(|e| panic!("{src}: {e:?}"))
}

fn session(lines: &[&str]) -> Session {
    let mut s = Session::new();
    for line in lines {
        s.run(&expr(line));
    }
    s
}

fn axis(start: Option<f64>, end: Option<f64>) -> Axis3d {
    Axis3d {
        start,
        end,
        log: false,
        grid_lines: true,
        grid_frequency: Some(5),
    }
}

fn style() -> TraceStyle {
    TraceStyle {
        color: None,
        line_weight: None,
        line_style: None,
        symbol: None,
        mode: Some("surface".into()),
        automatic: false,
    }
}

/// A trace whose plot equation has the given slots (`""` is a placeholder).
fn trace(num_of_points: Option<u32>, slots: &[&str]) -> Trace3d {
    Trace3d {
        result_ref: None,
        num_of_points,
        style: style(),
        equation: slots
            .iter()
            .map(|src| match *src {
                "" => Equation::Empty,
                other => Equation::Expr(expr(other)),
            })
            .collect(),
    }
}

fn plot(traces: Vec<Trace3d>, x: (f64, f64), y: (f64, f64)) -> Plot3d {
    Plot3d {
        x_axis: axis(Some(x.0), Some(x.1)),
        y_axis: axis(Some(y.0), Some(y.1)),
        z_axis: axis(None, None),
        legend: false,
        equations: Vec::new(),
        traces,
        camera: None,
    }
}

fn run(model: &Plot3d, s: &Session) -> Plot3dData {
    evaluate_plot3d(model, s.env())
}

#[test]
fn surface_of_x_times_y_over_two_ranges() {
    let s = session(&["x := 0 .. 2", "y := 0 .. 3"]);
    let model = plot(vec![trace(None, &["x*y"])], (0.0, 2.0), (0.0, 3.0));
    let data = run(&model, &s);
    let t = &data.traces[0];
    assert!(t.error.is_none(), "{:?}", t.error);
    assert!(t.surface);
    // x runs along the columns (3 values), y along the rows (4 values).
    assert_eq!((t.rows, t.cols), (4, 3));
    for r in 0..4 {
        for c in 0..3 {
            let i = r * 3 + c;
            assert_eq!(t.x[i], c as f64, "x at {r},{c}");
            assert_eq!(t.y[i], r as f64, "y at {r},{c}");
            assert_eq!(t.z[i], (c * r) as f64, "z at {r},{c}");
        }
    }
    assert_eq!(t.gaps, 0);
    assert_eq!(data.x_range, (0.0, 2.0));
}

#[test]
fn curve_over_a_range_is_x_zero_and_the_expression() {
    let s = session(&["x := 0 .. 4"]);
    let model = plot(vec![trace(Some(41), &["x", ""])], (0.0, 4.0), (0.0, 4.0));
    let t = &run(&model, &s).traces[0];
    assert!(!t.surface);
    // The range decides the values (5 of them), not num-of-points.
    assert_eq!((t.rows, t.cols), (5, 1));
    assert_eq!(t.x, vec![0.0, 1.0, 2.0, 3.0, 4.0]);
    assert!(t.y.iter().all(|&v| v == 0.0));
    assert_eq!(t.z, t.x);
}

#[test]
fn matrix_surface_takes_the_matrix_as_z() {
    let s = session(&["M := identity(3)"]);
    let model = plot(vec![trace(None, &["M"])], (0.0, 1.0), (0.0, 1.0));
    let data = run(&model, &s);
    let t = &data.traces[0];
    assert!(t.error.is_none(), "{:?}", t.error);
    assert!(t.surface);
    assert_eq!((t.rows, t.cols), (3, 3));
    for r in 0..3 {
        for c in 0..3 {
            let i = r * 3 + c;
            assert_eq!(t.z[i], if r == c { 1.0 } else { 0.0 });
            // X runs along the columns over the x domain, Y along the rows over the y domain.
            assert!((t.x[i] - c as f64 / 2.0).abs() < 1e-12);
            assert!((t.y[i] - r as f64 / 2.0).abs() < 1e-12);
        }
    }
}

#[test]
fn parametric_sphere_lies_on_the_unit_sphere() {
    let s = session(&["u := 0 .. 3", "v := 0 .. 4"]);
    let model = plot(
        vec![trace(None, &["sin(u)*cos(v)", "sin(u)*sin(v)", "cos(u)"])],
        (-1.0, 1.0),
        (-1.0, 1.0),
    );
    let t = &run(&model, &s).traces[0];
    assert!(t.error.is_none(), "{:?}", t.error);
    assert_eq!((t.rows, t.cols), (5, 4));
    for i in 0..t.x.len() {
        let norm = t.x[i].powi(2) + t.y[i].powi(2) + t.z[i].powi(2);
        assert!((norm - 1.0).abs() < 1e-12, "point {i}: |p|^2 = {norm}");
    }
}

#[test]
fn values_that_are_not_finite_are_gaps() {
    let s = session(&["x := -1 .. 1"]);
    let model = plot(vec![trace(None, &["1/x", ""])], (-1.0, 1.0), (0.0, 1.0));
    let t = &run(&model, &s).traces[0];
    assert!(t.error.is_none(), "{:?}", t.error);
    assert_eq!(t.gaps, 1);
    assert!(t.z[1].is_nan(), "1/0 is a gap");
    assert_eq!(t.z[0], -1.0);
    assert_eq!(t.z[2], 1.0);
}

#[test]
fn surface_holes_are_gaps_and_the_rest_is_kept() {
    // sqrt of a negative value is complex: a gap on part of the grid.
    let s = session(&["x := -1 .. 1", "y := 0 .. 1"]);
    let model = plot(
        vec![trace(None, &["sqrt(x) + 0*y"])],
        (-1.0, 1.0),
        (0.0, 1.0),
    );
    let t = &run(&model, &s).traces[0];
    assert!(t.error.is_none(), "{:?}", t.error);
    assert_eq!((t.rows, t.cols), (2, 3));
    assert!(t.z[0].is_nan() && t.z[3].is_nan());
    assert_eq!(t.z[2], 1.0);
    assert_eq!(t.gaps, 2);
}

#[test]
fn huge_grids_are_thinned_and_noted() {
    let s = session(&["x := 0 .. 999", "y := 0 .. 999"]);
    let model = plot(vec![trace(None, &["x + y"])], (0.0, 999.0), (0.0, 999.0));
    let data = run(&model, &s);
    let t = &data.traces[0];
    assert!(t.error.is_none(), "{:?}", t.error);
    let side = MAX_SURFACE_SIDE;
    assert_eq!((t.rows, t.cols), (side, side));
    assert_eq!(t.z.len(), side * side);
    assert!(data.notes.iter().any(|n| n.contains("thinned")));
    // The first and last values are kept.
    assert_eq!(t.x[side - 1], 999.0);
}

#[test]
fn undefined_variables_use_the_axis_domain_and_num_of_points() {
    // `a` and `b` are not defined: they are sampled over the x and y axis domains with
    // num-of-points - 1 = 4 values each.
    let s = session(&[]);
    let model = plot(vec![trace(Some(5), &["a + b"])], (0.0, 1.0), (0.0, 2.0));
    let t = &run(&model, &s).traces[0];
    assert!(t.error.is_none(), "{:?}", t.error);
    assert_eq!((t.rows, t.cols), (4, 4));
    assert_eq!(t.x[0], 0.0);
    assert_eq!(t.x[3], 1.0);
    assert_eq!(t.y[3 * 4], 2.0);
    assert_eq!(t.z[3 * 4 + 3], 3.0);
}

#[test]
fn unsupported_slot_and_missing_matrix_are_reported() {
    let s = session(&["x := 0 .. 1", "k := 2"]);
    let mut broken = trace(None, &[]);
    broken.equation = vec![Equation::Unsupported("cannot convert".into())];
    let t = &run(&plot(vec![broken], (0.0, 1.0), (0.0, 1.0)), &s).traces[0];
    let error = t.error.as_ref().expect("an error");
    assert!(error.unsupported);

    // A scalar with no variables is not a plot.
    let t = &run(&plot(vec![trace(None, &["k"])], (0.0, 1.0), (0.0, 1.0)), &s).traces[0];
    let error = t.error.as_ref().expect("an error");
    assert!(error.unsupported);
    assert!(error.message.contains("matrix"), "{}", error.message);
}

#[test]
fn an_empty_first_slot_is_an_empty_trace() {
    let s = session(&[]);
    let t = &run(&plot(vec![trace(None, &[""])], (0.0, 1.0), (0.0, 1.0)), &s).traces[0];
    assert!(t.error.is_none());
    assert_eq!((t.rows, t.cols), (0, 0));
}

#[test]
fn stated_axes_are_the_ranges_and_missing_ones_come_from_the_data() {
    let s = session(&["x := 0 .. 2", "y := 0 .. 3"]);
    let model = plot(vec![trace(None, &["x*y"])], (-5.0, 5.0), (0.0, 3.0));
    let data = run(&model, &s);
    assert_eq!(data.x_range, (-5.0, 5.0));
    assert_eq!(data.z_range, (0.0, 6.0));
}

#[test]
fn no_panic_on_an_empty_model() {
    let s = session(&[]);
    let data = run(&plot(Vec::new(), (0.0, 0.0), (1.0, 0.0)), &s);
    assert!(data.traces.is_empty());
    assert_eq!(data.x_range, (0.0, 1.0));
    assert!(data.notes.iter().any(|n| n.contains("x axis")));
}
