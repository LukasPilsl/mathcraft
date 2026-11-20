//! Unit tests for the region model: hostile input, classification, text and evaluation order.

use std::collections::HashMap;

use super::*;
use crate::package::{make_zip, WORKSHEET, WORKSHEET_RELS};

const HEAD: &str = r#"<worksheet xmlns="http://schemas.mathsoft.com/worksheet50"
    xmlns:ml="http://schemas.mathsoft.com/math50"><regions>"#;
const TAIL: &str = "</regions></worksheet>";

fn package(worksheet: &str) -> Package {
    Package {
        worksheet: worksheet.to_owned(),
        ..Package::default()
    }
}

fn build(worksheet: &str) -> Result<Worksheet, ImportError> {
    Worksheet::from_package(&package(worksheet))
}

fn sheet(body: &str) -> String {
    format!("{HEAD}{body}{TAIL}")
}

const SECTION: &str =
    r#"<Section xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">"#;

/// A `Xaml/Document.xaml` whose body is `body`, inside a `<Section>`.
fn document(body: &str) -> String {
    format!("{SECTION}{body}</Section>")
}

/// A real-shaped `.mcdx` package: the worksheet, its relationships, and one `.XamlPackage` that
/// relationship `rId1` links to. The package holds `xaml` as its `Xaml/Document.xaml`.
fn package_with_flow(worksheet: &str, xaml: &str) -> Package {
    const RELS: &str = r#"<?xml version="1.0" encoding="utf-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/flowDocument" Target="/mathcad/xaml/FlowDocument1.XamlPackage" Id="rId1" /></Relationships>"#;
    let flow = make_zip(&[("Xaml/Document.xaml", xaml.as_bytes())]);
    let bytes = make_zip(&[
        (WORKSHEET, worksheet.as_bytes()),
        (WORKSHEET_RELS, RELS.as_bytes()),
        ("mathcad/xaml/FlowDocument1.XamlPackage", &flow),
    ]);
    Package::from_bytes(&bytes).expect("package opens")
}

const VALID: &str = r#"<region region-id="0" actualWidth="60" actualHeight="25" top="10" left="20">
  <math resultRef="0"><ml:define><ml:id labels="VARIABLE">x</ml:id><ml:real>2</ml:real></ml:define></math>
</region>
<region region-id="1" actualWidth="60" actualHeight="25" top="50" left="20">
  <text item-idref="rId1"/>
</region>
<region region-id="2" actualWidth="60" actualHeight="25" top="90" left="20">
  <math resultRef="1"><ml:eval><ml:id labels="VARIABLE">x</ml:id></ml:eval></math>
</region>"#;

#[test]
fn valid_sheet_classifies_each_region() {
    let xaml = document("<Paragraph><Run>Hello world</Run></Paragraph>");
    let ws = Worksheet::from_package(&package_with_flow(&sheet(VALID), &xaml))
        .expect("valid sheet builds");
    let kinds: Vec<&RegionKind> = ws.regions.iter().map(|r| &r.kind).collect();
    assert!(matches!(kinds[0], RegionKind::Math(Expr::Define { .. })));
    assert_eq!(kinds[1], &RegionKind::Text("Hello world".into()));
    assert!(matches!(kinds[2], RegionKind::Math(Expr::Eval { .. })));
    assert_eq!(ws.regions[0].result_ref, Some(0));
    assert_eq!(ws.regions[0].pos, Some((20.0, 10.0)));
    assert_eq!(ws.regions[0].size, Some((60.0, 25.0)));
    assert_eq!(ws.report.region_count, 3);
    assert!(ws.report.unsupported.is_empty());
}

#[test]
fn every_proper_prefix_of_a_sheet_is_an_error_not_a_panic() {
    let full = sheet(VALID);
    for end in 0..full.len() {
        if !full.is_char_boundary(end) {
            continue;
        }
        let result = build(&full[..end]);
        assert!(result.is_err(), "prefix of length {end} must be an error");
    }
}

#[test]
fn odd_xml_inputs_return_errors_not_panics() {
    let cases: Vec<String> = vec![
        String::new(),
        "not xml at all".to_string(),
        "<worksheet/><worksheet/>".to_string(),
        "<worksheet><regions><region/></regions></worksheet>".to_string(),
        sheet(r#"<region region-id="x" actualWidth="1" actualHeight="1"/>"#),
        sheet(r#"<region region-id="0" actualWidth="1" actualHeight="1" top="abc" left="0"/>"#),
        sheet(
            r#"<region region-id="0" actualWidth="1" actualHeight="1"><math resultRef="-1"/></region>"#,
        ),
        "<foo/>".to_string(),
    ];
    for xml in cases {
        assert!(build(&xml).is_err(), "expected error for {xml:?}");
    }
}

#[test]
fn region_without_content_is_unsupported_and_kept() {
    let ws = build(&sheet(
        r#"<region region-id="5" actualWidth="1" actualHeight="1" top="0" left="0"/>"#,
    ))
    .expect("builds");
    assert_eq!(ws.regions.len(), 1);
    assert_eq!(ws.report.unsupported.len(), 1);
    assert_eq!(ws.report.unsupported[0].region_id, 5);
    assert!(ws.report.unsupported[0].reason.contains("no content"));
}

#[test]
fn region_with_two_contents_is_unsupported() {
    let ws = build(&sheet(
        r#"<region region-id="6" actualWidth="1" actualHeight="1" top="0" left="0"><text>a</text><text>b</text></region>"#,
    ))
    .expect("builds");
    assert!(matches!(
        &ws.regions[0].kind,
        RegionKind::Unsupported { reason } if reason.contains("2 content elements")
    ));
}

#[test]
fn unknown_content_element_is_unsupported_with_its_name() {
    let ws = build(&sheet(
        r#"<region region-id="7" actualWidth="1" actualHeight="1" top="0" left="0"><picture/></region>"#,
    ))
    .expect("builds");
    assert!(matches!(
        &ws.regions[0].kind,
        RegionKind::Unsupported { reason } if reason.contains("<picture>")
    ));
}

#[test]
fn math_that_cannot_convert_is_unsupported_not_dropped() {
    let ws = build(&sheet(
        r#"<region region-id="8" actualWidth="1" actualHeight="1" top="0" left="0"><math><ml:frobnicate/></math></region>"#,
    ))
    .expect("builds");
    assert_eq!(ws.report.region_count, 1);
    assert!(matches!(
        &ws.regions[0].kind,
        RegionKind::Unsupported { reason } if reason.contains("cannot convert math")
    ));
}

#[test]
fn deep_region_nesting_is_an_error_and_bounded_depth_is_accepted() {
    let nest = |levels: usize| {
        let mut xml = String::new();
        for i in 0..levels {
            xml.push_str(&format!(
                r#"<region region-id="{i}" actualWidth="1" actualHeight="1" top="0" left="0"><solveblock><regions>"#
            ));
        }
        for _ in 0..levels {
            xml.push_str("</regions></solveblock></region>");
        }
        sheet(&xml)
    };
    assert!(build(&nest(MAX_REGION_DEPTH)).is_ok());
    assert!(matches!(
        build(&nest(MAX_REGION_DEPTH + 2)),
        Err(ImportError::Nesting)
    ));
}

#[test]
fn solve_block_members_are_children_and_keyword_is_read() {
    let xml = sheet(
        r#"<region region-id="10" actualWidth="1" actualHeight="1" top="100" left="10"><solveblock><regions>
          <region region-id="11" actualWidth="1" actualHeight="1" top="5" left="5" solve-block-category="guess-value"><math><ml:define><ml:id labels="VARIABLE">v</ml:id><ml:real>1</ml:real></ml:define></math></region>
          <region region-id="12" actualWidth="1" actualHeight="1" top="20" left="5" solve-block-category="solver"><math><ml:eval><ml:apply><ml:id labels="KEYWORD">find</ml:id><ml:sequence><ml:id labels="VARIABLE">v</ml:id></ml:sequence></ml:apply></ml:eval></math></region>
        </regions></solveblock></region>"#,
    );
    let ws = build(&xml).expect("builds");
    let block = &ws.regions[0];
    assert!(matches!(
        &block.kind,
        RegionKind::SolveBlock(SolveBlock { keyword: Some(k) }) if k == "find"
    ));
    assert_eq!(block.children.len(), 2);
    assert_eq!(
        block.children[0].solve_category.as_deref(),
        Some("guess-value")
    );
    assert_eq!(block.children[1].solve_category.as_deref(), Some("solver"));
    assert_eq!(ws.report.region_count, 3);
}

#[test]
fn evaluation_order_sorts_rows_then_columns_and_skips_containers() {
    // Tops/lefts: A(10,50) B(10,20) C(5,100) D(none) E(10.00001,5). E shares B's row (0.1 pt).
    let xml = sheet(
        r#"<region region-id="1" actualWidth="1" actualHeight="1" top="10" left="50"><text>a</text></region>
        <region region-id="2" actualWidth="1" actualHeight="1" top="10" left="20"><text>b</text></region>
        <region region-id="3" actualWidth="1" actualHeight="1" top="5" left="100"><text>c</text></region>
        <region region-id="4" actualWidth="1" actualHeight="1"><text>d</text></region>
        <region region-id="5" actualWidth="1" actualHeight="1" top="10.00001" left="5"><text>e</text></region>
        <region region-id="6" actualWidth="1" actualHeight="1" top="200" left="0"><solveblock><regions>
          <region region-id="8" actualWidth="1" actualHeight="1" top="30" left="0" solve-block-category="constraint"><text>m2</text></region>
          <region region-id="7" actualWidth="1" actualHeight="1" top="5" left="0" solve-block-category="guess-value"><text>m1</text></region>
        </regions></solveblock></region>"#,
    );
    let ws = build(&xml).expect("builds");
    let order: Vec<u32> = ws.evaluation_order().iter().map(|r| r.id).collect();
    // Top-level: 3 (row 5), 5 and 2 (row 10, left 5 then 20), 1 (row 10, left 50), 6 is a
    // container whose members follow it: 7 (top 5) then 8 (top 30); 4 has no position, last.
    assert_eq!(order, vec![3, 5, 2, 1, 7, 8, 4]);
}

#[test]
fn evaluation_order_is_stable_for_equal_positions() {
    let xml = sheet(
        r#"<region region-id="1" actualWidth="1" actualHeight="1" top="10" left="10"><text>a</text></region>
        <region region-id="2" actualWidth="1" actualHeight="1" top="10" left="10"><text>b</text></region>"#,
    );
    let ws = build(&xml).expect("builds");
    let order: Vec<u32> = ws.evaluation_order().iter().map(|r| r.id).collect();
    assert_eq!(order, vec![1, 2]);
    let again: Vec<u32> = ws.evaluation_order().iter().map(|r| r.id).collect();
    assert_eq!(order, again);
}

#[test]
fn settings_are_read_and_absent_settings_are_none() {
    let mut pkg = package(&sheet(""));
    pkg.calculation = Some(
        r#"<calculation xmlns="http://schemas.ptc.com/mathcad/settings/calculation10">
          <builtInVariables array-origin="1" convergence-tolerance="0.001" constraint-tolerance="0.01"/>
          <units><currentUnitSystem name="us"/></units></calculation>"#
            .to_owned(),
    );
    let ws = Worksheet::from_package(&pkg).expect("builds");
    assert_eq!(ws.settings.array_origin, Some(1));
    assert_eq!(ws.settings.convergence_tolerance, Some(0.001));
    assert_eq!(ws.settings.constraint_tolerance, Some(0.01));
    assert_eq!(ws.settings.unit_system.as_deref(), Some("us"));

    let bare = build(&sheet("")).expect("builds");
    assert_eq!(bare.settings, Settings::default());
}

#[test]
fn text_region_reads_flow_document_with_umlauts_entities_and_line_breaks() {
    let xaml = document(concat!(
        "<Paragraph><Run>Prüfung über Größe — ß µ °</Run><LineBreak/>",
        "<Run>zweite  Zeile &amp; &lt;x&gt;</Run></Paragraph>\n  ",
        "<Paragraph><Run>Absatz 2</Run></Paragraph>",
        "<Paragraph><Run></Run></Paragraph>",
    ));
    let body = r#"<region region-id="3" actualWidth="60" actualHeight="25" top="10" left="20"><text item-idref="rId1"/></region>"#;
    let ws = Worksheet::from_package(&package_with_flow(&sheet(body), &xaml)).expect("builds");
    assert_eq!(
        ws.regions[0].kind,
        RegionKind::Text("Prüfung über Größe — ß µ °\nzweite  Zeile & <x>\nAbsatz 2".into())
    );
    assert!(ws.report.notes.is_empty());
}

#[test]
fn text_region_with_missing_flow_document_is_empty_with_a_note() {
    let body = r#"<region region-id="4" actualWidth="60" actualHeight="25" top="10" left="20"><text item-idref="rId9"/></region>"#;
    let xaml = document("<Paragraph><Run>not used</Run></Paragraph>");
    // The relationship exists, but rId9 does not.
    let ws = Worksheet::from_package(&package_with_flow(&sheet(body), &xaml)).expect("builds");
    assert_eq!(ws.regions[0].kind, RegionKind::Text("".into()));
    assert_eq!(ws.report.notes.len(), 1);
    assert_eq!(ws.report.notes[0].region_id, 4);
    assert!(ws.report.notes[0].reason.contains("rId9"));

    // No worksheet relationships part at all: the same fallback, no panic.
    let bare = build(&sheet(body)).expect("builds without relationships");
    assert_eq!(bare.regions[0].kind, RegionKind::Text("".into()));
    assert_eq!(bare.report.notes.len(), 1);
}

#[test]
fn malformed_flow_document_is_empty_with_a_note() {
    let body = r#"<region region-id="5" actualWidth="60" actualHeight="25" top="10" left="20"><text item-idref="rId1"/></region>"#;
    let ws = Worksheet::from_package(&package_with_flow(&sheet(body), "<Section><Run>"))
        .expect("a malformed flow document does not fail the import");
    assert_eq!(ws.regions[0].kind, RegionKind::Text("".into()));
    assert_eq!(ws.report.notes.len(), 1);
    assert!(ws.report.notes[0].reason.contains("malformed"));
}

#[test]
fn text_region_never_collects_text_from_nested_regions() {
    // Like sheet 2_rata_2 region 6: an empty FlowDocument and a nested inline math region.
    let body = r#"<region region-id="6" actualWidth="200" actualHeight="25" top="245" left="56"><text item-idref="rId1"><FlowDocument FontFamily="Tahoma"/><regions><region region-id="7" actualWidth="83" actualHeight="25" text-caret-position="1"><math resultRef="5"><ml:eval><ml:id labels="VARIABLE">a</ml:id></ml:eval></math></region></regions></text></region>"#;
    let xaml = document("<Paragraph><Run>Hallo</Run></Paragraph>");
    let ws = Worksheet::from_package(&package_with_flow(&sheet(body), &xaml)).expect("builds");
    let text = &ws.regions[0];
    assert_eq!(text.kind, RegionKind::Text("Hallo".into()));
    assert_eq!(text.children.len(), 1, "inline region is kept as a child");
    assert_eq!(text.children[0].id, 7);
    assert!(matches!(
        text.children[0].kind,
        RegionKind::Math(Expr::Eval { .. })
    ));
    assert_eq!(ws.report.region_count, 2);
    assert!(ws.report.notes.is_empty());
}

#[test]
fn non_finite_positions_and_sizes_are_absent() {
    let body = r#"<region region-id="0" actualWidth="NaN" actualHeight="inf" top="NaN" left="1e400"><text item-idref="rId1"/></region>
        <region region-id="1" actualWidth="5" actualHeight="6" top="inf" left="2"><text item-idref="rId1"/></region>
        <region region-id="2" actualWidth="5" actualHeight="6" top="1" left="2"><text item-idref="rId1"/></region>"#;
    let xaml = document("<Paragraph><Run>x</Run></Paragraph>");
    let ws = Worksheet::from_package(&package_with_flow(&sheet(body), &xaml)).expect("builds");
    assert_eq!(ws.regions[0].pos, None);
    assert_eq!(ws.regions[0].size, None);
    assert_eq!(ws.regions[1].pos, None, "infinite top is absent");
    assert_eq!(ws.regions[1].size, Some((5.0, 6.0)));
    assert_eq!(ws.regions[2].pos, Some((2.0, 1.0)));
    // Absent positions sort after positioned regions, in document order.
    let order: Vec<u32> = ws.evaluation_order().iter().map(|r| r.id).collect();
    assert_eq!(order, vec![2, 0, 1]);
}

#[test]
fn evaluation_rows_are_compared_at_tenth_of_a_point() {
    // 100.04 and 100.0000001 both round to 100.0, so ids 2 and 3 share a row and sort by left.
    // 100.06 rounds to 100.1, so id 1 sorts after the row even though it is only 0.02 pt away.
    let body = r#"<region region-id="1" actualWidth="1" actualHeight="1" top="100.06" left="0"><text item-idref="rId1"/></region>
        <region region-id="2" actualWidth="1" actualHeight="1" top="100.04" left="50"><text item-idref="rId1"/></region>
        <region region-id="3" actualWidth="1" actualHeight="1" top="100.0000001" left="10"><text item-idref="rId1"/></region>"#;
    let xaml = document("<Paragraph><Run>x</Run></Paragraph>");
    let ws = Worksheet::from_package(&package_with_flow(&sheet(body), &xaml)).expect("builds");
    let order: Vec<u32> = ws.evaluation_order().iter().map(|r| r.id).collect();
    assert_eq!(order, vec![3, 2, 1]);
}

#[test]
fn malformed_settings_attribute_fails_the_import() {
    let mut pkg = package(&sheet(""));
    pkg.calculation = Some(
        r#"<calculation xmlns="http://schemas.ptc.com/mathcad/settings/calculation10">
          <builtInVariables array-origin="x"/></calculation>"#
            .to_owned(),
    );
    let err = Worksheet::from_package(&pkg).expect_err("a malformed setting is an error");
    assert!(matches!(
        err,
        ImportError::Dom(DomError::BadAttr {
            attr: "array-origin",
            ..
        })
    ));
}

#[test]
fn malformed_result_xml_does_not_fail_the_import() {
    let mut pkg = package(&sheet(""));
    pkg.result = Some("<resultsList><broken".to_owned());
    assert!(
        Worksheet::from_package(&pkg).is_ok(),
        "result.xml is not parsed by the worksheet import"
    );
}

/// Regions that all name one 3 MiB flow document share one text allocation, and the distinct
/// text stays within [`MAX_TEXT_TOTAL`].
#[test]
fn regions_sharing_one_flow_document_share_one_text_allocation() {
    const MIB: usize = 1024 * 1024;
    const REGIONS: usize = 1000;
    let big = "a".repeat(3 * MIB);
    let xaml = document(&format!("<Paragraph><Run>{big}</Run></Paragraph>"));
    let mut body = String::new();
    for i in 0..REGIONS {
        body.push_str(&format!(
            r#"<region region-id="{i}" actualWidth="1" actualHeight="1" top="{i}" left="0"><text item-idref="rId1"/></region>"#
        ));
    }
    let ws = Worksheet::from_package(&package_with_flow(&sheet(&body), &xaml))
        .expect("1000 regions over one flow document build");
    let texts: Vec<&TextBody> = ws
        .regions
        .iter()
        .map(|r| match &r.kind {
            RegionKind::Text(text) => text,
            other => panic!("expected text, got {other:?}"),
        })
        .collect();
    assert_eq!(texts.len(), REGIONS);
    assert!(
        texts.iter().all(|t| t.same_allocation(texts[0])),
        "one allocation"
    );
    assert_eq!(texts[0].len(), 3 * MIB);
    // Total stored text: each distinct allocation once.
    let mut distinct: HashMap<usize, usize> = HashMap::new();
    for t in &texts {
        distinct.insert(t.as_ptr() as usize, t.len());
    }
    let stored: usize = distinct.values().sum();
    assert!(stored <= MAX_TEXT_TOTAL, "stored text {stored} bytes");
    assert!(ws.report.notes.is_empty());
}

/// `count` empty regions: each is an `Unsupported` region with no content, which is enough to
/// count against [`MAX_REGIONS`] without any other work.
fn empty_regions(count: usize) -> String {
    let mut body = String::with_capacity(count * 60);
    for i in 0..count {
        body.push_str(&format!(
            r#"<region region-id="{i}" actualWidth="1" actualHeight="1"/>"#
        ));
    }
    sheet(&body)
}

#[test]
fn exactly_max_regions_is_accepted_and_one_more_is_rejected() {
    let ws = build(&empty_regions(MAX_REGIONS)).expect("MAX_REGIONS regions are accepted");
    assert_eq!(ws.report.region_count, MAX_REGIONS);
    assert!(matches!(
        build(&empty_regions(MAX_REGIONS + 1)),
        Err(ImportError::TooManyRegions { limit }) if limit == MAX_REGIONS
    ));
}

/// Nested regions count too: one outer solve block with its members, in total at the cap or one
/// over it.
#[test]
fn nested_regions_count_towards_max_regions() {
    let nested = |members: usize| {
        let mut body = String::new();
        for i in 0..members {
            body.push_str(&format!(
                r#"<region region-id="{}" actualWidth="1" actualHeight="1"/>"#,
                i + 1
            ));
        }
        sheet(&format!(
            r#"<region region-id="0" actualWidth="1" actualHeight="1"><solveblock><regions>{body}</regions></solveblock></region>"#
        ))
    };
    let ws = build(&nested(MAX_REGIONS - 1)).expect("outer plus members at the cap is accepted");
    assert_eq!(ws.report.region_count, MAX_REGIONS);
    assert!(matches!(
        build(&nested(MAX_REGIONS)),
        Err(ImportError::TooManyRegions { .. })
    ));
}
