use super::*;

#[test]
fn the_stem_keeps_only_ascii_letters_digits_dash_and_underscore() {
    assert_eq!(safe_stem("x&calc"), "xcalc");
    assert_eq!(safe_stem("a b"), "ab");
    assert_eq!(safe_stem("../../etc"), "etc");
    assert_eq!(safe_stem("Ünïcode_1-2"), "ncode_1-2");
    assert_eq!(safe_stem("plain-Name_9"), "plain-Name_9");
}

#[test]
fn an_empty_or_unusable_stem_falls_back_to_worksheet() {
    assert_eq!(safe_stem(""), "worksheet");
    assert_eq!(safe_stem("日本語"), "worksheet");
    assert_eq!(safe_stem("&^%"), "worksheet");
}

#[test]
fn a_long_stem_is_cut_to_the_limit() {
    let long = "a".repeat(200);
    assert_eq!(safe_stem(&long).len(), MAX_STEM);
}

#[test]
fn print_files_get_distinct_names_and_are_removed() {
    let first = write_print_file("unit-test", b"%PDF-1").expect("first file");
    let second = write_print_file("unit-test", b"%PDF-2").expect("second file");
    assert_ne!(first, second);
    assert_eq!(std::fs::read(&first).expect("read"), b"%PDF-1");
    assert!(first
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("mathcraft-unit-test-") && n.ends_with(".pdf")));
    remove_print_files();
    assert!(!first.exists());
    assert!(!second.exists());
}
