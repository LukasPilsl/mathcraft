//! Generates the XML of a new or edited 2D plot (`xyPlot`, `polarPlot`) from its model.
//!
//! The shape is the one the samples show (`documentation/examples`, checked on `3_rata_3_Kurve...`,
//! `17_rata_schiefer_wurf` and the polar plot of `12_rata_Kreisdiagramme`): title and legend, the
//! traces with their `traceStyle`, `graph-size`, then one element per axis with its grid, label,
//! markers, `plotEquations` and domain. A plot that was read from a file and not changed is copied
//! from the source instead (see `worksheet.rs`), so these defaults only reach plots made here.
//!
//! Not in any sample, so *inferred* (see `documentation/UNVERIFIED_FORMATS.md`): the values of
//! `graph-size`, `legendWidth`, `positionticmark` and `gridFrequency` of a new plot, the `<title>`
//! text, the trace `name` attribute, `scale-type="log"`, and `<gridLines />` being absent for an
//! axis without grid lines. A new trace has no `resultRef`: `result.xml` is written empty then, and
//! Prime calculates the plot when it opens the file.

use super::expr::write_math;
use super::xml::{push_attr, push_text};
use super::WriteError;
use crate::regions::{Axis2d, Equation, Plot2d, TraceStyle};

/// The XML of the `<plot>` element for `plot`.
pub(super) fn write_plot(plot: &Plot2d) -> Result<String, WriteError> {
    let (kind, x_tag, y_tag, x_domain, y_domain) = if plot.polar {
        (
            "polarPlot",
            "angularAxis",
            "radialAxis",
            "angularDomain",
            "radialDomain",
        )
    } else {
        ("xyPlot", "xAxis", "yAxis", "xyDomain", "xyDomain")
    };
    let mut out = String::new();
    // `origin-positioning` is on the samples' x-y plots only, not on their polar plot.
    out.push_str(if plot.polar {
        "<plot>"
    } else {
        r#"<plot origin-positioning="true">"#
    });
    out.push('<');
    out.push_str(kind);
    out.push('>');
    match &plot.title {
        Some(title) => {
            out.push_str("<title>");
            push_text(&mut out, title);
            out.push_str("</title>");
        }
        None => out.push_str("<title />"),
    }
    if plot.legend {
        out.push_str("<legend />");
    }
    out.push_str("<traces>");
    for trace in &plot.traces {
        out.push_str("<trace");
        if let Some(n) = trace.num_of_points {
            out.push_str(&format!(" num-of-points=\"{n}\""));
        }
        if let Some(r) = trace.result_ref {
            out.push_str(&format!(" resultRef=\"{r}\""));
        }
        if let Some(name) = &trace.name {
            out.push_str(" name=\"");
            push_attr(&mut out, name);
            out.push('"');
        }
        out.push('>');
        write_style(&mut out, &trace.style);
        out.push_str("</trace>");
    }
    out.push_str("</traces>");
    out.push_str(r#"<graph-size width="230.4" height="230.4" />"#);
    out.push_str("<axes>");
    write_axis(
        &mut out,
        &plot.x_axis,
        x_tag,
        x_domain,
        "PlotBoundaryBottom",
        !plot.polar,
    )?;
    write_axis(
        &mut out,
        &plot.y_axis,
        y_tag,
        y_domain,
        "PlotBoundaryRight",
        true,
    )?;
    out.push_str("</axes></");
    out.push_str(kind);
    out.push_str("></plot>");
    Ok(out)
}

fn write_style(out: &mut String, style: &TraceStyle) {
    out.push_str("<traceStyle");
    if let Some([r, g, b, a]) = style.color {
        out.push_str(&format!(" color=\"#{a:02X}{r:02X}{g:02X}{b:02X}\""));
    }
    if let Some(symbol) = &style.symbol {
        out.push_str(" symbol=\"");
        push_attr(out, symbol);
        out.push('"');
    }
    if let Some(weight) = style.line_weight {
        out.push_str(&format!(" line-weight=\"{weight}\""));
    }
    if let Some(line_style) = &style.line_style {
        out.push_str(" line-style=\"");
        push_attr(out, line_style);
        out.push('"');
    }
    if style.automatic {
        out.push_str(" automaticStyle=\"true\"");
    }
    match &style.mode {
        Some(mode) => {
            out.push('>');
            push_text(out, mode);
            out.push_str("</traceStyle>");
        }
        None => out.push_str(" />"),
    }
}

fn write_axis(
    out: &mut String,
    axis: &Axis2d,
    tag: &str,
    domain_tag: &str,
    legend_position: &str,
    second_math: bool,
) -> Result<(), WriteError> {
    out.push_str(&format!(
        "<{tag} rank=\"1\" legend-position=\"{legend_position}\""
    ));
    for (name, value) in [("start", axis.start), ("end", axis.end)] {
        if let Some(v) = value {
            if !v.is_finite() {
                return Err(WriteError::Invalid(format!("`{name}` is not finite")));
            }
            out.push_str(&format!(" {name}=\"{v}\""));
        }
    }
    out.push('>');
    out.push_str(
        r#"<axisLine position="origin" positionticmark="3" legendWidth="71.4666666666667" />"#,
    );
    out.push_str("<axisGrid>");
    out.push_str(&format!(
        "<gridFrequency>{}</gridFrequency>",
        axis.grid_frequency.unwrap_or(11)
    ));
    out.push_str(r#"<gridLabels display="true" />"#);
    if axis.grid_lines {
        out.push_str("<gridLines />");
    }
    out.push_str(r#"<tickMarks display="true" /></axisGrid><axisLabel /><markers />"#);
    out.push_str("<plotEquations>");
    for eq in &axis.equations {
        out.push_str("<plotEquation>");
        out.push_str(&format!("<math>{}</math>", write_inner(eq)?));
        if second_math {
            out.push_str("<math><ml:placeholder /></math>");
        }
        out.push_str("</plotEquation>");
    }
    out.push_str("</plotEquations>");
    out.push_str(&format!(
        "<{domain_tag} scale-type=\"{}\" auto-scale=\"true\">",
        if axis.log { "log" } else { "linear" }
    ));
    for (name, bound) in [
        ("startValue", &axis.domain_start),
        ("endValue", &axis.domain_end),
    ] {
        match bound {
            Some(eq) => out.push_str(&format!("<{name}>{}</{name}>", write_inner(eq)?)),
            None => out.push_str(&format!("<{name} />")),
        }
    }
    out.push_str(&format!("</{domain_tag}></{tag}>"));
    Ok(())
}

/// The XML of one equation, without `<math>`; a placeholder for an empty one.
fn write_inner(eq: &Equation) -> Result<String, WriteError> {
    match eq {
        Equation::Expr(expr) => write_math(expr),
        Equation::Empty => Ok("<ml:placeholder />".to_owned()),
        Equation::Unsupported(reason) => Err(WriteError::Unsupported(format!(
            "a plot equation cannot be written: {reason}"
        ))),
    }
}
