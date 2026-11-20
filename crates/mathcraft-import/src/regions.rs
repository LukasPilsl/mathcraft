//! Typed worksheet model built from `mathcad/worksheet.xml` (REBUILD_GUIDE step 2).
//!
//! [`Worksheet::from_package`] turns every `<region>` of a [`Package`] into a [`RegionModel`].
//! A region holds one content element: `<math>` (converted to an [`Expr`]), `<text>`, `<plot>`
//! or `<solveblock>`. A solve block lists its member regions as `children`, in document order.
//! Every `<region>` element in the sheet appears exactly once in the tree, nested ones included.
//!
//! The text of a `<text>` region is not stored in the worksheet part. It is in the flow document
//! that the region's `item-idref` names (see [`Package::flow_document`]). A flow document that is
//! missing or malformed gives the region empty text and an [`ImportNote`].
//!
//! A region that cannot be classified or converted is kept as [`RegionKind::Unsupported`] with a
//! reason, and is listed in [`ImportReport`]. Malformed XML and hostile nesting return an
//! [`ImportError`]. Nothing is dropped and nothing panics.
//!
//! Coordinates are in points from the top-left corner of the page. [`RegionModel::pos`] is
//! `(left, top)`. A solve-block member's position is relative to its block, not to the page.

use std::collections::HashMap;
use std::path::Path;

use mathcraft_core::eval::{MatrixDisplay, NumberFormat, Radix, ResultFormat};
use mathcraft_core::expr::Expr;
use roxmltree::Node;
use thiserror::Error;

use crate::dom::{self, DomError, CALCULATION, WS};
use crate::expr::convert_math;
use crate::package::{Package, PackageError};
use crate::page::PageSettings;
use crate::results::{ResultData, ResultTable};
use crate::rich_text::{self, TextBody};

mod plot;
mod plot3d;
#[cfg(test)]
mod tests;

pub(crate) use plot::plot_info;
pub use plot::{Axis2d, AxisLook, Equation, Plot2d, PlotMarker, PlotTrace, TraceStyle};
pub use plot3d::{Axis3d, Plot3d, Trace3d};

/// Deepest `<region>` nesting accepted. Real sheets nest a solve block one level deep. Deeper
/// input is an [`ImportError::Nesting`], so recursion stays bounded.
pub const MAX_REGION_DEPTH: usize = 16;

/// Largest number of `<region>` elements accepted, at any nesting depth. Real sheets have a few
/// dozen (the sample worksheets have at most 33). A sheet over the cap is an
/// [`ImportError::TooManyRegions`], so the model's size is bounded by this count and not by the
/// file's size alone.
pub const MAX_REGIONS: usize = 20_000;

/// Largest total of the text of all [`RegionKind::Text`] values, in bytes. Each distinct text is
/// counted once, however many regions share it (see [`RegionKind::Text`]). Exceeding it is an
/// [`ImportError::TextTooLarge`].
pub const MAX_TEXT_TOTAL: usize = 64 * 1024 * 1024;

/// Errors from building a worksheet model. Unsupported region content is not an error; it is
/// [`RegionKind::Unsupported`].
#[derive(Debug, Error)]
pub enum ImportError {
    /// The `.mcdx` package could not be read.
    #[error(transparent)]
    Package(#[from] PackageError),
    /// A part is not well-formed XML, or a required attribute is missing or malformed.
    #[error(transparent)]
    Dom(#[from] DomError),
    /// The worksheet root element is not `<worksheet>`.
    #[error("worksheet root is <{0}>, expected <worksheet>")]
    Root(String),
    /// Regions are nested deeper than [`MAX_REGION_DEPTH`].
    #[error("regions nested deeper than {MAX_REGION_DEPTH} levels")]
    Nesting,
    /// The worksheet has more `<region>` elements than `limit` ([`MAX_REGIONS`]).
    #[error("more than {limit} regions")]
    TooManyRegions {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// The distinct text of the worksheet's text regions is more than `limit`
    /// ([`MAX_TEXT_TOTAL`]).
    #[error("text of text regions is more than {limit} bytes in total")]
    TextTooLarge {
        /// The limit that was exceeded, in bytes.
        limit: usize,
    },
}

/// A parsed worksheet: its region tree, calculation settings and import report.
#[derive(Debug, Clone, PartialEq)]
pub struct Worksheet {
    /// Top-level regions in document order. Solve-block members are in their block's `children`.
    pub regions: Vec<RegionModel>,
    /// Calculation settings from `settings/calculation.xml`.
    pub settings: Settings,
    /// Paper, orientation and margin type from `settings/presentation.xml`.
    pub page: PageSettings,
    /// Counts, unsupported regions and notes.
    pub report: ImportReport,
    /// Text regions of the page header (`mathcad/header.xml`), in document order. They are drawn
    /// on every page, not on the sheet. Empty when the header is empty. See [`Worksheet::from_package`].
    pub header: Vec<RegionModel>,
    /// Text regions of the page footer (`mathcad/footer.xml`), as [`Worksheet::header`].
    pub footer: Vec<RegionModel>,
}

/// Calculation settings that affect evaluation. Each field is `None` when the file does not
/// state it.
///
/// A malformed value, such as `array-origin="x"`, is not treated as absent. It fails the whole
/// import with [`ImportError::Dom`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    /// `builtInVariables/@array-origin`: the value of `ORIGIN` (0 or 1).
    pub array_origin: Option<u32>,
    /// `builtInVariables/@convergence-tolerance`.
    pub convergence_tolerance: Option<f64>,
    /// `builtInVariables/@constraint-tolerance`.
    pub constraint_tolerance: Option<f64>,
    /// `units/currentUnitSystem/@name`, for example `si`, `us` or `cgs`.
    pub unit_system: Option<String>,
    /// `builtInVariables/@prn-precision`: the value of `PRNPRECISION`.
    pub prn_precision: Option<u32>,
    /// The result display from `presentation.xml` (`ws:results`), `None` when the file has no
    /// `general` element. Display only: it never changes an evaluation.
    pub result_format: Option<ResultFormat>,
}

/// Whether two sets of settings evaluate the same way. A display change (result format,
/// `PRNPRECISION`) keeps stored results valid.
pub(crate) fn same_evaluation(a: &Settings, b: &Settings) -> bool {
    a.array_origin == b.array_origin
        && a.convergence_tolerance == b.convergence_tolerance
        && a.constraint_tolerance == b.constraint_tolerance
        && a.unit_system == b.unit_system
}

/// The `ws:results/general` element of `presentation.xml` as a result format. Attributes that the
/// samples do not state keep their Prime defaults; the writer only changes the ones it reads.
fn read_result_format(xml: &str) -> Result<Option<ResultFormat>, DomError> {
    let doc = dom::parse(xml)?;
    let general = doc.descendants().find(|n| {
        n.has_tag_name("general") && n.parent().is_some_and(|p| p.has_tag_name("results"))
    });
    general.map(general_format).transpose()
}

/// The result format of a math region (`<math><resultFormat><general .../>`), `None` when the
/// region states none. Only the `general` element is read; another format element is not a
/// general format and is left as it is (`None`).
pub(crate) fn math_result_format(math: Node<'_, '_>) -> Result<Option<ResultFormat>, DomError> {
    let general = math
        .children()
        .filter(|c| c.is_element() && c.tag_name().name() == "resultFormat")
        .flat_map(|rf| rf.children())
        .find(|c| c.is_element() && c.tag_name().name() == "general");
    general.map(general_format).transpose()
}

/// The `resultFormat/matrix` element of a math region as a matrix display, the default when the
/// region states none. Attributes that the element does not state keep their defaults. The size
/// and offset are `rows,cols` pairs; a size with a zero side is refused.
///
/// The attribute names come from the samples (`size`, `offset`, `show-indices`,
/// `expand-nested-arrays`). The meaning of `offset` (first visible row and column) is assumed.
pub(crate) fn math_matrix_display(math: Node<'_, '_>) -> Result<MatrixDisplay, DomError> {
    const EL: &str = "matrix";
    let mut display = MatrixDisplay::default();
    let element = math
        .children()
        .filter(|c| c.is_element() && c.tag_name().name() == "resultFormat")
        .flat_map(|rf| rf.children())
        .find(|c| c.is_element() && c.tag_name().name() == EL);
    let Some(matrix) = element else {
        return Ok(display);
    };
    let pair = |attr: &'static str| -> Result<Option<(u32, u32)>, DomError> {
        let Some(value) = dom::parse_attr::<String>(matrix, EL, attr)? else {
            return Ok(None);
        };
        let bad = || DomError::BadAttr {
            element: EL,
            attr,
            value: value.clone(),
        };
        let (a, b) = value.split_once(',').ok_or_else(bad)?;
        let a = a.trim().parse::<u32>().map_err(|_| bad())?;
        let b = b.trim().parse::<u32>().map_err(|_| bad())?;
        Ok(Some((a, b)))
    };
    if let Some((rows, cols)) = pair("size")? {
        if rows == 0 || cols == 0 {
            return Err(DomError::BadAttr {
                element: EL,
                attr: "size",
                value: format!("{rows},{cols}"),
            });
        }
        display.rows = rows;
        display.cols = cols;
    }
    if let Some((row, col)) = pair("offset")? {
        display.offset_row = row;
        display.offset_col = col;
    }
    if let Some(v) = dom::parse_attr::<bool>(matrix, EL, "show-indices")? {
        display.show_indices = v;
    }
    if let Some(v) = dom::parse_attr::<bool>(matrix, EL, "expand-nested-arrays")? {
        display.expand_nested = v;
    }
    Ok(display)
}

/// The `display` attribute spelling of a number format. UNVERIFIED: no sample states a number
/// format other than the default (see documentation/UNVERIFIED_FORMATS.md).
pub(crate) fn number_attr(number: NumberFormat) -> &'static str {
    match number {
        NumberFormat::General => "general",
        NumberFormat::Decimal => "decimal",
        NumberFormat::Scientific => "scientific",
        NumberFormat::Engineering => "engineering",
        NumberFormat::Percent => "percent",
    }
}

fn number_from_attr(value: &str) -> Option<NumberFormat> {
    [
        NumberFormat::General,
        NumberFormat::Decimal,
        NumberFormat::Scientific,
        NumberFormat::Engineering,
        NumberFormat::Percent,
    ]
    .into_iter()
    .find(|n| number_attr(*n) == value)
}

/// A `general` element as a result format. Attributes that the element does not state keep their
/// Prime defaults.
fn general_format(general: Node<'_, '_>) -> Result<ResultFormat, DomError> {
    const EL: &str = "general";
    let mut format = ResultFormat::default();
    if let Some(v) = dom::parse_attr::<u8>(general, EL, "precision")? {
        format.precision = v;
    }
    if let Some(v) = dom::parse_attr::<bool>(general, EL, "show-trailing-zeros")? {
        format.trailing_zeros = v;
    }
    if let Some(v) = dom::parse_attr::<String>(general, EL, "radix")? {
        format.radix = Radix::from_attr(&v).ok_or_else(|| DomError::BadAttr {
            element: EL,
            attr: "radix",
            value: v.clone(),
        })?;
    }
    if let Some(v) = dom::parse_attr::<u16>(general, EL, "zero-threshold")? {
        format.zero_threshold = v;
    }
    if let Some(v) = dom::parse_attr::<u8>(general, EL, "exponential-threshold")? {
        format.exponential_threshold = v;
    }
    if let Some(v) = dom::parse_attr::<String>(general, EL, "imaginary-value")? {
        format.imaginary_j = match v.as_str() {
            "i" => false,
            "j" => true,
            _ => {
                return Err(DomError::BadAttr {
                    element: EL,
                    attr: "imaginary-value",
                    value: v,
                })
            }
        };
    }
    if let Some(v) = dom::parse_attr::<String>(general, EL, "display")? {
        format.number = number_from_attr(&v).ok_or_else(|| DomError::BadAttr {
            element: EL,
            attr: "display",
            value: v.clone(),
        })?;
    }
    if let Some(v) = dom::parse_attr::<bool>(general, EL, "digit-grouping")? {
        format.digit_grouping = v;
    }
    if let Err(err) = format.clone().checked() {
        return Err(DomError::BadAttr {
            element: EL,
            attr: "display",
            value: err.to_string(),
        });
    }
    Ok(format)
}

/// One `<region>`.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionModel {
    /// `region-id`, unique in the sheet.
    pub id: u32,
    /// `(left, top)` in points. `None` when either attribute is absent or not a finite number.
    pub pos: Option<(f64, f64)>,
    /// `(actualWidth, actualHeight)` in points. `None` when either attribute is absent or not a
    /// finite number.
    pub size: Option<(f64, f64)>,
    /// `resultRef` of the region's `<math>`, matching `result-id` in `result.xml`.
    pub result_ref: Option<u32>,
    /// What the region holds, and its converted content.
    pub kind: RegionKind,
    /// `solve-block-category` (`guess-value`, `constraint` or `solver`) when the region is a
    /// member of a solve block.
    pub solve_category: Option<String>,
    /// The region's own result format (`<math><resultFormat><general>`), `None` when it follows
    /// the worksheet's format. Display only.
    pub result_format: Option<ResultFormat>,
    /// The region's matrix display (`<math><resultFormat><matrix>`), the default when the region
    /// states none. Stored per region only. Display only.
    pub matrix: MatrixDisplay,
    /// Regions nested inside this one, in document order. For a solve block these are its
    /// members. For a text region these are the inline regions placed in the text (such as
    /// math), which are not part of the text. Empty when nothing is nested.
    pub children: Vec<RegionModel>,
}

/// The content of a region.
#[derive(Debug, Clone, PartialEq)]
pub enum RegionKind {
    /// A `<math>` region: a definition (`:=`), an evaluation (`=`), a symbolic evaluation or a
    /// bare expression. The variant of the [`Expr`] says which.
    Math(Expr),
    /// A text region. Its plain text is the flow document's text: one paragraph per line, joined
    /// with `\n`, and `\n` also for a line break inside a paragraph. Spaces are kept as written.
    /// The rich text (runs with their styles) is kept with it, see [`TextBody`]. Nested regions
    /// are not part of it (see [`RegionModel::children`]).
    ///
    /// Many regions can name the same flow document. Those regions then share one allocation,
    /// and [`MAX_TEXT_TOTAL`] counts that allocation once. Equal texts from different flow
    /// documents are not merged.
    Text(TextBody),
    /// A solve block. Its members are the region's `children`.
    SolveBlock(SolveBlock),
    /// A plot, as much as the model needs for now.
    Plot(PlotInfo),
    /// An area: a collapsible group. The regions it holds are the region's `children`. See
    /// [`AreaModel`].
    Area(AreaModel),
    /// An image, drawn at the region's size. See [`ImageModel`].
    Image(ImageModel),
    /// A table (`<spec-table>`): a presentation of its member defines. See [`TableModel`].
    Table(TableModel),
    /// A page-break marker: the content below it starts a new page. It has no content of its own.
    // UNVERIFIED: no sample has a page break. The element name `page-break` is a guess.
    PageBreak,
    /// Content this model does not handle yet. `reason` says why.
    Unsupported {
        /// Human-readable reason, naming the element concerned.
        reason: String,
    },
}

/// A table region (`<spec-table>`). Each member is a `<math>` define (`a := [1;4;7]·m`), kept as
/// its own expression with its `resultRef`. The table is only a presentation: each member is
/// evaluated like a define region. A member that cannot be converted is `Err` with its reason, and
/// the others still run. The member order is document order.
#[derive(Debug, Clone, PartialEq)]
pub struct TableModel {
    /// The members in document order.
    pub members: Vec<TableMember>,
}

/// One member of a [`TableModel`].
#[derive(Debug, Clone, PartialEq)]
pub struct TableMember {
    /// `resultRef` of the member's `<math>`, matching `result-id` in `result.xml`.
    pub result_ref: Option<u32>,
    /// The converted define, or the reason it cannot be converted.
    pub expr: Result<Expr, String>,
}

/// A solve block's keyword (`find`, `minerr`, ...), if the block has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolveBlock {
    /// Text of the first `KEYWORD` identifier in the block.
    pub keyword: Option<String>,
}

/// An area (`<area>`). Its display state and border are stored; its regions are the enclosing
/// [`RegionModel::children`]. UNVERIFIED: no sample has an area, so the element and attribute
/// names are guesses (see `documentation/UNVERIFIED_FORMATS.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AreaModel {
    /// The area is collapsed: its regions are hidden, and it shows only its header.
    pub collapsed: bool,
    /// The display state (collapsed or expanded) is locked. The area is then not edited.
    pub locked: bool,
    /// The area's border is hidden. Hidden borders are not printed.
    pub hide_border: bool,
}

/// An image region: the bytes of a PNG or JPEG part. The part is named by `item` (the
/// relationship `Id`). UNVERIFIED: no sample has an image part (see `UNVERIFIED_FORMATS.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageModel {
    /// The worksheet relationship `Id` the image part is linked by. Empty for an image that has
    /// not been written yet; the writer then gives it a new part.
    pub item: String,
    /// The encoded image file, as stored in the package.
    pub bytes: std::sync::Arc<[u8]>,
}

impl ImageModel {
    /// The media type of the bytes, from their signature: `image/png` or `image/jpeg`. `None` for
    /// anything else.
    pub fn media_type(&self) -> Option<&'static str> {
        if self
            .bytes
            .starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])
        {
            Some("image/png")
        } else if self.bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            Some("image/jpeg")
        } else {
            None
        }
    }
}

/// A plot: the plot type, the stored results of its traces, its axis ranges and, for a 2D plot,
/// the model of its traces and axes (see [`Plot2d`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PlotInfo {
    /// Local name of the plot type element, for example `xyPlot`. Empty if the plot has none.
    pub kind: String,
    /// `resultRef` of each `<trace>`, which points to its data in `result.xml`.
    pub traces: Vec<u32>,
    /// `(start, end)` of the x axis, when the plot states both.
    pub x_range: Option<(f64, f64)>,
    /// `(start, end)` of the y axis, when the plot states both.
    pub y_range: Option<(f64, f64)>,
    /// The 2D model. `None` for a plot type that is not 2D (`plot3D`) or not known.
    /// Boxed, with the 3D model, so that `RegionKind` stays small.
    pub plot: Option<Box<Plot2d>>,
    /// The 3D model of a `plot3D`. `None` for every other plot type.
    /// Boxed: the 3D model is large and would make every `RegionKind` large.
    pub plot3d: Option<Box<Plot3d>>,
}

/// Counts and problems found while building a [`Worksheet`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Every region in the tree, nested ones included.
    pub region_count: usize,
    /// Every region whose kind is [`RegionKind::Unsupported`], in document order.
    pub unsupported: Vec<UnsupportedRegion>,
    /// Problems that did not stop the import, in document order. The affected region still
    /// appears in the model, with a fallback value.
    pub notes: Vec<ImportNote>,
}

/// One unsupported region, as listed in [`ImportReport`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedRegion {
    /// `region-id` of the region.
    pub region_id: u32,
    /// Why it is unsupported.
    pub reason: String,
}

/// A recoverable problem found while building the model, listed in [`ImportReport::notes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportNote {
    /// `region-id` of the region concerned.
    pub region_id: u32,
    /// Human-readable description, including what fallback the region received.
    pub reason: String,
}

impl Worksheet {
    /// Opens the `.mcdx` file at `path` and builds its worksheet model.
    ///
    /// # Errors
    /// The errors of [`Package::open`] and [`Worksheet::from_package`].
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ImportError> {
        Self::from_package(&Package::open(path)?)
    }

    /// Builds the worksheet model from the parts of a package.
    ///
    /// `settings/calculation.xml` is read here, and a malformed attribute in it fails the import.
    /// `result.xml` is not parsed here, so a malformed `result.xml` does not fail the import. Parse
    /// it with [`crate::results::parse_results`] to check it.
    ///
    /// # Errors
    /// [`ImportError::Dom`] for malformed XML in the worksheet or settings, or a malformed
    /// required or optional attribute there. [`ImportError::Root`] when the worksheet root is not
    /// `<worksheet>`, [`ImportError::Nesting`] when regions are nested deeper than
    /// [`MAX_REGION_DEPTH`], [`ImportError::TooManyRegions`] above [`MAX_REGIONS`] and
    /// [`ImportError::TextTooLarge`] above [`MAX_TEXT_TOTAL`]. Unsupported content and a missing
    /// flow document are not errors.
    pub fn from_package(pkg: &Package) -> Result<Self, ImportError> {
        let doc = dom::parse(&pkg.worksheet)?;
        let root = doc.root_element();
        if !root.has_tag_name((WS, "worksheet")) {
            return Err(ImportError::Root(root.tag_name().name().to_owned()));
        }
        let mut builder = Builder {
            pkg,
            notes: Vec::new(),
            count: 0,
            texts: HashMap::new(),
            text_total: 0,
        };
        let regions = direct_regions(root)
            .into_iter()
            .map(|node| builder.region(node, 0))
            .collect::<Result<Vec<_>, _>>()?;
        let settings =
            Settings::from_parts(pkg.calculation.as_deref(), pkg.presentation.as_deref())?;
        let page = PageSettings::from_presentation(pkg.presentation.as_deref())?;
        let header = builder.furniture(pkg.header.as_deref())?;
        let footer = builder.furniture(pkg.footer.as_deref())?;
        let report = report_for(&regions, builder.notes);
        Ok(Self {
            regions,
            settings,
            page,
            report,
            header,
            footer,
        })
    }

    /// Every region in document order, depth first, solve-block members included. Solve blocks
    /// themselves are included too.
    pub fn all_regions(&self) -> Vec<&RegionModel> {
        let mut out = Vec::new();
        walk(&self.regions, &mut out);
        out
    }

    /// Joins each region with its stored result. Returns `(region_id, data)` for every region
    /// whose `result_ref` is in `table`, in document order. Regions without a `result_ref`, or
    /// whose result is missing from `table`, are not listed.
    pub fn attach_stored_results<'a>(&self, table: &'a ResultTable) -> Vec<(u32, &'a ResultData)> {
        self.all_regions()
            .into_iter()
            .filter_map(|region| {
                let data = table.by_id.get(&region.result_ref?)?;
                Some((region.id, data))
            })
            .collect()
    }

    /// The order in which Mathcad evaluates the sheet. The rule is:
    ///
    /// 1. Within one list of siblings (the top-level regions, or the members of one solve
    ///    block), a region with a position comes before a region without one.
    /// 2. Positioned regions sort by `top`, then by `left`. Each value is rounded to the nearest
    ///    0.1 pt before comparing, so regions whose rounded values are equal share a row (or a
    ///    column). This absorbs floating-point noise such as 100.0000001 against 100.0. It does
    ///    not absorb noise across a rounding boundary: 100.04 rounds to 100.0 and 100.06 rounds
    ///    to 100.1, so those two regions sort into different rows although they differ by only
    ///    0.02 pt.
    /// 3. Regions without a position follow all positioned siblings, in document order.
    /// 4. Ties keep document order. The sort is stable, so the same file always gives the same
    ///    order.
    /// 5. A solve block is a container. It is not returned. Its members are returned right
    ///    where the block would be, in their own order (rules 1 to 4 apply inside the block,
    ///    using member positions, which are relative to the block).
    ///
    /// The result is every region that is not a solve block, each exactly once.
    pub fn evaluation_order(&self) -> Vec<&RegionModel> {
        let mut out = Vec::new();
        order_list(&self.regions, &mut out);
        out
    }

    /// The same order as [`Worksheet::evaluation_order`], grouped by solve block: a solve block is
    /// one [`Step::SolveBlock`] holding its members in order, so an evaluator can run the block as
    /// a unit. Other regions are [`Step::Region`]. The members' own nested regions follow the
    /// block as ordinary steps.
    pub fn evaluation_steps(&self) -> Vec<Step<'_>> {
        let mut out = Vec::new();
        steps_list(&self.regions, &mut out);
        out
    }
}

/// One step of the evaluation order (see [`Worksheet::evaluation_steps`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Step<'a> {
    /// A region that is run on its own.
    Region(&'a RegionModel),
    /// A solve block, run as a unit. `members` are its member regions in evaluation order.
    SolveBlock {
        /// The block region itself (its kind is [`RegionKind::SolveBlock`]).
        block: &'a RegionModel,
        /// The member regions, sorted by the rules of [`Worksheet::evaluation_order`].
        members: Vec<&'a RegionModel>,
    },
}

impl Settings {
    /// The settings of a package: `calculation.xml` and `presentation.xml`, each optional.
    ///
    /// # Errors
    /// [`DomError`] for malformed XML or a malformed or out-of-range attribute.
    fn from_parts(calculation: Option<&str>, presentation: Option<&str>) -> Result<Self, DomError> {
        let mut settings = match calculation {
            Some(xml) => Settings::from_xml(xml)?,
            None => Settings::default(),
        };
        if let Some(xml) = presentation {
            settings.result_format = read_result_format(xml)?;
        }
        Ok(settings)
    }

    fn from_xml(xml: &str) -> Result<Self, DomError> {
        let doc = dom::parse(xml)?;
        let mut settings = Settings::default();
        let vars = doc
            .descendants()
            .find(|n| n.has_tag_name((CALCULATION, "builtInVariables")));
        if let Some(vars) = vars {
            const EL: &str = "builtInVariables";
            settings.array_origin = dom::parse_attr(vars, EL, "array-origin")?;
            settings.convergence_tolerance = dom::parse_attr(vars, EL, "convergence-tolerance")?;
            settings.constraint_tolerance = dom::parse_attr(vars, EL, "constraint-tolerance")?;
            settings.prn_precision = dom::parse_attr(vars, EL, "prn-precision")?;
        }
        settings.unit_system = doc
            .descendants()
            .find(|n| n.has_tag_name((CALCULATION, "currentUnitSystem")))
            .and_then(|n| n.attribute("name"))
            .map(str::to_owned);
        Ok(settings)
    }
}

/// Builds region models and collects the notes for [`ImportReport`] as it goes.
struct Builder<'p> {
    pkg: &'p Package,
    notes: Vec<ImportNote>,
    /// Regions started so far, at any depth. Checked against [`MAX_REGIONS`].
    count: usize,
    /// Extracted text per flow document, or the reason it is malformed. The key is the address
    /// and length of the document's text in the package. Each distinct document is parsed once,
    /// and regions that name it share its `Arc`.
    texts: HashMap<(usize, usize), Result<TextBody, String>>,
    /// Total length of the `Ok` texts in `texts`. Checked against [`MAX_TEXT_TOTAL`].
    text_total: usize,
}

impl Builder<'_> {
    fn region(&mut self, node: Node<'_, '_>, depth: usize) -> Result<RegionModel, ImportError> {
        const EL: &str = "region";
        if depth > MAX_REGION_DEPTH {
            return Err(ImportError::Nesting);
        }
        self.count += 1;
        if self.count > MAX_REGIONS {
            return Err(ImportError::TooManyRegions { limit: MAX_REGIONS });
        }
        let id: u32 = dom::required_attr(node, EL, "region-id")?;
        // A non-finite coordinate (NaN, inf, or a value such as 1e400 that overflows) is absent.
        let top = dom::parse_attr::<f64>(node, EL, "top")?.filter(|v| v.is_finite());
        let left = dom::parse_attr::<f64>(node, EL, "left")?.filter(|v| v.is_finite());
        let width = dom::parse_attr::<f64>(node, EL, "actualWidth")?.filter(|v| v.is_finite());
        let height = dom::parse_attr::<f64>(node, EL, "actualHeight")?.filter(|v| v.is_finite());

        let contents: Vec<Node<'_, '_>> = node.children().filter(|c| c.is_element()).collect();
        let (kind, result_ref) = self.classify(id, &contents)?;
        let (result_format, matrix) = match contents.first() {
            Some(math) if contents.len() == 1 && math.has_tag_name((WS, "math")) => {
                (math_result_format(*math)?, math_matrix_display(*math)?)
            }
            _ => (None, MatrixDisplay::default()),
        };
        let children = direct_regions(node)
            .into_iter()
            .map(|child| self.region(child, depth + 1))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(RegionModel {
            id,
            pos: top.zip(left).map(|(t, l)| (l, t)),
            size: width.zip(height),
            result_ref,
            kind,
            solve_category: node.attribute("solve-block-category").map(str::to_owned),
            result_format,
            matrix,
            children,
        })
    }

    /// Classifies the single content element of region `id`. Returns the kind and the region's
    /// result reference, which is the `resultRef` of its `<math>`.
    fn classify(
        &mut self,
        id: u32,
        contents: &[Node<'_, '_>],
    ) -> Result<(RegionKind, Option<u32>), ImportError> {
        let [content] = contents else {
            let reason = if contents.is_empty() {
                "region has no content element".to_owned()
            } else {
                format!(
                    "region has {} content elements, expected one",
                    contents.len()
                )
            };
            return Ok((RegionKind::Unsupported { reason }, None));
        };
        let content = *content;
        let name = content.tag_name().name();
        if content.tag_name().namespace() != Some(WS) {
            let reason = format!("region content <{name}> is not a worksheet element");
            return Ok((RegionKind::Unsupported { reason }, None));
        }
        match name {
            "math" => {
                let result_ref = dom::parse_attr(content, "math", "resultRef")?;
                let kind = match convert_math(content) {
                    Ok(expr) => RegionKind::Math(expr),
                    Err(err) => RegionKind::Unsupported {
                        reason: format!("cannot convert math: {err}"),
                    },
                };
                Ok((kind, result_ref))
            }
            "text" => Ok((RegionKind::Text(self.text(id, content)?), None)),
            "plot" => Ok((RegionKind::Plot(plot::plot_info(content)), None)),
            "solveblock" => Ok((
                RegionKind::SolveBlock(SolveBlock {
                    keyword: keyword_of(content),
                }),
                None,
            )),
            "spec-table" => Ok((RegionKind::Table(self.table(id, content)), None)),
            // UNVERIFIED: `<area>` and `<image>` are guessed (see `UNVERIFIED_FORMATS.md`).
            "area" => Ok((RegionKind::Area(area_model(content)), None)),
            "image" => {
                let item = content.attribute("item-idref").unwrap_or_default();
                let kind = match self.pkg.image(item) {
                    Some(bytes) => RegionKind::Image(ImageModel {
                        item: item.to_owned(),
                        bytes: std::sync::Arc::from(bytes),
                    }),
                    None => RegionKind::Unsupported {
                        reason: format!("image `{item}` is not in the package"),
                    },
                };
                Ok((kind, None))
            }
            // UNVERIFIED: no sample has a page break. The element name is a guess.
            "page-break" => Ok((RegionKind::PageBreak, None)),
            other => Ok((
                RegionKind::Unsupported {
                    reason: format!("region content <{other}> is not supported yet"),
                },
                None,
            )),
        }
    }

    /// The members of a `<spec-table>`: each `<math>` child, converted. A member that fails is noted
    /// and kept as `Err`, so the others still run.
    fn table(&mut self, id: u32, element: Node<'_, '_>) -> TableModel {
        let mut members = Vec::new();
        for math in element.children().filter(|c| is_ws(*c, "math")) {
            let result_ref = match dom::parse_attr::<u32>(math, "math", "resultRef") {
                Ok(r) => r,
                Err(err) => {
                    self.notes.push(ImportNote {
                        region_id: id,
                        reason: format!("table member has a bad resultRef: {err}"),
                    });
                    None
                }
            };
            let expr = convert_math(math).map_err(|err| err.to_string());
            if let Err(reason) = &expr {
                self.notes.push(ImportNote {
                    region_id: id,
                    reason: format!("table member cannot be converted: {reason}"),
                });
            }
            members.push(TableMember { result_ref, expr });
        }
        TableModel { members }
    }

    /// The regions of a header or footer part: the `<region>` elements under its `<regions>`, read
    /// like worksheet regions. A missing or empty part gives no regions.
    // UNVERIFIED: no sample has header or footer content (every sample has `<regions />`). The
    // assumption is that the part has the worksheet's `<regions>`/`<region>`/`<text item-idref>`
    // shape, with the text in a flow document of the package.
    fn furniture(&mut self, xml: Option<&str>) -> Result<Vec<RegionModel>, ImportError> {
        let Some(xml) = xml else {
            return Ok(Vec::new());
        };
        let doc = dom::parse(xml)?;
        let Some(regions) = doc.root_element().children().find(|n| is_ws(*n, "regions")) else {
            return Ok(Vec::new());
        };
        direct_regions(regions)
            .into_iter()
            .map(|node| self.region(node, 0))
            .collect()
    }

    /// The text of a `<text>` region: the `<Run>` text of the flow document its `item-idref`
    /// names. The `<text>` element's own content, including any nested `<region>`, is not read.
    /// A flow document that is missing or malformed gives empty text and a note.
    ///
    /// Each distinct flow document is parsed once. Regions that name it share the same `Arc`.
    /// The distinct texts together are limited to [`MAX_TEXT_TOTAL`].
    fn text(&mut self, id: u32, element: Node<'_, '_>) -> Result<TextBody, ImportError> {
        let pkg = self.pkg;
        let Some(item) = element.attribute("item-idref") else {
            return Ok(self.empty_text(id, "text region has no item-idref".to_owned()));
        };
        let Some(xaml) = pkg.flow_document(item) else {
            let problem = match pkg.flow_problem(item) {
                Some(reason) => format!("flow document `{item}` cannot be read: {reason}"),
                None => format!("flow document `{item}` is not in the package"),
            };
            return Ok(self.empty_text(id, problem));
        };
        let key = (xaml.as_ptr() as usize, xaml.len());
        let extracted = match self.texts.get(&key) {
            Some(known) => known.clone(),
            None => {
                let extracted = rich_text::parse(xaml)
                    .map(TextBody::from_rich)
                    .map_err(|err| err.to_string());
                if let Ok(text) = &extracted {
                    self.text_total = self.text_total.saturating_add(text.len());
                    if self.text_total > MAX_TEXT_TOTAL {
                        return Err(ImportError::TextTooLarge {
                            limit: MAX_TEXT_TOTAL,
                        });
                    }
                }
                self.texts.insert(key, extracted.clone());
                extracted
            }
        };
        match extracted {
            Ok(text) => Ok(text),
            Err(err) => {
                Ok(self.empty_text(id, format!("flow document `{item}` is malformed: {err}")))
            }
        }
    }

    fn empty_text(&mut self, region_id: u32, problem: String) -> TextBody {
        self.notes.push(ImportNote {
            region_id,
            reason: format!("{problem}; text is empty"),
        });
        TextBody::from_plain("")
    }
}

/// The attributes of an `<area>`. A missing or unrecognised flag is `false`.
fn area_model(node: Node<'_, '_>) -> AreaModel {
    let flag = |name: &str| matches!(node.attribute(name), Some("true" | "1"));
    AreaModel {
        collapsed: flag("collapsed"),
        locked: flag("locked"),
        hide_border: flag("hide-border"),
    }
}

/// The text of the first `KEYWORD` identifier inside a solve block, such as `find`.
fn keyword_of(node: Node<'_, '_>) -> Option<String> {
    node.descendants()
        .find(|n| {
            n.is_element()
                && n.tag_name().name() == "id"
                && n.attribute("labels") == Some("KEYWORD")
        })
        .map(|n| dom::text_of(n).trim().to_owned())
}

/// The `<region>` elements directly under `node`, in document order. A region nested inside
/// another region is not returned. It belongs to that region's children.
fn direct_regions<'a, 'i>(node: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
    let mut found = Vec::new();
    let mut stack: Vec<Node<'a, 'i>> = node.children().rev().collect();
    while let Some(child) = stack.pop() {
        if !child.is_element() {
            continue;
        }
        if is_ws(child, "region") {
            found.push(child);
        } else {
            stack.extend(child.children().rev());
        }
    }
    found
}

fn is_ws(node: Node<'_, '_>, local: &str) -> bool {
    node.is_element() && node.tag_name().namespace() == Some(WS) && node.tag_name().name() == local
}

/// Appends `list` and, depth first, all members of its regions, in document order.
fn walk<'a>(list: &'a [RegionModel], out: &mut Vec<&'a RegionModel>) {
    for region in list {
        out.push(region);
        walk(&region.children, out);
    }
}

fn report_for(regions: &[RegionModel], notes: Vec<ImportNote>) -> ImportReport {
    let mut all = Vec::new();
    walk(regions, &mut all);
    let unsupported = all
        .iter()
        .filter_map(|region| match &region.kind {
            RegionKind::Unsupported { reason } => Some(UnsupportedRegion {
                region_id: region.id,
                reason: reason.clone(),
            }),
            _ => None,
        })
        .collect();
    ImportReport {
        region_count: all.len(),
        unsupported,
        notes,
    }
}

/// Appends the evaluation order of one sibling list, and of each region's members after it.
fn order_list<'a>(list: &'a [RegionModel], out: &mut Vec<&'a RegionModel>) {
    // `sort_by_key` is stable, so equal keys keep document order.
    let mut siblings: Vec<&RegionModel> = list.iter().collect();
    siblings.sort_by_key(|region| sort_key(region));
    for region in siblings {
        if !matches!(region.kind, RegionKind::SolveBlock(_)) {
            out.push(region);
        }
        order_list(&region.children, out);
    }
}

/// The steps of one sibling list (see [`Worksheet::evaluation_steps`]).
fn steps_list<'a>(list: &'a [RegionModel], out: &mut Vec<Step<'a>>) {
    let mut siblings: Vec<&RegionModel> = list.iter().collect();
    siblings.sort_by_key(|region| sort_key(region));
    for region in siblings {
        if matches!(region.kind, RegionKind::SolveBlock(_)) {
            let mut members: Vec<&RegionModel> = region.children.iter().collect();
            members.sort_by_key(|member| sort_key(member));
            out.push(Step::SolveBlock {
                block: region,
                members: members.clone(),
            });
            for member in members {
                steps_list(&member.children, out);
            }
        } else {
            out.push(Step::Region(region));
            steps_list(&region.children, out);
        }
    }
}

/// Positioned regions first, by row (`top`) then column (`left`), each compared at 0.1 pt
/// resolution (see [`Worksheet::evaluation_order`]). Unpositioned regions follow, all with the
/// same key.
fn sort_key(region: &RegionModel) -> (u8, i64, i64) {
    match region.pos {
        Some((left, top)) => (0, grid(top), grid(left)),
        None => (1, 0, 0),
    }
}

fn grid(value: f64) -> i64 {
    // Saturating cast: NaN becomes 0 and huge values clamp. Positions are finite by
    // construction, so neither case arises from a parsed sheet. Neither panics.
    (value * 10.0).round() as i64
}
