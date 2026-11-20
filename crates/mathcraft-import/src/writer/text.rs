//! Text regions: which `.XamlPackage` each one uses, and the XAML written for new ones.
//!
//! A text region names a worksheet relationship (`item-idref`), and the relationship names a
//! `.XamlPackage` whose `Xaml/Document.xaml` holds the text. [`plan`] decides, for each text
//! region of the model, whether it keeps its source relationship and part (when the text is
//! unchanged) or gets a new part. New parts take the first free `mathcad/xaml/FlowDocumentN`
//! name, and their relationship IDs come from a hash of the region and text, so the same model
//! always gets the same names.
//!
//! The reader joins paragraphs with `\n` and drops empty paragraphs at the end (see
//! `rich_text::parse`). [`document_xml`] writes the text so that it reads back unchanged: one
//! paragraph per line, and the newlines at the end of the text become `LineBreak`s inside the
//! last paragraph.

use std::collections::{HashMap, HashSet};
use std::io::Write;

use roxmltree::Node;

use super::package::{zip_options, ZIP_NAME_CONTENT_TYPES};
use super::xml::{check_chars, push_attr, push_text};
use super::WriteError;
use crate::dom::{self, WS};
use crate::package::{Package, MAX_AUX_PART};
use crate::regions::{RegionKind, RegionModel, Worksheet};
use crate::rich_text::{Align, RichText, RunStyle, TextBody};

/// The relationship type of a text region's flow document, as the samples write it.
pub(super) const FLOW_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/flowDocument";
/// Namespace of the XAML that holds a text region's text.
const XAML_NS: &str = "http://schemas.microsoft.com/winfx/2006/xaml/presentation";

/// The text regions of a written worksheet, and the parts and relationships they need.
#[derive(Debug, Default)]
pub(super) struct TextPlan {
    /// The `item-idref` of each text region, by region ID.
    pub items: HashMap<u32, String>,
    /// Relationship elements to add to the worksheet relationships, as raw XML.
    pub new_rels: Vec<String>,
    /// Name and bytes of each new `.XamlPackage`.
    pub new_parts: Vec<(String, Vec<u8>)>,
}

/// One relationship of the source worksheet: its `Id` and its raw XML.
#[derive(Debug, Clone)]
pub(super) struct Relationship {
    pub id: String,
    pub raw: String,
}

/// Plans the text regions of `model`. `source` is the source package and its model, when there
/// is one. `taken_names` and `taken_ids` are the part names and relationship IDs already in use.
pub(super) fn plan(
    model: &Worksheet,
    source: Option<(&Package, &Worksheet)>,
    taken_names: &HashSet<String>,
    taken_ids: &HashSet<String>,
) -> Result<TextPlan, WriteError> {
    let source_items = match source {
        Some((pkg, _)) => source_items(&pkg.worksheet)?,
        None => HashMap::new(),
    };
    let source_regions: Vec<&RegionModel> = match source {
        Some((_, src_model)) => src_model.all_regions(),
        None => Vec::new(),
    };
    let mut plan = TextPlan::default();
    let mut names = taken_names.clone();
    let mut ids = taken_ids.clone();
    plan_regions(
        &mut plan,
        &model.all_regions(),
        &source_regions,
        &source_items,
        source.map(|(pkg, _)| pkg),
        &mut names,
        &mut ids,
    )?;
    Ok(plan)
}

/// Plans the text regions of the page header and footer, adding their parts and relationships to
/// `plan`. Each part's `item-idref`s go in the returned plan, the header's first and the footer's
/// second. The worksheet relationships are shared with the body, so the names and IDs of the body
/// plan are taken.
pub(super) fn plan_furniture(
    model: &Worksheet,
    source: Option<(&Package, &Worksheet)>,
    plan: &mut TextPlan,
    taken_names: &HashSet<String>,
    taken_ids: &HashSet<String>,
) -> Result<(TextPlan, TextPlan), WriteError> {
    let mut names = taken_names.clone();
    names.extend(plan.new_parts.iter().map(|(name, _)| name.clone()));
    let mut ids = taken_ids.clone();
    ids.extend(plan.items.values().cloned());
    let mut out = Vec::with_capacity(2);
    for footer in [false, true] {
        let regions: Vec<&RegionModel> = if footer {
            model.footer.iter().collect()
        } else {
            model.header.iter().collect()
        };
        let (source_regions, source_items): (Vec<&RegionModel>, HashMap<u32, String>) = match source
        {
            Some((pkg, src_model)) => {
                let (src_regions, xml) = if footer {
                    (&src_model.footer, &pkg.footer)
                } else {
                    (&src_model.header, &pkg.header)
                };
                let items = match xml {
                    Some(xml) => source_items(xml)?,
                    None => HashMap::new(),
                };
                (src_regions.iter().collect(), items)
            }
            None => (Vec::new(), HashMap::new()),
        };
        let mut part = TextPlan::default();
        plan_regions(
            &mut part,
            &regions,
            &source_regions,
            &source_items,
            source.map(|(pkg, _)| pkg),
            &mut names,
            &mut ids,
        )?;
        // The new relationships and parts go to the body's lists: one worksheet relationships part
        // names them all, and the returned plan keeps only the item IDs of its own regions.
        plan.new_rels.append(&mut part.new_rels);
        plan.new_parts.append(&mut part.new_parts);
        out.push(part);
    }
    let footer = out.pop().unwrap_or_default();
    let header = out.pop().unwrap_or_default();
    Ok((header, footer))
}

/// Plans each text region of `regions`. A region keeps its source relationship when its text is
/// unchanged; otherwise it gets a new part and relationship, which are added to `plan`. `names`
/// and `ids` are the part names and relationship IDs in use, and are extended.
fn plan_regions(
    plan: &mut TextPlan,
    regions: &[&RegionModel],
    source_regions: &[&RegionModel],
    source_items: &HashMap<u32, String>,
    pkg: Option<&Package>,
    names: &mut HashSet<String>,
    ids: &mut HashSet<String>,
) -> Result<(), WriteError> {
    let mut source_by_id = HashMap::new();
    for region in source_regions {
        source_by_id.entry(region.id).or_insert(*region);
    }
    let mut next_number = 1usize;
    for region in regions {
        let RegionKind::Text(text) = &region.kind else {
            continue;
        };
        check_chars(text).map_err(|e| e.in_region(region.id))?;
        if let Some(item) = reusable_item(region.id, text, pkg, &source_by_id, source_items) {
            plan.items.insert(region.id, item);
            continue;
        }
        let name = loop {
            let candidate = format!("mathcad/xaml/FlowDocument{next_number}.XamlPackage");
            next_number += 1;
            if !names.contains(&candidate) {
                break candidate;
            }
        };
        let id = fresh_id(region.id, text, ids);
        plan.new_rels.push(format!(
            "<Relationship Type=\"{FLOW_TYPE}\" Target=\"/{name}\" Id=\"{id}\" />"
        ));
        plan.new_parts
            .push((name.clone(), xaml_package(text.rich())?));
        names.insert(name);
        plan.items.insert(region.id, id);
    }
    Ok(())
}

/// The source relationship a text region keeps. It is kept when the source region has the same
/// text, and the source package has a readable flow document for that relationship.
fn reusable_item(
    region_id: u32,
    text: &TextBody,
    pkg: Option<&Package>,
    source_by_id: &HashMap<u32, &RegionModel>,
    source_items: &HashMap<u32, String>,
) -> Option<String> {
    let pkg = pkg?;
    let src = source_by_id.get(&region_id)?;
    match &src.kind {
        RegionKind::Text(src_text) if src_text.rich() == text.rich() => {}
        _ => return None,
    }
    let item = source_items.get(&region_id)?;
    pkg.flow_document(item).map(|_| item.clone())
}

/// The `item-idref` of each text region in a source worksheet, by region ID. A text element
/// without an ID is skipped.
pub(super) fn source_items(worksheet_xml: &str) -> Result<HashMap<u32, String>, WriteError> {
    let doc = dom::parse(worksheet_xml)
        .map_err(|e| WriteError::Source(crate::regions::ImportError::Dom(e)))?;
    let mut items = HashMap::new();
    for region in doc.descendants().filter(|n| super::xml::is_region(*n)) {
        let Some(id) = region
            .attribute("region-id")
            .and_then(|v| v.trim().parse::<u32>().ok())
        else {
            continue;
        };
        if let Some(item) = text_item_of(region) {
            items.entry(id).or_insert(item);
        }
    }
    Ok(items)
}

/// The `item-idref` of the `<text>` element directly under `region`, if there is one.
fn text_item_of(region: Node<'_, '_>) -> Option<String> {
    region
        .children()
        .find(|c| {
            c.is_element() && c.tag_name().namespace() == Some(WS) && c.tag_name().name() == "text"
        })
        .and_then(|text| text.attribute("item-idref").map(str::to_owned))
}

/// A relationship ID that is not in `taken` yet. It is a hash of the region and text, so the
/// same region and text always get the same ID. The ID is recorded in `taken`.
pub(super) fn fresh_id(region_id: u32, text: &str, taken: &mut HashSet<String>) -> String {
    let mut salt = 0u32;
    loop {
        let id = format!(
            "R{:016x}",
            fnv1a(format!("{region_id}:{salt}:{text}").as_bytes())
        );
        if taken.insert(id.clone()) {
            return id;
        }
        salt += 1;
    }
}

/// 64-bit FNV-1a. A fixed, portable hash, so the IDs do not depend on the platform.
pub(super) fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The `Xaml/Document.xaml` of a rich text. Each line that is not broken starts a `Paragraph`;
/// a broken line (one that starts with a line break) is a `LineBreak` in the paragraph before
/// it. Runs whose style differs from the base style set their formatting on the `Run`.
///
/// The text reads back unchanged: [`RichText::from_plain`] marks the trailing newlines as broken
/// lines, so they are written as `LineBreak`s in the last paragraph, as the reader expects.
pub(super) fn document_xml(rich: &RichText) -> Result<String, WriteError> {
    let mut out = String::new();
    match &rich.section_attrs {
        None => out.push_str(&format!(
            "<Section xmlns=\"{XAML_NS}\" xml:space=\"preserve\" FontFamily=\"Tahoma\" \
             FontSize=\"14.666666666666668\" Foreground=\"#FF000000\">"
        )),
        Some(attrs) => {
            out.push_str(&format!("<Section xmlns=\"{XAML_NS}\""));
            for (name, value) in attrs {
                out.push(' ');
                out.push_str(name);
                out.push_str("=\"");
                push_attr(&mut out, value);
                out.push('"');
            }
            out.push('>');
        }
    }
    let mut open = false;
    for line in &rich.lines {
        if !open || !line.broken {
            if open {
                out.push_str("</Paragraph>");
            }
            out.push_str("<Paragraph");
            if line.align != Align::Left {
                out.push_str(&format!(" TextAlignment=\"{}\"", line.align.xaml_name()));
            }
            out.push('>');
            open = true;
        } else {
            out.push_str("<LineBreak/>");
        }
        for run in &line.runs {
            check_chars(&run.text)?;
            // UNVERIFIED: the `Hyperlink`/`NavigateUri` shape is the XAML one; no sample has a link.
            let link = match run.style.link.as_deref() {
                Some(uri) if !uri.is_empty() => {
                    check_chars(uri)?;
                    out.push_str("<Hyperlink NavigateUri=\"");
                    push_attr(&mut out, uri);
                    out.push_str("\">");
                    true
                }
                _ => false,
            };
            out.push_str("<Run");
            push_run_attrs(&mut out, &run.style, &rich.base);
            out.push('>');
            push_text(&mut out, &run.text);
            out.push_str("</Run>");
            if link {
                out.push_str("</Hyperlink>");
            }
        }
    }
    if open {
        out.push_str("</Paragraph>");
    }
    out.push_str("</Section>");
    Ok(out)
}

/// The formatting attributes of a `Run` that differ from the base style.
fn push_run_attrs(out: &mut String, style: &RunStyle, base: &RunStyle) {
    if style.family != base.family {
        out.push_str(" FontFamily=\"");
        push_attr(out, &style.family);
        out.push('"');
    }
    if style.size != base.size {
        out.push_str(&format!(" FontSize=\"{}\"", style.size));
    }
    if style.bold != base.bold {
        out.push_str(if style.bold {
            " FontWeight=\"Bold\""
        } else {
            " FontWeight=\"Normal\""
        });
    }
    if style.italic != base.italic {
        out.push_str(if style.italic {
            " FontStyle=\"Italic\""
        } else {
            " FontStyle=\"Normal\""
        });
    }
    if style.underline != base.underline {
        out.push_str(if style.underline {
            " TextDecorations=\"Underline\""
        } else {
            " TextDecorations=\"None\""
        });
    }
    if style.colour != base.colour {
        let [r, g, b, a] = style.colour;
        out.push_str(&format!(" Foreground=\"#{a:02X}{r:02X}{g:02X}{b:02X}\""));
    }
}

/// A new `.XamlPackage` (a ZIP) holding `text`, with the three entries of the samples.
pub(super) fn xaml_package(rich: &RichText) -> Result<Vec<u8>, WriteError> {
    let document = document_xml(rich)?;
    if document.len() as u64 > MAX_AUX_PART {
        return Err(WriteError::PartTooLarge {
            part: "Xaml/Document.xaml".to_owned(),
            size: document.len() as u64,
            limit: MAX_AUX_PART,
        });
    }
    let content_types = concat!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
        "<Default Extension=\"xaml\" ContentType=\"application/vnd.ms-wpf.xaml+xml\" />",
        "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\" />",
        "</Types>"
    );
    let rels = concat!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        "<Relationship Type=\"http://schemas.microsoft.com/wpf/2005/10/xaml/entry\" ",
        "Target=\"/Xaml/Document.xaml\" Id=\"R1\" />",
        "</Relationships>"
    );
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip_options();
    for (name, body) in [
        (ZIP_NAME_CONTENT_TYPES, content_types),
        ("_rels/.rels", rels),
        ("Xaml/Document.xaml", document.as_str()),
    ] {
        writer.start_file(name, options)?;
        writer.write_all(body.as_bytes())?;
    }
    Ok(writer.finish()?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_ids_are_unique_and_stable() {
        let mut taken = HashSet::new();
        let a = fresh_id(3, "t", &mut taken);
        let b = fresh_id(3, "t", &mut taken);
        assert_ne!(a, b);
        let mut again = HashSet::new();
        assert_eq!(fresh_id(3, "t", &mut again), a);
    }

    #[test]
    fn styled_text_round_trips_through_xaml() {
        let xaml = concat!(
            "<Section xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\" ",
            "xml:space=\"preserve\" FontFamily=\"Tahoma\" FontSize=\"14\" Foreground=\"#FF000000\">",
            "<Paragraph TextAlignment=\"Center\"><Run>plain </Run>",
            "<Bold><Run FontStyle=\"Italic\" Foreground=\"#FFFF0000\" ",
            "TextDecorations=\"Underline\">x &amp; y</Run></Bold></Paragraph>",
            "<Paragraph><Run FontSize=\"20\">big</Run><LineBreak/><Run>next</Run></Paragraph>",
            "</Section>"
        );
        let rich = crate::rich_text::parse(xaml).expect("parses");
        let written = document_xml(&rich).expect("writes");
        let again = crate::rich_text::parse(&written).expect("reads back");
        assert_eq!(again.plain(), rich.plain());
        assert_eq!(again.lines, rich.lines);
        assert_eq!(again.base, rich.base);
        assert!(written.contains("TextAlignment=\"Center\""));
        assert!(written.contains("FontWeight=\"Bold\""));
    }

    #[test]
    fn hyperlinks_round_trip_through_xaml() {
        let xaml = concat!(
            "<Section xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\" ",
            "xml:space=\"preserve\" FontFamily=\"Tahoma\" FontSize=\"14\" Foreground=\"#FF000000\">",
            "<Paragraph><Run>go </Run><Hyperlink NavigateUri=\"https://ex.org/?a=1&amp;b=&quot;2&quot;\">",
            "<Run FontWeight=\"Bold\">link &amp; more</Run></Hyperlink></Paragraph>",
            "</Section>"
        );
        let rich = crate::rich_text::parse(xaml).expect("parses");
        let written = document_xml(&rich).expect("writes");
        assert!(
            written.contains("<Hyperlink NavigateUri=\"https://ex.org/?a=1&amp;b=&quot;2&quot;\">")
        );
        let again = crate::rich_text::parse(&written).expect("reads back");
        assert_eq!(again.lines, rich.lines);
        assert_eq!(
            again.lines[0].runs[1].style.link,
            rich.lines[0].runs[1].style.link
        );
        assert!(again.lines[0].runs[0].style.link.is_none());
    }
}
