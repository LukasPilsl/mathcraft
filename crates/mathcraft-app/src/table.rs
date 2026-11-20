//! A table region (`<spec-table>`) as the screen draws it, and the geometry of that drawing.
//!
//! The table is a presentation of its member defines (`a := [1;4;7]·m`). Each member becomes one
//! column: its variable name is the header, its unit is the unit row (shown in parentheses, as
//! Prime shows it), and the matrix elements are the values, one per row. A rule separates the
//! header rows from the values, as in Prime's screenshot.
//!
//! [`table_geometry`] is pure: it returns the positions of the cells and rules in points, so the
//! layout is tested without a window (see `table_tests.rs`). The canvas only paints them.

use mathcraft_core::expr::Expr;
use mathcraft_import::regions::TableModel;

/// Height of one row, in points.
pub const ROW_HEIGHT: f32 = 16.0;

/// One column of the table: a member define that is a matrix with an optional unit.
#[derive(Debug, Clone, PartialEq)]
pub struct TableColumn {
    /// The variable name (`a`). For a member that is not a define of a matrix, the member's text.
    pub header: String,
    /// The unit as written (`m`, `m/s`), when the member has one.
    pub unit: Option<String>,
    /// The matrix elements in order, as written (`1`, `4`, `7`).
    pub values: Vec<String>,
}

/// A table region, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct TableView {
    pub columns: Vec<TableColumn>,
}

/// What a cell shows. The unit row is drawn differently from the values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Header,
    Unit,
    Value,
}

/// One text cell. `x` and `y` are the centre of the cell, in points from the region's top-left.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub kind: CellKind,
    pub text: String,
    pub x: f32,
    pub y: f32,
}

/// A horizontal or vertical rule, `(x1, y1, x2, y2)` in points from the region's top-left.
pub type Rule = [f32; 4];

/// The drawing of a table in a region of a given size.
#[derive(Debug, Clone, PartialEq)]
pub struct TableGeometry {
    pub cells: Vec<Cell>,
    pub rules: Vec<Rule>,
}

impl TableView {
    /// The table of a [`TableModel`]. A member that is not a define of a matrix still gets a column
    /// (its header is its text and it has no values), so no member is dropped from the picture.
    pub fn of(model: &TableModel) -> Self {
        let columns = model
            .members
            .iter()
            .map(|member| match &member.expr {
                Ok(expr) => column_of(expr),
                Err(reason) => TableColumn {
                    header: reason.clone(),
                    unit: None,
                    values: Vec::new(),
                },
            })
            .collect();
        Self { columns }
    }
}

/// The column of one member define: `name := matrix unit`, or a bare matrix (no unit).
fn column_of(expr: &Expr) -> TableColumn {
    let Expr::Define {
        target,
        expr: value,
    } = expr
    else {
        return TableColumn {
            header: expr.to_string(),
            unit: None,
            values: Vec::new(),
        };
    };
    let header = match target.as_ref() {
        Expr::Id(id) => id.name.clone(),
        other => other.to_string(),
    };
    let (matrix, unit) = match value.as_ref() {
        Expr::Scale { value, unit } => (value.as_ref(), Some(unit.to_string())),
        other => (other, None),
    };
    let values = match matrix {
        Expr::Matrix { elements, .. } => elements.iter().map(ToString::to_string).collect(),
        _ => Vec::new(),
    };
    TableColumn {
        header,
        unit,
        values,
    }
}

/// Lays the table out across a region `width` points wide.
///
/// The columns share the width equally and centre their cells. Rows are [`ROW_HEIGHT`] high: the
/// header row, the unit row, a rule, then the values. The rule spans the full width.
pub fn table_geometry(view: &TableView, width: f32) -> TableGeometry {
    let count = view.columns.len().max(1) as f32;
    let column_width = width / count;
    let mut cells = Vec::new();
    let mut rules = Vec::new();
    let header_y = ROW_HEIGHT * 0.5;
    let unit_y = ROW_HEIGHT * 1.5;
    let rule_y = ROW_HEIGHT * 2.0;
    for (i, column) in view.columns.iter().enumerate() {
        let x = column_width * (i as f32 + 0.5);
        cells.push(Cell {
            kind: CellKind::Header,
            text: column.header.clone(),
            x,
            y: header_y,
        });
        if let Some(unit) = &column.unit {
            cells.push(Cell {
                kind: CellKind::Unit,
                text: format!("({unit})"),
                x,
                y: unit_y,
            });
        }
        for (row, value) in column.values.iter().enumerate() {
            cells.push(Cell {
                kind: CellKind::Value,
                text: value.clone(),
                x,
                y: rule_y + ROW_HEIGHT * (row as f32 + 0.5),
            });
        }
    }
    // A rule under the header rows, as in the screenshot. No rule without columns.
    if !view.columns.is_empty() {
        rules.push([0.0, rule_y, width, rule_y]);
    }
    TableGeometry { cells, rules }
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
