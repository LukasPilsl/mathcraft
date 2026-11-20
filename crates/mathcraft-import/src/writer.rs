//! Writer for `.mcdx` worksheet packages: the inverse of the reader (`regions.rs`, `expr.rs`).
//!
//! [`write_package`] turns a [`Worksheet`] model into the bytes of a `.mcdx` ZIP. The model is
//! the one the reader builds, so a sheet read from a file and written back reads to an equal
//! model again (see `tests/writer_samples.rs`).
//!
//! # What is written, and what is copied
//!
//! * `mathcad/worksheet.xml` is always regenerated from the model. Each `<math>` is written from
//!   its [`Expr`] (see [`expr`]), each `<text>` from its text, and every region gets its
//!   `region-id`, position, size, `resultRef` and `solve-block-category` from the model.
//! * Parts the model does not understand are copied byte for byte from the source package:
//!   headers, footers, document properties, presentation settings, integration, images, unknown
//!   XML parts, and every `.XamlPackage` whose text is unchanged. The ZIP entries are recompressed,
//!   but their contents are identical.
//! * Content that the model keeps as [`RegionKind::Unsupported`] is copied from the source region,
//!   element by element. A [`RegionKind::Plot`] that reads equal to the source's plot is copied
//!   too. A 2D plot that is new or changed is generated from its model (see `plot.rs`); a 3D plot
//!   without a matching source is an error.
//! * Attributes and child elements of a region that the model does not read (for example
//!   `width`, `height`, `text-caret-position`, and a `math` element's `resultFormat`) are copied
//!   from the source region with the same `region-id`.
//! * `mathcad/result.xml` is copied when the regions and settings are unchanged. Otherwise it is
//!   written empty (`<resultsList/>`), so Prime calculates every region when the file is opened.
//!   Results are never computed here.
//! * `mathcad/settings/calculation.xml` is copied when the settings are unchanged. Otherwise
//!   the attribute values of the four modelled settings are replaced in place, and the other
//!   settings in the file are kept.
//!
//! A worksheet built without a source gets a generated package, described below.
//!
//! # Format notes: what was inferred
//!
//! The reader is the spec for everything below. The points marked *inferred* are not confirmed by
//! a sample file, and are not confirmed by Prime. Check them by opening a written file in Prime.
//!
//! * Each `ml:apply` operator and each operand order follows the reader's mapping table in
//!   `expr.rs`. Mathcad writes the same elements (checked against the samples).
//! * A subscripted name is an `ml:id` whose only child is an XAML `Span` with a `pw:Subscript`
//!   (the sample shape, see `expr.rs`).
//! * A text region is `<text item-idref="...">`, followed by its inline `FlowDocument` element
//!   and a `<regions>` wrapper for nested regions. The text itself is in the
//!   `mathcad/xaml/FlowDocumentN.XamlPackage` that the worksheet relationship names (samples).
//!   *Inferred*: a new text region's inline `FlowDocument` attributes (Tahoma, 14.67 pt, black).
//! * A new `.XamlPackage` is a ZIP with `[Content_Types].xml`, `_rels/.rels` (entry relationship
//!   to `/Xaml/Document.xaml`) and `Xaml/Document.xaml`, as in the samples. The document is one
//!   `Section` with one `Paragraph` per line. A `\n` that ends the text, or a run of them, becomes
//!   a `LineBreak`, since the reader drops empty trailing paragraphs. *Inferred*: the section's
//!   font attributes.
//! * A solve block is `<solveblock>` with its member regions in a `<regions>` wrapper (samples).
//!   The solve block's keyword is not stored: it is derived from the members on reading, so the
//!   writer ignores [`SolveBlock::keyword`].
//! * A new (no-source) package has `[Content_Types].xml`, `_rels/.rels`, the worksheet,
//!   `mathcad/_rels/worksheet.xml.rels`, `mathcad/result.xml` and `mathcad/settings/calculation.xml`.
//!   *Inferred*: the calculation template (the attributes are the ones the reader reads, with the
//!   sample values for the rest) and the minimal `_rels/.rels` (the worksheet only).
//! * Every generated XML part starts with a UTF-8 byte order mark, as Prime writes them.
//! * The output is deterministic: entries are written in a fixed order, ZIP timestamps are fixed
//!   at 1980-01-01, and new part names and relationship IDs are derived from the content.
//!
//! # Limits
//!
//! The output is checked against the reader's size caps ([`crate::package`]), so every written
//! file can be read back. Regions and text are capped as the reader caps them.

use std::collections::HashSet;

use thiserror::Error;

use crate::package::{PackageError, MAX_AUX_PART, MAX_MAIN_PART};
use crate::regions::{
    ImportError, RegionKind, RegionModel, Worksheet, MAX_REGIONS, MAX_REGION_DEPTH, MAX_TEXT_TOTAL,
};

pub mod expr;
mod image;
mod package;
mod plot;
mod result_format;
mod text;
mod worksheet;
mod xml;

#[cfg(test)]
mod tests;

/// Largest total decompressed size, in bytes, of all preserved source entries copied into one
/// package. A larger total fails the write with [`WriteError::PartTooLarge`].
pub const MAX_COPIED_TOTAL: u64 = 512 * 1024 * 1024;

/// Largest decompressed size, in bytes, of one preserved source entry that is copied. Parts the
/// reader does not read have no cap of their own, so the writer applies this one. A larger entry
/// fails the write with [`WriteError::PartTooLarge`].
pub const MAX_COPIED_PART: u64 = 256 * 1024 * 1024;

/// Errors from writing a package.
#[derive(Debug, Error)]
pub enum WriteError {
    /// The model holds content the writer cannot represent yet, or cannot copy from its source.
    /// The message names the region.
    #[error("cannot write: {0}")]
    Unsupported(String),
    /// The model is not valid: a duplicate region ID, an empty or untrimmed name, a number that
    /// does not read back, text with characters XML cannot hold, and so on.
    #[error("invalid worksheet model: {0}")]
    Invalid(String),
    /// The source package could not be read.
    #[error("cannot read the source package: {0}")]
    Source(#[from] ImportError),
    /// A part to write, or a source part to copy, is larger than its limit.
    #[error("`{part}` is {size} bytes, over the {limit}-byte limit")]
    PartTooLarge {
        /// The part name.
        part: String,
        /// Its size in bytes.
        size: u64,
        /// The limit it exceeds.
        limit: u64,
    },
    /// A calculation setting cannot be written into the source's `calculation.xml`.
    #[error("settings: {0}")]
    Settings(String),
    /// The ZIP archive could not be written or read.
    #[error("ZIP: {0}")]
    Zip(#[from] zip::result::ZipError),
    /// An I/O error while building the archive in memory.
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
}

impl WriteError {
    /// Prefixes an [`WriteError::Unsupported`] or [`WriteError::Invalid`] message with the region
    /// it came from. Other errors pass through unchanged.
    pub(super) fn in_region(self, region_id: u32) -> Self {
        match self {
            WriteError::Unsupported(msg) => {
                WriteError::Unsupported(format!("region {region_id}: {msg}"))
            }
            WriteError::Invalid(msg) => WriteError::Invalid(format!("region {region_id}: {msg}")),
            other => other,
        }
    }
}

/// Writes `model` as the bytes of a `.mcdx` ZIP.
///
/// With `source` (the bytes of the package the model was read from), parts the model does not
/// understand are copied from it (see the module docs). Without a source, the package is generated
/// from the model alone.
///
/// # Errors
/// [`WriteError::Invalid`] for a model that is not valid, [`WriteError::Unsupported`] for content
/// the writer cannot represent, [`WriteError::Source`] when `source` is not a readable package,
/// [`WriteError::PartTooLarge`] when a part is over its size limit, and the ZIP and I/O errors.
pub fn write_package(model: &Worksheet, source: Option<&[u8]>) -> Result<Vec<u8>, WriteError> {
    write_package_with_report(model, source).map(|(bytes, _)| bytes)
}

/// Writes `model` like [`write_package`], and also returns the warnings: one for each region
/// written with a file format that is not verified against Prime (the result number format and
/// the digit grouping, see `documentation/UNVERIFIED_FORMATS.md`). Empty when nothing unverified
/// was written. The bytes are the same as [`write_package`]'s.
///
/// # Errors
/// As [`write_package`].
pub fn write_package_with_report(
    model: &Worksheet,
    source: Option<&[u8]>,
) -> Result<(Vec<u8>, Vec<String>), WriteError> {
    validate(model)?;
    package::write(model, source)
}

/// Checks the model-wide rules: region IDs are unique, the regions and the text are within the
/// reader's limits.
fn validate(model: &Worksheet) -> Result<(), WriteError> {
    // Depth first: `all_regions` recurses, so a deep model must be refused before it is walked.
    check_region_depth(&model.regions, 0)?;
    let all = model.all_regions();
    if all.len() > MAX_REGIONS {
        return Err(WriteError::Invalid(format!(
            "more than {MAX_REGIONS} regions"
        )));
    }
    let mut seen = HashSet::new();
    let mut text_total: usize = 0;
    for region in &all {
        if !seen.insert(region.id) {
            return Err(WriteError::Invalid(format!(
                "region id {} is used more than once",
                region.id
            )));
        }
        if let RegionKind::Text(text) = &region.kind {
            text_total = text_total.saturating_add(text.len());
        }
    }
    if text_total > MAX_TEXT_TOTAL {
        return Err(WriteError::Invalid(format!(
            "text of text regions is more than {MAX_TEXT_TOTAL} bytes in total"
        )));
    }
    Ok(())
}

/// Refuses region nesting the reader refuses: a region at `depth` (top level is 0) past
/// [`MAX_REGION_DEPTH`]. Recursion stops at the limit.
fn check_region_depth(regions: &[RegionModel], depth: usize) -> Result<(), WriteError> {
    for region in regions {
        if depth > MAX_REGION_DEPTH {
            return Err(WriteError::Invalid(format!(
                "regions nested deeper than {MAX_REGION_DEPTH} levels"
            )));
        }
        check_region_depth(&region.children, depth + 1)?;
    }
    Ok(())
}

/// The size limit the reader applies to a generated part of this name.
fn limit_of(part: &str) -> u64 {
    use crate::package::{CALCULATION, RESULT, WORKSHEET};
    if part == WORKSHEET || part == RESULT || part == CALCULATION {
        MAX_MAIN_PART
    } else {
        MAX_AUX_PART
    }
}

/// Checks a generated part against the reader's limit for it.
fn check_size(part: &str, size: usize) -> Result<(), WriteError> {
    let limit = limit_of(part);
    if size as u64 > limit {
        return Err(WriteError::PartTooLarge {
            part: part.to_owned(),
            size: size as u64,
            limit,
        });
    }
    Ok(())
}

impl From<PackageError> for WriteError {
    fn from(err: PackageError) -> Self {
        WriteError::Source(ImportError::Package(err))
    }
}
