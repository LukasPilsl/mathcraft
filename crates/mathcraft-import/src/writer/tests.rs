//! Unit tests of the writer: expressions, regions, text, settings, preserved parts and the
//! generated package. Every check reads the written bytes back with the reader.

use std::collections::BTreeMap;
use std::io::Read;

use mathcraft_core::expr::{BinaryOp, Command, Expr, Id, IdKind, Number, UnaryOp};
use zip::ZipArchive;

use crate::dom;
use crate::expr::{convert_math, MAX_DEPTH};
use crate::package::{make_zip, Package};
use crate::regions::{
    AreaModel, ImageModel, ImportReport, RegionKind, RegionModel, Settings, SolveBlock, Worksheet,
    MAX_REGION_DEPTH,
};
use crate::results::parse_results;
use crate::writer::expr::write_math;
use crate::writer::{write_package, WriteError};

fn var(name: &str) -> Expr {
    Expr::Id(Id::new(name, IdKind::Variable))
}

fn unit(name: &str) -> Expr {
    Expr::Id(Id::new(name, IdKind::Unit))
}

fn real(text: &str) -> Expr {
    Expr::Number(Number::Real {
        value: text.parse().expect("test number"),
        text: text.to_owned(),
    })
}

fn region(id: u32, kind: RegionKind) -> RegionModel {
    RegionModel {
        id,
        pos: None,
        size: None,
        result_ref: None,
        kind,
        solve_category: None,
        children: Vec::new(),
        result_format: None,
        matrix: Default::default(),
    }
}

fn math(id: u32, expr: Expr) -> RegionModel {
    let mut r = region(id, RegionKind::Math(expr));
    r.result_ref = Some(id);
    r.pos = Some((18.9, 37.8 + f64::from(id)));
    r.size = Some((60.04, 25.6));
    r
}

fn text(id: u32, text: &str) -> RegionModel {
    let mut r = region(id, RegionKind::Text(text.into()));
    r.pos = Some((18.9, 200.0 + f64::from(id)));
    r.size = Some((200.0, 30.0));
    r
}

fn sheet(regions: Vec<RegionModel>, settings: Settings) -> Worksheet {
    Worksheet {
        regions,
        settings,
        page: Default::default(),
        report: ImportReport::default(),
        header: Vec::new(),
        footer: Vec::new(),
    }
}

fn full_settings() -> Settings {
    Settings {
        array_origin: Some(0),
        convergence_tolerance: Some(0.001),
        constraint_tolerance: Some(0.001),
        unit_system: Some("si".to_owned()),
        // A file written from scratch always states PRNPRECISION (4, Prime default).
        prn_precision: Some(4),
        result_format: None,
    }
}

/// Reads a written package and returns its model, as the reader sees it.
fn reread(bytes: &[u8]) -> Worksheet {
    let pkg = Package::from_bytes(bytes).expect("written package reads");
    Worksheet::from_package(&pkg).expect("written worksheet models")
}

/// Every entry of a ZIP as name to contents.
fn entries(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut out = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("entry");
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read entry");
        let name = entry.name().expect("entry name").into_owned();
        out.insert(name, data);
    }
    out
}

/// Converts the XML of one expression back, as the reader does for a `<math>` element.
fn read_expr(xml: &str) -> Expr {
    let wrapped = format!(
        r#"<math xmlns="http://schemas.mathsoft.com/worksheet50" xmlns:ml="http://schemas.mathsoft.com/math50">{xml}</math>"#
    );
    let doc = dom::parse(&wrapped).expect("written XML is well-formed");
    convert_math(doc.root_element()).expect("written XML converts")
}

fn round_trip(expr: &Expr) {
    let xml = write_math(expr).expect("writes");
    assert_eq!(&read_expr(&xml), expr, "round trip of {xml}");
}

#[test]
fn every_written_shape_reads_back_equal() {
    let x = var("x");
    let two = real("2");
    let cases = vec![
        real("2.50"),
        real("-3"),
        Expr::Id(Id {
            name: "v".into(),
            kind: IdKind::Variable,
            subscript: Some(Box::new(real("02"))),
        }),
        Expr::Id(Id {
            name: "v".into(),
            kind: IdKind::Variable,
            subscript: Some(Box::new(var("max"))),
        }),
        Expr::unary(UnaryOp::Neg, x.clone()),
        Expr::unary(UnaryOp::Abs, x.clone()),
        Expr::unary(UnaryOp::Transpose, x.clone()),
        Expr::binary(BinaryOp::Add, x.clone(), two.clone()),
        Expr::binary(BinaryOp::Sub, x.clone(), two.clone()),
        Expr::binary(BinaryOp::Mul, x.clone(), two.clone()),
        Expr::binary(BinaryOp::Div, x.clone(), two.clone()),
        Expr::binary(BinaryOp::Pow, x.clone(), two.clone()),
        Expr::binary(BinaryOp::Eq, x.clone(), two.clone()),
        Expr::binary(BinaryOp::Gt, x.clone(), two.clone()),
        Expr::binary(BinaryOp::Lt, x.clone(), two.clone()),
        Expr::Call {
            func: Id::new("sin", IdKind::Function),
            args: vec![x.clone()],
        },
        Expr::Call {
            func: Id::new("rand", IdKind::Function),
            args: vec![],
        },
        Expr::Call {
            func: Id::new("max", IdKind::Function),
            args: vec![x.clone(), two.clone(), var("y")],
        },
        Expr::Range {
            from: Box::new(real("1")),
            step: Some(Box::new(real("2"))),
            to: Box::new(real("10")),
        },
        Expr::Range {
            from: Box::new(real("1")),
            step: None,
            to: Box::new(real("10")),
        },
        Expr::Matrix {
            rows: 2,
            cols: 2,
            elements: vec![real("1"), real("0"), real("0"), real("1")],
        },
        Expr::Index {
            base: Box::new(var("M")),
            indices: vec![real("1")],
        },
        Expr::Index {
            base: Box::new(var("M")),
            indices: vec![real("1"), var("j")],
        },
        Expr::NthRoot {
            degree: None,
            radicand: Box::new(x.clone()),
        },
        Expr::NthRoot {
            degree: Some(Box::new(real("3"))),
            radicand: Box::new(x.clone()),
        },
        Expr::Parens(Box::new(Expr::binary(
            BinaryOp::Add,
            x.clone(),
            two.clone(),
        ))),
        Expr::Define {
            target: Box::new(Expr::Call {
                func: Id::new("f", IdKind::Function),
                args: vec![var("t")],
            }),
            expr: Box::new(Expr::binary(BinaryOp::Mul, x.clone(), var("t"))),
        },
        Expr::Define {
            target: Box::new(var("a")),
            expr: Box::new(Expr::Scale {
                value: Box::new(real("40")),
                unit: Box::new(unit("deg")),
            }),
        },
        Expr::Eval {
            expr: Box::new(x.clone()),
            unit_override: None,
        },
        Expr::Eval {
            expr: Box::new(x.clone()),
            unit_override: Some(Box::new(unit("m"))),
        },
        Expr::SymEval {
            expr: Box::new(var("e")),
            command: None,
            result: None,
        },
        Expr::SymEval {
            expr: Box::new(var("e")),
            command: Some(Command(vec![Expr::Id(Id::new("solve", IdKind::Function))])),
            result: Some(Box::new(var("x"))),
        },
        Expr::SymEval {
            expr: Box::new(Expr::SymEval {
                expr: Box::new(var("e")),
                command: Some(Command(vec![Expr::Id(Id::new("solve", IdKind::Function))])),
                result: None,
            }),
            command: Some(Command(vec![var("x")])),
            result: Some(Box::new(var("y"))),
        },
        Expr::SymEval {
            expr: Box::new(var("e")),
            command: Some(Command(vec![var("factor"), var("x"), real("2")])),
            result: None,
        },
        Expr::Scale {
            value: Box::new(real("6")),
            unit: Box::new(Expr::binary(BinaryOp::Pow, var("x"), real("3"))),
        },
        Expr::Scale {
            value: Box::new(real("0.75")),
            unit: Box::new(unit("m")),
        },
        Expr::Placeholder,
    ];
    for case in &cases {
        round_trip(case);
    }
}

#[test]
fn unrepresentable_expressions_are_refused() {
    let refused = [
        Expr::GlobalDefine {
            target: Box::new(var("a")),
            expr: Box::new(real("1")),
        },
        Expr::binary(BinaryOp::Ne, var("a"), real("1")),
        Expr::binary(BinaryOp::Le, var("a"), real("1")),
        Expr::unary(UnaryOp::Factorial, var("a")),
        // Operators whose stored form no sample shows (UNVERIFIED_FORMATS.md): refused, never guessed.
        Expr::unary(UnaryOp::Norm, var("a")),
        Expr::binary(BinaryOp::Cross, var("a"), var("b")),
        Expr::Gradient {
            vars: vec![],
            body: Box::new(Expr::call("f", vec![var("a")])),
        },
        Expr::Derivative {
            var: Box::new(Id {
                name: "x".into(),
                kind: IdKind::Variable,
                subscript: None,
            }),
            order: None,
            body: Box::new(var("x")),
            partial: true,
        },
        Expr::Matrix {
            rows: 2,
            cols: 2,
            elements: vec![real("1")],
        },
        Expr::Scale {
            value: Box::new(var("a")),
            unit: Box::new(real("2")),
        },
        Expr::NthRoot {
            degree: Some(Box::new(Expr::Placeholder)),
            radicand: Box::new(var("a")),
        },
        Expr::Eval {
            expr: Box::new(var("a")),
            unit_override: Some(Box::new(Expr::Placeholder)),
        },
        Expr::SymEval {
            expr: Box::new(var("a")),
            command: Some(Command(vec![Expr::Placeholder])),
            result: None,
        },
        Expr::Id(Id {
            name: " a".into(),
            kind: IdKind::Variable,
            subscript: None,
        }),
        Expr::Id(Id {
            name: "v".into(),
            kind: IdKind::Variable,
            subscript: Some(Box::new(Expr::Number(Number::Real {
                value: 1.5,
                text: "1,5".into(),
            }))),
        }),
    ];
    for expr in &refused {
        assert!(write_math(expr).is_err(), "must refuse {expr:?}");
    }
    let bad_real = Expr::Number(Number::Real {
        value: 2.0,
        text: "2.5".into(),
    });
    assert!(matches!(write_math(&bad_real), Err(WriteError::Invalid(_))));
}

#[test]
fn regions_round_trip_from_scratch() {
    let mut block_member_def = math(
        4,
        Expr::Define {
            target: Box::new(var("x")),
            expr: Box::new(real("1")),
        },
    );
    block_member_def.solve_category = Some("guess-value".to_owned());
    let mut block_member_constraint = math(5, Expr::binary(BinaryOp::Gt, var("x"), real("0")));
    block_member_constraint.solve_category = Some("constraint".to_owned());
    let block = RegionModel {
        children: vec![block_member_def, block_member_constraint],
        result_format: None,
        matrix: Default::default(),
        ..region(3, RegionKind::SolveBlock(SolveBlock { keyword: None }))
    };
    let no_position = math(
        6,
        Expr::Matrix {
            rows: 1,
            cols: 2,
            elements: vec![
                Expr::Scale {
                    value: Box::new(real("45")),
                    unit: Box::new(unit("deg")),
                },
                var("y"),
            ],
        },
    );
    let mut without_position = no_position;
    without_position.pos = None;
    without_position.size = None;
    let model = sheet(
        vec![
            math(
                0,
                Expr::Define {
                    target: Box::new(var("a")),
                    expr: Box::new(Expr::Scale {
                        value: Box::new(real("3")),
                        unit: Box::new(unit("m")),
                    }),
                },
            ),
            math(
                1,
                Expr::Eval {
                    expr: Box::new(Expr::binary(BinaryOp::Mul, var("a"), real("2"))),
                    unit_override: Some(Box::new(unit("kg"))),
                },
            ),
            text(2, "first line\nsecond\n"),
            block,
            without_position,
        ],
        full_settings(),
    );
    let bytes = write_package(&model, None).expect("writes from scratch");
    let back = reread(&bytes);
    assert_eq!(back.regions, model.regions, "regions round trip");
    assert_eq!(back.settings, model.settings, "settings round trip");
}

#[test]
fn solve_block_keyword_is_written_on_its_member_and_read_back() {
    let guess = math(
        1,
        Expr::Define {
            target: Box::new(var("x")),
            expr: Box::new(real("1")),
        },
    );
    let find = math(
        2,
        Expr::Eval {
            expr: Box::new(Expr::Call {
                func: Id::new("find", IdKind::Function),
                args: vec![var("x")],
            }),
            unit_override: None,
        },
    );
    let block = RegionModel {
        children: vec![guess, find],
        result_format: None,
        matrix: Default::default(),
        ..region(
            0,
            RegionKind::SolveBlock(SolveBlock {
                keyword: Some("find".to_owned()),
            }),
        )
    };
    let model = sheet(vec![block], full_settings());
    let bytes = write_package(&model, None).expect("writes");
    let back = reread(&bytes);
    assert_eq!(back.regions, model.regions);
}

#[test]
fn keyword_absent_from_members_is_refused() {
    let block = RegionModel {
        children: vec![math(1, var("x"))],
        result_format: None,
        matrix: Default::default(),
        ..region(
            0,
            RegionKind::SolveBlock(SolveBlock {
                keyword: Some("minerr".to_owned()),
            }),
        )
    };
    let model = sheet(vec![block], full_settings());
    assert!(matches!(
        write_package(&model, None),
        Err(WriteError::Unsupported(msg)) if msg.contains("minerr")
    ));
}

#[test]
fn text_round_trips_through_the_reader() {
    let texts = [
        "",
        "\n",
        "\n\n",
        "a",
        "a\nb",
        "a\n",
        "a\n\n",
        "a\n\nb\n",
        "  spaced  ",
        "x & <y> \"z\"",
        "tab\there\r\nwindows",
        "ünïcödé ∞ 𝔸",
    ];
    for (i, body) in texts.iter().enumerate() {
        let model = sheet(
            vec![text(u32::try_from(i).expect("small"), body)],
            full_settings(),
        );
        let bytes = write_package(&model, None).expect("writes");
        let back = reread(&bytes);
        assert_eq!(back.regions, model.regions, "text {body:?}");
    }
}

/// A text region at the top of the page, for a header or footer.
fn furniture_text(id: u32, body: &str, top: f64) -> RegionModel {
    let mut r = text(id, body);
    r.pos = Some((18.9, top));
    r.size = Some((400.0, 14.0));
    r
}

#[test]
fn header_footer_and_page_break_round_trip_from_scratch() {
    let mut page_break = region(2, RegionKind::PageBreak);
    page_break.pos = Some((18.9, 300.0));
    let mut model = sheet(vec![text(1, "body"), page_break], full_settings());
    model.header = vec![furniture_text(10, "Page {page} of {pages}", 10.0)];
    model.footer = vec![furniture_text(11, "Beam {filename}", 780.0)];
    let bytes = write_package(&model, None).expect("writes");
    let back = reread(&bytes);
    assert_eq!(back.regions, model.regions);
    assert_eq!(back.header, model.header);
    assert_eq!(back.footer, model.footer);
}

#[test]
fn header_with_a_region_that_is_not_text_is_refused() {
    let mut model = sheet(Vec::new(), full_settings());
    model.header = vec![region(10, RegionKind::PageBreak)];
    assert!(write_package(&model, None).is_err());
}

#[test]
fn changed_header_is_written_and_other_parts_are_kept() {
    let source = source_package();
    let mut model =
        Worksheet::from_package(&Package::from_bytes(&source).expect("reads")).expect("models");
    model.header = vec![furniture_text(10, "Header", 10.0)];
    let out = write_package(&model, Some(&source)).expect("writes");
    assert_eq!(reread(&out).header, model.header);
    let before = entries(&source);
    let after = entries(&out);
    for name in before.keys() {
        if matches!(
            name.as_str(),
            "mathcad/worksheet.xml" | "mathcad/_rels/worksheet.xml.rels" | "mathcad/header.xml"
        ) {
            continue;
        }
        assert_eq!(
            after.get(name),
            before.get(name),
            "part `{name}` must be copied unchanged"
        );
    }
}

#[test]
fn output_is_deterministic() {
    let model = sheet(
        vec![math(0, var("a")), text(1, "hello\nworld")],
        full_settings(),
    );
    let first = write_package(&model, None).expect("writes");
    let second = write_package(&model, None).expect("writes");
    assert_eq!(first, second);
}

#[test]
fn empty_worksheet_writes_and_reads() {
    let model = sheet(Vec::new(), Settings::default());
    let bytes = write_package(&model, None).expect("empty sheet writes");
    assert!(
        bytes.len() < 16 * 1024,
        "empty package is {} bytes",
        bytes.len()
    );
    let back = reread(&bytes);
    assert!(back.regions.is_empty());
}

#[test]
fn duplicate_region_ids_are_refused() {
    let model = sheet(vec![math(0, var("a")), math(0, var("b"))], full_settings());
    assert!(matches!(
        write_package(&model, None),
        Err(WriteError::Invalid(msg)) if msg.contains("more than once")
    ));
}

#[test]
fn plot_without_source_is_refused() {
    let model = sheet(
        vec![region(
            0,
            RegionKind::Plot(crate::regions::PlotInfo {
                kind: "xyPlot".into(),
                traces: vec![],
                x_range: None,
                y_range: None,
                plot: None,
                plot3d: None,
            }),
        )],
        full_settings(),
    );
    assert!(matches!(
        write_package(&model, None),
        Err(WriteError::Unsupported(_))
    ));
}

/// A source package that holds parts the model does not read: an image, an unknown XML part, a
/// text region with its XamlPackage, a plot, and a region attribute the model does not read.
/// A presentation part with an attribute and a child that the page model does not read.
const PRESENTATION_SRC: &str = concat!(
    r#"<presentation xmlns="http://schemas.ptc.com/mathcad/settings/presentation10">"#,
    r#"<pageModel paper-code="A4" grid-size="Standard" orientation="Portrait" "#,
    r#"margin-type="Standard" kept-attr="yes" />"#,
    r#"<mathRendering kept-child="yes" /></presentation>"#
);

fn source_package() -> Vec<u8> {
    let worksheet = concat!(
        r#"<worksheet xmlns="http://schemas.mathsoft.com/worksheet50" "#,
        r#"xmlns:ml="http://schemas.mathsoft.com/math50" "#,
        r#"xmlns:u="http://schemas.mathsoft.com/units10" "#,
        r#"xmlns:ws="http://schemas.mathsoft.com/worksheet50">"#,
        r#"<regions>"#,
        r#"<region region-id="0" actualWidth="60" actualHeight="20" top="10" left="20" width="99">"#,
        r#"<math resultRef="0"><ml:define><ml:id labels="VARIABLE" xml:space="preserve">a</ml:id>"#,
        r#"<ml:real>3</ml:real></ml:define></math></region>"#,
        r#"<region region-id="1" actualWidth="80" actualHeight="20" top="40" left="20">"#,
        r#"<math resultRef="1"><ml:eval><ml:apply><ml:mult/><ml:id labels="VARIABLE">a</ml:id>"#,
        r#"<ml:real>2</ml:real></ml:apply><ml:unitOverride><ml:placeholder/></ml:unitOverride>"#,
        r#"</ml:eval></math></region>"#,
        r#"<region region-id="2" actualWidth="200" actualHeight="30" top="70" left="20">"#,
        r#"<text item-idref="Rtext1"><FlowDocument xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" FontFamily="Tahoma" /></text></region>"#,
        r#"<region region-id="3" actualWidth="50" actualHeight="20" top="110" left="20">"#,
        r#"<plot><unknownPlot><keep>kept</keep></unknownPlot></plot></region>"#,
        r#"</regions></worksheet>"#
    );
    let rels = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/flowDocument" Target="/mathcad/xaml/Keep.XamlPackage" Id="Rtext1" />"#,
        r#"<Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="/mathcad/images/logo.png" Id="Rimg" />"#,
        r#"</Relationships>"#
    );
    let xaml = make_zip(&[(
        "Xaml/Document.xaml",
        br#"<Section xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"><Paragraph><Run>kept text</Run></Paragraph></Section>"#,
    )]);
    let calculation = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<calculation xmlns="http://schemas.ptc.com/mathcad/settings/calculation10">"#,
        r#"<builtInVariables array-origin="0" convergence-tolerance="0.001" "#,
        r#"constraint-tolerance="0.001" prn-precision="4" random-seed="1" />"#,
        r#"<calculationBehavior automatic-recalculation="true" />"#,
        r#"<units><currentUnitSystem name="si" /></units></calculation>"#
    );
    let result = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<resultsList xmlns:ml="http://schemas.mathsoft.com/math50" "#,
        r#"xmlns:u="http://schemas.mathsoft.com/units10" "#,
        r#"xmlns="http://schemas.mathsoft.com/result10">"#,
        r#"<resultData result-id="0" calculation-status="Synchronized" />"#,
        r#"<resultData result-id="1" calculation-status="Synchronized" />"#,
        r#"</resultsList>"#
    );
    make_zip(&[
        ("[Content_Types].xml", br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml" /><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml" /><Default Extension="XamlPackage" ContentType="application/zip" /></Types>"#),
        ("_rels/.rels", b"<Relationships/>"),
        ("docProps/core.xml", b"<coreProperties/>"),
        ("mathcad/worksheet.xml", worksheet.as_bytes()),
        ("mathcad/_rels/worksheet.xml.rels", rels.as_bytes()),
        ("mathcad/xaml/Keep.XamlPackage", &xaml),
        ("mathcad/images/logo.png", &[0x89, b'P', b'N', b'G', 0xFF, 0x00, 0x7F]),
        ("mathcad/extra/unknown.xml", b"<unknown attr='kept'/>"),
        ("mathcad/settings/calculation.xml", calculation.as_bytes()),
        (
            "mathcad/settings/presentation.xml",
            PRESENTATION_SRC.as_bytes(),
        ),
        ("mathcad/result.xml", result.as_bytes()),
        (
            "mathcad/header.xml",
            br#"<header xmlns="http://schemas.mathsoft.com/worksheet50"><regions /></header>"#,
        ),
    ])
}

#[test]
fn unchanged_source_round_trips_with_every_other_part_identical() {
    let source = source_package();
    let model =
        Worksheet::from_package(&Package::from_bytes(&source).expect("reads")).expect("models");
    let out = write_package(&model, Some(&source)).expect("writes");
    let before = entries(&source);
    let after = entries(&out);
    for name in before.keys() {
        if name == "mathcad/worksheet.xml" || name == "mathcad/_rels/worksheet.xml.rels" {
            continue;
        }
        assert_eq!(
            after.get(name),
            before.get(name),
            "part `{name}` must be copied unchanged"
        );
    }
    assert_eq!(reread(&out).regions, model.regions);
    assert_eq!(reread(&out).settings, model.settings);
}

#[test]
fn changed_region_clears_results_and_keeps_other_parts() {
    let source = source_package();
    let mut model =
        Worksheet::from_package(&Package::from_bytes(&source).expect("reads")).expect("models");
    model.regions[0] = math(
        0,
        Expr::Define {
            target: Box::new(var("a")),
            expr: Box::new(real("5")),
        },
    );
    let out = write_package(&model, Some(&source)).expect("writes");
    let after = entries(&out);
    let before = entries(&source);
    assert_eq!(
        after["mathcad/xaml/Keep.XamlPackage"],
        before["mathcad/xaml/Keep.XamlPackage"]
    );
    assert_eq!(
        after["mathcad/images/logo.png"],
        before["mathcad/images/logo.png"]
    );
    assert_eq!(
        after["mathcad/extra/unknown.xml"],
        before["mathcad/extra/unknown.xml"]
    );
    let written = Package::from_bytes(&out).expect("reads");
    let table =
        parse_results(written.result.as_deref().expect("result part")).expect("results parse");
    assert!(
        table.by_id.is_empty(),
        "stored results are dropped after a change"
    );
    assert_eq!(reread(&out).regions, model.regions);
}

#[test]
fn changed_page_is_patched_in_place_and_keeps_unknown_content() {
    let source = source_package();
    let mut model =
        Worksheet::from_package(&Package::from_bytes(&source).expect("reads")).expect("models");
    assert_eq!(model.page.paper_code, "A4");
    model.page.paper_code = "Letter".to_owned();
    model.page.orientation = "Landscape".to_owned();
    let out = write_package(&model, Some(&source)).expect("writes");
    let pres = String::from_utf8(entries(&out)["mathcad/settings/presentation.xml"].clone())
        .expect("utf8");
    assert!(pres.contains(r#"paper-code="Letter""#), "{pres}");
    assert!(pres.contains(r#"orientation="Landscape""#), "{pres}");
    assert!(pres.contains(r#"grid-size="Standard""#), "{pres}");
    assert!(pres.contains(r#"kept-attr="yes""#), "{pres}");
    assert!(
        pres.contains(r#"<mathRendering kept-child="yes""#),
        "{pres}"
    );
    assert_eq!(reread(&out).page, model.page);
}

#[test]
fn page_settings_without_a_source_write_a_presentation_part() {
    let mut model = sheet(Vec::new(), Settings::default());
    model.page.paper_code = "Legal".to_owned();
    model.page.margin_type = "Narrow".to_owned();
    let bytes = write_package(&model, None).expect("writes");
    assert_eq!(reread(&bytes).page, model.page);
}

#[test]
fn unchanged_page_leaves_the_presentation_part_byte_identical() {
    let source = source_package();
    let model =
        Worksheet::from_package(&Package::from_bytes(&source).expect("reads")).expect("models");
    let out = write_package(&model, Some(&source)).expect("writes");
    assert_eq!(
        entries(&out)["mathcad/settings/presentation.xml"],
        entries(&source)["mathcad/settings/presentation.xml"]
    );
}

#[test]
fn changed_setting_is_patched_in_place() {
    let source = source_package();
    let mut model =
        Worksheet::from_package(&Package::from_bytes(&source).expect("reads")).expect("models");
    model.settings.array_origin = Some(1);
    let out = write_package(&model, Some(&source)).expect("writes");
    let calc =
        String::from_utf8(entries(&out)["mathcad/settings/calculation.xml"].clone()).expect("utf8");
    assert!(calc.contains(r#"array-origin="1""#), "{calc}");
    assert!(
        calc.contains(r#"random-seed="1""#),
        "other settings are kept: {calc}"
    );
    assert!(calc.contains(r#"calculationBehavior automatic-recalculation="true""#));
    assert_eq!(reread(&out).settings, model.settings);
}

#[test]
fn changed_text_gets_a_new_part_and_the_old_one_stays() {
    let source = source_package();
    let mut model =
        Worksheet::from_package(&Package::from_bytes(&source).expect("reads")).expect("models");
    model.regions[2].kind = RegionKind::Text("new\nline".into());
    let out = write_package(&model, Some(&source)).expect("writes");
    let after = entries(&out);
    let before = entries(&source);
    assert_eq!(
        after["mathcad/xaml/Keep.XamlPackage"],
        before["mathcad/xaml/Keep.XamlPackage"]
    );
    assert!(after.contains_key("mathcad/xaml/FlowDocument1.XamlPackage"));
    assert_eq!(reread(&out).regions, model.regions);
}

#[test]
fn source_with_unchanged_text_reuses_its_part() {
    let source = source_package();
    let model =
        Worksheet::from_package(&Package::from_bytes(&source).expect("reads")).expect("models");
    let out = write_package(&model, Some(&source)).expect("writes");
    let after = entries(&out);
    assert!(
        !after.keys().any(|k| k.contains("FlowDocument")),
        "no new part for unchanged text"
    );
    let pkg = Package::from_bytes(&out).expect("reads");
    assert_eq!(
        pkg.flow_document("Rtext1").map(|t| t.contains("kept text")),
        Some(true)
    );
}

#[test]
fn generated_package_has_the_package_level_parts() {
    let model = sheet(vec![math(0, var("a"))], full_settings());
    let bytes = write_package(&model, None).expect("writes");
    let names: Vec<String> = entries(&bytes).keys().cloned().collect();
    for part in [
        "[Content_Types].xml",
        "_rels/.rels",
        "mathcad/worksheet.xml",
        "mathcad/_rels/worksheet.xml.rels",
        "mathcad/result.xml",
        "mathcad/settings/calculation.xml",
    ] {
        assert!(
            names.iter().any(|n| n == part),
            "missing {part} in {names:?}"
        );
    }
}

#[test]
fn settings_default_when_unset() {
    let model = sheet(vec![math(0, var("a"))], Settings::default());
    let bytes = write_package(&model, None).expect("writes");
    let back = reread(&bytes);
    assert_eq!(back.settings.array_origin, Some(0));
    assert_eq!(back.settings.unit_system.as_deref(), Some("si"));
}

#[test]
fn no_source_is_read_back_by_the_package_reader() {
    let model = sheet(vec![math(0, var("a"))], full_settings());
    let bytes = write_package(&model, None).expect("writes");
    let pkg = Package::from_bytes(&bytes).expect("package reads");
    assert!(pkg.result.is_some());
    assert!(pkg.calculation.is_some());
}

/// `levels` solve blocks, each holding the next, with a math region at the bottom. The bottom
/// region is at depth `levels`.
fn chain(levels: usize) -> RegionModel {
    let mut inner = math(levels as u32, var("a"));
    for depth in (0..levels).rev() {
        inner = RegionModel {
            children: vec![inner],
            result_format: None,
            matrix: Default::default(),
            ..region(
                depth as u32,
                RegionKind::SolveBlock(SolveBlock { keyword: None }),
            )
        };
    }
    inner
}

#[test]
fn region_nesting_past_the_reader_limit_is_refused() {
    // The reader refuses a region deeper than MAX_REGION_DEPTH (top level is depth 0).
    let deepest_ok = sheet(vec![chain(MAX_REGION_DEPTH)], full_settings());
    assert!(write_package(&deepest_ok, None).is_ok());
    let too_deep = sheet(vec![chain(MAX_REGION_DEPTH + 2)], full_settings());
    assert!(matches!(
        write_package(&too_deep, None),
        Err(WriteError::Invalid(msg)) if msg.contains("nested deeper")
    ));
}

#[test]
fn expression_nesting_past_the_reader_limit_is_refused() {
    fn parens(depth: usize) -> Expr {
        let mut e = var("a");
        for _ in 0..depth {
            e = Expr::Parens(Box::new(e));
        }
        e
    }
    assert!(write_math(&parens(MAX_DEPTH)).is_ok());
    assert!(matches!(
        write_math(&parens(150)),
        Err(WriteError::Invalid(msg)) if msg.contains("nested deeper")
    ));
}

#[test]
fn non_finite_settings_are_refused() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut settings = full_settings();
        settings.convergence_tolerance = Some(value);
        let model = sheet(vec![math(0, var("a"))], settings.clone());
        assert!(matches!(
            write_package(&model, None),
            Err(WriteError::Settings(_))
        ));
        settings.convergence_tolerance = Some(0.001);
        settings.constraint_tolerance = Some(value);
        let model = sheet(vec![math(0, var("a"))], settings);
        assert!(matches!(
            write_package(&model, None),
            Err(WriteError::Settings(_))
        ));
    }
}

/// Bytes with a PNG signature and a short payload. The writer checks only the signature.
fn png_bytes(payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    bytes.extend_from_slice(payload);
    bytes
}

#[test]
fn area_and_image_round_trip_from_scratch() {
    let mut area = region(
        0,
        RegionKind::Area(AreaModel {
            collapsed: true,
            locked: true,
            hide_border: true,
        }),
    );
    area.pos = Some((18.9, 40.0));
    area.size = Some((400.0, 120.0));
    area.children = vec![math(1, var("x")), text(2, "inside the area")];
    let mut image = region(
        3,
        RegionKind::Image(ImageModel {
            item: String::new(),
            bytes: png_bytes(b"payload").into(),
        }),
    );
    image.pos = Some((18.9, 200.0));
    image.size = Some((120.0, 90.0));
    let model = sheet(vec![area, image], full_settings());

    let out = write_package(&model, None).expect("writes");
    let back = reread(&out);
    let RegionKind::Area(read_area) = &back.regions[0].kind else {
        panic!("area reads back as an area");
    };
    assert_eq!(
        *read_area,
        AreaModel {
            collapsed: true,
            locked: true,
            hide_border: true
        }
    );
    assert_eq!(back.regions[0].children.len(), 2);
    assert_eq!(
        back.regions[0].children[1].kind,
        model.regions[0].children[1].kind
    );
    let RegionKind::Image(read_image) = &back.regions[1].kind else {
        panic!("image reads back as an image");
    };
    assert_eq!(read_image.bytes, png_bytes(b"payload").into());
    assert!(read_image.item.starts_with('R'));
    assert_eq!(back.regions[1].size, Some((120.0, 90.0)));

    let files = entries(&out);
    assert!(files.contains_key("mathcad/media/Image1.png"));
    let types = String::from_utf8_lossy(&files["[Content_Types].xml"]).into_owned();
    assert!(types.contains("Extension=\"png\" ContentType=\"image/png\""));
}

#[test]
fn image_with_unknown_format_is_refused() {
    let model = sheet(
        vec![region(
            0,
            RegionKind::Image(ImageModel {
                item: String::new(),
                bytes: b"GIF89a".to_vec().into(),
            }),
        )],
        full_settings(),
    );
    assert!(matches!(
        write_package(&model, None),
        Err(WriteError::Unsupported(_))
    ));
}

#[test]
fn unchanged_image_keeps_its_source_part_and_relationship() {
    let payload = png_bytes(b"sample");
    let worksheet = concat!(
        r#"<worksheet xmlns="http://schemas.mathsoft.com/worksheet50" "#,
        r#"xmlns:ml="http://schemas.mathsoft.com/math50">"#,
        r#"<regions><region region-id="0" actualWidth="120" actualHeight="90" top="10" left="20">"#,
        r#"<image item-idref="Rimg1" width="120" /></region></regions></worksheet>"#
    );
    let rels = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" "#,
        r#"Target="/mathcad/media/Image1.png" Id="Rimg1" /></Relationships>"#
    );
    let source = make_zip(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml" /><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml" /><Default Extension="png" ContentType="image/png" /></Types>"#,
        ),
        ("mathcad/worksheet.xml", worksheet.as_bytes()),
        ("mathcad/_rels/worksheet.xml.rels", rels.as_bytes()),
        ("mathcad/media/Image1.png", &payload),
    ]);
    let model = reread_source(&source);
    assert!(matches!(model.regions[0].kind, RegionKind::Image(_)));
    let out = write_package(&model, Some(&source)).expect("writes");
    let files = entries(&out);
    assert_eq!(files["mathcad/media/Image1.png"], payload);
    let back = reread(&out);
    assert_eq!(back.regions, model.regions);
    assert!(
        !String::from_utf8_lossy(&files["mathcad/_rels/worksheet.xml.rels"]).contains("Image2")
    );
}

/// The model of a source package given as bytes.
fn reread_source(bytes: &[u8]) -> Worksheet {
    Worksheet::from_package(&Package::from_bytes(bytes).expect("source reads")).expect("models")
}
