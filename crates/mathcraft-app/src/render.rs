//! Headless rendering of worksheets to PNG, without a window or a display.
//!
//! The same canvas the window draws ([`crate::canvas::show`]) runs frame by frame on an egui
//! context; each frame is drawn by egui-wgpu into an offscreen texture and encoded as PNG. It needs
//! a wgpu adapter (Metal on macOS, Vulkan or a software rasterizer such as lavapipe on Linux);
//! without one [`Renderer::new`] returns [`RenderError::Renderer`].
//!
//! * [`Renderer::render_page`]: one page of the page layout (paper, margins, header and footer).
//! * [`Renderer::render_region`]: one region, cropped, with a padding.
//! * [`plot_data`]: the sampled series of a 2D plot as numbers (no GPU needed).
//! * [`Renderer::render_window`]: the whole app window with a ribbon tab or a dialog staged
//!   (used by `examples/screenshot.rs`).
//!
//! Every image is capped to [`DEFAULT_MAX_SIDE`] pixels on its longest side unless the options say
//! otherwise: the scale is lowered to fit, never the content cropped. Only the part of the sheet
//! that is wanted is rasterized, so a page far down a long sheet costs the same as the first.
//! This module is behind the `render` cargo feature (it is not in the shipped window app).

use std::fmt;
use std::io::Cursor;
use std::path::Path;
use std::time::{Duration, Instant};

use eframe::egui;
use egui_wgpu::{RenderState, ScreenDescriptor};
use mathcraft_import::page::PageSettings;

use crate::app::{App, Staged};
use crate::canvas::{self, Overlay};
use crate::edit::document::Document;
use crate::i18n::{self, Lang};
use crate::load;
use crate::pages::Geometry;
use crate::viewmodel::{Body, Item, ViewModel};

/// Default longest side of a rendered image, in pixels (keeps the token cost of an image small).
pub const DEFAULT_MAX_SIDE: u32 = 1600;
/// Space between the page and the edge of the sheet in the page layout, in points (`canvas.rs`).
const DESK_MARGIN: f32 = 40.0;
/// Longest wait for a worksheet to load and evaluate.
const LOAD_TIMEOUT: Duration = Duration::from_secs(180);
/// Longest wait for the GPU to finish one frame.
const GPU_TIMEOUT: Duration = Duration::from_secs(60);
/// One frame at 60 frames per second, on the simulated clock.
const FRAME_SECONDS: f64 = 1.0 / 60.0;
/// Frames run after a dialog opens, so its fade-in (about 0.1 s) finishes before the capture.
const SETTLE_FRAMES: usize = 45;
/// Frames run before a page or region capture: fonts and image textures, and the area fade-in
/// (about 0.1 s, so at least 6 frames at 60 per second).
const PAGE_FRAMES: usize = 12;

/// Why a render did not produce an image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// No wgpu adapter or renderer could be created here.
    Renderer(String),
    /// The renderer works, but the sheet, the request or the output failed.
    Failed(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::Renderer(m) => write!(f, "no GPU renderer available here ({m})"),
            RenderError::Failed(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for RenderError {}

fn failed<T>(message: impl Into<String>) -> Result<T, RenderError> {
    Err(RenderError::Failed(message.into()))
}

/// An encoded PNG and its size in pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Png {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// An evaluated worksheet and its page setup: what the renderer draws.
#[derive(Debug, Clone)]
pub struct Sheet {
    pub vm: ViewModel,
    pub page: PageSettings,
}

impl Sheet {
    /// Opens and evaluates the `.mcdx` file at `path`.
    ///
    /// # Errors
    /// The loader's message when the file cannot be opened or imported.
    pub fn open(path: &Path) -> Result<Self, RenderError> {
        let loaded = load::open_file(path, None).map_err(RenderError::Failed)?;
        Ok(Self {
            page: loaded.doc.page().clone(),
            vm: loaded.vm,
        })
    }

    /// Evaluates an in-memory document (for example one being edited by an MCP session).
    pub fn from_document(doc: &Document) -> Self {
        Self {
            vm: load::evaluate_document(doc),
            page: doc.page().clone(),
        }
    }
}

/// Number of pages of the sheet's page layout, or `None` when it has none. The same count that
/// [`Renderer::render_page`] checks its `page` against.
pub fn page_count(sheet: &Sheet) -> Option<usize> {
    crate::app::page_geometry_for(true, &sheet.page, &sheet.vm).map(|g| g.count)
}

/// Which region: its stable id, or its position in the view model's item list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionRef {
    Id(u32),
    Index(usize),
}

/// Options of [`Renderer::render_page`].
#[derive(Debug, Clone)]
pub struct PageOptions {
    /// Page number, counted from 1.
    pub page: usize,
    /// Resolution: 72 dpi is one pixel per point. Lowered when the image would pass `max_side`.
    pub dpi: f32,
    /// When set, the image is this many pixels wide instead (still capped by `max_side`).
    pub width_px: Option<u32>,
    pub show_grid: bool,
    /// Longest side of the image in pixels.
    pub max_side: u32,
    pub theme: egui::ThemePreference,
}

impl Default for PageOptions {
    fn default() -> Self {
        Self {
            page: 1,
            dpi: 96.0,
            width_px: None,
            show_grid: false,
            max_side: DEFAULT_MAX_SIDE,
            theme: egui::ThemePreference::Light,
        }
    }
}

/// Options of [`Renderer::render_region`].
#[derive(Debug, Clone)]
pub struct RegionOptions {
    /// Pixels per point. Lowered when the image would pass `max_side`.
    pub scale: f32,
    /// Space around the region, in points.
    pub padding: f32,
    pub show_grid: bool,
    pub max_side: u32,
    pub theme: egui::ThemePreference,
}

impl Default for RegionOptions {
    fn default() -> Self {
        Self {
            scale: 2.0,
            padding: 8.0,
            show_grid: false,
            max_side: DEFAULT_MAX_SIDE,
            theme: egui::ThemePreference::Light,
        }
    }
}

/// Options of [`Renderer::render_window`]: the whole app window.
#[derive(Debug, Clone)]
pub struct WindowOptions {
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub lang: Lang,
    pub theme: egui::ThemePreference,
    /// Tabs, dialogs and windows to show, applied in order after the worksheet has loaded.
    pub staged: Vec<Staged>,
    /// Scripted input (clicks, typing, keys), played in order after `staged`.
    pub script: Vec<Step>,
}

/// One scripted input of the harness (`screenshot --click X,Y --type TEXT --key NAME`). Points are
/// window positions in points.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Click(f32, f32),
    DoubleClick(f32, f32),
    Type(String),
    /// A key by name (`Backspace`, `Delete`, `Tab`, `Enter`, `Escape`, arrows, letters) with
    /// optional `ctrl+` / `shift+` prefixes.
    Key(egui::Key, egui::Modifiers),
}

/// Parses `Backspace`, `shift+Tab`, `ctrl+.` and the like.
pub fn parse_key(text: &str) -> Option<(egui::Key, egui::Modifiers)> {
    let mut modifiers = egui::Modifiers::NONE;
    let mut name = text.trim();
    while let Some((head, tail)) = name.split_once('+') {
        if tail.is_empty() {
            break;
        }
        match head.to_ascii_lowercase().as_str() {
            "ctrl" | "cmd" | "command" => {
                modifiers.ctrl = true;
                modifiers.command = true;
            }
            "shift" => modifiers.shift = true,
            "alt" => modifiers.alt = true,
            _ => return None,
        }
        name = tail;
    }
    let key = egui::Key::from_name(name).or_else(|| match name.to_ascii_lowercase().as_str() {
        "backspace" => Some(egui::Key::Backspace),
        "delete" => Some(egui::Key::Delete),
        "tab" => Some(egui::Key::Tab),
        "enter" => Some(egui::Key::Enter),
        "escape" | "esc" => Some(egui::Key::Escape),
        "left" => Some(egui::Key::ArrowLeft),
        "right" => Some(egui::Key::ArrowRight),
        "up" => Some(egui::Key::ArrowUp),
        "down" => Some(egui::Key::ArrowDown),
        _ => None,
    })?;
    Some((key, modifiers))
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            width: 1400.0,
            height: 900.0,
            scale: 1.0,
            lang: Lang::English,
            theme: egui::ThemePreference::Light,
            staged: Vec::new(),
            script: Vec::new(),
        }
    }
}

/// The item of `region`, or an error naming what is available.
fn find_item(vm: &ViewModel, region: RegionRef) -> Result<&Item, RenderError> {
    let item = match region {
        RegionRef::Id(id) => vm.items.iter().find(|i| i.region_id == id),
        RegionRef::Index(i) => vm.items.get(i),
    };
    match item {
        Some(item) => Ok(item),
        None => failed(format!(
            "no such region ({region:?}); the sheet has {} drawn regions",
            vm.items.len()
        )),
    }
}

/// Longest accepted window side, in points, for [`Renderer::render_window`]. Larger windows would
/// need an offscreen texture the GPU may refuse, so the request fails early instead.
pub const MAX_WINDOW_SIDE: f32 = 8192.0;

/// Checks the page options before any GPU work. NaN and non-positive values are rejected.
fn validate_page(options: &PageOptions) -> Result<(), RenderError> {
    if options.dpi.is_nan() || options.dpi <= 0.0 || options.width_px == Some(0) {
        return failed("dpi and width must be positive");
    }
    Ok(())
}

/// Checks the region options before any GPU work. NaN and non-positive scales are rejected.
fn validate_region(options: &RegionOptions) -> Result<(), RenderError> {
    if options.scale.is_nan() || options.scale <= 0.0 {
        return failed("scale must be positive");
    }
    Ok(())
}

/// Checks the window size before any GPU work: each side must be above 0 and at most
/// [`MAX_WINDOW_SIDE`] points.
fn validate_window(options: &WindowOptions) -> Result<(), RenderError> {
    let side_ok = |v: f32| v > 0.0 && v <= MAX_WINDOW_SIDE;
    if !side_ok(options.width) || !side_ok(options.height) {
        return failed(format!(
            "window width and height must be between 0 and {MAX_WINDOW_SIDE} points (got {} x {})",
            options.width, options.height
        ));
    }
    Ok(())
}

/// The scale that keeps a `w` x `h` point area within `max_side` pixels, never above `wanted`.
fn capped_scale(wanted: f32, w: f32, h: f32, max_side: u32) -> f32 {
    let longest = w.max(h).max(1.0);
    wanted.min(max_side.max(1) as f32 / longest)
}

/// One offscreen window: the egui context and the GPU state that draws it.
struct Screen {
    ctx: egui::Context,
    gpu: RenderState,
    /// Size in points, and pixels per point.
    size: egui::Vec2,
    pixels_per_point: f32,
    last: Option<egui::FullOutput>,
    /// The simulated clock, in seconds.
    time: f64,
    /// Input events for the next frame.
    pending: Vec<egui::Event>,
}

impl Screen {
    fn new(gpu: &RenderState, theme: egui::ThemePreference, size: egui::Vec2, ppp: f32) -> Self {
        // A headless context has no operating-system theme to follow, so the theme is set here.
        let ctx = egui::Context::default();
        ctx.set_theme(theme);
        Self {
            ctx,
            gpu: gpu.clone(),
            size,
            pixels_per_point: ppp,
            last: None,
            time: 0.0,
            pending: Vec::new(),
        }
    }

    /// Runs one frame. Every frame's texture changes are applied, so the font atlas is complete
    /// whenever a frame is drawn. Each frame advances the clock by 1/60 s so that fades finish.
    fn frame(&mut self, run: &mut dyn FnMut(&egui::Context)) {
        let input = self.input();
        self.run_frame(input, run);
    }

    /// One frame of the app: its raw-input hook (as eframe calls it), then its UI.
    fn frame_app(&mut self, app: &mut App) {
        let mut input = self.input();
        let ctx = self.ctx.clone();
        app.take_draft_tabs(&ctx, &mut input);
        self.run_frame(input, &mut |ctx| app.ui(ctx));
    }

    fn input(&mut self) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
            time: Some(self.time),
            events: std::mem::take(&mut self.pending),
            ..Default::default()
        }
    }

    fn run_frame(&mut self, input: egui::RawInput, run: &mut dyn FnMut(&egui::Context)) {
        self.time += FRAME_SECONDS;
        self.ctx.set_pixels_per_point(self.pixels_per_point);
        let output = self.ctx.clone().run(input, |ctx| run(ctx));
        let mut renderer = self.gpu.renderer.write();
        for (id, delta) in &output.textures_delta.set {
            renderer.update_texture(&self.gpu.device, &self.gpu.queue, *id, delta);
        }
        for id in &output.textures_delta.free {
            renderer.free_texture(id);
        }
        drop(renderer);
        self.last = Some(output);
    }

    /// Plays one scripted input and runs a few frames so its effects settle.
    fn play(&mut self, step: &Step, app: &mut App) {
        let mut run = |screen: &mut Self, events: Vec<egui::Event>| {
            screen.pending = events;
            screen.frame_app(app);
        };
        let button = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        match step {
            Step::Click(x, y) | Step::DoubleClick(x, y) => {
                let pos = egui::pos2(*x, *y);
                run(self, vec![egui::Event::PointerMoved(pos)]);
                let clicks = if matches!(step, Step::DoubleClick(..)) {
                    2
                } else {
                    1
                };
                for _ in 0..clicks {
                    run(self, vec![button(pos, true)]);
                    run(self, vec![button(pos, false)]);
                }
            }
            Step::Type(text) => {
                for c in text.chars() {
                    run(self, vec![egui::Event::Text(c.to_string())]);
                }
            }
            Step::Key(key, modifiers) => {
                let key_event = |pressed| egui::Event::Key {
                    key: *key,
                    physical_key: Some(*key),
                    pressed,
                    repeat: false,
                    modifiers: *modifiers,
                };
                run(self, vec![key_event(true), key_event(false)]);
            }
        }
        for _ in 0..SETTLE_FRAMES {
            run(self, Vec::new());
        }
    }

    /// Draws the last frame into an offscreen texture and reads it back.
    fn paint(&mut self) -> Result<image::RgbaImage, String> {
        use egui_wgpu::wgpu;
        let output = self.last.take().ok_or("no frame was run")?;
        let ppp = self.pixels_per_point;
        let width = ((self.size.x * ppp).round() as u32).max(1);
        let height = ((self.size.y * ppp).round() as u32).max(1);
        let limit = self.gpu.device.limits().max_texture_dimension_2d;
        if width > limit || height > limit {
            return Err(format!(
                "image {width}x{height} px passes the GPU limit of {limit} px"
            ));
        }
        let screen = ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point: ppp,
        };
        let primitives = self.ctx.tessellate(output.shapes, ppp);
        let device = &self.gpu.device;
        let queue = &self.gpu.queue;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("render encoder"),
        });
        let user_buffers = self.gpu.renderer.write().update_buffers(
            device,
            queue,
            &mut encoder,
            &primitives,
            &screen,
        );
        let renderer = self.gpu.renderer.read();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("render texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.gpu.target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("render pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            renderer.render(&mut pass, &primitives, &screen);
        }
        queue.submit(user_buffers.into_iter().chain([encoder.finish()]));
        texture_to_image(device, queue, &texture)
    }
}

/// Copies a texture to the CPU and returns it as an RGBA image (rows padded for the GPU copy).
fn texture_to_image(
    device: &egui_wgpu::wgpu::Device,
    queue: &egui_wgpu::wgpu::Queue,
    texture: &egui_wgpu::wgpu::Texture,
) -> Result<image::RgbaImage, String> {
    use egui_wgpu::wgpu;
    let (width, height) = (texture.width(), texture.height());
    let unpadded = width as usize * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
    let padded = unpadded.div_ceil(align) * align;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("render readback"),
        size: (padded * height as usize) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("render readback encoder"),
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded as u32),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let submission = queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(GPU_TIMEOUT),
        })
        .map_err(|e| format!("GPU poll failed: {e}"))?;
    receiver
        .recv()
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let data = slice.get_mapped_range();
    let rows = data
        .chunks_exact(padded)
        .flat_map(|row| row[..unpadded].iter().copied())
        .collect::<Vec<u8>>();
    drop(data);
    buffer.unmap();
    image::RgbaImage::from_raw(width, height, rows).ok_or_else(|| "bad image size".to_string())
}

/// Encodes an RGBA image as PNG.
///
/// # Errors
/// [`RenderError::Failed`] when the encoder fails.
pub fn encode_png(image: &image::RgbaImage) -> Result<Png, RenderError> {
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .map_err(|e| RenderError::Failed(e.to_string()))?;
    Ok(Png {
        bytes,
        width: image.width(),
        height: image.height(),
    })
}

/// How the canvas is laid out for one capture.
struct Layout {
    pages: Option<Geometry>,
    /// The size of the whole canvas, in points.
    canvas_size: egui::Vec2,
    show_grid: bool,
}

/// The GPU state shared by all renders. Create it once and reuse it.
#[derive(Clone)]
pub struct Renderer {
    gpu: RenderState,
}

impl Renderer {
    /// Creates the GPU state: the default wgpu instance and an adapter, with egui-wgpu on top.
    ///
    /// # Errors
    /// [`RenderError::Renderer`] when no adapter is available.
    pub fn new() -> Result<Self, RenderError> {
        let setup = egui_wgpu::WgpuSetup::CreateNew(egui_wgpu::WgpuSetupCreateNew::default());
        let instance = pollster::block_on(setup.new_instance());
        let config = egui_wgpu::WgpuConfiguration {
            wgpu_setup: setup,
            ..Default::default()
        };
        pollster::block_on(RenderState::create(
            &config,
            &instance,
            None,
            egui_wgpu::RendererOptions::PREDICTABLE,
        ))
        .map(|gpu| Self { gpu })
        .map_err(|e| RenderError::Renderer(e.to_string()))
    }

    /// Draws `view.1` points of the canvas starting at `view.0` (canvas points) at `view.2` pixels
    /// per point. Only that part is rasterized.
    fn capture(
        &self,
        sheet: &Sheet,
        layout: &Layout,
        theme: egui::ThemePreference,
        view: (egui::Vec2, egui::Vec2, f32),
    ) -> Result<Png, RenderError> {
        let (offset, size, ppp) = view;
        let mut screen = Screen::new(&self.gpu, theme, size, ppp);
        crate::fonts::install(&screen.ctx);
        let overlay = Overlay {
            hide_grid: !layout.show_grid,
            grid_pt: canvas::grid_points(&sheet.page),
            pages: layout.pages,
            ..Overlay::default()
        };
        let canvas_size = layout.canvas_size;
        let mut run = |ctx: &egui::Context| {
            // The canvas sits in a fixed-size area placed at the negative offset, so the screen
            // shows just the wanted part of a possibly very tall sheet.
            egui::Area::new(egui::Id::new("mathcraft_render"))
                .order(egui::Order::Background)
                .constrain(false)
                .fixed_pos(egui::Pos2::ZERO - offset)
                .show(ctx, |ui| {
                    ui.set_min_size(canvas_size);
                    ui.set_max_size(canvas_size);
                    canvas::show(ui, &sheet.vm, 1.0, false, &overlay);
                });
        };
        for _ in 0..PAGE_FRAMES {
            screen.frame(&mut run);
        }
        let image = screen.paint().map_err(RenderError::Failed)?;
        encode_png(&image)
    }

    /// Renders one page of the page layout (paper, margins, header and footer, regions).
    ///
    /// # Errors
    /// [`RenderError::Failed`] when the page does not exist or the image cannot be made.
    pub fn render_page(&self, sheet: &Sheet, options: &PageOptions) -> Result<Png, RenderError> {
        validate_page(options)?;
        let Some(g) = crate::app::page_geometry_for(true, &sheet.page, &sheet.vm) else {
            return failed("no page layout");
        };
        if options.page == 0 || options.page > g.count {
            return failed(format!(
                "page {} does not exist; the sheet has {} page(s)",
                options.page, g.count
            ));
        }
        let [w, h] = g.paper;
        let wanted = match options.width_px {
            Some(px) => px as f32 / w,
            None => options.dpi / 72.0,
        };
        let ppp = capped_scale(wanted, w, h, options.max_side);
        let layout = Layout {
            pages: Some(g),
            canvas_size: egui::vec2(
                w + 2.0 * DESK_MARGIN,
                g.count as f32 * h + 2.0 * DESK_MARGIN,
            ),
            show_grid: options.show_grid,
        };
        let offset = egui::vec2(DESK_MARGIN, DESK_MARGIN + g.page_top(options.page - 1));
        self.capture(
            sheet,
            &layout,
            options.theme,
            (offset, egui::vec2(w, h), ppp),
        )
    }

    /// Renders one region, cropped to its rectangle plus `padding` points on every side.
    ///
    /// # Errors
    /// [`RenderError::Failed`] when the region does not exist or the image cannot be made.
    pub fn render_region(
        &self,
        sheet: &Sheet,
        region: RegionRef,
        options: &RegionOptions,
    ) -> Result<Png, RenderError> {
        validate_region(options)?;
        let item = find_item(&sheet.vm, region)?;
        let [ox, oy] = sheet.vm.frame.origin();
        let [x, y, w, h] = item.rect;
        let pad = options.padding.max(0.0);
        let (width, height) = (w.max(1.0) + 2.0 * pad, h.max(1.0) + 2.0 * pad);
        let ppp = capped_scale(options.scale, width, height, options.max_side);
        // The list layout (no pages): the content frame starts at the frame origin.
        let layout = Layout {
            pages: None,
            canvas_size: egui::vec2(
                sheet.vm.page[0].max(ox + x + w + pad),
                sheet.vm.page[1].max(oy + y + h + pad),
            ),
            show_grid: options.show_grid,
        };
        let offset = egui::vec2(ox + x - pad, oy + y - pad);
        self.capture(
            sheet,
            &layout,
            options.theme,
            (offset, egui::vec2(width, height), ppp),
        )
    }

    /// Renders the app window on the worksheet at `path` (or an empty one), with `options.staged`
    /// shown: ribbon tabs, dialogs, menus. This is what `examples/screenshot.rs` writes.
    ///
    /// # Errors
    /// [`RenderError::Failed`] when the size is out of range (see [`MAX_WINDOW_SIDE`]), the
    /// worksheet does not load in time or the image fails.
    pub fn render_window(
        &self,
        path: Option<&Path>,
        options: &WindowOptions,
    ) -> Result<Png, RenderError> {
        validate_window(options)?;
        i18n::set_language(options.lang);
        let mut screen = Screen::new(
            &self.gpu,
            options.theme,
            egui::vec2(options.width, options.height),
            options.scale,
        );
        let mut app: Option<App> = None;
        let mut path = path.map(Path::to_path_buf);
        // The first frame creates the app (fonts, options, load); then wait for the load. Staging
        // comes after the load, so tabs and dialogs open on the loaded worksheet.
        let start = Instant::now();
        loop {
            screen.frame(&mut |ctx| {
                let app = app.get_or_insert_with(|| App::with_context(ctx, path.take()));
                app.ui(ctx);
            });
            if !app.as_ref().is_some_and(App::is_busy) {
                break;
            }
            if start.elapsed() > LOAD_TIMEOUT {
                return failed("timed out waiting for the worksheet to load");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if let Some(app) = app.as_mut() {
            // Settle frames first, then stage on the loaded worksheet.
            screen.frame(&mut |ctx| app.ui(ctx));
            for step in &options.staged {
                app.stage(step.clone());
            }
        }
        for _ in 0..SETTLE_FRAMES {
            if let Some(app) = app.as_mut() {
                screen.frame(&mut |ctx| app.ui(ctx));
            }
        }
        if let Some(app) = app.as_mut() {
            for step in &options.script {
                screen.play(step, app);
            }
        }
        let image = screen.paint().map_err(RenderError::Failed)?;
        encode_png(&image)
    }
}

/// One trace of a 2D plot, as numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceSamples {
    pub name: Option<String>,
    /// Polylines of `[x, y]` in data units. A gap in the data starts a new one.
    pub segments: Vec<Vec<[f64; 2]>>,
}

/// The sampled series of a 2D plot, as the engine evaluated them (SI base units).
#[derive(Debug, Clone, PartialEq)]
pub struct PlotSamples {
    pub region_id: u32,
    pub title: Option<String>,
    pub x_range: (f64, f64),
    pub y_range: (f64, f64),
    pub log_x: bool,
    pub log_y: bool,
    pub traces: Vec<TraceSamples>,
}

/// The sampled series of the 2D plot `region`. Needs no GPU.
///
/// # Errors
/// [`RenderError::Failed`] when the region does not exist or is not an evaluated 2D plot.
pub fn plot_data(vm: &ViewModel, region: RegionRef) -> Result<PlotSamples, RenderError> {
    let item = find_item(vm, region)?;
    let Body::Plot(view) = &item.body else {
        return failed(format!(
            "region {} is not an evaluated 2D plot",
            item.region_id
        ));
    };
    Ok(PlotSamples {
        region_id: item.region_id,
        title: view.title.clone(),
        x_range: (view.x.min, view.x.max),
        y_range: (view.y.min, view.y.max),
        log_x: view.x.log,
        log_y: view.y.log,
        traces: view
            .traces
            .iter()
            .map(|t| TraceSamples {
                name: t.name.clone(),
                segments: t.segments.clone(),
            })
            .collect(),
    })
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
