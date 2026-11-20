use super::*;
use crate::fonts::Face;
use crate::glyphs;
use mathcraft_core::parser::{parse, DefaultIdContext};

fn expr(src: &str) -> Expr {
    parse(src, &DefaultIdContext).expect("test expression parses")
}

fn lay_src(src: &str) -> MathBox {
    layout(&expr(src), 12.0)
}

/// Text primitives of a box, in order: (text, size).
fn texts(b: &MathBox) -> Vec<(String, f32)> {
    b.prims
        .iter()
        .filter_map(|p| match p {
            Prim::Text { text, size, .. } => Some((text.clone(), *size)),
            _ => None,
        })
        .collect()
}

fn rects(b: &MathBox) -> Vec<(f32, f32, f32, f32)> {
    b.prims
        .iter()
        .filter_map(|p| match p {
            Prim::Rect { x, y, w, h } => Some((*x, *y, *w, *h)),
            _ => None,
        })
        .collect()
}

fn assert_sane(b: &MathBox) {
    assert!(b.width.is_finite() && b.width >= 0.0, "width {}", b.width);
    assert!(b.ascent.is_finite() && b.ascent >= 0.0);
    assert!(b.descent.is_finite() && b.descent >= 0.0);
}

#[test]
fn text_has_positive_size_and_width_grows_with_content() {
    let short = lay_src("a");
    let long = lay_src("abc");
    assert!(short.width > 0.0 && short.height() > 0.0);
    assert!(long.width > short.width);
}

#[test]
fn binary_width_is_monotone_in_operands() {
    let a = lay_src("a+b");
    let b = lay_src("a+b+cc");
    assert!(b.width > a.width, "{} > {}", b.width, a.width);
}

#[test]
fn fraction_is_taller_than_numerator_plus_denominator() {
    let frac = lay_src("a/b");
    let num = lay_src("a");
    let den = lay_src("b");
    assert!(frac.height() > num.height() + den.height());
    // The bar is drawn between numerator and denominator.
    assert_eq!(rects(&frac).len(), 1);
}

#[test]
fn exponent_is_smaller_than_base_and_raised() {
    let b = lay_src("x^2");
    let t = texts(&b);
    assert_eq!(t[0].0, "x");
    assert_eq!(t[1].0, "2");
    assert!(t[1].1 < t[0].1, "script shrinks");
    let raised = b.prims.iter().any(|p| match p {
        Prim::Text { y, text, .. } => text == "2" && *y < 0.0,
        _ => false,
    });
    assert!(raised, "exponent sits above the baseline");
}

#[test]
fn nested_exponents_shrink_further() {
    let b = lay_src("x^(y^z)");
    let sizes: Vec<f32> = texts(&b).iter().map(|(_, s)| *s).collect();
    let z = sizes.last().copied().expect("z drawn");
    assert!(
        z < 12.0 * 0.7 + 1e-3,
        "nested script smaller than first level: {z}"
    );
}

#[test]
fn literal_subscript_is_lowered() {
    let b = lay_src("F_sy");
    let lowered = b.prims.iter().any(|p| match p {
        Prim::Text { y, text, .. } => text == "sy" && *y > 0.0,
        _ => false,
    });
    assert!(lowered);
}

#[test]
fn matrix_is_a_grid_of_its_dimensions() {
    let m = Expr::matrix(2, 3, (0..6).map(|i| Expr::num(f64::from(i))).collect());
    let b = layout(&m, 12.0);
    assert_sane(&b);
    let digits: Vec<f32> = b
        .prims
        .iter()
        .filter_map(|p| match p {
            Prim::Text { y, text, .. }
                if text.len() == 1 && text.chars().all(|c| c.is_ascii_digit()) =>
            {
                Some(*y)
            }
            _ => None,
        })
        .collect();
    assert_eq!(digits.len(), 6);
    // Two distinct baselines: two rows.
    let mut rows: Vec<i64> = digits.iter().map(|y| (*y * 100.0).round() as i64).collect();
    rows.sort_unstable();
    rows.dedup();
    assert_eq!(rows.len(), 2);
}

#[test]
fn parentheses_follow_content_height() {
    let short = lay_src("(a)");
    let tall = lay_src("(a/b)");
    assert!(tall.height() > short.height());
    assert!(tall.height() >= lay_src("a/b").height());
}

#[test]
fn sqrt_and_nth_root_are_sane_and_wider_than_radicand() {
    let r = lay_src("sqrt(x)");
    assert_sane(&r);
    assert!(r.width > lay_src("x").width);
    let n = lay_src("root(x, 3)");
    assert_sane(&n);
}

#[test]
fn absolute_value_draws_two_bars() {
    let b = layout(&Expr::unary(UnaryOp::Abs, Expr::var("x")), 12.0);
    assert_eq!(rects(&b).len(), 2);
}

#[test]
fn the_word_inf_is_drawn_as_the_infinity_sign() {
    let all: Vec<String> = texts(&lay_src("integral(x, 0, inf, x)"))
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert!(all.iter().any(|t| t == "∞"), "{all:?}");
    assert!(!all.iter().any(|t| t == "inf"), "{all:?}");
}

#[test]
fn a_megabyte_name_is_cut_to_a_short_ellipsis_text() {
    let name = "v".repeat(1_000_000);
    let b = layout(&Expr::var(&name), 12.0);
    assert_sane(&b);
    let drawn = texts(&b);
    assert_eq!(drawn.len(), 1);
    let (text, _) = &drawn[0];
    assert_eq!(text.chars().count(), glyphs::MAX_TOKEN_CHARS);
    assert!(text.ends_with(glyphs::ELLIPSIS));
}

#[test]
fn a_thousand_by_a_thousand_matrix_is_a_placeholder_and_lays_out_fast() {
    let start = std::time::Instant::now();
    let big = Expr::Matrix {
        rows: 1000,
        cols: 1000,
        elements: Vec::new(),
    };
    let b = layout(&big, 12.0);
    assert_sane(&b);
    assert_eq!(texts(&b), vec![("[matrix]".to_string(), 12.0)]);
    assert!(rects(&b).is_empty());
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
}

#[test]
fn a_matrix_at_the_cell_cap_is_drawn_as_a_grid() {
    // 100 x 100 is exactly MAX_MATRIX_CELLS: drawn, not replaced.
    let cells: Vec<Expr> = (0..10_000).map(|_| Expr::var("a")).collect();
    let b = layout(
        &Expr::Matrix {
            rows: 100,
            cols: 100,
            elements: cells,
        },
        12.0,
    );
    assert_sane(&b);
    assert!(texts(&b).iter().all(|(t, _)| t != "[matrix]"));
}

#[test]
fn arrows_and_comparisons_are_drawn_with_their_real_glyphs() {
    // The unit override arrow of an evaluation is the arrow glyph of the bundled math face.
    let eval = Expr::Eval {
        expr: Box::new(Expr::var("x")),
        unit_override: Some(Box::new(Expr::var("mm"))),
    };
    let drawn: Vec<String> = texts(&layout(&eval, 12.0))
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert!(drawn.iter().any(|t| t == "→"), "{drawn:?}");
    assert!(drawn.iter().all(|t| t != "->"), "{drawn:?}");
    let ne = Expr::binary(BinaryOp::Ne, Expr::var("a"), Expr::var("b"));
    assert!(texts(&layout(&ne, 12.0)).iter().any(|(t, _)| t == "≠"));
}

#[test]
fn variables_are_italic_and_function_names_units_and_numbers_are_upright() {
    let faces = |b: &MathBox| -> Vec<(String, Face)> {
        b.prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { text, face, .. } => Some((text.clone(), *face)),
                _ => None,
            })
            .collect()
    };
    let var = faces(&lay_src("x"));
    assert_eq!(var, vec![("x".to_string(), Face::Italic)]);
    let greek = faces(&lay_src("θ"));
    assert_eq!(greek, vec![("θ".to_string(), Face::Italic)]);
    let call = faces(&lay_src("sin(x)"));
    assert!(
        call.iter().any(|(t, f)| t == "sin" && *f == Face::Roman),
        "{call:?}"
    );
    assert!(
        call.iter().any(|(t, f)| t == "x" && *f == Face::Italic),
        "{call:?}"
    );
    let num = faces(&lay_src("12"));
    assert_eq!(num, vec![("12".to_string(), Face::Roman)]);
    let unit = Expr::Scale {
        value: Box::new(Expr::Number(Number::Real {
            value: 2.0,
            text: "2".to_string(),
        })),
        unit: Box::new(Expr::Id(Id {
            name: "mm".to_string(),
            kind: IdKind::Unit,
            subscript: None,
        })),
    };
    let units = faces(&layout(&unit, 12.0));
    assert!(
        units.iter().any(|(t, f)| t == "mm" && *f == Face::Roman),
        "{units:?}"
    );
}

#[test]
fn all_forms_produce_finite_boxes() {
    // Sources the parser accepts; the ones it rejects are skipped, and the count is checked.
    let sources = [
        "a=5",
        "sin(x)",
        "x..y",
        "x,1..10",
        "5 m",
        "v[1]",
        "-x",
        "a<=b",
        "a*b",
        "!x",
        "x'",
        "[1,2;3,4]",
        "3/4",
        "x^2/y",
        "sqrt(x)",
        "(a+b)*c",
    ];
    let mut parsed = 0;
    for src in sources {
        if let Ok(e) = parse(src, &DefaultIdContext) {
            parsed += 1;
            let b = layout(&e, 12.0);
            assert_sane(&b);
            assert!(b.width > 0.0, "{src} has width");
        }
    }
    assert!(parsed >= 12, "only {parsed} sources parsed");
    // Forms the text reader cannot express, built directly.
    for e in [
        Expr::Define {
            target: Box::new(Expr::var("a")),
            expr: Box::new(Expr::num(5.0)),
        },
        Expr::Eval {
            expr: Box::new(Expr::var("a")),
            unit_override: None,
        },
        Expr::unary(UnaryOp::Transpose, Expr::var("M")),
        Expr::unary(UnaryOp::Abs, Expr::var("x")),
    ] {
        assert_sane(&layout(&e, 12.0));
    }
}

/// Runs `f` on a thread with a 2 MiB stack, as the layout must not overflow there.
fn on_small_stack<F: FnOnce() + Send + 'static>(f: F) {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(f)
        .expect("spawn")
        .join()
        .expect("no panic or overflow");
}

fn nested_neg(depth: usize) -> Expr {
    let mut e = Expr::var("x");
    for _ in 0..depth {
        e = Expr::unary(UnaryOp::Neg, e);
    }
    e
}

fn nested_fractions(depth: usize) -> Expr {
    let mut e = Expr::var("x");
    for _ in 0..depth {
        e = Expr::binary(BinaryOp::Div, Expr::var("x"), e);
    }
    e
}

#[test]
fn deep_negation_nesting_is_laid_out_without_overflow() {
    on_small_stack(|| {
        let b98 = layout(&nested_neg(98), 12.0);
        assert_sane(&b98);
        let b400 = layout(&nested_neg(400), 12.0);
        assert_sane(&b400);
        // Beyond MAX_DEPTH the layout stops with a placeholder text.
        assert!(texts(&b400).iter().any(|(t, _)| t == "[too deep]"));
    });
}

#[test]
fn ninety_eight_deep_fractions_have_finite_sizes() {
    on_small_stack(|| {
        let b = layout(&nested_fractions(98), 12.0);
        assert_sane(&b);
        assert!(b.height() > 0.0);
        // 98 levels are within MAX_DEPTH, so no level is cut to the placeholder.
        assert!(
            !texts(&b).iter().any(|(t, _)| t == "[too deep]"),
            "98 nested fractions must lay out in full"
        );
    });
}

#[test]
fn ten_thousand_deep_expression_does_not_overflow() {
    on_small_stack(|| {
        let e = nested_neg(10_000);
        let b = layout(&e, 12.0);
        assert_sane(&b);
    });
}

#[test]
fn powers_nest_with_shrinking_sizes_to_the_limit() {
    on_small_stack(|| {
        let mut e = Expr::var("x");
        for _ in 0..300 {
            e = Expr::binary(BinaryOp::Pow, Expr::var("x"), e);
        }
        let b = layout(&e, 12.0);
        assert_sane(&b);
    });
}

#[test]
fn every_sample_math_region_lays_out_finite() {
    require_samples!();
    use crate::viewmodel::Body;
    use mathcraft_import::regions::{RegionKind, Worksheet};
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples");
    let mut files = 0;
    let mut math_regions = 0;
    let mut laid_out = 0;
    let entries = std::fs::read_dir(&dir).expect("examples directory");
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("mcdx") {
            continue;
        }
        let Ok(ws) = Worksheet::open(&path) else {
            continue;
        };
        files += 1;
        // Math regions of the worksheet, and the math boxes the view model lays out for it.
        let in_sheet = ws
            .all_regions()
            .iter()
            .filter(|r| matches!(r.kind, RegionKind::Math(_)))
            .count();
        let model = crate::load::load_file(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let mut in_model = 0;
        for item in &model.items {
            if let Body::Math { math, .. } = &item.body {
                in_model += 1;
                assert!(
                    math.width.is_finite() && math.width > 0.0,
                    "{path:?} item {}",
                    item.region_id
                );
                assert!(math.ascent.is_finite() && math.descent.is_finite());
                assert!(math.ascent + math.descent > 0.0);
            }
        }
        assert_eq!(in_model, in_sheet, "{path:?}: math regions laid out");
        math_regions += in_sheet;
        laid_out += in_model;
    }
    assert_eq!(files, 24, "all 24 tracked samples load");
    assert_eq!(
        laid_out, math_regions,
        "laid-out math regions over all samples"
    );
}

/// The four calculus operators of the operations sample draw their own glyphs, not a flat text.
#[test]
fn imported_calculus_operators_draw_their_glyphs() {
    let derivative = texts(&lay_src("derivative(x, r)"));
    assert!(
        derivative.iter().any(|(t, _)| t == "d"),
        "derivative draws d"
    );
    let integral = texts(&lay_src("integral(p, i, f, x)"));
    assert!(integral.iter().any(|(t, _)| t == "∫"), "integral draws ∫");
    let sum = texts(&lay_src("sum(f, 1, 2, y)"));
    assert!(sum.iter().any(|(t, _)| t == "Σ"), "summation draws Σ");
    let limit = lay_src("limit(x, 9, o)");
    let limit_texts: Vec<String> = texts(&limit).into_iter().map(|(t, _)| t).collect();
    assert!(limit_texts.iter().any(|t| t == "lim"), "limit draws lim");
    assert!(
        limit_texts.iter().any(|t| t == "→"),
        "limit draws the arrow"
    );
    assert!(
        limit_texts.iter().any(|t| t == "9"),
        "limit draws its target"
    );
    assert_sane(&limit);
}

fn faces_of(b: &MathBox) -> Vec<(String, Face)> {
    b.prims
        .iter()
        .filter_map(|p| match p {
            Prim::Text { text, face, .. } => Some((text.clone(), *face)),
            _ => None,
        })
        .collect()
}

/// A worksheet may label a function name as a variable (or not at all): it still draws upright.
#[test]
fn function_names_are_upright_whatever_their_label() {
    let call = Expr::Call {
        func: Id::new("cos", IdKind::Variable),
        args: vec![Expr::var("phi")],
    };
    let faces = faces_of(&layout(&call, 12.0));
    assert!(
        faces.iter().any(|(t, f)| t == "cos" && *f == Face::Roman),
        "{faces:?}"
    );
    assert!(
        faces.iter().any(|(t, f)| t == "phi" && *f == Face::Italic),
        "{faces:?}"
    );
    let bare = Expr::Id(Id::new("ln", IdKind::Variable));
    assert_eq!(
        faces_of(&layout(&bare, 12.0)),
        vec![("ln".to_string(), Face::Roman)]
    );
    // A user variable is still italic.
    let var = Expr::Id(Id::new("tau", IdKind::Variable));
    assert_eq!(
        faces_of(&layout(&var, 12.0)),
        vec![("tau".to_string(), Face::Italic)]
    );
}

/// An empty limit body or target is an outlined box, never a stray letter.
#[test]
fn an_empty_limit_slot_is_a_box_not_a_letter() {
    let limit = Expr::Limit {
        var: Box::new(Id::new("x", IdKind::Variable)),
        target: Box::new(Expr::Placeholder),
        body: Box::new(Expr::Placeholder),
    };
    let b = layout(&limit, 12.0);
    assert_sane(&b);
    let drawn: Vec<String> = texts(&b).into_iter().map(|(t, _)| t).collect();
    assert!(!drawn.iter().any(|t| t == "o"), "{drawn:?}");
    assert!(!drawn.iter().any(|t| t == "■"), "{drawn:?}");
    assert!(rects(&b).is_empty(), "the placeholder is an outline");
}

/// A sum with an upper limit: the limit sits above the sign, not beside the body.
#[test]
fn a_summation_upper_limit_sits_above_the_sign() {
    let sum = Expr::Accumulate {
        op: Accumulation::Sum,
        var: Box::new(Id::new("i", IdKind::Variable)),
        from: Box::new(Expr::num(1.0)),
        to: Box::new(Expr::var("n")),
        body: Box::new(Expr::var("y")),
    };
    let b = layout(&sum, 12.0);
    assert_sane(&b);
    let at = |s: &str| -> (f32, f32, f32) {
        b.prims
            .iter()
            .find_map(|p| match p {
                Prim::Text {
                    x, y, text, size, ..
                } if text == s => Some((*x, *y, *size)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{s} drawn"))
    };
    let (sign_x, sign_y, sign_size) = at("Σ");
    let (n_x, n_y, n_size) = at("n");
    let (body_x, body_y, _) = at("y");
    // The upper limit is above the sign's top, and no further right than the sign's right edge.
    assert!(n_y < sign_y - 0.78 * sign_size + 1e-3, "upper at {n_y}");
    assert!(n_size < sign_size);
    let sign_right = sign_x + text_width("Σ", sign_size);
    assert!(
        n_x <= sign_right + 1e-3,
        "upper at x {n_x}, sign ends {sign_right}"
    );
    // The body starts after the sign, not under the limit.
    assert!(body_x > sign_right, "body {body_x} sign {sign_right}");
    assert!((body_y - sign_y).abs() < 1e-3);
}

/// A matrix result: a grid in brackets, its cells as text, a minus sign as a true minus, and the
/// shared unit after the grid.
#[test]
fn a_matrix_result_is_a_grid_with_brackets_and_its_unit() {
    let cells: Vec<String> = ["1", "-2.5", "3", "4"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let b = matrix_result(2, 2, &cells, "m/s", 12.0);
    assert_sane(&b);
    let drawn: Vec<String> = texts(&b).into_iter().map(|(t, _)| t).collect();
    assert!(drawn.iter().any(|t| t == "−2.5"), "{drawn:?}");
    assert!(drawn.iter().any(|t| t == "m/s"), "{drawn:?}");
    // One pair of square brackets: a stroke on each side.
    let strokes = b
        .prims
        .iter()
        .filter(|p| matches!(p, Prim::Line { .. }))
        .count();
    assert_eq!(strokes, 2);
    // Two rows: the cells sit on two baselines.
    let mut rows: Vec<i64> = b
        .prims
        .iter()
        .filter_map(|p| match p {
            Prim::Text { y, text, .. } if ["1", "−2.5", "3", "4"].contains(&text.as_str()) => {
                Some((*y * 100.0).round() as i64)
            }
            _ => None,
        })
        .collect();
    rows.sort_unstable();
    rows.dedup();
    assert_eq!(rows.len(), 2);
}

#[test]
fn a_matrix_result_without_a_unit_is_just_the_grid() {
    let cells: Vec<String> = vec!["1".to_string()];
    let with = matrix_result(1, 1, &cells, "", 12.0);
    let plain = matrix(1, 1, vec![text("1", 12.0)], 12.0);
    assert!((with.width - plain.width).abs() < 1e-4);
}

/// A 1 x 400 result is drawn no wider than the capped grid: the columns past the cap are not laid
/// out, and an ellipsis column marks them.
#[test]
fn a_wide_matrix_result_is_capped_at_the_column_limit() {
    let cells: Vec<String> = (0..400).map(|i| i.to_string()).collect();
    let wide = matrix_result(1, 400, &cells, "", 12.0);
    // The grid the cap draws: the first 19 columns, then the ellipsis column.
    let mut shown: Vec<String> = cells[..MAX_RESULT_COLUMNS - 1].to_vec();
    shown.push("…".to_string());
    let capped = matrix_result(1, MAX_RESULT_COLUMNS, &shown, "", 12.0);
    assert!((wide.width - capped.width).abs() < 1e-4);
    assert!(texts(&wide).iter().any(|(t, _)| t == "…"));
}

/// Symbolic keywords (`solve`, `factor`, ...) are command words: upright. Their arguments stay
/// variables, in italic.
#[test]
fn symbolic_keywords_are_upright_and_their_arguments_italic() {
    let faces = faces_of(&lay_src("x -> solve, x"));
    assert!(
        faces.iter().any(|(t, f)| t == "solve" && *f == Face::Roman),
        "{faces:?}"
    );
    assert!(
        faces.iter().any(|(t, f)| t == "x" && *f == Face::Italic),
        "{faces:?}"
    );
    let factor = faces_of(&lay_src("(a^2 - b^2) -> factor"));
    assert!(
        factor
            .iter()
            .any(|(t, f)| t == "factor" && *f == Face::Roman),
        "{factor:?}"
    );
}

/// Left-to-right reading of a box: text by x, round brackets as `(` and `)` (the layout pushes a
/// pair's left stroke before its right one). Only meaningful without fractions, radicals, bars
/// and square brackets.
fn flat(b: &MathBox) -> String {
    let mut items: Vec<(f32, String)> = Vec::new();
    let mut n = 0;
    for p in &b.prims {
        match p {
            Prim::Text { x, text, .. } => items.push((*x, text.clone())),
            Prim::Line { points, .. } => {
                let x = points.iter().map(|q| q[0]).fold(f32::MAX, f32::min);
                items.push((x, if n % 2 == 0 { "(" } else { ")" }.to_string()));
                n += 1;
            }
            Prim::Rect { .. } => {}
        }
    }
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    items.into_iter().map(|i| i.1).collect::<Vec<_>>().concat()
}

#[test]
fn precedence_brackets_are_drawn_where_the_tree_has_none() {
    // Source text -> flat reading. Superscripts read inline, `−` is the true minus.
    let table = [
        ("(xx+1)^3", "(xx+1)3"),
        ("(xx+1)!", "(xx+1)!"),
        ("(xx-1)%", "(xx−1)%"),
        ("(xx*yy)^2", "(xx·yy)2"),
        ("(-2)^2", "(−2)2"),
        ("-xx^2", "−xx2"),
        ("-(xx+1)", "−(xx+1)"),
        ("-(xx*yy)", "−(xx·yy)"),
        ("xx^(yy^zz)", "xxyyzz"),
        ("(xx^yy)^zz", "(xxyy)zz"),
        ("xx-(yy-zz)", "xx−(yy−zz)"),
        ("xx-yy-zz", "xx−yy−zz"),
        ("xx+(yy+zz)", "xx+(yy+zz)"),
        ("(xx+yy)*zz", "(xx+yy)·zz"),
        ("xx*(yy+zz)", "xx·(yy+zz)"),
        ("xx*yy+zz", "xx·yy+zz"),
        ("xx-(-yy)", "xx−(−yy)"),
        ("xx*(-yy)", "xx·(−yy)"),
        ("(xx==yy)==zz", "(xx=yy)=zz"),
        ("sin((xx+1)^2)", "sin((xx+1)2)"),
        ("(xx+1)^(yy+1)", "(xx+1)yy+1"),
        ("(xx)^2", "(xx)2"),
    ];
    for (src, want) in table {
        assert_eq!(flat(&lay_src(src)), want, "{src}");
    }
}

#[test]
fn fractions_need_no_brackets_around_their_operands() {
    let b = lay_src("(xx+1)/(yy-1)");
    assert!(b.prims.iter().all(|p| !matches!(p, Prim::Line { .. })));
    // A division as a power base keeps its brackets; an exponent never has any.
    assert_eq!(flat(&lay_src("(xx+1)^(1/3)")).matches('(').count(), 1);
    assert!(lay_src("(1+1/xx)^xx")
        .prims
        .iter()
        .any(|p| matches!(p, Prim::Line { .. })));
}

#[test]
fn new_operators_are_typeset() {
    // Cross product: an infix cross sign.
    let cross = texts(&lay_src("u×v"));
    assert!(cross.iter().any(|(t, _)| t.contains('×')), "{cross:?}");
    // Norm: two bars on each side, no text bars.
    let norm = lay_src("‖v‖");
    assert_eq!(rects(&norm).len(), 4);
    assert_sane(&norm);
    assert!(rects(&norm).len() > rects(&lay_src("|v|")).len());
    // Partial derivative: the partial sign, never `d`.
    let partial = texts(&lay_src("partial(x, x^2)"));
    assert!(partial.iter().any(|(t, _)| t == "∂"), "{partial:?}");
    assert!(!partial.iter().any(|(t, _)| t == "d"), "{partial:?}");
    let nth = texts(&lay_src("partial(x, 2, x^2)"));
    assert!(nth.iter().any(|(t, _)| t == "∂"), "{nth:?}");
    // The ordinary derivative keeps `d`.
    let plain = texts(&lay_src("derivative(x, x^2)"));
    assert!(plain.iter().any(|(t, _)| t == "d"), "{plain:?}");
    // Gradient: nabla with the variables beside it.
    let grad = texts(&lay_src("gradient(f(a, b), a, b)"));
    assert!(grad.iter().any(|(t, _)| t == "∇"), "{grad:?}");
    assert!(grad.iter().any(|(t, _)| t == "a"), "{grad:?}");
    assert_sane(&lay_src("gradient(f(a, b))"));
    // Factorial: the operand then `!`.
    let fact = texts(&lay_src("n!"));
    assert!(fact.iter().any(|(t, _)| t == "!"), "{fact:?}");
}
