//! The worksheet part, `mathcad/worksheet.xml`: the region tree of the model.
//!
//! Each region is written from the model: its ID, position, size, `resultRef` and solve-block
//! category, and its content (math from its [`Expr`], text from the text plan). Content the model
//! keeps as a plot or as unsupported is copied from the source region with the same ID (see the
//! module docs of [`crate::writer`]). Attributes and children of a region that the model does
//! not read are copied from that source region too.

use std::cell::RefCell;
use std::collections::HashMap;

use mathcraft_core::eval::{MatrixDisplay, ResultFormat};
use roxmltree::Node;

use super::expr::write_math_keyword;
use super::image::ImagePlan;
use super::result_format;
use super::text::TextPlan;
use super::xml::{
    check_chars, is_region, push_attr, push_unmodelled_attrs, raw_children, raw_element,
    root_start_tag,
};
use super::WriteError;
use crate::dom;
use crate::regions::{ImportError, RegionKind, RegionModel, Worksheet};

/// The root element written when there is no source: the namespaces the reader expects.
const GENERATED_ROOT: &str = concat!(
    r#"<worksheet xmlns="http://schemas.mathsoft.com/worksheet50" "#,
    r#"xmlns:ws="http://schemas.mathsoft.com/worksheet50" "#,
    r#"xmlns:ml="http://schemas.mathsoft.com/math50" "#,
    r#"xmlns:u="http://schemas.mathsoft.com/units10" "#,
    r#"xmlns:p="http://schemas.mathsoft.com/provenance10">"#
);

/// The inline `FlowDocument` of a new text region. *Inferred* from the samples' attributes.
const FLOW_DOCUMENT_DEFAULT: &str = concat!(
    r#"<FlowDocument xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" "#,
    r##"FontFamily="Tahoma" FontStyle="Normal" FontWeight="Normal" FontSize="14.6666666666667" "##,
    r##"Foreground="#FF000000" Background="#00FFFFFF" TextAlignment="Left" />"##
);

/// Attributes of a `<region>` the writer writes from the model. Every other attribute is copied.
const REGION_MODELLED_ATTRS: &[&str] = &[
    "region-id",
    "actualWidth",
    "actualHeight",
    "top",
    "left",
    "solve-block-category",
];

/// What the region writer reads from the source: the source's regions by ID, and its text.
struct Cx<'a, 'i> {
    /// The source worksheet text, the same string its document was parsed from. `None` without a
    /// source.
    xml: Option<&'a str>,
    /// The source region with each ID (the first one, if an ID repeats).
    source: HashMap<u32, Node<'a, 'i>>,
    /// The `item-idref` of each text region in the output.
    plan: &'a TextPlan,
    /// The `item-idref` of each image region in the output.
    images: &'a ImagePlan,
    /// The warnings of the regions written so far (see [`result_format::unverified_warning`]).
    warnings: RefCell<Vec<String>>,
}

impl Cx<'_, '_> {
    /// Records that the region `region_id` was written with a format that is not verified.
    fn note_unverified(&self, region_id: u32) {
        self.warnings
            .borrow_mut()
            .push(result_format::unverified_warning(region_id));
    }

    /// Records that the region `region_id` was written with the unverified structure `what`
    /// (a name for the user, for example `area`).
    fn note_structure(&self, region_id: u32, what: &str) {
        self.warnings
            .borrow_mut()
            .push(result_format::structure_warning(region_id, what));
    }
}

/// The XML of `mathcad/worksheet.xml`, without its byte order mark.
///
/// `source` is the source worksheet text, if there is a source. Every text region of `model`
/// must have an entry in `plan`.
///
/// # Errors
/// [`WriteError::Invalid`] and [`WriteError::Unsupported`] for regions that cannot be written,
/// and [`WriteError::Source`] when `source` is not well-formed.
pub(super) fn write(
    model: &Worksheet,
    source: Option<&str>,
    plan: &TextPlan,
    images: &ImagePlan,
) -> Result<(String, Vec<String>), WriteError> {
    let doc = match source {
        Some(xml) => Some(dom::parse(xml).map_err(|e| WriteError::Source(ImportError::Dom(e)))?),
        None => None,
    };
    let mut cx = Cx {
        xml: source,
        source: HashMap::new(),
        plan,
        images,
        warnings: RefCell::new(Vec::new()),
    };
    let mut out = String::new();
    match (&doc, source) {
        (Some(doc), Some(xml)) => {
            let root = doc.root_element();
            for region in doc.descendants().filter(|n| is_region(*n)) {
                if let Some(id) = region_id_of(region) {
                    cx.source.entry(id).or_insert(region);
                }
            }
            let start = root_start_tag(xml, root).ok_or_else(|| {
                WriteError::Invalid("the source worksheet has no readable root tag".to_owned())
            })?;
            out.push_str(&start);
            out.push_str(&raw_children(xml, root, &["regions"])?);
        }
        _ => out.push_str(GENERATED_ROOT),
    }
    out.push_str("<regions>");
    for region in &model.regions {
        write_region(&mut out, region, &cx, None)?;
    }
    out.push_str("</regions></worksheet>");
    Ok((out, cx.warnings.into_inner()))
}

/// The XML of the page header or footer part (`mathcad/header.xml` or `mathcad/footer.xml`),
/// without its byte order mark. Its regions are text regions, written as the worksheet's are. A
/// source part keeps its root element and its other children; without a source the root is
/// generated under the same namespaces as the worksheet.
///
/// # Errors
/// [`WriteError::Invalid`] when the source has no readable root, [`WriteError::Unsupported`] for
/// a region that is not text, and [`WriteError::Source`] when `source` is not well-formed.
pub(super) fn write_furniture(
    regions: &[RegionModel],
    source: Option<&str>,
    plan: &TextPlan,
    footer: bool,
) -> Result<(String, Vec<String>), WriteError> {
    let doc = match source {
        Some(xml) => Some(dom::parse(xml).map_err(|e| WriteError::Source(ImportError::Dom(e)))?),
        None => None,
    };
    let images = ImagePlan::default();
    let mut cx = Cx {
        xml: source,
        source: HashMap::new(),
        plan,
        images: &images,
        warnings: RefCell::new(Vec::new()),
    };
    let name = if footer { "footer" } else { "header" };
    let mut out = String::new();
    let closing = match (&doc, source) {
        (Some(doc), Some(xml)) => {
            let root = doc.root_element();
            for region in doc.descendants().filter(|n| is_region(*n)) {
                if let Some(id) = region_id_of(region) {
                    cx.source.entry(id).or_insert(region);
                }
            }
            let start = root_start_tag(xml, root).ok_or_else(|| {
                WriteError::Invalid(format!("the source {name} part has no readable root tag"))
            })?;
            out.push_str(&start);
            out.push_str(&raw_children(xml, root, &["regions"])?);
            root.tag_name().name().to_owned()
        }
        _ => {
            out.push_str(&GENERATED_ROOT.replacen("<worksheet ", &format!("<{name} "), 1));
            name.to_owned()
        }
    };
    out.push_str("<regions>");
    for region in regions {
        if !matches!(region.kind, RegionKind::Text(_)) {
            return Err(WriteError::Unsupported(format!(
                "the {name} holds text regions only"
            )));
        }
        write_region(&mut out, region, &cx, None)?;
    }
    out.push_str("</regions>");
    out.push_str(&format!("</{closing}>"));
    Ok((out, cx.warnings.into_inner()))
}

fn region_id_of(region: Node<'_, '_>) -> Option<u32> {
    region
        .attribute("region-id")
        .and_then(|v| v.trim().parse::<u32>().ok())
}

/// The child element of `node` in the worksheet namespace with local name `name`, if any.
fn ws_child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|c| {
        c.is_element() && c.tag_name().namespace() == Some(dom::WS) && c.tag_name().name() == name
    })
}

/// The solve block keyword still to write, as `(region ID of the member that holds it, keyword)`.
type KeywordAt<'k> = Option<(u32, &'k str)>;

/// The region ID of the first member (in document order, nested members included) whose math
/// can hold the keyword `keyword` as a `KEYWORD` identifier.
fn keyword_region(members: &[RegionModel], keyword: &str) -> Result<Option<u32>, WriteError> {
    for member in members {
        if let RegionKind::Math(expr) = &member.kind {
            let (_, placed) =
                write_math_keyword(expr, Some(keyword)).map_err(|e| e.in_region(member.id))?;
            if placed {
                return Ok(Some(member.id));
            }
        }
        if let Some(id) = keyword_region(&member.children, keyword)? {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

fn write_region(
    out: &mut String,
    region: &RegionModel,
    cx: &Cx<'_, '_>,
    keyword: KeywordAt<'_>,
) -> Result<(), WriteError> {
    write_region_inner(out, region, cx, keyword).map_err(|e| e.in_region(region.id))
}

fn write_region_inner(
    out: &mut String,
    region: &RegionModel,
    cx: &Cx<'_, '_>,
    keyword: KeywordAt<'_>,
) -> Result<(), WriteError> {
    let source = cx.source.get(&region.id).copied();
    out.push_str(&format!("<region region-id=\"{}\"", region.id));
    if let Some((width, height)) = region.size {
        push_number(out, "actualWidth", width)?;
        push_number(out, "actualHeight", height)?;
    }
    if let Some((left, top)) = region.pos {
        push_number(out, "top", top)?;
        push_number(out, "left", left)?;
    }
    if let Some(category) = &region.solve_category {
        check_chars(category)?;
        out.push_str(" solve-block-category=\"");
        push_attr(out, category);
        out.push('"');
    }
    if let (Some(src), Some(xml)) = (source, cx.xml) {
        push_unmodelled_attrs(out, xml, src, REGION_MODELLED_ATTRS);
    }
    out.push('>');

    match &region.kind {
        RegionKind::Math(expr) => {
            no_children(region, "a math region")?;
            out.push_str("<math");
            if let Some(result_ref) = region.result_ref {
                out.push_str(&format!(" resultRef=\"{result_ref}\""));
            }
            let math = source.and_then(|s| ws_child(s, "math"));
            if let (Some(math), Some(xml)) = (math, cx.xml) {
                push_unmodelled_attrs(out, xml, math, &["resultRef"]);
            }
            out.push('>');
            let own = keyword
                .filter(|(member, _)| *member == region.id)
                .map(|(_, word)| word);
            let (xml, _) = write_math_keyword(expr, own)?;
            out.push_str(&xml);
            // Display metadata that is not math. Copied as it is, except for a changed general
            // format, which is patched attribute by attribute.
            let mut wrote_format = false;
            if let (Some(math), Some(xml)) = (math, cx.xml) {
                let source_format = crate::regions::math_result_format(math)
                    .map_err(|e| WriteError::Invalid(format!("result format: {e}")))?;
                let source_matrix = crate::regions::math_matrix_display(math)
                    .map_err(|e| WriteError::Invalid(format!("matrix display: {e}")))?;
                for child in math
                    .children()
                    .filter(|c| c.is_element() && c.tag_name().name() == "resultFormat")
                {
                    wrote_format = true;
                    let mut raw = raw_element(xml, child)?;
                    if let Some(new) = &region.result_format {
                        let old = source_format.clone().unwrap_or_default();
                        if *new != old {
                            raw = result_format::patch_result_format(&raw, new, &old)?;
                            if result_format::writes_unverified(new, &old) {
                                cx.note_unverified(region.id);
                            }
                        }
                    }
                    if region.matrix != source_matrix {
                        raw = result_format::patch_matrix(&raw, &region.matrix, &source_matrix)?;
                    }
                    out.push_str(&raw);
                }
            }
            if !wrote_format {
                let general = region
                    .result_format
                    .as_ref()
                    .filter(|format| **format != ResultFormat::default());
                let matrix = (region.matrix != MatrixDisplay::default()).then_some(&region.matrix);
                if general.is_some() || matrix.is_some() {
                    out.push_str(&result_format::new_result_format(general, matrix)?);
                }
                if let Some(format) = general {
                    if result_format::writes_unverified(format, &ResultFormat::default()) {
                        cx.note_unverified(region.id);
                    }
                }
            }
            out.push_str("</math>");
        }
        RegionKind::Text(body) => {
            // UNVERIFIED: the hyperlink element is the XAML guess, see `writer/text.rs`.
            if body
                .rich()
                .lines
                .iter()
                .any(|l| l.runs.iter().any(|r| r.style.link.is_some()))
            {
                cx.note_structure(region.id, "hyperlink");
            }
            let item = cx.plan.items.get(&region.id).ok_or_else(|| {
                WriteError::Invalid("text region has no relationship in the plan".to_owned())
            })?;
            out.push_str("<text item-idref=\"");
            push_attr(out, item);
            out.push('"');
            let text = source.and_then(|s| ws_child(s, "text"));
            if let (Some(text), Some(xml)) = (text, cx.xml) {
                push_unmodelled_attrs(out, xml, text, &["item-idref"]);
            }
            out.push('>');
            match (text, cx.xml) {
                (Some(text), Some(xml)) => {
                    out.push_str(&raw_children(xml, text, &["regions"])?);
                }
                _ => out.push_str(FLOW_DOCUMENT_DEFAULT),
            }
            write_nested(out, region, cx, true, keyword)?;
            out.push_str("</text>");
        }
        RegionKind::SolveBlock(block) => {
            out.push_str("<solveblock>");
            let raw = source.and_then(|s| ws_child(s, "solveblock"));
            if let (Some(raw), Some(xml)) = (raw, cx.xml) {
                out.push_str(&raw_children(xml, raw, &["regions"])?);
            }
            // The keyword is written where the first member holds it, as the reader finds it.
            let members = match block.keyword.as_deref() {
                Some(word) => {
                    let id = keyword_region(&region.children, word)?.ok_or_else(|| {
                        WriteError::Unsupported(format!(
                            "solve block keyword `{word}` is not held by any member"
                        ))
                    })?;
                    Some((id, word))
                }
                None => None,
            };
            write_nested(out, region, cx, true, members)?;
            out.push_str("</solveblock>");
        }
        RegionKind::Plot(info) => {
            no_children(region, "a plot region")?;
            let plot = source.and_then(|s| ws_child(s, "plot"));
            match (plot, cx.xml) {
                // A plot that was read and not changed keeps its source XML, byte for byte.
                (Some(plot), Some(xml)) if crate::regions::plot_info(plot) == *info => {
                    out.push_str(&raw_element(xml, plot)?)
                }
                _ => {
                    match &info.plot {
                        Some(model) => out.push_str(&super::plot::write_plot(model)?),
                        None => return Err(WriteError::Unsupported(
                            "only 2D plots can be generated, and the source has no plot to copy"
                                .to_owned(),
                        )),
                    }
                }
            }
        }
        RegionKind::Table(model) => {
            no_children(region, "a table region")?;
            out.push_str("<spec-table");
            let table = source.and_then(|s| ws_child(s, "spec-table"));
            if let (Some(table), Some(xml)) = (table, cx.xml) {
                push_unmodelled_attrs(out, xml, table, &[]);
            }
            out.push('>');
            // Each member is a `<math>` define with its own `resultRef`, as the reader expects.
            for member in &model.members {
                let expr = member.expr.as_ref().map_err(|reason| {
                    WriteError::Unsupported(format!("table member cannot be converted: {reason}"))
                })?;
                out.push_str("<math");
                if let Some(result_ref) = member.result_ref {
                    out.push_str(&format!(" resultRef=\"{result_ref}\""));
                }
                out.push('>');
                let (xml, _) = write_math_keyword(expr, None)?;
                out.push_str(&xml);
                out.push_str("</math>");
            }
            out.push_str("</spec-table>");
        }
        RegionKind::Area(area) => {
            cx.note_structure(region.id, "area");
            // UNVERIFIED: the element and attribute names are guesses (no sample has an area).
            out.push_str("<area");
            out.push_str(&format!(
                " collapsed=\"{}\" locked=\"{}\" hide-border=\"{}\"",
                area.collapsed, area.locked, area.hide_border
            ));
            let raw = source.and_then(|s| ws_child(s, "area"));
            if let (Some(raw), Some(xml)) = (raw, cx.xml) {
                push_unmodelled_attrs(out, xml, raw, &["collapsed", "locked", "hide-border"]);
            }
            out.push('>');
            if let (Some(raw), Some(xml)) = (raw, cx.xml) {
                out.push_str(&raw_children(xml, raw, &["regions"])?);
            }
            write_nested(out, region, cx, true, keyword)?;
            out.push_str("</area>");
        }
        RegionKind::Image(_) => {
            no_children(region, "an image region")?;
            cx.note_structure(region.id, "image");
            // UNVERIFIED: `<image item-idref>` is a guess (no sample has an image region).
            let item = cx.images.items.get(&region.id).ok_or_else(|| {
                WriteError::Invalid("image region has no relationship in the plan".to_owned())
            })?;
            out.push_str("<image item-idref=\"");
            push_attr(out, item);
            out.push('"');
            let raw = source.and_then(|s| ws_child(s, "image"));
            if let (Some(raw), Some(xml)) = (raw, cx.xml) {
                push_unmodelled_attrs(out, xml, raw, &["item-idref"]);
            }
            out.push_str(" />");
        }
        // A page break of the source keeps its content. A new one is written as `<page-break />`,
        // the element that is not known for certain (see `RegionKind::PageBreak`).
        RegionKind::PageBreak => {
            no_children(region, "a page break")?;
            match (source, cx.xml) {
                (Some(src), Some(xml)) => out.push_str(&raw_children(xml, src, &["region"])?),
                _ => {
                    cx.note_structure(region.id, "page break");
                    out.push_str("<page-break />");
                }
            }
        }
        RegionKind::Unsupported { reason } => {
            let (Some(src), Some(xml)) = (source, cx.xml) else {
                return Err(WriteError::Unsupported(format!(
                    "unsupported content ({reason}) has no source to copy from"
                )));
            };
            out.push_str(&raw_children(xml, src, &["region"])?);
            write_nested(out, region, cx, false, keyword)?;
        }
    }
    out.push_str("</region>");
    Ok(())
}

/// Writes the nested regions of `region`. With `wrapped`, they go in a `<regions>` element.
fn write_nested(
    out: &mut String,
    region: &RegionModel,
    cx: &Cx<'_, '_>,
    wrapped: bool,
    keyword: KeywordAt<'_>,
) -> Result<(), WriteError> {
    if wrapped {
        out.push_str("<regions>");
    }
    for child in &region.children {
        write_region(out, child, cx, keyword)?;
    }
    if wrapped {
        out.push_str("</regions>");
    }
    Ok(())
}

fn no_children(region: &RegionModel, what: &str) -> Result<(), WriteError> {
    if region.children.is_empty() {
        Ok(())
    } else {
        Err(WriteError::Invalid(format!(
            "{what} cannot hold nested regions"
        )))
    }
}

/// Appends ` name="value"` for a finite number, in the shortest form that reads back exactly.
fn push_number(out: &mut String, name: &str, value: f64) -> Result<(), WriteError> {
    if !value.is_finite() {
        return Err(WriteError::Invalid(format!(
            "`{name}` is not a finite number"
        )));
    }
    out.push_str(&format!(" {name}=\"{value}\""));
    Ok(())
}
