//! Reader for `mathcad/result.xml` (MCDX_FORMAT.md section 3).
//!
//! Entries are keyed by `result-id`, which matches `resultRef` in `worksheet.xml`. Element names
//! are matched by local name only, so the `ml:`, `u:` and default namespaces are not checked.
//! Values are kept in the SI base units stored by Prime.
//!
//! A `resultData` with no value element (the empty `<resultData .../>` form, or one that only
//! carries `<Warnings>`) becomes [`ResultValue::Empty`]. Any value element this module does not
//! recognise becomes [`ResultValue::Unknown`] with the element name, so one odd entry never
//! fails the whole file.

use std::collections::BTreeMap;

use roxmltree::{Document, Node};
use thiserror::Error;

/// All results in one `result.xml`, keyed by `result-id`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultTable {
    pub by_id: BTreeMap<u32, ResultData>,
}

/// One `<resultData>` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultData {
    pub status: Status,
    pub value: ResultValue,
}

/// The `calculation-status` attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Synchronized,
    Pending,
    Other(String),
}

/// One `<u:unitReference>`. Missing `power-numerator` / `power-denominator` mean 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitRef {
    pub unit: String,
    pub num: i32,
    pub den: i32,
}

/// The value part of a result.
#[derive(Debug, Clone, PartialEq)]
pub enum ResultValue {
    /// No value stored for this entry (for example a text or definition region).
    Empty,
    /// `<ml:real>` with no units.
    Real(f64),
    /// `<ml:unitedValue>`: `value` is in SI base units, `units` names the displayed unit.
    United { value: f64, units: Vec<UnitRef> },
    /// `<ml:matrix rows cols>`; `elements` are in document order and may be nested matrices.
    Matrix {
        rows: usize,
        cols: usize,
        elements: Vec<ResultValue>,
    },
    /// The first `<engineError>` of an `<engineErrors>` block.
    Error { code: String, message: String },
    /// `<Trace2dResult>` (plot data).
    Trace2d(Trace2d),
    /// Reserved for symbolic results. No sample contains one, and its element name is not known
    /// yet, so this variant is never produced at present.
    Symbolic(String),
    /// A value element this module does not recognise. Holds the element name.
    Unknown(String),
}

/// Plot data from `<Trace2dResult>`. `TraceType` is `Range`, `Parametric` or `Vector` in the
/// samples. Range and result points hold the same number of values, except `Vector` traces
/// which hold one point. `NaN` values are kept as `NaN`.
#[derive(Debug, Clone, PartialEq)]
pub struct Trace2d {
    pub trace_type: String,
    /// `RangeInfo` `Min` and `Max` (range variable).
    pub range_min: f64,
    pub range_max: f64,
    /// Units of the range variable, from `RangeInfo`'s `unitMonomial` (may be empty).
    pub range_units: Vec<UnitRef>,
    /// `ResultInfo` `Min` and `Max` (plotted values).
    pub result_min: f64,
    pub result_max: f64,
    /// `RangePoints` values (x of a range plot, parameter of a parametric plot).
    pub range_points: Vec<f64>,
    /// `ResultPoints` values (y of a range plot, coordinate of a parametric plot).
    pub result_points: Vec<f64>,
}

impl Trace2d {
    /// The stored points as `(x, y)` pairs in sampling order, where `None` is a gap (a NaN in
    /// either value). `RangePoints` holds the first coordinate and `ResultPoints` the second, for
    /// every trace type, so a parametric or range trace is `(RangePoints[i], ResultPoints[i])`. A
    /// `Vector` trace has one point. Fails when the two vectors differ in length.
    pub fn points(&self) -> Result<Vec<Option<(f64, f64)>>, String> {
        if self.range_points.len() != self.result_points.len() {
            return Err(format!(
                "stored RangePoints ({}) and ResultPoints ({}) differ in length",
                self.range_points.len(),
                self.result_points.len()
            ));
        }
        Ok(self
            .range_points
            .iter()
            .zip(&self.result_points)
            .map(|(&x, &y)| (!x.is_nan() && !y.is_nan()).then_some((x, y)))
            .collect())
    }
}

/// Errors that stop a whole `result.xml` from being read. Unknown value shapes never cause one.
#[derive(Debug, Error)]
pub enum ResultError {
    #[error("result.xml is not well-formed XML: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("root element is `{0}`, expected `resultsList`")]
    Root(String),
    #[error("resultData has a missing or non-numeric result-id: {0:?}")]
    BadId(Option<String>),
    #[error("result-id {0} appears more than once")]
    Duplicate(u32),
}

/// Parses the text of a `result.xml` into a [`ResultTable`].
pub fn parse_results(xml: &str) -> Result<ResultTable, ResultError> {
    let xml = xml.trim_start_matches('\u{feff}');
    let doc = Document::parse(xml)?;
    let root = doc.root_element();
    if local(root) != "resultsList" {
        return Err(ResultError::Root(local(root).to_string()));
    }

    let mut by_id = BTreeMap::new();
    for node in elements(root).filter(|n| local(*n) == "resultData") {
        let id_attr = node.attribute("result-id");
        let id = id_attr
            .and_then(|s| s.trim().parse::<u32>().ok())
            .ok_or_else(|| ResultError::BadId(id_attr.map(str::to_string)))?;
        let data = ResultData {
            status: parse_status(node.attribute("calculation-status")),
            value: parse_data_value(node),
        };
        if by_id.insert(id, data).is_some() {
            return Err(ResultError::Duplicate(id));
        }
    }
    Ok(ResultTable { by_id })
}

fn local<'a>(node: Node<'a, '_>) -> &'a str {
    node.tag_name().name()
}

fn elements<'a, 'input>(node: Node<'a, 'input>) -> impl Iterator<Item = Node<'a, 'input>> {
    node.children().filter(|n| n.is_element())
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    elements(node).find(|n| local(*n) == name)
}

fn parse_status(attr: Option<&str>) -> Status {
    match attr {
        Some("Synchronized") => Status::Synchronized,
        Some("Pending") => Status::Pending,
        other => Status::Other(other.unwrap_or_default().to_string()),
    }
}

/// Finds the value of one `<resultData>`: the `<ml:result>` payload, a `<Trace2dResult>`, or an
/// `<engineErrors>` block. `<Warnings>` is ignored.
fn parse_data_value(node: Node) -> ResultValue {
    let Some(first) = elements(node).find(|n| local(*n) != "Warnings") else {
        return ResultValue::Empty;
    };
    match local(first) {
        "result" => match elements(first).next() {
            Some(payload) => parse_value(payload),
            None => ResultValue::Unknown("result".to_string()),
        },
        "Trace2dResult" => trace_of(first)
            .map(ResultValue::Trace2d)
            .unwrap_or_else(|| ResultValue::Unknown("Trace2dResult".to_string())),
        "engineErrors" => {
            error_of(first).unwrap_or_else(|| ResultValue::Unknown("engineErrors".to_string()))
        }
        other => ResultValue::Unknown(other.to_string()),
    }
}

/// Parses a value element, or returns `Unknown` with its name.
fn parse_value(node: Node) -> ResultValue {
    value_of(node).unwrap_or_else(|| ResultValue::Unknown(local(node).to_string()))
}

fn value_of(node: Node) -> Option<ResultValue> {
    match local(node) {
        "real" => Some(ResultValue::Real(parse_f64(node.text())?)),
        "unitedValue" => {
            let value = parse_f64(child(node, "real")?.text())?;
            let units = match child(node, "unitMonomial") {
                Some(m) => parse_units(m)?,
                None => Vec::new(),
            };
            Some(ResultValue::United { value, units })
        }
        "matrix" => {
            let rows = node.attribute("rows")?.trim().parse().ok()?;
            let cols = node.attribute("cols")?.trim().parse().ok()?;
            Some(ResultValue::Matrix {
                rows,
                cols,
                elements: elements(node).map(parse_value).collect(),
            })
        }
        _ => None,
    }
}

/// Reads the `<u:unitReference>` children of a `<u:unitMonomial>`.
fn parse_units(monomial: Node) -> Option<Vec<UnitRef>> {
    elements(monomial)
        .filter(|n| local(*n) == "unitReference")
        .map(|n| {
            let power = |attr: &str| -> Option<i32> {
                match n.attribute(attr) {
                    Some(s) => s.trim().parse().ok(),
                    None => Some(1),
                }
            };
            Some(UnitRef {
                unit: n.attribute("unit")?.to_string(),
                num: power("power-numerator")?,
                den: power("power-denominator")?,
            })
        })
        .collect()
}

/// Reads the first `<engineError>`: its `<errorCode>` and `<resource-string>` (the message).
fn error_of(node: Node) -> Option<ResultValue> {
    let err = child(node, "engineError")?;
    let code = child(err, "errorCode")?.text()?.trim().to_string();
    let message = child(err, "resource-string")?.text()?.trim().to_string();
    Some(ResultValue::Error { code, message })
}

fn trace_of(node: Node) -> Option<Trace2d> {
    let range_info = child(node, "RangeInfo")?;
    let result_info = child(node, "ResultInfo")?;
    let data = child(node, "Data")?;
    let range_units = match child(range_info, "unitMonomial") {
        Some(m) => parse_units(m)?,
        None => Vec::new(),
    };
    Some(Trace2d {
        trace_type: node.attribute("TraceType")?.to_string(),
        range_min: attr_f64(range_info, "Min")?,
        range_max: attr_f64(range_info, "Max")?,
        range_units,
        result_min: attr_f64(result_info, "Min")?,
        result_max: attr_f64(result_info, "Max")?,
        range_points: vector_of(child(data, "RangePoints")?)?,
        result_points: vector_of(child(data, "ResultPoints")?)?,
    })
}

/// Reads the bracketed value list of the first `<DataVectors>` under a points element.
fn vector_of(points: Node) -> Option<Vec<f64>> {
    let vectors = child(points, "DataVectors")?;
    let text = vectors.text()?.trim();
    let inner = text.strip_prefix('[')?.strip_suffix(']')?.trim();
    if inner.is_empty() {
        return Some(Vec::new());
    }
    inner.split(',').map(|s| parse_f64(Some(s))).collect()
}

fn attr_f64(node: Node, name: &str) -> Option<f64> {
    parse_f64(node.attribute(name))
}

fn parse_f64(text: Option<&str>) -> Option<f64> {
    text?.trim().parse::<f64>().ok()
}

/// Plot data of one 3D trace, as Prime stores it in `<ml:result>`: a matrix of four. Element 0 is
/// the trace kind (`0` for a curve, `2` for a surface in the samples), element 1 holds the three
/// coordinate matrices `X`, `Y`, `Z` (each `rows` x `cols`, row-major), element 2 the six bounds
/// `[xmin, xmax, ymin, ymax, zmin, zmax]`, and element 3 a flag (`1` in every sample, not read).
/// A curve is `rows` x 1. NaN values are kept as NaN.
#[derive(Debug, Clone, PartialEq)]
pub struct Trace3dStored {
    pub kind: f64,
    pub rows: usize,
    pub cols: usize,
    /// `X`, `Y` and `Z` values, row-major, `rows * cols` each.
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
    /// `[xmin, xmax, ymin, ymax, zmin, zmax]`.
    pub bounds: Vec<f64>,
}

/// Reads a stored 3D trace (see [`Trace3dStored`]). `None` when the value has another shape.
pub fn trace3d_of(value: &ResultValue) -> Option<Trace3dStored> {
    let ResultValue::Matrix {
        rows: 4,
        cols: 1,
        elements,
    } = value
    else {
        return None;
    };
    let [kind, coords, bounds, _flag] = elements.as_slice() else {
        return None;
    };
    let ResultValue::Real(kind) = kind else {
        return None;
    };
    let ResultValue::Matrix {
        rows: 3,
        cols: 1,
        elements: parts,
    } = coords
    else {
        return None;
    };
    let mut shape = None;
    let mut values = Vec::with_capacity(3);
    for part in parts {
        let ResultValue::Matrix {
            rows,
            cols,
            elements,
        } = part
        else {
            return None;
        };
        if *shape.get_or_insert((*rows, *cols)) != (*rows, *cols) {
            return None;
        }
        let data: Vec<f64> = elements
            .iter()
            .map(|e| match e {
                ResultValue::Real(v) => Some(*v),
                _ => None,
            })
            .collect::<Option<Vec<f64>>>()?;
        if data.len() != rows * cols {
            return None;
        }
        values.push(data);
    }
    let ResultValue::Matrix {
        rows: 6,
        cols: 1,
        elements: bounds,
    } = bounds
    else {
        return None;
    };
    let bounds: Vec<f64> = bounds
        .iter()
        .map(|e| match e {
            ResultValue::Real(v) => Some(*v),
            _ => None,
        })
        .collect::<Option<Vec<f64>>>()?;
    let (rows, cols) = shape?;
    let [x, y, z]: [Vec<f64>; 3] = values.try_into().ok()?;
    Some(Trace3dStored {
        kind: *kind,
        rows,
        cols,
        x,
        y,
        z,
        bounds,
    })
}
