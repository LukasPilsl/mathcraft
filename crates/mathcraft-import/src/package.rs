//! ZIP package access for `.mcdx` files.
//!
//! A `.mcdx` is a ZIP of XML parts (see `documentation/MCDX_FORMAT.md`, section 1). This module
//! reads the parts Mathcraft needs as UTF-8 strings. It parses no XML except the worksheet
//! relationships, which say where each text region's flow document is stored.
//!
//! Every part is read through a size cap, so a hostile archive with a small file size and a
//! huge decompressed entry cannot exhaust memory. See [`MAX_MAIN_PART`], [`MAX_AUX_PART`] and
//! [`MAX_FLOW_TOTAL`].
//!
//! # Worst-case memory
//!
//! The caps bound the decompressed text held by [`Package`] to about 320 MiB: four main parts
//! (`worksheet.xml`, `result.xml`, `calculation.xml`, `presentation.xml`) at [`MAX_MAIN_PART`]
//! each, which is 256 MiB, plus the flow documents at [`MAX_FLOW_TOTAL`], which is 64 MiB. The
//! region model is built on top of this and is bounded separately (see
//! [`crate::regions::MAX_REGIONS`] and [`crate::regions::MAX_TEXT_TOTAL`]). These are limits on the
//! decompressed text, not a measured resident size.
//!
//! A single flow document over [`MAX_AUX_PART`] is not skipped. It aborts the whole import with
//! [`PackageError::PartTooLarge`], by design: a sheet with an oversized text region is treated as
//! damaged input, not as a sheet with one region missing.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Cursor, Read, Seek};
use std::path::Path;
use std::sync::Arc;

use thiserror::Error;
use zip::result::ZipError;
use zip::ZipArchive;

/// Part name of the worksheet: regions and their math.
pub const WORKSHEET: &str = "mathcad/worksheet.xml";
/// Part name of the worksheet's relationships. Each `<text item-idref>` names one of its `Id`s.
pub const WORKSHEET_RELS: &str = "mathcad/_rels/worksheet.xml.rels";
/// Part inside a `.XamlPackage` that holds the flow document (the text, as `<Run>` elements).
const FLOW_DOCUMENT_PART: &str = "Xaml/Document.xaml";
/// Part name of the calculated results, matched to regions by number.
pub const RESULT: &str = "mathcad/result.xml";
/// Part name of the calculation and unit settings.
pub const CALCULATION: &str = "mathcad/settings/calculation.xml";
/// Part name of the display settings.
pub const PRESENTATION: &str = "mathcad/settings/presentation.xml";
/// Part name of the page header.
pub const HEADER: &str = "mathcad/header.xml";
/// Part name of the page footer.
pub const FOOTER: &str = "mathcad/footer.xml";
/// Part name of the links to other programs.
pub const INTEGRATION: &str = "mathcad/integration.xml";
/// Part name of the document properties (title, author, dates).
pub const CORE_PROPS: &str = "docProps/core.xml";
/// Part name of the application properties.
pub const APP_PROPS: &str = "docProps/app.xml";

/// Largest decompressed size, in bytes, of a main part: `worksheet.xml`, `result.xml`,
/// `calculation.xml` and `presentation.xml`. Real worksheets are far smaller; a part over the
/// cap is rejected with [`PackageError::PartTooLarge`] rather than read into memory.
pub const MAX_MAIN_PART: u64 = 64 * 1024 * 1024;

/// Largest decompressed size, in bytes, of an auxiliary part: each `.XamlPackage`, its
/// `Xaml/Document.xaml`, the worksheet relationships, the header, footer, integration and
/// document-property parts.
pub const MAX_AUX_PART: u64 = 4 * 1024 * 1024;

/// Largest total size, in bytes, of all distinct flow documents (their `Xaml/Document.xaml`
/// text) together. Each distinct flow document counts once, however many relationship `Id`s
/// link to it.
pub const MAX_FLOW_TOTAL: u64 = 64 * 1024 * 1024;

/// Largest total size, in bytes, of the image parts read into [`Package::images`].
pub const MAX_IMAGE_TOTAL: u64 = 64 * 1024 * 1024;

/// Largest number of worksheet relationships examined. Relationships past this count are
/// ignored, so their text regions report that the flow document is not in the package. Real
/// worksheets have a few hundred at most.
pub const MAX_RELATIONSHIPS: usize = 100_000;

/// Largest number of entries in the `.mcdx` archive. A real worksheet has a few dozen parts; more
/// is a crafted archive, refused with [`PackageError::TooManyEntries`] before any part is read.
pub const MAX_ENTRIES: usize = 10_000;

/// Errors from opening a `.mcdx` package.
#[derive(Debug, Error)]
pub enum PackageError {
    /// The file could not be read.
    #[error("cannot read package: {0}")]
    Io(#[from] std::io::Error),
    /// The bytes are not a readable ZIP archive, or a part could not be decompressed.
    #[error("invalid ZIP package: {0}")]
    Zip(#[from] ZipError),
    /// The archive holds more than [`MAX_ENTRIES`] entries.
    #[error("the archive has more than {limit} entries")]
    TooManyEntries {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// A required part is absent from the archive.
    #[error("required part missing: {0}")]
    MissingPart(&'static str),
    /// A part, or the total of the flow documents, decompresses to more than its size limit.
    #[error("`{part}` is larger than the {limit}-byte limit")]
    PartTooLarge {
        /// The part name, or `flow documents` for the total of the flow documents.
        part: String,
        /// The limit that was exceeded, in bytes.
        limit: u64,
    },
}

/// The XML parts of one `.mcdx` package, as UTF-8 text.
///
/// Only `worksheet.xml` is required. Every other part is `None` when the archive does not
/// contain it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Package {
    /// `mathcad/worksheet.xml`, the regions and their math.
    pub worksheet: String,
    /// Flow documents of the worksheet's text regions, keyed by relationship `Id` (the value of
    /// a `<text item-idref>`). Each value is the `Xaml/Document.xaml` text of the linked
    /// `.XamlPackage`. Relationships that link to the same target share one text. A relationship
    /// whose target is missing, or is not a readable package, has no entry here; see
    /// [`Package::flow_problem`]. Read it with [`Package::flow_document`].
    pub flow_documents: BTreeMap<String, Arc<str>>,
    /// The reason a flow document could not be read, keyed by relationship `Id`, for targets that
    /// exist but are not readable packages (or whose `Xaml/Document.xaml` cannot be read). A
    /// missing target has no entry here.
    pub flow_problems: BTreeMap<String, Arc<str>>,
    /// The bytes of the image parts the worksheet relationships link to (`.png`, `.jpg`,
    /// `.jpeg`), keyed by relationship `Id` (the value of an `<image item-idref>`). An image over
    /// [`MAX_AUX_PART`], or past [`MAX_IMAGE_TOTAL`] in all, has no entry. Read it with
    /// [`Package::image`]. UNVERIFIED: no sample has an image part (see `UNVERIFIED_FORMATS.md`).
    pub images: BTreeMap<String, Arc<[u8]>>,
    /// `mathcad/result.xml`, the stored results.
    pub result: Option<String>,
    /// `mathcad/settings/calculation.xml`.
    pub calculation: Option<String>,
    /// `mathcad/settings/presentation.xml`.
    pub presentation: Option<String>,
    /// `mathcad/header.xml`.
    pub header: Option<String>,
    /// `mathcad/footer.xml`.
    pub footer: Option<String>,
    /// `mathcad/integration.xml`.
    pub integration: Option<String>,
    /// `docProps/core.xml`.
    pub core_props: Option<String>,
    /// `docProps/app.xml`.
    pub app_props: Option<String>,
}

impl Package {
    /// Opens the `.mcdx` file at `path` and reads its XML parts.
    ///
    /// # Errors
    /// Returns [`PackageError::Io`] if the file cannot be read, [`PackageError::Zip`] if it is
    /// not a valid archive, [`PackageError::MissingPart`] if `worksheet.xml` is absent, and
    /// [`PackageError::PartTooLarge`] if a part exceeds its size limit.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PackageError> {
        Self::from_reader(File::open(path)?)
    }

    /// Reads a `.mcdx` package from its in-memory bytes.
    ///
    /// # Errors
    /// The same as [`Package::open`], without the file-system error.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PackageError> {
        Self::from_reader(Cursor::new(bytes))
    }

    /// The `Xaml/Document.xaml` text of the flow document that the worksheet relationship `id`
    /// links to, or `None` when there is none. `id` is the `item-idref` of a `<text>` region.
    pub fn flow_document(&self, id: &str) -> Option<&str> {
        self.flow_documents.get(id).map(|text| &**text)
    }

    /// The reason the flow document that relationship `id` links to could not be read, or `None`
    /// when the target is missing, readable, or `id` is unknown.
    pub fn flow_problem(&self, id: &str) -> Option<&str> {
        self.flow_problems.get(id).map(|reason| &**reason)
    }

    /// The bytes of the image that relationship `id` links to, or `None` when there is none. `id`
    /// is the `item-idref` of an `<image>` region.
    pub fn image(&self, id: &str) -> Option<&[u8]> {
        self.images.get(id).map(|bytes| &**bytes)
    }

    fn from_reader<R: Read + Seek>(reader: R) -> Result<Self, PackageError> {
        let mut zip = ZipArchive::new(reader)?;
        if zip.len() > MAX_ENTRIES {
            return Err(PackageError::TooManyEntries { limit: MAX_ENTRIES });
        }
        let worksheet = read_part(&mut zip, WORKSHEET, MAX_MAIN_PART)?
            .ok_or(PackageError::MissingPart(WORKSHEET))?;
        let (flow_documents, flow_problems) = flow_documents(&mut zip)?;
        let images = linked_images(&mut zip)?;
        Ok(Self {
            worksheet,
            flow_documents,
            flow_problems,
            images,
            result: read_part(&mut zip, RESULT, MAX_MAIN_PART)?,
            calculation: read_part(&mut zip, CALCULATION, MAX_MAIN_PART)?,
            presentation: read_part(&mut zip, PRESENTATION, MAX_MAIN_PART)?,
            header: read_part(&mut zip, HEADER, MAX_AUX_PART)?,
            footer: read_part(&mut zip, FOOTER, MAX_AUX_PART)?,
            integration: read_part(&mut zip, INTEGRATION, MAX_AUX_PART)?,
            core_props: read_part(&mut zip, CORE_PROPS, MAX_AUX_PART)?,
            app_props: read_part(&mut zip, APP_PROPS, MAX_AUX_PART)?,
        })
    }
}

/// The flow documents and the reasons for the unreadable links, each keyed by relationship `Id`.
type FlowMaps = (BTreeMap<String, Arc<str>>, BTreeMap<String, Arc<str>>);

/// What one flow-document target holds.
#[derive(Clone)]
enum Flow {
    /// The target is not in the archive. Its relationships have no entry.
    Absent,
    /// The target exists but cannot be read as a flow document. The reason is kept.
    Unreadable(Arc<str>),
    /// The target's `Xaml/Document.xaml` text.
    Text(Arc<str>),
}

// Test-only count of flow-document targets that were decompressed, to check that each target is
// read once however many relationships link to it.
#[cfg(test)]
thread_local! {
    static TARGET_LOADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Reads the flow documents linked from the worksheet, keyed by relationship `Id`, and the
/// reasons for the links that could not be read.
///
/// Each distinct target is decompressed once and shared by every relationship that links to it.
/// The total of the distinct flow documents is capped by [`MAX_FLOW_TOTAL`].
///
/// Tolerant of damage in the linking: a missing or malformed relationships part, a target that
/// is absent, and a target that is not a readable `.XamlPackage` are all skipped without error.
/// The region model then reports the affected text region. Only a size-limit violation is an
/// error.
fn flow_documents<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<FlowMaps, PackageError> {
    let mut found = BTreeMap::new();
    let mut problems = BTreeMap::new();
    let Some(rels_xml) = read_part(zip, WORKSHEET_RELS, MAX_AUX_PART)? else {
        return Ok((found, problems));
    };
    let Ok(rels) = roxmltree::Document::parse(&rels_xml) else {
        return Ok((found, problems));
    };
    let mut loaded: BTreeMap<String, Flow> = BTreeMap::new();
    let mut total: u64 = 0;
    for rel in rels
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "Relationship")
        .take(MAX_RELATIONSHIPS)
    {
        let (Some(id), Some(target)) = (rel.attribute("Id"), rel.attribute("Target")) else {
            continue;
        };
        if found.contains_key(id)
            || problems.contains_key(id)
            || !target.to_ascii_lowercase().ends_with(".xamlpackage")
        {
            continue;
        }
        let name = resolve_target(target);
        let flow = match loaded.get(&name) {
            Some(flow) => flow.clone(),
            None => {
                let flow = load_flow(zip, &name, &mut total)?;
                loaded.insert(name, flow.clone());
                flow
            }
        };
        match flow {
            Flow::Absent => {}
            Flow::Unreadable(reason) => {
                problems.insert(id.to_owned(), reason);
            }
            Flow::Text(text) => {
                found.insert(id.to_owned(), text);
            }
        }
    }
    Ok((found, problems))
}

/// Reads the image parts linked from the worksheet (`.png`, `.jpg`, `.jpeg`), keyed by
/// relationship `Id`. Tolerant like [`flow_documents`]: a missing or malformed relationships part,
/// an absent target, and an image over [`MAX_AUX_PART`] are skipped. The total is capped at
/// [`MAX_IMAGE_TOTAL`]: an image that would pass it is skipped, and so are later ones.
fn linked_images<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
) -> Result<BTreeMap<String, Arc<[u8]>>, PackageError> {
    let mut found = BTreeMap::new();
    let Some(rels_xml) = read_part(zip, WORKSHEET_RELS, MAX_AUX_PART)? else {
        return Ok(found);
    };
    let Ok(rels) = roxmltree::Document::parse(&rels_xml) else {
        return Ok(found);
    };
    let mut total: u64 = 0;
    for rel in rels
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "Relationship")
        .take(MAX_RELATIONSHIPS)
    {
        let (Some(id), Some(target)) = (rel.attribute("Id"), rel.attribute("Target")) else {
            continue;
        };
        let lower = target.to_ascii_lowercase();
        if found.contains_key(id)
            || !(lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg"))
        {
            continue;
        }
        let name = resolve_target(target);
        let bytes = match read_bytes(zip, &name, MAX_AUX_PART) {
            Ok(Some(bytes)) => bytes,
            Ok(None) | Err(PackageError::PartTooLarge { .. }) => continue,
            Err(err) => return Err(err),
        };
        let size = bytes.len() as u64;
        if total.saturating_add(size) > MAX_IMAGE_TOTAL {
            break;
        }
        total += size;
        found.insert(id.to_owned(), Arc::from(bytes));
    }
    Ok(found)
}

/// Decompresses one flow-document target and reads its `Xaml/Document.xaml`.
///
/// `total` is the running size of the distinct flow documents read so far. Exceeding
/// [`MAX_FLOW_TOTAL`] is an error. A target that is not a readable package is `Unreadable`, not
/// an error.
fn load_flow<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    total: &mut u64,
) -> Result<Flow, PackageError> {
    #[cfg(test)]
    TARGET_LOADS.with(|loads| loads.set(loads.get() + 1));
    let Some(bytes) = read_bytes(zip, name, MAX_AUX_PART)? else {
        return Ok(Flow::Absent);
    };
    let mut inner = match ZipArchive::new(Cursor::new(bytes)) {
        Ok(inner) => inner,
        Err(err) => {
            return Ok(Flow::Unreadable(
                format!("not a readable package: {err}").into(),
            ))
        }
    };
    match read_part(&mut inner, FLOW_DOCUMENT_PART, MAX_AUX_PART) {
        Ok(Some(text)) => {
            *total = total.saturating_add(text.len() as u64);
            if *total > MAX_FLOW_TOTAL {
                return Err(PackageError::PartTooLarge {
                    part: "flow documents".to_owned(),
                    limit: MAX_FLOW_TOTAL,
                });
            }
            Ok(Flow::Text(text.into()))
        }
        Ok(None) => Ok(Flow::Unreadable(
            format!("no `{FLOW_DOCUMENT_PART}` part").into(),
        )),
        Err(err @ PackageError::PartTooLarge { .. }) => Err(err),
        Err(err) => Ok(Flow::Unreadable(err.to_string().into())),
    }
}

/// The part name a relationship target names. The rels part sits in `mathcad/_rels`, so a
/// relative target is relative to `mathcad/`, and a leading `/` makes it package-absolute.
fn resolve_target(target: &str) -> String {
    match target.strip_prefix('/') {
        Some(absolute) => absolute.to_owned(),
        None => format!("mathcad/{target}"),
    }
}

/// Reads at most `limit` bytes from `reader`. Returns [`PackageError::PartTooLarge`] if there are
/// more. The buffer grows as bytes arrive. The declared size of the entry is never trusted for
/// allocation.
fn read_capped(reader: impl Read, part: &str, limit: u64) -> Result<Vec<u8>, PackageError> {
    let mut bytes = Vec::new();
    // One byte past the limit is enough to tell "too large" from "exactly at the limit". Reading
    // stops there, so no more than `limit + 1` bytes are ever decompressed.
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(PackageError::PartTooLarge {
            part: part.to_owned(),
            limit,
        });
    }
    Ok(bytes)
}

/// Reads one named part as raw bytes, capped at `limit`. Returns `None` when the archive has no
/// such entry.
fn read_bytes<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>, PackageError> {
    let entry = match zip.by_name(name) {
        Ok(entry) => entry,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    read_capped(entry, name, limit).map(Some)
}

/// Reads one named part as text, capped at `limit` bytes. Returns `None` when the archive has no
/// such entry.
fn read_part<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Result<Option<String>, PackageError> {
    let Some(bytes) = read_bytes(zip, name, limit)? else {
        return Ok(None);
    };
    let mut text = String::from_utf8(bytes)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.utf8_error()))?;
    // Prime writes a UTF-8 byte order mark at the start of some parts. Drop it.
    if text.starts_with('\u{feff}') {
        text.drain(..'\u{feff}'.len_utf8());
    }
    Ok(Some(text))
}

/// Builds a ZIP archive in memory from `(part name, bytes)` pairs. Test helper shared with the
/// region model tests.
#[cfg(test)]
pub(crate) fn make_zip(parts: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in parts {
        writer.start_file(*name, options).expect("start part");
        writer.write_all(body).expect("write part");
    }
    writer.finish().expect("finish zip").into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: usize = 1024 * 1024;

    /// Builds an archive whose entries are stored uncompressed. Deflating the multi-megabyte
    /// test parts costs seconds in a debug build, and the size caps do not depend on compression.
    /// Use [`deflated_zip`] only where an inner `.XamlPackage` must stay under its own cap.
    fn stored_zip(parts: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::{CompressionMethod, ZipWriter};

        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body) in parts {
            writer.start_file(*name, options).expect("start part");
            writer.write_all(body).expect("write part");
        }
        writer.finish().expect("finish zip").into_inner()
    }

    /// Builds an archive with Deflate at best compression. Used for an inner `.XamlPackage` whose
    /// compressed size must fit its cap, which an uncompressed copy of a 4 MiB document would not.
    fn deflated_zip(parts: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::{CompressionMethod, ZipWriter};

        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(9));
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body) in parts {
            writer.start_file(*name, options).expect("start part");
            writer.write_all(body).expect("write part");
        }
        writer.finish().expect("finish zip").into_inner()
    }

    #[test]
    fn reads_worksheet_and_optional_parts() {
        let bytes = make_zip(&[
            (WORKSHEET, b"<worksheet/>"),
            (RESULT, "\u{feff}<resultsList/>".as_bytes()),
            (CORE_PROPS, b"<coreProperties/>"),
        ]);
        let pkg = Package::from_bytes(&bytes).expect("package opens");
        assert_eq!(pkg.worksheet, "<worksheet/>");
        assert_eq!(pkg.result.as_deref(), Some("<resultsList/>"));
        assert_eq!(pkg.core_props.as_deref(), Some("<coreProperties/>"));
        assert!(pkg.calculation.is_none());
        assert!(pkg.presentation.is_none());
        assert!(pkg.header.is_none());
        assert!(pkg.footer.is_none());
        assert!(pkg.integration.is_none());
        assert!(pkg.app_props.is_none());
    }

    /// A flow document package: a ZIP holding `Xaml/Document.xaml`, as `.XamlPackage` files are.
    fn xaml_package(document: &str) -> Vec<u8> {
        make_zip(&[(FLOW_DOCUMENT_PART, document.as_bytes())])
    }

    const RELS: &str = r#"<?xml version="1.0" encoding="utf-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/flowDocument" Target="/mathcad/xaml/FlowDocument3.XamlPackage" Id="R72ddd" /><Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/flowDocument" Target="xaml/Relative.XamlPackage" Id="Rrel" /><Relationship Type="x" Target="/mathcad/missing.XamlPackage" Id="Rgone" /><Relationship Type="x" Target="/mathcad/image.png" Id="Rimg" /></Relationships>"#;

    #[test]
    fn flow_documents_are_resolved_from_worksheet_relationships() {
        let bytes = make_zip(&[
            (WORKSHEET, b"<worksheet/>"),
            (WORKSHEET_RELS, RELS.as_bytes()),
            (
                "mathcad/xaml/FlowDocument3.XamlPackage",
                &xaml_package("<Section>Absolute</Section>"),
            ),
            (
                "mathcad/xaml/Relative.XamlPackage",
                &xaml_package("<Section>Relative</Section>"),
            ),
            ("mathcad/image.png", b"not a package"),
        ]);
        let pkg = Package::from_bytes(&bytes).expect("package opens");
        assert_eq!(
            pkg.flow_document("R72ddd"),
            Some("<Section>Absolute</Section>")
        );
        assert_eq!(
            pkg.flow_document("Rrel"),
            Some("<Section>Relative</Section>")
        );
        assert_eq!(pkg.flow_document("Rgone"), None, "absent target is skipped");
        assert_eq!(
            pkg.flow_document("Rimg"),
            None,
            "non-package target is skipped"
        );
        assert_eq!(pkg.flow_document("nope"), None);
        assert_eq!(
            pkg.flow_problem("Rgone"),
            None,
            "absent target has no reason"
        );
    }

    #[test]
    fn damaged_flow_linking_is_skipped_not_an_error() {
        let bytes = make_zip(&[
            (WORKSHEET, b"<worksheet/>"),
            (WORKSHEET_RELS, b"<Relationships"),
            (
                "mathcad/xaml/FlowDocument1.XamlPackage",
                &xaml_package("<Section/>"),
            ),
        ]);
        let pkg = Package::from_bytes(&bytes).expect("malformed rels does not fail the package");
        assert!(pkg.flow_documents.is_empty());
        assert!(pkg.flow_problems.is_empty());
    }

    #[test]
    fn unreadable_flow_target_keeps_its_reason() {
        const RELS_BAD: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/mathcad/xaml/Bad.XamlPackage" Id="Rbad" /></Relationships>"#;
        let bytes = make_zip(&[
            (WORKSHEET, b"<worksheet/>"),
            (WORKSHEET_RELS, RELS_BAD.as_bytes()),
            ("mathcad/xaml/Bad.XamlPackage", b"not a zip archive"),
        ]);
        let pkg = Package::from_bytes(&bytes).expect("package opens");
        assert_eq!(pkg.flow_document("Rbad"), None);
        assert!(pkg
            .flow_problem("Rbad")
            .is_some_and(|reason| reason.contains("not a readable package")));
    }

    #[test]
    fn missing_worksheet_is_an_error() {
        let bytes = make_zip(&[(RESULT, b"<resultsList/>")]);
        let err = Package::from_bytes(&bytes).expect_err("worksheet is required");
        assert!(matches!(err, PackageError::MissingPart(WORKSHEET)));
    }

    #[test]
    fn non_zip_bytes_are_an_error() {
        let err = Package::from_bytes(b"not a zip file at all").expect_err("not a zip");
        assert!(matches!(err, PackageError::Zip(_)));
    }

    #[test]
    fn missing_file_is_an_io_error() {
        let err = Package::open("/nonexistent/path/to/sheet.mcdx").expect_err("no such file");
        assert!(matches!(err, PackageError::Io(_)));
    }

    /// A reader that counts the bytes it hands out.
    struct Counting<R> {
        inner: R,
        count: u64,
    }

    impl<R: Read> Read for Counting<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.count += n as u64;
            Ok(n)
        }
    }

    #[test]
    fn read_capped_stops_one_byte_past_the_limit() {
        let mut reader = Counting {
            inner: std::io::repeat(0).take(70 * 1024 * 1024),
            count: 0,
        };
        let err = read_capped(&mut reader, "big", MAX_MAIN_PART).expect_err("over the cap");
        assert!(matches!(err, PackageError::PartTooLarge { limit, .. } if limit == MAX_MAIN_PART));
        assert_eq!(
            reader.count,
            MAX_MAIN_PART + 1,
            "no more than cap + 1 is read"
        );
    }

    #[test]
    fn read_capped_accepts_exactly_the_limit() {
        let data = [7u8; 10];
        assert_eq!(read_capped(&data[..], "p", 10).expect("at limit"), data);
        assert!(matches!(
            read_capped(&data[..], "p", 9),
            Err(PackageError::PartTooLarge { limit: 9, .. })
        ));
    }

    #[test]
    fn oversized_worksheet_is_rejected_with_its_name() {
        let big = vec![0u8; 70 * MIB];
        let bytes = stored_zip(&[(WORKSHEET, &big)]);
        let err = Package::from_bytes(&bytes).expect_err("worksheet over the cap");
        assert!(matches!(
            err,
            PackageError::PartTooLarge { ref part, limit }
                if part == WORKSHEET && limit == MAX_MAIN_PART
        ));
    }

    #[test]
    fn oversized_xaml_package_is_rejected() {
        let big = vec![0u8; MAX_AUX_PART as usize + 1];
        let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/mathcad/xaml/Big.XamlPackage" Id="Rbig" /></Relationships>"#;
        let bytes = stored_zip(&[
            (WORKSHEET, b"<worksheet/>"),
            (WORKSHEET_RELS, rels.as_bytes()),
            ("mathcad/xaml/Big.XamlPackage", &big),
        ]);
        let err = Package::from_bytes(&bytes).expect_err("XamlPackage over the cap");
        assert!(matches!(err, PackageError::PartTooLarge { limit, .. } if limit == MAX_AUX_PART));
    }

    #[test]
    fn oversized_document_xaml_is_rejected() {
        let big = vec![0u8; MAX_AUX_PART as usize + 1];
        let flow = deflated_zip(&[(FLOW_DOCUMENT_PART, &big)]);
        let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/mathcad/xaml/Doc.XamlPackage" Id="Rdoc" /></Relationships>"#;
        let bytes = stored_zip(&[
            (WORKSHEET, b"<worksheet/>"),
            (WORKSHEET_RELS, rels.as_bytes()),
            ("mathcad/xaml/Doc.XamlPackage", &flow),
        ]);
        let err = Package::from_bytes(&bytes).expect_err("Document.xaml over the cap");
        assert!(matches!(
            err,
            PackageError::PartTooLarge { ref part, limit }
                if part == FLOW_DOCUMENT_PART && limit == MAX_AUX_PART
        ));
    }

    #[test]
    fn document_xaml_exactly_at_the_cap_is_accepted() {
        let at_cap = vec![b'a'; MAX_AUX_PART as usize];
        let flow = deflated_zip(&[(FLOW_DOCUMENT_PART, &at_cap)]);
        let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/mathcad/xaml/Edge.XamlPackage" Id="Redge" /></Relationships>"#;
        let bytes = stored_zip(&[
            (WORKSHEET, b"<worksheet/>"),
            (WORKSHEET_RELS, rels.as_bytes()),
            ("mathcad/xaml/Edge.XamlPackage", &flow),
        ]);
        let pkg = Package::from_bytes(&bytes).expect("exactly at the cap is accepted");
        assert_eq!(
            pkg.flow_document("Redge").map(str::len),
            Some(MAX_AUX_PART as usize)
        );
    }

    #[test]
    fn relationships_to_one_target_share_one_decompression() {
        let target_text = "x".repeat(3 * MIB);
        let flow = stored_zip(&[(FLOW_DOCUMENT_PART, target_text.as_bytes())]);
        let mut rels = String::from(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        );
        for i in 0..200 {
            rels.push_str(&format!(
                r#"<Relationship Target="/mathcad/xaml/Shared.XamlPackage" Id="R{i}" />"#
            ));
        }
        rels.push_str("</Relationships>");
        let bytes = stored_zip(&[
            (WORKSHEET, b"<worksheet/>"),
            (WORKSHEET_RELS, rels.as_bytes()),
            ("mathcad/xaml/Shared.XamlPackage", &flow),
        ]);
        TARGET_LOADS.with(|loads| loads.set(0));
        let pkg = Package::from_bytes(&bytes).expect("package opens");
        assert_eq!(
            TARGET_LOADS.with(std::cell::Cell::get),
            1,
            "one decompression"
        );
        assert_eq!(pkg.flow_documents.len(), 200);
        let first = pkg.flow_document("R0").expect("linked");
        let last = pkg.flow_document("R199").expect("linked");
        assert!(
            std::ptr::eq(first.as_ptr(), last.as_ptr()),
            "text is shared"
        );
        assert_eq!(first.len(), 3 * MIB);
    }

    #[test]
    fn total_of_distinct_flow_documents_is_capped() {
        let target_text = "x".repeat(3 * MIB);
        let mut rels = String::from(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        );
        let mut owned: Vec<(String, Vec<u8>)> = Vec::new();
        for i in 0..30 {
            let name = format!("mathcad/xaml/Distinct{i}.XamlPackage");
            rels.push_str(&format!(r#"<Relationship Target="/{name}" Id="R{i}" />"#));
            owned.push((
                name,
                stored_zip(&[(FLOW_DOCUMENT_PART, target_text.as_bytes())]),
            ));
        }
        rels.push_str("</Relationships>");
        let mut parts: Vec<(&str, &[u8])> = vec![
            (WORKSHEET, b"<worksheet/>"),
            (WORKSHEET_RELS, rels.as_bytes()),
        ];
        parts.extend(
            owned
                .iter()
                .map(|(name, bytes)| (name.as_str(), bytes.as_slice())),
        );
        let bytes = stored_zip(&parts);
        let err = Package::from_bytes(&bytes).expect_err("30 x 3 MiB exceeds the total");
        assert!(matches!(
            err,
            PackageError::PartTooLarge { ref part, limit }
                if part == "flow documents" && limit == MAX_FLOW_TOTAL
        ));
    }
}
