//! Unit tests of the golden tally and the strict failure count. The classification rules are
//! tested in `mathcraft-import` (`compare/tests.rs`).

use super::*;

#[test]
fn weak_error_passes_are_counted_in_the_tally_and_the_summary() {
    let weak = Category::WeakPass("bad%_divide".to_string());
    let mut t = Tally::default();
    t.add(&Category::Pass);
    t.add(&weak);
    assert_eq!(t.pass, 2, "a weak pass is still a pass");
    assert_eq!(t.weak_pass, 1);

    let run = GoldenRun {
        files: vec![FileReport {
            name: "a.mcdx".to_string(),
            file_error: None,
            notes: Vec::new(),
            rows: vec![RegionRow {
                region_id: 1,
                plot: false,
                expr: "x".to_string(),
                category: weak,
                expected: None,
                actual: None,
            }],
        }],
    };
    let text = crate::render_run(&run, false);
    assert!(text.contains("weak error passes: 1"), "{text}");
}

#[test]
fn strict_failures_include_engine_errors_without_reference() {
    let row = RegionRow {
        region_id: 1,
        plot: false,
        expr: "x".to_string(),
        category: Category::EngineErrorNoRef("undefined name `x`".to_string()),
        expected: None,
        actual: None,
    };
    let run = GoldenRun {
        files: vec![FileReport {
            name: "a.mcdx".to_string(),
            file_error: None,
            notes: Vec::new(),
            rows: vec![row],
        }],
    };
    assert_eq!(run.failure_count(), 1);
}

#[test]
fn tally_counts_each_category_and_reason() {
    let mut t = Tally::default();
    t.add(&Category::Pass);
    t.add(&Category::Unsupported("plot".into()));
    t.add(&Category::Unsupported("plot".into()));
    t.add(&Category::NoReference);
    t.add(&Category::EngineErrorNoRef("x".into()));
    assert_eq!(t.pass, 1);
    assert_eq!(t.unsupported_total(), 2);
    assert_eq!(t.unsupported["plot"], 2);
    assert_eq!(t.no_reference, 1);
    assert_eq!(t.engine_error_no_ref, 1);
    assert_eq!(t.engine_error, 0);
}
