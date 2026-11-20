//! 3D plots (`<plot3D>`): the model read from the plot XML.
//!
//! What the stored XML says, as far as it is known (checked on the two 3D plots of the samples,
//! `13_rata_plot1` and `14_rata_plot2`):
//!
//! * `<axes>` holds `xAxis`, `yAxis` and `zAxis`. `start` and `end` are the axis as Prime shows it
//!   (auto-scaled axes may show a wider or narrower range than the data, see `evaluate_plots3d`).
//!   The domain of the x and y axes is `xyDomain`, that of z is `zDomain`.
//! * `<plotEquations>` is one list for the whole plot (not one per axis). Each `<plotEquation>`
//!   has one `<math>` per coordinate slot. In the samples the first `<math>` is the expression
//!   and the second is a placeholder. Trace `i` uses plotEquation `i` (the same rule as 2D).
//! * `<traces>` holds `<trace3D>` elements with `resultRef` (the data in `result.xml`),
//!   `num-of-points` and a `<traceStyle>`.
//! * `<CameraTransformMatrix>` holds 16 `<Cell>` values (row-major 4x4). Stored, not drawn with
//!   yet: the viewer picks its own view.
//! * Not in any sample (inferred, see `evaluate_plots3d`): a parametric plotEquation with three
//!   `<math>` slots (x, y, z expressions), which the evaluator reads when the second and third
//!   slots are not placeholders.

use roxmltree::Node;

use super::plot::{children_named, equation_of_math, find_child, finite_attr, trace_style};
use super::plot::{Equation, TraceStyle};

/// A 3D plot: its three axes, the plot equations and the traces.
#[derive(Debug, Clone, PartialEq)]
pub struct Plot3d {
    /// `xAxis`, `yAxis`, `zAxis` in that order.
    pub x_axis: Axis3d,
    pub y_axis: Axis3d,
    pub z_axis: Axis3d,
    /// Whether the plot has a `<legend>` element.
    pub legend: bool,
    /// The `<math>` slots of each `<plotEquation>`, in order.
    pub equations: Vec<Vec<Equation>>,
    /// The traces in document order. Trace `i` uses equation `i`.
    pub traces: Vec<Trace3d>,
    /// `CameraTransformMatrix` cells, row-major. `None` when absent or malformed.
    pub camera: Option<[f64; 16]>,
}

/// One axis of a 3D plot.
#[derive(Debug, Clone, PartialEq)]
pub struct Axis3d {
    /// `start` attribute, as Prime displays it.
    pub start: Option<f64>,
    /// `end` attribute, as Prime displays it.
    pub end: Option<f64>,
    /// Logarithmic scale (`scale-type` of the domain contains `log`). Stored, not drawn yet.
    pub log: bool,
    /// Whether the axis has `<gridLines>`.
    pub grid_lines: bool,
    /// `gridFrequency` as written (inferred, not confirmed).
    pub grid_frequency: Option<u32>,
}

/// One trace of a 3D plot.
#[derive(Debug, Clone, PartialEq)]
pub struct Trace3d {
    /// `resultRef` of the trace's data in `result.xml`.
    pub result_ref: Option<u32>,
    /// `num-of-points` as written. Used only when no range variable is defined (see
    /// `evaluate_plots3d`).
    pub num_of_points: Option<u32>,
    /// The trace's style.
    pub style: TraceStyle,
    /// The `<math>` slots of the trace's plot equation (see [`Plot3d::equations`]).
    pub equation: Vec<Equation>,
}

/// Reads a `<plot3D>` element.
pub(super) fn read_plot3d(node: Node<'_, '_>) -> Plot3d {
    let axes = find_child(node, "axes");
    let axis = |name: &str| {
        axes.and_then(|a| find_child(a, name))
            .map_or_else(empty_axis, axis3d)
    };
    let equations: Vec<Vec<Equation>> = find_child(node, "plotEquations")
        .map(|list| {
            children_named(list, "plotEquation")
                .into_iter()
                .map(|pe| {
                    children_named(pe, "math")
                        .into_iter()
                        .map(equation_of_math)
                        .collect()
                })
                .collect()
        })
        .unwrap_or_default();
    let traces = find_child(node, "traces")
        .map(|t| children_named(t, "trace3D"))
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, trace)| Trace3d {
            result_ref: trace
                .attribute("resultRef")
                .and_then(|v| v.trim().parse().ok()),
            num_of_points: trace
                .attribute("num-of-points")
                .and_then(|v| v.trim().parse().ok()),
            style: trace_style(trace),
            equation: equation_at(&equations, i),
        })
        .collect();
    Plot3d {
        x_axis: axis("xAxis"),
        y_axis: axis("yAxis"),
        z_axis: axis("zAxis"),
        legend: find_child(node, "legend").is_some(),
        equations,
        traces,
        camera: camera_cells(node),
    }
}

/// The slots of trace `index`. A trace past the end of the list takes the last equation, as in
/// the 2D plots. No equations at all gives one `Unsupported` slot, so the mismatch is visible.
fn equation_at(equations: &[Vec<Equation>], index: usize) -> Vec<Equation> {
    equations
        .get(index)
        .or_else(|| equations.last())
        .cloned()
        .unwrap_or_else(|| {
            vec![Equation::Unsupported(format!(
                "no plot equation for trace {}",
                index + 1
            ))]
        })
}

fn empty_axis() -> Axis3d {
    Axis3d {
        start: None,
        end: None,
        log: false,
        grid_lines: false,
        grid_frequency: None,
    }
}

/// One `<xAxis>`, `<yAxis>` or `<zAxis>`. The domain is the child ending in `Domain`.
fn axis3d(axis: Node<'_, '_>) -> Axis3d {
    let grid = find_child(axis, "axisGrid");
    let domain = axis
        .children()
        .find(|c| c.is_element() && c.tag_name().name().ends_with("Domain"));
    Axis3d {
        start: finite_attr(axis, "start"),
        end: finite_attr(axis, "end"),
        log: domain
            .and_then(|d| d.attribute("scale-type"))
            .is_some_and(|s| s.to_ascii_lowercase().contains("log")),
        grid_lines: grid.is_some_and(|g| find_child(g, "gridLines").is_some()),
        grid_frequency: grid
            .and_then(|g| find_child(g, "gridFrequency"))
            .and_then(|f| f.text())
            .and_then(|t| t.trim().parse().ok()),
    }
}

/// The 16 cells of `<CameraTransformMatrix>`, row-major.
fn camera_cells(plot: Node<'_, '_>) -> Option<[f64; 16]> {
    let matrix = find_child(plot, "CameraTransformMatrix")?;
    let cells = find_child(matrix, "Cells")?;
    let values: Vec<f64> = children_named(cells, "Cell")
        .into_iter()
        .map(|c| {
            c.text()?
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
        })
        .collect::<Option<Vec<f64>>>()?;
    <[f64; 16]>::try_from(values).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use roxmltree::Document;

    const PLOT: &str = r##"<plot3D xmlns="http://schemas.mathsoft.com/worksheet50" xmlns:ml="http://schemas.mathsoft.com/math50"><traces>
        <trace3D num-of-points="41" resultRef="3"><traceStyle color="#FF000000" automaticStyle="true">curve</traceStyle></trace3D>
        <trace3D num-of-points="41" resultRef="5"><traceStyle color="#FF00FF00">surface</traceStyle></trace3D>
      </traces>
      <axes>
        <xAxis start="0" end="4"><axisGrid><gridFrequency>5</gridFrequency><gridLines /></axisGrid>
          <xyDomain scale-type="linear"/></xAxis>
        <yAxis start="0" end="4"><xyDomain scale-type="log"/></yAxis>
        <zAxis start="0" end="5"><zDomain scale-type="linear"/></zAxis>
      </axes>
      <plotEquations>
        <plotEquation><math><ml:id xmlns:ml="http://schemas.mathsoft.com/math50" labels="VARIABLE">x</ml:id></math><math><ml:placeholder xmlns:ml="http://schemas.mathsoft.com/math50"/></math></plotEquation>
        <plotEquation><math><ml:placeholder xmlns:ml="http://schemas.mathsoft.com/math50"/></math></plotEquation>
      </plotEquations>
      <CameraTransformMatrix Rows="4" Columns="4"><Cells>
        <Cell>1</Cell><Cell>0</Cell><Cell>0</Cell><Cell>0</Cell>
        <Cell>0</Cell><Cell>1</Cell><Cell>0</Cell><Cell>0</Cell>
        <Cell>0</Cell><Cell>0</Cell><Cell>1</Cell><Cell>0</Cell>
        <Cell>0</Cell><Cell>0</Cell><Cell>0</Cell><Cell>1</Cell>
      </Cells></CameraTransformMatrix></plot3D>"##;

    fn parse(xml: &str) -> Plot3d {
        let doc = Document::parse(xml).unwrap();
        read_plot3d(doc.root_element())
    }

    #[test]
    fn reads_axes_traces_and_equations() {
        let plot = parse(PLOT);
        assert_eq!(plot.x_axis.start, Some(0.0));
        assert_eq!(plot.x_axis.end, Some(4.0));
        assert_eq!(plot.x_axis.grid_frequency, Some(5));
        assert!(plot.x_axis.grid_lines);
        assert!(plot.y_axis.log);
        assert_eq!(plot.z_axis.end, Some(5.0));
        assert_eq!(plot.traces.len(), 2);
        assert_eq!(plot.traces[0].result_ref, Some(3));
        assert_eq!(plot.traces[1].result_ref, Some(5));
        assert_eq!(plot.traces[1].num_of_points, Some(41));
        assert_eq!(plot.traces[1].style.mode.as_deref(), Some("surface"));
        assert_eq!(plot.equations.len(), 2);
        assert_eq!(plot.equations[0].len(), 2);
        assert!(matches!(plot.equations[0][0], Equation::Expr(_)));
        assert_eq!(plot.equations[0][1], Equation::Empty);
        // Trace 2 uses equation 2 (the list is shorter): it takes the last one.
        assert_eq!(plot.traces[1].equation[0], Equation::Empty);
        assert_eq!(plot.camera.map(|c| c[0]), Some(1.0));
    }

    #[test]
    fn missing_cells_give_no_camera_and_no_equations_give_unsupported() {
        let plot = parse("<plot3D><traces><trace3D resultRef=\"1\"/></traces></plot3D>");
        assert_eq!(plot.camera, None);
        assert!(matches!(
            plot.traces[0].equation[0],
            Equation::Unsupported(_)
        ));
        assert_eq!(plot.x_axis.start, None);
    }
}
