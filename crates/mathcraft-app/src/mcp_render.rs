//! The renderer behind the MCP render tools (`render_page`, `render_region`, `render_plot_data`,
//! `app_screenshot`): egui-wgpu into an offscreen texture. Each job runs on a thread of its own
//! (`spawn`), so the window's UI thread never waits for a render; the 30 s deadline is enforced by
//! the server (`mathcraft_mcp::tools::render::RENDER_DEADLINE`). The MCP crate cannot depend on
//! this crate (the app hosts the live MCP endpoint), so it defines the `RenderBackend` trait and
//! the binaries call [`install`] once at start.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, OnceLock};

use crate::render::{
    self, PageOptions, PlotSamples, RegionOptions, RegionRef, RenderError, Renderer, Sheet,
};
use mathcraft_doc::Document;
use serde_json::{json, Value};

use mathcraft_mcp::tools::render::{Image, PageRequest, RegionRequest, RenderBackend, RenderJob};
use mathcraft_mcp::tools::ToolOutput;
use mathcraft_mcp::ToolError;

/// Most points of a plot returned; longer series are thinned evenly.
const MAX_PLOT_POINTS: usize = 1000;
/// Default resolution of a page, in dpi (the app's default).
const DEFAULT_DPI: f64 = 96.0;

/// The GPU state, created on the first render (never at start-up) and kept for the life of the
/// process. An error (no adapter) is kept too, so the adapter is not searched again on every call.
static RENDERER: OnceLock<Result<Renderer, String>> = OnceLock::new();
/// Set while a job runs on its thread.
static BUSY: AtomicBool = AtomicBool::new(false);

/// Clears `BUSY` when dropped (also when the job panics).
struct Idle;

impl Drop for Idle {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

/// The shared renderer, or `render_unavailable`.
fn renderer() -> Result<&'static Renderer, ToolError> {
    match RENDERER.get_or_init(|| Renderer::new().map_err(|e| e.to_string())) {
        Ok(renderer) => Ok(renderer),
        Err(message) => Err(ToolError::RenderUnavailable(message.clone())),
    }
}

/// Answers a receiver with one ready result (the job never runs).
fn answered(
    result: Result<ToolOutput, ToolError>,
) -> mpsc::Receiver<Result<ToolOutput, ToolError>> {
    let (tx, rx) = mpsc::channel();
    let _ = tx.send(result);
    rx
}

/// Runs `job` on a thread with the engine's stack size and returns the receiver of its answer.
/// One render at a time: a job that starts while the previous one still runs (after its deadline)
/// is answered `render_failed` at once.
fn spawn_job(job: RenderJob) -> mpsc::Receiver<Result<ToolOutput, ToolError>> {
    if BUSY
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return answered(Err(ToolError::RenderFailed(
            "an earlier render is still running; try again in a few seconds".into(),
        )));
    }
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .stack_size(mathcraft_import::evaluate::STACK_SIZE)
        .spawn(move || {
            let result = {
                let _idle = Idle;
                job()
            };
            // The flag is cleared before the reply, so the next call sees it free.
            let _ = tx.send(result);
        });
    match spawned {
        Ok(_) => rx,
        Err(_) => {
            // The closure (and its job) was dropped with the failed spawn.
            BUSY.store(false, Ordering::SeqCst);
            answered(Err(ToolError::Internal(
                "cannot start the render thread".into(),
            )))
        }
    }
}

fn failure(e: RenderError) -> ToolError {
    match e {
        RenderError::Renderer(m) => ToolError::RenderUnavailable(m),
        RenderError::Failed(m) => ToolError::RenderFailed(m),
    }
}

fn image(png: render::Png) -> Image {
    Image {
        png: png.bytes,
        width: png.width,
        height: png.height,
    }
}

/// The page count is checked before the GPU is touched, so a page past the end is
/// `invalid_params` even without an adapter. Runs on the job's thread.
fn page(document: Document, request: PageRequest) -> Result<Image, ToolError> {
    let sheet = Sheet::from_document(&document);
    let count = render::page_count(&sheet)
        .ok_or_else(|| ToolError::RenderFailed("the worksheet has no page layout".into()))?;
    if request.page > count {
        return Err(ToolError::InvalidParams(format!(
            "page {} does not exist; the worksheet has {count} page(s)",
            request.page
        )));
    }
    let renderer = renderer()?;
    let options = PageOptions {
        page: request.page,
        dpi: request.dpi.unwrap_or(DEFAULT_DPI) as f32,
        width_px: request.width_px,
        show_grid: request.show_grid,
        max_side: request.max_side,
        ..PageOptions::default()
    };
    renderer
        .render_page(&sheet, &options)
        .map(image)
        .map_err(failure)
}

fn region(document: Document, request: RegionRequest) -> Result<Image, ToolError> {
    let renderer = renderer()?;
    let sheet = Sheet::from_document(&document);
    let options = RegionOptions {
        scale: request.scale as f32,
        show_grid: request.show_grid,
        max_side: request.max_side,
        ..RegionOptions::default()
    };
    renderer
        .render_region(&sheet, RegionRef::Id(request.region_id), &options)
        .map(image)
        .map_err(failure)
}

fn plot(document: Document, region_id: u32) -> Result<Value, ToolError> {
    // No renderer here: the sampler only evaluates the sheet.
    let sheet = Sheet::from_document(&document);
    let samples = render::plot_data(&sheet.vm, RegionRef::Id(region_id)).map_err(|e| match e {
        RenderError::Renderer(m) | RenderError::Failed(m) => ToolError::InvalidParams(m),
    })?;
    Ok(samples_json(&samples))
}

/// The samples as JSON. A trace longer than [`MAX_PLOT_POINTS`] keeps every `stride`-th point
/// of each of its segments, so the reply stays a sane size.
fn samples_json(s: &PlotSamples) -> Value {
    let total: usize = s
        .traces
        .iter()
        .flat_map(|t| t.segments.iter())
        .map(Vec::len)
        .sum();
    let stride = total.div_ceil(MAX_PLOT_POINTS).max(1);
    let traces: Vec<Value> = s
        .traces
        .iter()
        .map(|t| {
            let segments: Vec<Vec<[f64; 2]>> = t
                .segments
                .iter()
                .map(|seg| seg.iter().step_by(stride).copied().collect())
                .collect();
            json!({"name": t.name, "segments": segments})
        })
        .collect();
    json!({
        "region_id": s.region_id,
        "title": s.title,
        "x_range": [s.x_range.0, s.x_range.1],
        "y_range": [s.y_range.0, s.y_range.1],
        "log_x": s.log_x,
        "log_y": s.log_y,
        "points_total": total,
        "stride": stride,
        "traces": traces,
    })
}

struct Gpu;

impl RenderBackend for Gpu {
    fn page(&self, document: Document, request: PageRequest) -> Result<Image, ToolError> {
        page(document, request)
    }

    fn region(&self, document: Document, request: RegionRequest) -> Result<Image, ToolError> {
        region(document, request)
    }

    fn plot(&self, document: Document, region_id: u32) -> Result<Value, ToolError> {
        plot(document, region_id)
    }

    fn spawn(
        &self,
        job: RenderJob,
    ) -> Result<mpsc::Receiver<Result<ToolOutput, ToolError>>, RenderJob> {
        Ok(spawn_job(job))
    }
}

/// Makes the render tools of `mathcraft-mcp` use this renderer. Call once, before serving. Cheap:
/// the GPU device is only created by the first render.
pub fn install() {
    mathcraft_mcp::tools::render::set_render_backend(Box::new(Gpu));
}
