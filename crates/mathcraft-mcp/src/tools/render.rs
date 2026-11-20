//! Render tools: the AI looks at what it made.
//!
//! * `render_page`, `render_region`: a PNG as MCP image content, with a one-line text summary.
//! * `render_plot_data`: the sampled series of a 2D plot as numbers (structured JSON).
//!
//! Arguments are checked before any work starts. The GPU work (feature `render`) runs on a worker
//! thread with a 30 s deadline, so the JSON-RPC loop never waits longer. A render that overruns
//! answers `render_failed` and keeps running in the background; until it ends, new renders are
//! refused with `render_failed` rather than queued. Without the feature, or without a GPU adapter,
//! the tools answer `render_unavailable`. Plot data needs no GPU, but it needs the feature too,
//! because the sampler lives in the `render` module of `mathcraft-app`.

use std::time::Duration;

use serde_json::{json, Value};

use mathcraft_doc::{Doc, Document};

use crate::error::ToolError;
use crate::schema::{self, Args};
use crate::tools::{Annotations, Content, Deferred, Tool, ToolCtx, ToolOutput, ToolTable};

/// Longest side of an image unless `max_side` says otherwise (the app's `DEFAULT_MAX_SIDE`).
const DEFAULT_MAX_SIDE: u32 = 1600;
/// Ceiling of `max_side` and `width_px`, in pixels.
const MAX_SIDE: u32 = 4096;
const MAX_PAGE: u64 = 10_000;
const MAX_DPI: f64 = 1200.0;
const MAX_SCALE: f64 = 16.0;
/// Default pixels per point of a region (the app's default).
const DEFAULT_SCALE: f64 = 2.0;

pub fn register(t: &mut ToolTable) {
    t.add(Tool {
        name: "render_page",
        description: "One page of the worksheet as a PNG image (paper, margins, header, footer and regions), so you can look at the layout. The longest side is capped (default 1600 px, at most 4096). Also returns a text line with the size and the number of regions whose top edge is on that page.".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("page", int("Page number, counted from 1 (default 1)", 1, MAX_PAGE)),
                ("dpi", number("Resolution in dpi, 72 is one pixel per point (default 96). Lowered when the image would pass max_side. Not with width_px.", MAX_DPI)),
                ("width_px", int("Image width in pixels instead of dpi (still capped by max_side). Not with dpi.", 1, u64::from(MAX_SIDE))),
                ("show_grid", schema::boolean("Draw the grid (default false)")),
                ("max_side", int("Longest side in pixels (default 1600, at most 4096)", 1, u64::from(MAX_SIDE))),
            ],
            &["document_id"],
        ),
        annotations: Annotations::read_only("Render page"),
        handler: render_page,
    });
    t.add(Tool {
        name: "render_region",
        description: "One region of the worksheet, cropped to its rectangle, as a PNG image. Ids come from read_outline. The longest side is capped (default 1600 px, at most 4096).".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("region_id", schema::integer("Region id from read_outline")),
                ("scale", number("Pixels per point (default 2). Lowered when the image would pass max_side.", MAX_SCALE)),
                ("show_grid", schema::boolean("Draw the grid (default false)")),
                ("max_side", int("Longest side in pixels (default 1600, at most 4096)", 1, u64::from(MAX_SIDE))),
            ],
            &["document_id", "region_id"],
        ),
        annotations: Annotations::read_only("Render region"),
        handler: render_region,
    });
    t.add(Tool {
        name: "render_plot_data",
        description: "The sampled series of a 2D plot region as numbers: x and y pairs as the engine evaluated them (SI base units), with the axis ranges and the traces. Long series are thinned evenly (see `stride`). Use it to check plotted values.".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("region_id", schema::integer("Region id of a 2D plot, from read_outline")),
            ],
            &["document_id", "region_id"],
        ),
        annotations: Annotations::read_only("Render plot data"),
        handler: render_plot_data,
    });
}

/// A schema for an integer in `min..=max`.
fn int(description: &str, min: u64, max: u64) -> Value {
    json!({"type": "integer", "minimum": min, "maximum": max, "description": description})
}

/// A schema for a number above 0 and at most `max`.
fn number(description: &str, max: f64) -> Value {
    json!({"type": "number", "exclusiveMinimum": 0, "maximum": max, "description": description})
}

/// A rendered PNG and its size in pixels.
pub struct Image {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Checked arguments of `render_page`.
pub struct PageRequest {
    pub page: usize,
    pub dpi: Option<f64>,
    pub width_px: Option<u32>,
    pub show_grid: bool,
    pub max_side: u32,
}

/// Checked arguments of `render_region`.
pub struct RegionRequest {
    pub region_id: u32,
    pub scale: f64,
    pub show_grid: bool,
    pub max_side: u32,
}

pub(crate) fn render_page(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&[
        "document_id",
        "page",
        "dpi",
        "width_px",
        "show_grid",
        "max_side",
    ])?;
    let dpi = bounded_number(args, "dpi", MAX_DPI)?;
    let width_px = bounded_int(args, "width_px", 1, u64::from(MAX_SIDE))?;
    if dpi.is_some() && width_px.is_some() {
        return Err(ToolError::InvalidParams(
            "pass `dpi` or `width_px`, not both".into(),
        ));
    }
    let request = PageRequest {
        page: bounded_int(args, "page", 1, MAX_PAGE)?.unwrap_or(1) as usize,
        dpi,
        width_px: width_px.map(|v| v as u32),
        show_grid: args.opt_bool("show_grid")?,
        max_side: max_side(args)?,
    };
    let page = request.page;
    let max = request.max_side;
    let snap = snapshot(ctx, args.str("document_id")?, None, Some(page))?;
    let regions_on_page = snap.regions_on_page;
    let document = snap.document;
    gpu::start(Box::new(move || {
        let image = gpu::page(document, request)?;
        let summary = format!(
            "Page {page} of the worksheet: {} x {} px PNG, {} region(s) with their top edge on this page.",
            image.width, image.height, regions_on_page
        );
        let structured = json!({
            "page": page,
            "width": image.width,
            "height": image.height,
            "max_side": max,
            "bytes": image.png.len(),
            "regions_on_page": regions_on_page,
        });
        Ok(image_output(summary, &image, structured))
    }))
}

fn render_region(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id", "region_id", "scale", "show_grid", "max_side"])?;
    let request = RegionRequest {
        region_id: args.region_id("region_id")?,
        scale: bounded_number(args, "scale", MAX_SCALE)?.unwrap_or(DEFAULT_SCALE),
        show_grid: args.opt_bool("show_grid")?,
        max_side: max_side(args)?,
    };
    let region_id = request.region_id;
    let max = request.max_side;
    let snap = snapshot(ctx, args.str("document_id")?, Some(region_id), None)?;
    let document = snap.document;
    gpu::start(Box::new(move || {
        let image = gpu::region(document, request)?;
        let summary = format!(
            "Region {region_id}: {} x {} px PNG, cropped to the region.",
            image.width, image.height
        );
        let structured = json!({
            "region_id": region_id,
            "width": image.width,
            "height": image.height,
            "max_side": max,
            "bytes": image.png.len(),
        });
        Ok(image_output(summary, &image, structured))
    }))
}

fn render_plot_data(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id", "region_id"])?;
    let region_id = args.region_id("region_id")?;
    let snap = snapshot(ctx, args.str("document_id")?, Some(region_id), None)?;
    let document = snap.document;
    gpu::start(Box::new(move || {
        gpu::plot(document, region_id).map(ToolOutput::json)
    }))
}

/// The document as the render needs it, taken under the backend lock. The render itself runs
/// after the lock is released, so a slow render does not hold up other tools.
struct Snapshot {
    document: Document,
    regions_on_page: usize,
}

/// Copies the document and checks the ids the call names. `region` must exist in the document;
/// `page` is only counted (the render checks that the page exists).
fn snapshot(
    ctx: &ToolCtx<'_>,
    document_id: &str,
    region: Option<u32>,
    page: Option<usize>,
) -> Result<Snapshot, ToolError> {
    let mut taken: Option<Snapshot> = None;
    ctx.backend.with_doc(document_id, &mut |doc| {
        if let Some(id) = region {
            if doc.region(id).is_none() {
                return Err(crate::error::no_region(id));
            }
        }
        taken = Some(Snapshot {
            document: (**doc).clone(),
            regions_on_page: page.map_or(0, |p| regions_on_page(doc, p)),
        });
        Ok(Value::Null)
    })?;
    taken.ok_or_else(|| ToolError::Internal("the document was not read".into()))
}

/// Regions whose top edge is on `page` (1-based). The same rule as `read_outline`'s `page`.
fn regions_on_page(doc: &Doc, page: usize) -> usize {
    let page_settings = doc.page();
    let margins = page_settings.margins();
    let printable = (page_settings.paper_points()[1] - margins.top - margins.bottom).max(1.0);
    doc.outline()
        .iter()
        .filter(|r| {
            let top = r.pos.map_or(0.0, |(_, y)| y);
            ((top / printable).floor().max(0.0) as usize) + 1 == page
        })
        .count()
}

fn image_output(summary: String, image: &Image, structured: Value) -> ToolOutput {
    ToolOutput {
        deferred: None,
        content: vec![
            Content::Text(summary),
            Content::Image {
                data: base64(&image.png),
                mime: "image/png",
            },
        ],
        structured: Some(structured),
    }
}

/// `max_side`, default 1600, never above 4096.
fn max_side(args: &Args<'_>) -> Result<u32, ToolError> {
    Ok(bounded_int(args, "max_side", 1, u64::from(MAX_SIDE))?
        .map_or(DEFAULT_MAX_SIDE, |v| v as u32))
}

/// An optional integer in `min..=max`. Negative, fractional and huge values are refused.
fn bounded_int(args: &Args<'_>, key: &str, min: u64, max: u64) -> Result<Option<u64>, ToolError> {
    match args.opt_u64(key)? {
        None => Ok(None),
        Some(v) if (min..=max).contains(&v) => Ok(Some(v)),
        Some(_) => Err(ToolError::InvalidParams(format!(
            "`{key}` must be from {min} to {max}"
        ))),
    }
}

/// An optional number above 0 and at most `max`. NaN, infinities, strings and zero are refused.
fn bounded_number(args: &Args<'_>, key: &str, max: f64) -> Result<Option<f64>, ToolError> {
    let value = match args.0.get(key) {
        None | Some(Value::Null) => return Ok(None),
        Some(v) => v,
    };
    match value.as_f64() {
        Some(n) if n.is_finite() && n > 0.0 && n <= max => Ok(Some(n)),
        _ => Err(ToolError::InvalidParams(format!(
            "`{key}` must be a number above 0 and at most {max}"
        ))),
    }
}

/// Standard base64 (RFC 4648, with padding), for the image content block.
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Longest a render may take before its tool answers `render_failed`.
pub const RENDER_DEADLINE: Duration = Duration::from_secs(30);

/// One render job: runs on a thread the backend gives it, and answers the tool call.
pub type RenderJob = Box<dyn FnOnce() -> Result<ToolOutput, ToolError> + Send>;

/// The renderer an embedding binary installs (the app crate owns the GPU code, so this crate does
/// not depend on it). Without one, the render tools answer `render_unavailable`.
pub trait RenderBackend: Send + Sync {
    /// One page as PNG. Called inside a [`RenderJob`].
    ///
    /// # Errors
    /// `render_unavailable`, `render_failed` or `invalid_params` (page past the end).
    fn page(&self, document: Document, request: PageRequest) -> Result<Image, ToolError>;
    /// One region as PNG. Called inside a [`RenderJob`].
    ///
    /// # Errors
    /// As [`RenderBackend::page`].
    fn region(&self, document: Document, request: RegionRequest) -> Result<Image, ToolError>;
    /// The sampled series of a 2D plot. Called inside a [`RenderJob`].
    ///
    /// # Errors
    /// As [`RenderBackend::page`].
    fn plot(&self, document: Document, region_id: u32) -> Result<Value, ToolError>;
    /// Starts `job` on a thread of its own and returns the receiver of its result, so the caller
    /// (the window's UI thread) never waits for a render. Gives the job back when the backend has
    /// no such thread: the job then runs inline. The default runs inline.
    fn spawn(
        &self,
        job: RenderJob,
    ) -> Result<std::sync::mpsc::Receiver<Result<ToolOutput, ToolError>>, RenderJob> {
        Err(job)
    }
}

static RENDER_BACKEND: std::sync::OnceLock<Box<dyn RenderBackend>> = std::sync::OnceLock::new();

/// Installs the renderer. Returns false when one is installed already (the first one stays).
pub fn set_render_backend(backend: Box<dyn RenderBackend>) -> bool {
    RENDER_BACKEND.set(backend).is_ok()
}

mod gpu {
    use std::time::Instant;

    use mathcraft_doc::Document;
    use serde_json::Value;

    use super::{Deferred, Image, PageRequest, RegionRequest, RenderJob, RENDER_BACKEND};
    use crate::error::ToolError;
    use crate::tools::ToolOutput;

    fn backend() -> Result<&'static dyn super::RenderBackend, ToolError> {
        RENDER_BACKEND.get().map(AsRef::as_ref).ok_or_else(|| {
            ToolError::RenderUnavailable("this build has no renderer installed".into())
        })
    }

    /// Runs a render job on the backend's thread, or inline when it has none. The answer is
    /// deferred: the server waits for it (headless) or the window polls it, both within the
    /// render deadline.
    pub fn start(job: RenderJob) -> Result<ToolOutput, ToolError> {
        match backend()?.spawn(job) {
            Ok(rx) => Ok(ToolOutput::deferred(Deferred {
                rx,
                deadline: Instant::now() + super::RENDER_DEADLINE,
                timed_out: ToolError::RenderFailed(format!(
                    "timed out after {} s; the render is still running in the background",
                    super::RENDER_DEADLINE.as_secs()
                )),
                lost: ToolError::RenderFailed("the render stopped unexpectedly".into()),
            })),
            Err(job) => job(),
        }
    }

    pub fn page(d: Document, r: PageRequest) -> Result<Image, ToolError> {
        backend()?.page(d, r)
    }

    pub fn region(d: Document, r: RegionRequest) -> Result<Image, ToolError> {
        backend()?.region(d, r)
    }

    pub fn plot(d: Document, id: u32) -> Result<Value, ToolError> {
        backend()?.plot(d, id)
    }
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_matches_the_rfc_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
