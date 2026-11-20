//! Evaluator-level checks for the file, design-of-experiments and image builtins: file access is
//! disabled by default and opt-in through the session's options, paths and links are refused,
//! the help examples come out as printed, and hostile sizes fail cleanly.

use std::sync::Arc;

use super::helpers::*;
use crate::eval::builtins::fileio::{
    FileAccess, FileOptions, FilePolicy, FileSwitches, MemFs, MAX_FILE_BYTES,
};
use crate::eval::value::Value;
use crate::eval::{EvalError, Outcome, Session};
use crate::expr::Expr;

const ON: FileSwitches = FileSwitches {
    read: true,
    write: true,
};

fn text(s: &str) -> Expr {
    Expr::Str(s.to_string())
}

/// Options that read and write the given files, in memory, with both switches on (the explicit
/// opt-in of the viewer's session).
fn mem_files(files: &[(&str, Vec<u8>)]) -> (FileOptions, Arc<MemFs>) {
    let fs = Arc::new(MemFs::new());
    for (name, bytes) in files {
        fs.put(name, FilePolicy::default(), bytes).expect("put");
    }
    let opts = FileOptions::with_access(fs.clone(), FilePolicy::default()).with_switches(ON);
    (opts, fs)
}

/// Runs `stmts` in one session with `files` as its file access; returns the last outcome.
fn run_with(files: FileOptions, stmts: &[Expr]) -> Outcome {
    let mut s = Session::with_file_options(files);
    let mut last = None;
    for st in stmts {
        last = Some(s.run(&evaluate_stmt(st.clone())));
    }
    last.expect("a statement")
}

fn value(out: Outcome) -> Value {
    expect_value(out)
}

fn el(v: &Value, i: usize, j: usize) -> f64 {
    matrix(v).get(i, j).expect("element").value.re
}

fn file_error(out: Outcome) -> String {
    match expect_error(out) {
        EvalError::FileAccess(text) => text,
        other => panic!("expected a file error, got {other:?}"),
    }
}

// ---- wiring: access is off by default and opt-in -------------------------------------------

#[test]
fn file_functions_refuse_without_access() {
    let msg = file_error(run_last(&[evaluate_stmt(call(
        "READCSV",
        vec![text("a.csv")],
    ))]));
    assert!(msg.contains("disabled"), "{msg}");
    let msg = file_error(run_last(&[evaluate_stmt(call(
        "WRITECSV",
        vec![text("a.csv"), mat(1, 1, vec![n(1.0)])],
    ))]));
    assert!(msg.contains("disabled"), "{msg}");
}

#[test]
fn csv_reads_a_file_the_worksheet_can_see() {
    let (files, _) = mem_files(&[("data.csv", b"1,2\n3,4\n".to_vec())]);
    let out = value(run_with(files, &[call("READCSV", vec![text("data.csv")])]));
    assert_eq!(matrix(&out).shape(), (2, 2));
    assert_eq!(el(&out, 1, 0), 3.0);
}

#[test]
fn csv_write_then_read_round_trips() {
    let (files, fs) = mem_files(&[]);
    let m = mat(2, 2, vec![n(1.0), n(3.0), n(2.0), n(4.0)]);
    let written = value(run_with(
        files.clone(),
        &[call("WRITECSV", vec![text("a.csv"), m.clone()])],
    ));
    assert_eq!(
        el(&written, 0, 1),
        3.0,
        "WRITECSV returns the matrix it wrote"
    );
    let back = value(run_with(files, &[call("READCSV", vec![text("a.csv")])]));
    assert_eq!(el(&back, 0, 1), 3.0);
    assert_eq!(el(&back, 1, 1), 4.0);
    assert!(fs
        .read("a.csv", FilePolicy::default(), MAX_FILE_BYTES)
        .is_ok());
}

#[test]
fn absolute_and_parent_names_are_refused_by_default() {
    let (files, _) = mem_files(&[("secret.csv", b"9\n".to_vec())]);
    for name in ["../secret.csv", "/etc/secret.csv", "a/../secret.csv"] {
        let msg = file_error(run_with(
            files.clone(),
            &[call("READCSV", vec![text(name)])],
        ));
        assert!(msg.contains("refused"), "{name}: {msg}");
    }
}

#[test]
fn oversized_file_is_refused_before_it_is_parsed() {
    let big = vec![b'1'; MAX_FILE_BYTES as usize + 1];
    let (files, _) = mem_files(&[("big.csv", big)]);
    let msg = file_error(run_with(files, &[call("READCSV", vec![text("big.csv")])]));
    assert!(msg.contains("too large"), "{msg}");
}

#[test]
fn missing_file_and_bad_content_are_clear_errors() {
    let (files, _) = mem_files(&[("bad.csv", b"1,x\n".to_vec())]);
    let msg = file_error(run_with(
        files.clone(),
        &[call("READCSV", vec![text("none.csv")])],
    ));
    assert!(msg.contains("not found"), "{msg}");
    let msg = file_error(run_with(files, &[call("READCSV", vec![text("bad.csv")])]));
    assert!(msg.contains("not a number"), "{msg}");
}

#[test]
fn names_must_be_text() {
    let (files, _) = mem_files(&[]);
    let out = run_with(files, &[call("READCSV", vec![n(1.0)])]);
    assert!(matches!(expect_error(out), EvalError::Unsupported(_)));
}

#[test]
fn real_directory_access_refuses_links_and_escapes() {
    let dir = std::env::temp_dir().join(format!("mathcraft-wiring-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).expect("mkdir");
    std::fs::write(dir.join("sub/ok.txt"), b"1\t2\n3\t4\n").expect("write");
    let outside = dir.with_extension("outside.csv");
    std::fs::write(&outside, b"9,9\n").expect("write outside");
    let opts = FileOptions::real_under(&dir.join("sub"))
        .expect("base")
        .with_switches(ON);

    let out = value(run_with(
        opts.clone(),
        &[call("READTEXT", vec![text("ok.txt")])],
    ));
    assert_eq!(el(&out, 1, 1), 4.0, "tab-delimited text is detected");
    let msg = file_error(run_with(
        opts.clone(),
        &[call("READCSV", vec![text("../../../etc/hosts")])],
    ));
    assert!(msg.contains("refused"), "{msg}");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, dir.join("sub/link.csv")).expect("symlink");
        let msg = file_error(run_with(opts, &[call("READCSV", vec![text("link.csv")])]));
        assert!(msg.contains("refused"), "{msg}");
    }
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&outside);
}

// ---- text and CSV formats --------------------------------------------------------------------

#[test]
fn readtext_detects_the_delimiter() {
    let (files, _) = mem_files(&[
        ("comma.txt", b"1,3\n2,4\n".to_vec()),
        ("space.txt", b"1   3\n2 4\n".to_vec()),
    ]);
    for name in ["comma.txt", "space.txt"] {
        let out = value(run_with(
            files.clone(),
            &[call("READTEXT", vec![text(name)])],
        ));
        assert_eq!((el(&out, 0, 1), el(&out, 1, 0)), (3.0, 2.0), "{name}");
    }
}

#[test]
fn writetext_writes_tab_delimited() {
    let (files, fs) = mem_files(&[]);
    let m = mat(1, 2, vec![n(5.0), n(6.5)]);
    value(run_with(
        files,
        &[call("WRITETEXT", vec![text("t.txt"), m])],
    ));
    let bytes = fs
        .read("t.txt", FilePolicy::default(), MAX_FILE_BYTES)
        .expect("read");
    assert_eq!(bytes, b"5\t6.5\n");
}

#[test]
fn ragged_csv_is_refused() {
    let (files, _) = mem_files(&[("r.csv", b"1,2\n3\n".to_vec())]);
    let msg = file_error(run_with(files, &[call("READCSV", vec![text("r.csv")])]));
    assert!(msg.contains("same number"), "{msg}");
}

// ---- switches, hostile files, quota -------------------------------------------------------------

#[test]
fn access_alone_does_not_enable_reading_or_writing() {
    // The worksheet has a base and a file, but both switches are off (the default).
    let (files, _) = mem_files(&[("data.csv", b"1,2\n".to_vec())]);
    let off = files.with_switches(FileSwitches::default());
    let msg = file_error(run_with(
        off.clone(),
        &[call("READCSV", vec![text("data.csv")])],
    ));
    assert!(msg.contains("reading is disabled"), "{msg}");
    let msg = file_error(run_with(
        off,
        &[call(
            "WRITECSV",
            vec![text("a.csv"), mat(1, 1, vec![n(1.0)])],
        )],
    ));
    assert!(
        msg.contains("writing is disabled") && msg.contains("F9"),
        "{msg}"
    );
}

#[test]
fn read_switch_alone_does_not_allow_writes() {
    let (files, fs) = mem_files(&[("data.csv", b"1,2\n".to_vec())]);
    let read_only = files.with_switches(FileSwitches {
        read: true,
        write: false,
    });
    value(run_with(
        read_only.clone(),
        &[call("READCSV", vec![text("data.csv")])],
    ));
    let msg = file_error(run_with(
        read_only,
        &[call(
            "WRITECSV",
            vec![text("a.csv"), mat(1, 1, vec![n(1.0)])],
        )],
    ));
    assert!(msg.contains("writing is disabled"), "{msg}");
    assert!(fs
        .read("a.csv", FilePolicy::default(), MAX_FILE_BYTES)
        .is_err());
}

#[test]
fn four_million_line_file_is_refused_quickly() {
    // 4,000,000 one-digit lines (8 MB): refused at the cell cap, not parsed in full.
    let (files, _) = mem_files(&[("lines.csv", b"1\n".repeat(4_000_000))]);
    let started = std::time::Instant::now();
    let msg = file_error(run_with(files, &[call("READCSV", vec![text("lines.csv")])]));
    assert!(msg.contains("too large"), "{msg}");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn lookup_eq_is_relative_so_small_values_do_not_match_each_other() {
    // lookup(1e-6, {1e-5, 2e-6, 3e-6}) used to return {10; 20; 30} with an absolute TOL.
    let search = mat(3, 1, vec![n(1e-5), n(2e-6), n(3e-6)]);
    let result = mat(3, 1, vec![n(10.0), n(20.0), n(30.0)]);
    let out = run_last(&[evaluate_stmt(call(
        "lookup",
        vec![n(1e-6), search.clone(), result.clone()],
    ))]);
    assert!(matches!(expect_error(out), EvalError::Unsupported(_)));
    let out = value(run_last(&[evaluate_stmt(call(
        "lookup",
        vec![n(2e-6), search, result],
    ))]));
    assert_eq!(el(&out, 0, 0), 20.0);
}

#[test]
fn vlookup_near_matches_the_help_example() {
    // The help example "Effect of TOL on Lookup Functions", step 7b: the rows whose time is near 1.5
    // are the nearest (1 and 2, at distance 0.5) and anything within TOL of that distance.
    // Row by row, as the help table is printed (matrices are row-major here).
    let table = mat(
        6,
        3,
        vec![
            n(1.0),
            n(96.5),
            n(2.6),
            n(2.0),
            n(97.1),
            n(3.1),
            n(2.0),
            n(97.5),
            n(3.3),
            n(4.0),
            n(97.1),
            n(3.5),
            n(5.0),
            n(98.3),
            n(3.8),
            n(6.0),
            n(98.5),
            n(3.9),
        ],
    );
    let out = value(run_last(&[evaluate_stmt(call(
        "vlookup",
        vec![n(1.5), table, n(2.0), text("near")],
    ))]));
    assert_eq!(matrix(&out).shape(), (3, 1));
    assert_eq!(
        (el(&out, 0, 0), el(&out, 1, 0), el(&out, 2, 0)),
        (2.6, 3.1, 3.3)
    );
}

#[test]
fn logpts_refuses_points_that_underflow_to_zero() {
    let out = run_last(&[evaluate_stmt(call(
        "logpts",
        vec![n(-400.0), n(2.0), n(3.0)],
    ))]);
    assert!(matches!(expect_error(out), EvalError::Unsupported(_)));
}

// ---- design of experiments -------------------------------------------------------------------

fn no_files_value(e: Expr) -> Value {
    value(run_last(&[evaluate_stmt(e)]))
}

#[test]
fn fullfact_two_levels_in_nist_order() {
    let v = no_files_value(call("fullfact", vec![n(2.0)]));
    assert_eq!(matrix(&v).shape(), (4, 4));
    // Run, Block, then factor 1 (fastest) and factor 2.
    assert_eq!((el(&v, 0, 2), el(&v, 0, 3)), (-1.0, -1.0));
    assert_eq!((el(&v, 1, 2), el(&v, 1, 3)), (1.0, -1.0));
    assert_eq!((el(&v, 2, 2), el(&v, 2, 3)), (-1.0, 1.0));
    assert_eq!(el(&v, 3, 1), 1.0, "block column");
    assert_eq!(el(&v, 3, 0), 4.0, "run column");
}

#[test]
fn boxwilson_two_factors_matches_the_help_example() {
    // Help: "Example: Second-Order Response Surface", the design X := boxwilson(2).
    let v = no_files_value(call("boxwilson", vec![n(2.0)]));
    assert_eq!(matrix(&v).shape(), (13, 4));
    let alpha = 1.414;
    let expected: [(f64, f64); 13] = [
        (-1.0, -1.0),
        (1.0, -1.0),
        (-1.0, 1.0),
        (1.0, 1.0),
        (0.0, 0.0),
        (0.0, 0.0),
        (0.0, 0.0),
        (0.0, 0.0),
        (0.0, 0.0),
        (alpha, 0.0),
        (-alpha, 0.0),
        (0.0, alpha),
        (0.0, -alpha),
    ];
    for (i, (a, b)) in expected.iter().enumerate() {
        assert!((el(&v, i, 2) - a).abs() < 1e-3, "row {} A", i + 1);
        assert!((el(&v, i, 3) - b).abs() < 1e-3, "row {} B", i + 1);
    }
}

#[test]
fn doelabel_converts_the_help_example_to_real_levels() {
    // Help: D := doelabel(X, Vals); row 10 is t = 92.07, T = 175; row 13 is T = 167.93.
    let x = call("boxwilson", vec![n(2.0)]);
    let vals = mat(
        2,
        5,
        vec![
            n(77.93),
            n(80.0),
            n(85.0),
            n(90.0),
            n(92.07),
            n(167.93),
            n(170.0),
            n(175.0),
            n(180.0),
            n(182.07),
        ],
    );
    let d = no_files_value(call("doelabel", vec![x, vals]));
    assert!((el(&d, 9, 2) - 92.07).abs() < 0.01);
    assert!((el(&d, 10, 2) - 77.93).abs() < 0.01);
    assert!((el(&d, 0, 2) - 80.0).abs() < 0.01);
    assert!((el(&d, 0, 3) - 170.0).abs() < 0.01);
    assert!((el(&d, 12, 3) - 167.93).abs() < 0.01);
    assert!((el(&d, 11, 3) - 182.07).abs() < 0.01);
}

#[test]
fn design_sizes_are_limited_and_unbuilt_designs_say_so() {
    assert!(matches!(
        eval_err(&call("fullfact", vec![n(16.0)])),
        EvalError::TooLarge(_)
    ));
    // A caller's centre-point count is checked before the points are built.
    assert!(matches!(
        eval_err(&call("boxwilson", vec![n(2.0), n(1e12)])),
        EvalError::TooLarge(_)
    ));
    assert!(matches!(
        eval_err(&call("boxwilson", vec![n(5.0)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        eval_err(&call("fullfact", vec![n(2.0), n(3.0)])),
        EvalError::Unsupported(_)
    ));
}

fn eval_err(e: &Expr) -> EvalError {
    match run_last(&[evaluate_stmt(e.clone())]) {
        Outcome::Error(err) => err,
        other => panic!("expected an error, got {other:?}"),
    }
}

// ---- image -----------------------------------------------------------------------------------

fn colour(r: f64, g: f64, b: f64) -> Vec<Expr> {
    vec![n(r), n(g), n(b)]
}

#[test]
fn hsv_of_the_help_primaries() {
    // Help: "Example: HLS and HSV Color Systems", rgb_to_hsv(Red), (Green), (Blue).
    let cases = [
        ([255.0, 0.0, 0.0], [0.0, 255.0, 255.0]),
        ([0.0, 255.0, 0.0], [120.0, 255.0, 255.0]),
        ([0.0, 0.0, 255.0], [240.0, 255.0, 255.0]),
    ];
    for (rgb, hsv) in cases {
        let v = no_files_value(call("rgb_to_hsv", colour(rgb[0], rgb[1], rgb[2])));
        for (k, want) in hsv.iter().enumerate() {
            assert!((el(&v, k, 0) - want).abs() < 1e-9, "{rgb:?} component {k}");
        }
    }
}

#[test]
fn hls_of_the_help_primaries() {
    let v = no_files_value(call("rgb_to_hls", colour(255.0, 0.0, 0.0)));
    assert!((el(&v, 0, 0)).abs() < 1e-9);
    assert!((el(&v, 1, 0) - 127.5).abs() < 1e-9);
    assert!((el(&v, 2, 0) - 255.0).abs() < 1e-9);
    let g = no_files_value(call("rgb_to_hls", colour(0.0, 255.0, 0.0)));
    assert!((el(&g, 0, 0) - 120.0).abs() < 1e-9);
}

#[test]
fn colour_conversions_round_trip() {
    let hsv = no_files_value(call("rgb_to_hsv", colour(0.0, 0.0, 255.0)));
    let back = no_files_value(call(
        "hsv_to_rgb",
        vec![mat(3, 1, (0..3).map(|k| n(el(&hsv, k, 0))).collect())],
    ));
    assert!((el(&back, 2, 0) - 255.0).abs() < 1e-6);
    assert!(el(&back, 0, 0).abs() < 1e-6);
    let hls = no_files_value(call("rgb_to_hls", colour(40.0, 200.0, 90.0)));
    let rgb = no_files_value(call(
        "hls_to_rgb",
        colour(el(&hls, 0, 0), el(&hls, 1, 0), el(&hls, 2, 0)),
    ));
    for (k, want) in [40.0, 200.0, 90.0].iter().enumerate() {
        assert!((el(&rgb, k, 0) - want).abs() < 1e-6, "channel {k}");
    }
}

#[test]
fn packed_image_matrix_converts_plane_by_plane() {
    // One pixel as three planes side by side: red 255, green 0, blue 0.
    let packed = mat(1, 3, vec![n(255.0), n(0.0), n(0.0)]);
    let v = no_files_value(call("rgb_to_hsv", vec![packed]));
    assert_eq!(matrix(&v).shape(), (1, 3));
    assert!((el(&v, 0, 0)).abs() < 1e-9);
    assert!((el(&v, 0, 1) - 255.0).abs() < 1e-9);
    assert!((el(&v, 0, 2) - 255.0).abs() < 1e-9);
}

#[test]
fn invertimage_clip_and_scale_follow_the_help() {
    let m = mat(1, 2, vec![n(0.0), n(255.0)]);
    let inv = no_files_value(call("invertimage", vec![m]));
    assert_eq!((el(&inv, 0, 0), el(&inv, 0, 1)), (255.0, 0.0));
    let big = mat(1, 2, vec![n(0.0), n(300.0)]);
    let inv = no_files_value(call("invertimage", vec![big]));
    assert_eq!((el(&inv, 0, 0), el(&inv, 0, 1)), (300.0, 0.0));

    let m = mat(1, 3, vec![n(-5.0), n(100.0), n(300.0)]);
    let c = no_files_value(call("clip", vec![m.clone()]));
    assert_eq!((el(&c, 0, 0), el(&c, 0, 2)), (0.0, 255.0));
    let c = no_files_value(call("clip", vec![m.clone(), n(10.0), n(20.0)]));
    assert_eq!((el(&c, 0, 1), el(&c, 0, 0)), (20.0, 10.0));

    let s = no_files_value(call("scale", vec![m, n(0.0), n(255.0)]));
    assert_eq!((el(&s, 0, 0), el(&s, 0, 2)), (0.0, 255.0));
    let flat = mat(1, 2, vec![n(7.0), n(7.0)]);
    let s = no_files_value(call("scale", vec![flat, n(0.0), n(255.0)]));
    assert_eq!(el(&s, 0, 1), 0.0, "a constant matrix gives the lower limit");
}

#[test]
fn image_functions_refuse_a_bad_shape() {
    let m = mat(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)]);
    assert!(matches!(
        eval_err(&call("rgb_to_hsv", vec![m])),
        EvalError::Unsupported(_)
    ));
}
