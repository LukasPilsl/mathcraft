//! Converts every `<math>` element of the 24 sample packages to an expression.
//!
//! Every math element, nested ones included, must convert without error. A count table is
//! printed (`-- --nocapture`). Hand-picked expressions are checked against their exact printed
//! form, with the XML they come from noted beside each one.

#[macro_use]
mod support;

use std::path::{Path, PathBuf};

use mathcraft_core::expr::Expr;
use mathcraft_import::dom::{self, WS};
use mathcraft_import::expr::convert_math;
use mathcraft_import::package::Package;

fn samples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples")
}

/// Per-file counts: every `<math>` element in the worksheet, how many of them sit in a region,
/// and conversion errors. The rest are plot equations (`plotEquation/math`).
struct FileReport {
    name: String,
    math: usize,
    in_regions: usize,
    errors: Vec<String>,
}

fn report(path: &Path) -> Result<FileReport, String> {
    let name = path
        .file_name()
        .expect("file name")
        .to_string_lossy()
        .into_owned();
    let pkg = Package::open(path).map_err(|e| format!("{name}: {e}"))?;
    let doc = dom::parse(&pkg.worksheet).map_err(|e| format!("{name}: {e}"))?;
    let regions = dom::regions(&doc).map_err(|e| format!("{name}: {e}"))?;

    // Every <math> element, at any depth. This includes the plot-equation ones.
    let all: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name((WS, "math")))
        .collect();
    let in_regions: Vec<_> = regions.iter().filter_map(|r| r.math).collect();
    for node in &in_regions {
        assert!(
            all.contains(node),
            "{name}: region math not in worksheet walk"
        );
    }
    for node in all.iter().filter(|n| !in_regions.contains(n)) {
        let parent = node.parent_element().map(|p| p.tag_name().name());
        assert_eq!(
            parent,
            Some("plotEquation"),
            "{name}: a <math> outside regions is not a plot equation"
        );
    }

    let mut errors = Vec::new();
    for node in &all {
        if let Err(e) = convert_math(*node) {
            let owner = node
                .ancestors()
                .find(|a| a.has_tag_name((WS, "region")))
                .and_then(|r| r.attribute("region-id"))
                .unwrap_or("plot");
            errors.push(format!("{name} region {owner}: {e}"));
        }
    }

    Ok(FileReport {
        name,
        math: all.len(),
        in_regions: in_regions.len(),
        errors,
    })
}

/// The expression of the `<math>` whose `resultRef` is `result_ref` in the named sample.
fn expr_for(sample: &str, result_ref: u32) -> Expr {
    let path = samples_dir().join(format!("{sample}.mcdx"));
    let pkg = Package::open(&path).expect("open sample");
    let doc = dom::parse(&pkg.worksheet).expect("parse worksheet");
    let regions = dom::regions(&doc).expect("regions parse");
    let matches: Vec<_> = regions
        .iter()
        .filter(|r| r.result_ref == Some(result_ref))
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "{sample}: resultRef {result_ref} not unique"
    );
    let node = matches[0].math.expect("region has math");
    convert_math(node).unwrap_or_else(|e| panic!("{sample} #{result_ref}: {e}"))
}

#[test]
fn every_math_element_in_every_sample_converts() {
    require_samples!();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(samples_dir())
        .expect("read examples dir")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "mcdx"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 24, "expected 24 sample packages");

    println!(
        "{:<48} {:>6} {:>10} {:>10} {:>8}",
        "file", "math", "in_region", "converted", "errors"
    );
    let mut total = 0;
    let mut failures = Vec::new();
    for path in &paths {
        match report(path) {
            Ok(r) => {
                let converted = r.math - r.errors.len();
                println!(
                    "{:<48} {:>6} {:>10} {:>10} {:>8}",
                    r.name,
                    r.math,
                    r.in_regions,
                    converted,
                    r.errors.len()
                );
                total += r.math;
                failures.extend(r.errors);
            }
            Err(e) => failures.push(e),
        }
    }
    println!("total math elements: {total}");
    assert_eq!(total, 704, "math element count across the 24 samples");
    assert!(total > 0, "no math elements found");
    assert!(
        failures.is_empty(),
        "conversion failures:\n{}",
        failures.join("\n")
    );
}

/// Exact printed forms, each checked against the XML noted in the comment.
#[test]
fn hand_picked_expressions_print_exactly() {
    require_samples!();
    let cases: &[(&str, u32, &str)] = &[
        // 1_rata_1, resultRef 0: symEval of (3+4)/(5^2-1), no command.
        ("1_rata_1", 0, "(3+4)/(5^2-1) ->"),
        // 1_rata_1, resultRef 1: minus(plus(div(3,4), div(1,3)), div(1,6)).
        ("1_rata_1", 1, "3/4+1/3-1/6 ->"),
        // 1_rata_1, resultRef 2: ln is a FUNCTION id, so it prints as a call.
        ("1_rata_1", 2, "ln(2+1/3)/4-2/7 ->"),
        // 1_rata_1, resultRef 3: top-level symEval, scale 6 x^3 inside a product.
        ("1_rata_1", 3, "x^5*(6 (x^3))/(x^(1/2)*x^(5/3)) ->"),
        // 1_rata_1, resultRef 4: nthRoot(5, 3 + 2*nthRoot(placeholder, 4)).
        ("1_rata_1", 4, "root(3+2*sqrt(4), 5) ->"),
        // 1_rata_1, resultRef 5: eval with an empty degree (square root) and no unit.
        ("1_rata_1", 5, "sqrt(5)="),
        // 2_rata_2, resultRef 3: symEval of a parens node with command `factor`.
        ("2_rata_2", 3, "(a^2-b^2) -> factor"),
        // 10_rata_18_UMP_Dreieck, resultRef 0: define A := 2x1 matrix, negative first entry.
        ("10_rata_18_UMP_Dreieck", 0, "A:=[-3; 2]"),
        // 12_rata_Kreisdiagramme, resultRef 0: range with a second value, scale 4 pi.
        ("12_rata_Kreisdiagramme", 0, "Φ:=0, 0.05 .. 4 (π)"),
        // 12_rata_Kreisdiagramme, resultRef 2: XAML subscript on the defined name.
        ("12_rata_Kreisdiagramme", 2, "ϕ_1:=0, 0.05 .. 2 (π)"),
        // 16_rata_Dreieck_flaeche_winkel, resultRef 7: function define with a symEval body.
        (
            "16_rata_Dreieck_flaeche_winkel",
            7,
            "a(x):=g(x, B, C) -> expand",
        ),
        // 17_rata_schiefer_wurf, resultRef 6: KEYWORD find, SYSTEM-labelled argument.
        // The core printer parenthesizes an Eval on the right of `:=` (ASSIGN binds looser
        // than RANGE), so the printed form is `t1:=(find(t)=)`.
        ("17_rata_schiefer_wurf", 6, "t1:=(find(t)=)"),
        // 17_rata_golfball_donau_lsg, resultRef 7: matrix define of subscripted names.
        ("17_rata_golfball_donau_lsg", 7, "[v_0, t_1]:=(find(v, t)=)"),
        // Mechanik_3_Spannungsberechnung, resultRef 5: define M_r with an eval body whose
        // unit override is a product, so it is parenthesized.
        (
            "Mechanik_3_Spannungsberechnung",
            5,
            "M_r:=(M*g*L_2->(N*m)=)",
        ),
    ];
    let mut failures = Vec::new();
    for (sample, result_ref, expected) in cases {
        let got = expr_for(sample, *result_ref).to_string();
        if got != *expected {
            failures.push(format!(
                "{sample} #{result_ref}: got `{got}`, want `{expected}`"
            ));
        }
    }
    assert!(failures.is_empty(), "mismatches:\n{}", failures.join("\n"));
}
