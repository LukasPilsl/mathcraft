//! Headless tests of the table drawing: the view of a `<spec-table>` and its cell geometry.
//! No window is opened; the canvas only paints what [`table_geometry`] returns.

use mathcraft_core::expr::{BinaryOp, Expr, Id, IdKind, Number, UnaryOp};
use mathcraft_import::regions::{TableMember, TableModel};

use super::{table_geometry, CellKind, TableColumn, TableView, ROW_HEIGHT};

fn num(text: &str) -> Expr {
    Expr::Number(Number::Real {
        value: text.parse().expect("number"),
        text: text.to_owned(),
    })
}

fn matrix(values: &[&str]) -> Expr {
    Expr::Matrix {
        rows: values.len(),
        cols: 1,
        elements: values.iter().map(|v| num(v)).collect(),
    }
}

fn unit(name: &str) -> Expr {
    Expr::Id(Id::new(name, IdKind::Unit))
}

/// `name := matrix unit`, the shape of a table member define.
fn define(name: &str, values: &[&str], unit_expr: Expr) -> Expr {
    Expr::Define {
        target: Box::new(Expr::Id(Id::new(name, IdKind::Variable))),
        expr: Box::new(Expr::Scale {
            value: Box::new(matrix(values)),
            unit: Box::new(unit_expr),
        }),
    }
}

/// The three members of the sample `Unbenannt.mcdx` table.
fn sample_model() -> TableModel {
    let member = |expr, result_ref| TableMember {
        result_ref: Some(result_ref),
        expr: Ok(expr),
    };
    TableModel {
        members: vec![
            member(define("a", &["1", "4", "7"], unit("m")), 4),
            member(define("b", &["2", "5", "8"], unit("s")), 5),
            member(
                define(
                    "c",
                    &["3", "8", "9"],
                    Expr::binary(BinaryOp::Div, unit("m"), unit("s")),
                ),
                6,
            ),
        ],
    }
}

fn texts(view: &TableView, kind: CellKind) -> Vec<String> {
    table_geometry(view, 120.0)
        .cells
        .into_iter()
        .filter(|c| c.kind == kind)
        .map(|c| c.text)
        .collect()
}

#[test]
fn sample_table_has_three_columns_named_a_b_c() {
    let view = TableView::of(&sample_model());
    let headers: Vec<&str> = view.columns.iter().map(|c| c.header.as_str()).collect();
    assert_eq!(headers, ["a", "b", "c"]);
    assert!(view.columns.iter().all(|c| c.values.len() == 3));
}

#[test]
fn unit_row_shows_each_unit_in_parentheses() {
    let view = TableView::of(&sample_model());
    assert_eq!(texts(&view, CellKind::Unit), ["(m)", "(s)", "(m/s)"]);
}

#[test]
fn values_are_the_matrix_elements_column_by_column() {
    let view = TableView::of(&sample_model());
    assert_eq!(
        texts(&view, CellKind::Value),
        ["1", "4", "7", "2", "5", "8", "3", "8", "9"]
    );
}

#[test]
fn unit_row_sits_below_header_and_the_rule_is_below_both() {
    let geometry = table_geometry(&TableView::of(&sample_model()), 120.0);
    let header = geometry
        .cells
        .iter()
        .find(|c| c.kind == CellKind::Header)
        .unwrap();
    let unit = geometry
        .cells
        .iter()
        .find(|c| c.kind == CellKind::Unit)
        .unwrap();
    assert!(header.y < unit.y, "header above unit row");
    assert_eq!(geometry.rules.len(), 1);
    let [x1, y1, x2, y2] = geometry.rules[0];
    assert_eq!((x1, x2), (0.0, 120.0), "rule spans the width");
    assert_eq!(y1, y2, "rule is horizontal");
    assert!(unit.y < y1, "rule below the unit row");
    let first_value = geometry
        .cells
        .iter()
        .find(|c| c.kind == CellKind::Value)
        .unwrap();
    assert!(y1 < first_value.y, "values below the rule");
}

#[test]
fn columns_are_centred_in_equal_slots() {
    let geometry = table_geometry(&TableView::of(&sample_model()), 120.0);
    let headers: Vec<f32> = geometry
        .cells
        .iter()
        .filter(|c| c.kind == CellKind::Header)
        .map(|c| c.x)
        .collect();
    assert_eq!(headers, [20.0, 60.0, 100.0]);
}

#[test]
fn value_rows_step_by_one_row_height() {
    let geometry = table_geometry(&TableView::of(&sample_model()), 120.0);
    let column_a: Vec<f32> = geometry
        .cells
        .iter()
        .filter(|c| c.kind == CellKind::Value && c.x == 20.0)
        .map(|c| c.y)
        .collect();
    assert_eq!(column_a.len(), 3);
    assert!((column_a[1] - column_a[0] - ROW_HEIGHT).abs() < 1e-4);
}

#[test]
fn bare_matrix_member_has_no_unit_and_no_unit_row() {
    let model = TableModel {
        members: vec![TableMember {
            result_ref: None,
            expr: Ok(Expr::Define {
                target: Box::new(Expr::Id(Id::new("q", IdKind::Variable))),
                expr: Box::new(matrix(&["5"])),
            }),
        }],
    };
    let geometry = table_geometry(&TableView::of(&model), 40.0);
    assert!(geometry.cells.iter().all(|c| c.kind != CellKind::Unit));
    assert_eq!(
        geometry
            .cells
            .iter()
            .filter(|c| c.kind == CellKind::Value)
            .count(),
        1
    );
}

#[test]
fn unconvertible_member_keeps_its_column_with_the_reason() {
    let model = TableModel {
        members: vec![
            TableMember {
                result_ref: None,
                expr: Err("cannot convert math: <ml:bogus>".to_owned()),
            },
            TableMember {
                result_ref: None,
                expr: Ok(define("b", &["2"], unit("s"))),
            },
        ],
    };
    let view = TableView::of(&model);
    assert_eq!(view.columns.len(), 2);
    assert!(view.columns[0].header.contains("bogus"));
    assert!(view.columns[0].values.is_empty());
}

#[test]
fn empty_table_draws_nothing() {
    let view = TableView::of(&TableModel { members: vec![] });
    let geometry = table_geometry(&view, 100.0);
    assert!(geometry.cells.is_empty() && geometry.rules.is_empty());
}

#[test]
fn negated_value_is_written_as_the_element() {
    let view = TableView {
        columns: vec![TableColumn {
            header: "z".into(),
            unit: None,
            values: vec![Expr::unary(UnaryOp::Neg, num("2")).to_string()],
        }],
    };
    assert_eq!(texts(&view, CellKind::Value), ["-2"]);
}
