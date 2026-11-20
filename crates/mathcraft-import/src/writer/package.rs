//! The ZIP package: which parts are generated, which are copied from the source, and how the
//! archive is assembled.
//!
//! Generated parts: the worksheet, the worksheet relationships, the calculation settings and the
//! results when they change, the content types when a new `.XamlPackage` needs one, and the whole
//! set of package-level parts when there is no source. Every other entry of the source is copied
//! in its original order, with the same contents.

use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use std::ops::Range;

use roxmltree::Node;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use super::image;
use super::text::{self, Relationship};
use super::worksheet;
use super::xml::push_attr;
use super::{check_size, WriteError, MAX_COPIED_PART, MAX_COPIED_TOTAL};
use crate::dom::{self, CALCULATION};
use crate::package::{
    Package, CALCULATION as CALCULATION_PART, FOOTER, HEADER, MAX_AUX_PART, PRESENTATION, RESULT,
    WORKSHEET, WORKSHEET_RELS,
};
use crate::page::{PageSettings, PRESENTATION_NS};
use crate::regions::{same_evaluation, Settings, Worksheet};
use mathcraft_core::eval::ResultFormat;

/// The content types part, which is generated only when it must change.
pub(super) const ZIP_NAME_CONTENT_TYPES: &str = "[Content_Types].xml";
/// The package relationships part.
const ZIP_NAME_ROOT_RELS: &str = "_rels/.rels";
/// Byte order mark that Prime writes at the start of its XML parts.
const BOM: &str = "\u{feff}";
const XML_DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>";

/// A result file with no results. Prime calculates every region when it opens such a file.
const EMPTY_RESULTS: &str = concat!(
    r#"<resultsList xmlns:ml="http://schemas.mathsoft.com/math50" "#,
    r#"xmlns:u="http://schemas.mathsoft.com/units10" "#,
    r#"xmlns="http://schemas.mathsoft.com/result10" />"#
);

/// The fixed ZIP options: deflate, and a fixed timestamp so the output is deterministic.
pub(super) fn zip_options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(6))
        .last_modified_time(DateTime::default())
}

/// The source package: its parts, its model, and the names of its ZIP entries.
struct Source<'b> {
    bytes: &'b [u8],
    package: Package,
    model: Worksheet,
    names: HashSet<String>,
}

impl<'b> Source<'b> {
    fn open(bytes: &'b [u8]) -> Result<Self, WriteError> {
        let package = Package::from_bytes(bytes)?;
        let model = Worksheet::from_package(&package)?;
        let archive = ZipArchive::new(Cursor::new(bytes)).map_err(WriteError::Zip)?;
        let mut names = HashSet::new();
        for name in archive.file_names() {
            names.insert(name?.into_owned());
        }
        Ok(Self {
            bytes,
            package,
            model,
            names,
        })
    }

    /// The text of one source entry, without its byte order mark, or `None` when it is absent.
    fn read_text(&self, name: &str) -> Result<Option<String>, WriteError> {
        let mut archive = ZipArchive::new(Cursor::new(self.bytes))?;
        let mut entry = match archive.by_name(name) {
            Ok(entry) => entry,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let bytes = read_capped(&mut entry, name, MAX_AUX_PART)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| WriteError::Invalid(format!("`{name}` is not UTF-8 text")))?;
        Ok(Some(text.strip_prefix(BOM).unwrap_or(&text).to_owned()))
    }
}

/// Reads at most `limit` bytes of `reader`. A larger entry is [`WriteError::PartTooLarge`].
fn read_capped(reader: impl Read, part: &str, limit: u64) -> Result<Vec<u8>, WriteError> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(WriteError::PartTooLarge {
            part: part.to_owned(),
            size: bytes.len() as u64,
            limit,
        });
    }
    Ok(bytes)
}

/// A generated XML part: the byte order mark, then the body.
fn xml_part(body: &str) -> Vec<u8> {
    format!("{BOM}{body}").into_bytes()
}

/// Writes the package for `model`. See the module docs for what is generated and what is copied.
/// The warnings are the unverified file formats the package was written with.
pub(super) fn write(
    model: &Worksheet,
    source: Option<&[u8]>,
) -> Result<(Vec<u8>, Vec<String>), WriteError> {
    let source = match source {
        Some(bytes) => Some(Source::open(bytes)?),
        None => None,
    };
    let existing = match &source {
        Some(src) => existing_relationships(src)?,
        None => Vec::new(),
    };
    let taken_names = source
        .as_ref()
        .map(|src| src.names.clone())
        .unwrap_or_default();
    let taken_ids: HashSet<String> = existing.iter().map(|r| r.id.clone()).collect();
    let source_pair = source.as_ref().map(|src| (&src.package, &src.model));
    let mut plan = text::plan(model, source_pair, &taken_names, &taken_ids)?;
    // The header and footer text share the worksheet's relationships, so they follow the body's.
    let (header_plan, footer_plan) =
        text::plan_furniture(model, source_pair, &mut plan, &taken_names, &taken_ids)?;
    // The image plan runs after the text plan, so that its names and IDs avoid the text's.
    let mut names_after_text = taken_names.clone();
    names_after_text.extend(plan.new_parts.iter().map(|(name, _)| name.clone()));
    let mut ids_after_text = taken_ids.clone();
    ids_after_text.extend(plan.items.values().cloned());
    ids_after_text.extend(header_plan.items.values().cloned());
    ids_after_text.extend(footer_plan.items.values().cloned());
    let images = image::plan(
        model,
        source.as_ref().map(|src| (&src.package, &src.model)),
        &names_after_text,
        &ids_after_text,
    )?;

    let (worksheet_xml, mut warnings) = worksheet::write(
        model,
        source.as_ref().map(|src| src.package.worksheet.as_str()),
        &plan,
        &images,
    )?;

    let mut generated: Vec<(String, Vec<u8>)> = Vec::new();

    // Content types: a `Default` for each extension a new part needs and the package lacks.
    let mut needed: Vec<(&str, &str)> = Vec::new();
    if !plan.new_parts.is_empty() {
        needed.push(("XamlPackage", "application/zip"));
    }
    needed.extend(images.defaults.iter().copied());
    match &source {
        None => {
            generated.push((
                ZIP_NAME_CONTENT_TYPES.to_owned(),
                xml_part(&generated_content_types(&images.defaults)),
            ));
            generated.push((
                ZIP_NAME_ROOT_RELS.to_owned(),
                xml_part(&generated_root_rels()),
            ));
        }
        Some(src) if !needed.is_empty() => match src.read_text(ZIP_NAME_CONTENT_TYPES)? {
            Some(types) => {
                let missing: Vec<(&str, &str)> = needed
                    .iter()
                    .copied()
                    .filter(|(ext, _)| !types.contains(&format!("Extension=\"{ext}\"")))
                    .collect();
                if !missing.is_empty() {
                    generated.push((
                        ZIP_NAME_CONTENT_TYPES.to_owned(),
                        xml_part(&with_defaults(&types, &missing)?),
                    ));
                }
            }
            None => generated.push((
                ZIP_NAME_CONTENT_TYPES.to_owned(),
                xml_part(&generated_content_types(&images.defaults)),
            )),
        },
        Some(_) => {}
    }

    let mut rels = String::from(XML_DECLARATION);
    rels.push_str(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for relationship in &existing {
        rels.push_str(&relationship.raw);
    }
    for element in plan.new_rels.iter().chain(&images.new_rels) {
        rels.push_str(element);
    }
    rels.push_str("</Relationships>");
    generated.push((WORKSHEET_RELS.to_owned(), xml_part(&rels)));

    let worksheet_body = format!("{XML_DECLARATION}{worksheet_xml}");
    generated.push((WORKSHEET.to_owned(), xml_part(&worksheet_body)));

    // The header and footer parts are written when their regions changed. An unchanged part is
    // copied from the source, so a sample with an empty one keeps its bytes.
    for (footer, part_name, plan) in [(false, HEADER, &header_plan), (true, FOOTER, &footer_plan)] {
        let (changed, source_xml) = match &source {
            Some(src) => {
                let (old, xml) = if footer {
                    (&src.model.footer, src.package.footer.as_deref())
                } else {
                    (&src.model.header, src.package.header.as_deref())
                };
                let new = if footer { &model.footer } else { &model.header };
                (old != new, xml)
            }
            None => {
                let new = if footer { &model.footer } else { &model.header };
                (!new.is_empty(), None)
            }
        };
        if changed {
            let regions = if footer { &model.footer } else { &model.header };
            let (body, furniture_warnings) =
                worksheet::write_furniture(regions, source_xml, plan, footer)?;
            warnings.extend(furniture_warnings);
            generated.push((
                part_name.to_owned(),
                xml_part(&format!("{XML_DECLARATION}{body}")),
            ));
        }
    }

    let keep_results = match &source {
        Some(src) => {
            src.names.contains(RESULT)
                && src.model.regions == model.regions
                && same_evaluation(&src.model.settings, &model.settings)
        }
        None => false,
    };
    if !keep_results {
        generated.push((
            RESULT.to_owned(),
            xml_part(&format!("{XML_DECLARATION}{EMPTY_RESULTS}")),
        ));
    }

    let keep_calculation = match &source {
        Some(src) => src.package.calculation.is_some() && src.model.settings == model.settings,
        None => false,
    };
    if !keep_calculation {
        check_settings_finite(&model.settings)?;
        // A patched file is the source's own text, declaration included. A template has none yet.
        let body = match &source {
            Some(src) => match &src.package.calculation {
                Some(old) => patch_settings(old, &model.settings, &src.model.settings)?,
                None => format!(
                    "{XML_DECLARATION}{}",
                    calculation_template(&model.settings)?
                ),
            },
            None => format!(
                "{XML_DECLARATION}{}",
                calculation_template(&model.settings)?
            ),
        };
        generated.push((CALCULATION_PART.to_owned(), xml_part(&body)));
    }

    // presentation.xml carries both the result format and the page model. It is written only when
    // one of them changed; a source part keeps all of its other content, and an unchanged one is
    // copied byte for byte.
    {
        let result_changed = match &source {
            Some(src) => src.model.settings.result_format != model.settings.result_format,
            None => false,
        };
        let page_changed = match &source {
            Some(src) => src.model.page != model.page,
            None => false,
        };
        let has_source_part = match &source {
            Some(src) => src.package.presentation.is_some(),
            None => false,
        };
        if !has_source_part || result_changed || page_changed {
            let body = match &source {
                Some(src) => match &src.package.presentation {
                    Some(old_xml) => {
                        let mut body = old_xml.to_string();
                        if result_changed {
                            let (Some(new), Some(old)) = (
                                model.settings.result_format.as_ref(),
                                src.model.settings.result_format.as_ref(),
                            ) else {
                                return Err(WriteError::Settings(
                                    "presentation.xml has no result format to change".to_owned(),
                                ));
                            };
                            body = patch_presentation(&body, new, old)?;
                        }
                        if page_changed {
                            body = patch_page(&body, &model.page, &src.model.page)?;
                        }
                        body
                    }
                    None => format!("{XML_DECLARATION}{}", presentation_template(&model.page)),
                },
                None => format!("{XML_DECLARATION}{}", presentation_template(&model.page)),
            };
            generated.push((PRESENTATION.to_owned(), xml_part(&body)));
        }
    }

    for (name, bytes) in plan.new_parts.into_iter().chain(images.new_parts) {
        generated.push((name, bytes));
    }

    for (name, bytes) in &generated {
        check_size(name, bytes.len())?;
    }

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in &generated {
        writer.start_file(name.as_str(), zip_options())?;
        writer.write_all(bytes)?;
    }
    if let Some(src) = &source {
        let generated_names: HashSet<&str> = generated.iter().map(|(n, _)| n.as_str()).collect();
        copy_preserved(&mut writer, src, &generated_names)?;
    }
    Ok((writer.finish()?.into_inner(), warnings))
}

/// Copies every source entry that is not generated, with its contents, in source order.
fn copy_preserved(
    writer: &mut ZipWriter<Cursor<Vec<u8>>>,
    src: &Source<'_>,
    generated: &HashSet<&str>,
) -> Result<(), WriteError> {
    let mut archive = ZipArchive::new(Cursor::new(src.bytes))?;
    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name()?.into_owned();
        if generated.contains(name.as_str()) {
            continue;
        }
        if entry.is_dir() {
            writer.add_directory(name, zip_options())?;
            continue;
        }
        let bytes = read_capped(&mut entry, &name, MAX_COPIED_PART)?;
        total = total.saturating_add(bytes.len() as u64);
        if total > MAX_COPIED_TOTAL {
            return Err(WriteError::PartTooLarge {
                part: name,
                size: total,
                limit: MAX_COPIED_TOTAL,
            });
        }
        writer.start_file(name, zip_options())?;
        writer.write_all(&bytes)?;
    }
    Ok(())
}

/// The relationships of the source worksheet, each with its raw XML. A malformed relationships
/// part gives no relationships. The text regions then get new ones, and the part is replaced.
fn existing_relationships(src: &Source<'_>) -> Result<Vec<Relationship>, WriteError> {
    let Some(xml) = src.read_text(WORKSHEET_RELS)? else {
        return Ok(Vec::new());
    };
    let Ok(doc) = roxmltree::Document::parse(&xml) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for node in doc
        .descendants()
        .filter(|n: &Node<'_, '_>| n.is_element() && n.tag_name().name() == "Relationship")
    {
        let Some(id) = node.attribute("Id") else {
            continue;
        };
        out.push(Relationship {
            id: id.to_owned(),
            raw: xml[node.range()].to_owned(),
        });
    }
    Ok(out)
}

/// `[Content_Types].xml` of a source package, with a `Default` for each of `defaults` added.
fn with_defaults(types: &str, defaults: &[(&str, &str)]) -> Result<String, WriteError> {
    let end = types.rfind("</Types>").ok_or_else(|| {
        WriteError::Invalid("[Content_Types].xml has no closing </Types>".to_owned())
    })?;
    let mut out = types.to_owned();
    let mut added = String::new();
    for (extension, content_type) in defaults {
        added.push_str(&format!(
            "<Default Extension=\"{extension}\" ContentType=\"{content_type}\" />"
        ));
    }
    out.insert_str(end, &added);
    Ok(out)
}

/// `[Content_Types].xml` for a package written from scratch: the defaults the package always
/// has, then `extra` (the image types, when the package has images).
fn generated_content_types(extra: &[(&str, &str)]) -> String {
    let mut types = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
        "<Default Extension=\"xml\" ContentType=\"application/vnd.openxmlformats-officedocument.mathprocessingml.mathcad.main+xml\" />",
        "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\" />",
        "<Default Extension=\"XamlPackage\" ContentType=\"application/zip\" />",
        "<Override PartName=\"/mathcad/settings/calculation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.mathprocessingml.mathcad.settings.calculation+xml\" />",
        "<Override PartName=\"/mathcad/result.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.mathprocessingml.mathcad.result+xml\" />",
        "</Types>"
    ));
    let end = types.rfind("</Types>").unwrap_or(types.len());
    let added: String = extra
        .iter()
        .map(|(ext, ct)| format!("<Default Extension=\"{ext}\" ContentType=\"{ct}\" />"))
        .collect();
    types.insert_str(end, &added);
    types
}

/// `_rels/.rels` for a package written from scratch: the worksheet is the main part.
fn generated_root_rels() -> String {
    concat!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        "<Relationship Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" ",
        "Target=\"/mathcad/worksheet.xml\" Id=\"R1\" />",
        "</Relationships>"
    )
    .to_owned()
}

/// `calculation.xml` for a package written from scratch. The attributes are the ones the reader
/// reads, with the sample values for the rest. Unset settings take the defaults of Prime.
fn calculation_template(settings: &Settings) -> Result<String, WriteError> {
    let mut unit = String::new();
    push_attr(&mut unit, settings.unit_system.as_deref().unwrap_or("si"));
    Ok(format!(
        concat!(
            r#"<calculation xmlns="{ns}" cache-worksheet="true">"#,
            r#"<builtInVariables array-origin="{origin}" convergence-tolerance="{conv}" "#,
            r#"constraint-tolerance="{constraint}" prn-precision="{prn}" random-seed="1" />"#,
            r#"<calculationBehavior automatic-recalculation="true" strings-use-origin="false" />"#,
            r#"<units><currentUnitSystem name="{unit}" /></units></calculation>"#
        ),
        ns = CALCULATION,
        origin = settings.array_origin.unwrap_or(0),
        conv = settings.convergence_tolerance.unwrap_or(0.001),
        constraint = settings.constraint_tolerance.unwrap_or(0.001),
        prn = settings.prn_precision.unwrap_or(4),
        unit = unit,
    ))
}

/// Refuses a setting that is NaN or infinite: it has no XML form that reads back.
fn check_settings_finite(settings: &Settings) -> Result<(), WriteError> {
    let values = [
        ("convergence tolerance", settings.convergence_tolerance),
        ("constraint tolerance", settings.constraint_tolerance),
    ];
    for (name, value) in values {
        if let Some(value) = value {
            if !value.is_finite() {
                return Err(WriteError::Settings(format!(
                    "{name} is {value}, not a finite number"
                )));
            }
        }
    }
    Ok(())
}

/// The settings file of the source, with the attribute values of the modelled settings replaced
/// where they differ from the source. Other settings are kept as they are.
///
/// # Errors
/// [`WriteError::Settings`] when a changed setting has no attribute in the source to hold it.
fn patch_settings(xml: &str, new: &Settings, old: &Settings) -> Result<String, WriteError> {
    let doc = dom::parse(xml).map_err(|e| WriteError::Settings(e.to_string()))?;
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    if let Some(value) = new
        .array_origin
        .filter(|_| new.array_origin != old.array_origin)
    {
        settings_edit(
            &doc,
            "builtInVariables",
            "array-origin",
            &value.to_string(),
            &mut edits,
        )?;
    }
    if let Some(value) = new
        .convergence_tolerance
        .filter(|_| new.convergence_tolerance != old.convergence_tolerance)
    {
        settings_edit(
            &doc,
            "builtInVariables",
            "convergence-tolerance",
            &value.to_string(),
            &mut edits,
        )?;
    }
    if let Some(value) = new
        .constraint_tolerance
        .filter(|_| new.constraint_tolerance != old.constraint_tolerance)
    {
        settings_edit(
            &doc,
            "builtInVariables",
            "constraint-tolerance",
            &value.to_string(),
            &mut edits,
        )?;
    }
    if let Some(name) = new
        .unit_system
        .as_deref()
        .filter(|_| new.unit_system != old.unit_system)
    {
        settings_edit(&doc, "currentUnitSystem", "name", name, &mut edits)?;
    }
    if let Some(value) = new
        .prn_precision
        .filter(|_| new.prn_precision != old.prn_precision)
    {
        settings_edit(
            &doc,
            "builtInVariables",
            "prn-precision",
            &value.to_string(),
            &mut edits,
        )?;
    }
    edits.sort_by(|a, b| b.0.start.cmp(&a.0.start));
    let mut out = xml.to_owned();
    for (range, value) in edits {
        let mut escaped = String::new();
        push_attr(&mut escaped, &value);
        out.replace_range(range, &escaped);
    }
    Ok(out)
}

/// The `presentation.xml` of the source with the result format's attributes replaced where they
/// differ. A setting the file cannot hold is refused, not dropped.
///
/// # Errors
/// [`WriteError::Settings`] for a changed number format or digit grouping (no attribute of the
/// file is known to hold them), a complex threshold, or a changed attribute the source does not
/// state.
fn patch_presentation(
    xml: &str,
    new: &ResultFormat,
    old: &ResultFormat,
) -> Result<String, WriteError> {
    // Prime's schema for this part is strict: an attribute no sample states could make it refuse
    // the whole file. The change is refused rather than guessed.
    if new.number != old.number || new.digit_grouping != old.digit_grouping {
        return Err(WriteError::Settings(
            "number format and digit grouping are not stored in presentation.xml (unverified)"
                .to_owned(),
        ));
    }
    if new.complex_threshold != old.complex_threshold {
        return Err(WriteError::Settings(
            "the complex threshold has no attribute in presentation.xml".to_owned(),
        ));
    }
    if new.complex_form != old.complex_form {
        return Err(WriteError::Settings(
            "the polar complex form is not stored in presentation.xml yet".to_owned(),
        ));
    }
    let doc = dom::parse(xml).map_err(|e| WriteError::Settings(e.to_string()))?;
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut edit = |attr: &str, value: String| -> Result<(), WriteError> {
        presentation_edit(&doc, attr, &value, &mut edits)
    };
    if new.precision != old.precision {
        edit("precision", new.precision.to_string())?;
    }
    if new.trailing_zeros != old.trailing_zeros {
        edit("show-trailing-zeros", new.trailing_zeros.to_string())?;
    }
    if new.radix != old.radix {
        edit("radix", new.radix.attr().to_owned())?;
    }
    if new.zero_threshold != old.zero_threshold {
        edit("zero-threshold", new.zero_threshold.to_string())?;
    }
    if new.exponential_threshold != old.exponential_threshold {
        edit(
            "exponential-threshold",
            new.exponential_threshold.to_string(),
        )?;
    }
    if new.imaginary_j != old.imaginary_j {
        let unit = if new.imaginary_j { "j" } else { "i" };
        edit("imaginary-value", unit.to_owned())?;
    }
    edits.sort_by(|a, b| b.0.start.cmp(&a.0.start));
    let mut out = xml.to_owned();
    for (range, value) in edits {
        let mut escaped = String::new();
        push_attr(&mut escaped, &value);
        out.replace_range(range, &escaped);
    }
    Ok(out)
}

/// Queues the replacement of one attribute of the `general` element under `results`.
fn presentation_edit(
    doc: &roxmltree::Document<'_>,
    attr: &str,
    value: &str,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Result<(), WriteError> {
    let node = doc
        .descendants()
        .find(|n| {
            n.has_tag_name("general") && n.parent().is_some_and(|p| p.has_tag_name("results"))
        })
        .ok_or_else(|| {
            WriteError::Settings("presentation.xml has no results/general element".to_owned())
        })?;
    let attribute = node.attribute_node(attr).ok_or_else(|| {
        WriteError::Settings(format!("<general> has no `{attr}` attribute to set"))
    })?;
    edits.push((attribute.range_value(), value.to_owned()));
    Ok(())
}

/// Queues the replacement of one attribute value in the settings document.
fn settings_edit(
    doc: &roxmltree::Document<'_>,
    element: &str,
    attr: &str,
    value: &str,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Result<(), WriteError> {
    let node = doc
        .descendants()
        .find(|n| n.has_tag_name((CALCULATION, element)))
        .ok_or_else(|| {
            WriteError::Settings(format!(
                "calculation.xml has no <{element}> to set `{attr}` on"
            ))
        })?;
    let attribute = node.attribute_node(attr).ok_or_else(|| {
        WriteError::Settings(format!("<{element}> has no `{attr}` attribute to set"))
    })?;
    edits.push((attribute.range_value(), value.to_owned()));
    Ok(())
}

/// A new presentation part that holds only the page model.
fn presentation_template(page: &PageSettings) -> String {
    let mut out = format!(r#"<presentation xmlns="{PRESENTATION_NS}"><pageModel "#);
    for (attr, value) in [
        ("paper-code", page.paper_code.as_str()),
        ("grid-size", page.grid_size.as_str()),
        ("orientation", page.orientation.as_str()),
        ("margin-type", page.margin_type.as_str()),
    ] {
        out.push_str(attr);
        out.push_str("=\"");
        push_attr(&mut out, value);
        out.push_str("\" ");
    }
    out.push_str("/></presentation>");
    out
}

/// The presentation part of the source, with the `pageModel` attributes that differ from the
/// source replaced. All other content is kept as it is.
///
/// # Errors
/// [`WriteError::Settings`] when a changed attribute is missing from the source's `pageModel`.
fn patch_page(xml: &str, new: &PageSettings, old: &PageSettings) -> Result<String, WriteError> {
    let doc = dom::parse(xml).map_err(|e| WriteError::Settings(e.to_string()))?;
    let node = doc
        .descendants()
        .find(|n| n.has_tag_name((PRESENTATION_NS, "pageModel")))
        .ok_or_else(|| WriteError::Settings("presentation.xml has no <pageModel>".to_owned()))?;
    let pairs = [
        ("paper-code", &new.paper_code, &old.paper_code),
        ("orientation", &new.orientation, &old.orientation),
        ("margin-type", &new.margin_type, &old.margin_type),
        ("grid-size", &new.grid_size, &old.grid_size),
    ];
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for (attr, value, previous) in pairs {
        if value == previous {
            continue;
        }
        let attribute = node.attribute_node(attr).ok_or_else(|| {
            WriteError::Settings(format!("<pageModel> has no `{attr}` attribute to set"))
        })?;
        let mut escaped = String::new();
        push_attr(&mut escaped, value);
        edits.push((attribute.range_value(), escaped));
    }
    edits.sort_by(|a, b| b.0.start.cmp(&a.0.start));
    let mut out = xml.to_owned();
    for (range, value) in edits {
        out.replace_range(range, &value);
    }
    Ok(out)
}
