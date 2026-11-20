//! Canvas: draws the view model on a paper-coloured page with a light blue grid, and the editing
//! overlay (selection, insertion point, draft). Thin: every decision is made in `viewmodel.rs`,
//! `mathlayout.rs` and `edit/session.rs`; this file reports clicks and draws what it is given.

use crate::i18n::tr;
use eframe::egui::text::{CCursor, LayoutJob, TextFormat};
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use mathcraft_import::rich_text::{Align as TextAlign, RichText, RunStyle};
use std::sync::Arc;

use crate::edit::resize::Corner;
use crate::edit::session::{DraftPointer, DraftView};
use crate::fonts::{self, Face};
use crate::furniture::{self, Fields};
use crate::glyphs::ELLIPSIS;
use crate::mathlayout::{MathBox, Prim};
use crate::pages::Geometry;
use crate::plot::{self, PlotView};
use crate::plot3d::{self, Angles, Plot3dView};
use crate::table::TableView;
use crate::theme;
use crate::viewmodel::{
    AreaView, Body, ImageView, Item, ResultLine, Tone, ViewModel, AREA_HEADER_PT, MATH_SIZE,
    RESULT_GAP, TEXT_SIZE,
};
use mathcraft_import::page::PageSettings;

/// Grid spacing in points for the Standard grid: 5 mm. Measured on Prime's screenshot 0003: an A4
/// sheet with the 25 mm standard margins has 42 cells across the paper and 32 inside the margins.
const GRID_PT: f32 = 14.173;
/// Gap in points left between a plot and the plot that starts on its right.
const PLOT_GAP_PT: f32 = 6.0;

/// Grid spacing in points at 100 % for a page's `grid-size`: `Fine` is half the Standard grid,
/// any other value is Standard.
pub fn grid_points(page: &PageSettings) -> f32 {
    if page.grid_size.eq_ignore_ascii_case("Fine") {
        GRID_PT / 2.0
    } else {
        GRID_PT
    }
}
/// Space between the page and the edge of the scroll area, in pixels.
const DESK_MARGIN: f32 = 40.0;
/// Page height used for the page counter, in points (US letter).
pub const PAGE_PT: f32 = 792.0;
/// Gap between a math region and its result, in points.
/// Size of the tick labels of 2D and 3D plots, in points at 100 % (Prime's are about this size).
const TICK_PT: f32 = 10.5;
/// Size of a plot's title, in points.
const TITLE_PT: f32 = 13.0;
/// Baseline offset below the text anchor, as a fraction of the font size.
const DESCENT: f32 = 0.53;

/// What the editing session wants drawn over the page.
#[derive(Debug, Clone, Copy, Default)]
pub struct Overlay<'a> {
    /// Regions the AI just changed, with the strength of their highlight (1 down to 0).
    pub flash: &'a [(u32, f32)],
    /// Regions outlined as selected.
    pub selected: &'a [u32],
    /// Region not drawn because the draft replaces it.
    pub hidden: Option<u32>,
    /// Insertion point, in page points.
    pub crosshair: Option<[f64; 2]>,
    /// The draft being typed.
    pub draft: Option<&'a DraftView>,
    /// Regions being dragged, and their displacement in page points. They are drawn moved.
    pub moving: Option<(&'a [u32], [f64; 2])>,
    /// The rubber band being dragged, as `[left, top, right, bottom]` in page points.
    pub marquee: Option<[f64; 4]>,
    /// The selected image and its frame in page points (region id, `[left, top, width, height]`).
    /// It is drawn with corner handles, and at the frame while a handle is dragged.
    pub image_frame: Option<(u32, [f64; 4])>,
    /// The grid is not drawn (the grid is on by default; the View tab turns it off).
    pub hide_grid: bool,
    /// Grid spacing in points at 100 % (see [`grid_points`]).
    pub grid_pt: f32,
    /// Each region is outlined with a thin border (off by default).
    pub region_borders: bool,
    /// The page layout, when the window shows pages instead of one endless sheet.
    pub pages: Option<Geometry>,
    /// The regions with a search match, and whether the match is the current one. A region is
    /// tinted (the current one more strongly); a match inside a region is not located more finely.
    pub search_hits: &'a [(u32, bool)],
    /// A region to scroll into view this frame (set by a search step).
    pub scroll_to: Option<u32>,
    /// A double click that started editing a region: the word under this point (stored frame of
    /// the display, as `Overlay::draft`) is selected in the draft. See `EditSession::take_word_point`.
    pub pending_word: Option<[f64; 2]>,
}

/// What the canvas reports back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Output {
    /// The page the visible centre is on (1-based) and the page count.
    pub page: (usize, usize),
    /// A click inside the page, in page points.
    pub click: Option<[f64; 2]>,
    /// The press that started a drag on the page, in page points.
    pub drag_start: Option<[f64; 2]>,
    /// The total displacement of the drag in page points, while it lasts.
    pub drag: Option<[f64; 2]>,
    /// True on the frame the mouse is released after a drag.
    pub drag_end: bool,
    /// A press, drag or double click inside the draft being typed. Such a mouse action edits the
    /// draft's text, so it is not also a click or a drag on the page.
    pub draft: Option<DraftPointer>,
    /// A click on the collapse icon of an area (its header box). The area's display state is
    /// toggled by the caller. Such a click is not also reported as `click`.
    pub area_toggle: Option<u32>,
    /// A double click in the top margin (`false`) or the bottom margin (`true`) of a page in page
    /// layout: the header or footer is to be edited.
    pub furniture: Option<bool>,
    /// The press that started a drag on a corner handle of the selected image.
    pub resize_start: Option<Corner>,
    /// The area chosen to be locked or unlocked in its context menu (right-click).
    pub area_lock: Option<u32>,
    /// The entry chosen in the worksheet's context menu (right-click on empty page).
    pub menu: Option<crate::menus::WsAction>,
    /// A double click on a region, outside the draft, in page points (stored frame).
    pub region_word: Option<[f64; 2]>,
}

/// Which margin of a page in the page layout a frame point is in: `Some(false)` for the top margin
/// (the header), `Some(true)` for the bottom margin (the footer), `None` elsewhere. `p` is in the
/// content frame, whose origin is the margin box corner, so the paper y is `p[1] + top`.
fn margin_part(g: &Geometry, p: [f64; 2]) -> Option<bool> {
    let [top, _, bottom, _] = g.margins.map(f64::from);
    let height = f64::from(g.paper[1]);
    let total = height * g.count as f64;
    let paper_y = p[1] + top;
    if !(0.0..total).contains(&paper_y) {
        return None;
    }
    let local = paper_y - height * (paper_y / height).floor();
    if local < top {
        Some(false)
    } else if local > height - bottom {
        Some(true)
    } else {
        None
    }
}

/// Draws the page and reports the page, clicks and drags on it.
pub fn show(
    ui: &mut Ui,
    vm: &ViewModel,
    zoom: f32,
    show_prime: bool,
    overlay: &Overlay<'_>,
) -> Output {
    // The sheet's size in points: the stacked pages in page layout, else the view model's page.
    let sheet_pt = match overlay.pages {
        Some(g) => [g.paper[0], g.count as f32 * g.paper[1]],
        None => vm.page,
    };
    let page_px = Vec2::new(sheet_pt[0] * zoom, sheet_pt[1] * zoom);
    // The content frame starts at the margin box: stored positions are measured from its corner.
    let [origin_x, origin_y] = vm.frame.origin();
    let frame_offset = Vec2::new(origin_x, origin_y) * zoom;
    let area = ui.available_size();
    // Page layout: the pages sit on a desk with a margin around them. List (draft) layout: no
    // desk and no pages, the sheet fills the canvas.
    let pad = if overlay.pages.is_some() {
        DESK_MARGIN
    } else {
        0.0
    };
    let content = Vec2::new(
        (page_px.x + 2.0 * pad).max(area.x),
        (page_px.y + 2.0 * pad).max(area.y),
    );
    let mut click = None;
    let mut drag_start = None;
    let mut drag = None;
    let mut drag_end = false;
    let mut draft_pointer = None;
    let mut area_toggle = None;
    let mut furniture_edit = None;
    let mut resize_start = None;
    let mut area_lock = None;
    let mut ws_menu = None;
    let mut region_word = None;
    // A drag moves regions, so it must not also scroll the page: the scroll bars and the wheel
    // still scroll.
    let no_drag_scroll = egui::scroll_area::ScrollSource {
        drag: false,
        ..egui::scroll_area::ScrollSource::ALL
    };
    let output = egui::ScrollArea::both()
        .auto_shrink([false, false])
        .scroll_source(no_drag_scroll)
        .show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(content, Sense::click_and_drag());
            let painter = ui.painter_at(rect);
            let desk = if overlay.pages.is_some() {
                theme::DESK
            } else {
                theme::PAPER
            };
            painter.rect_filled(rect, 0.0, desk);
            let left = rect.min.x + ((content.x - page_px.x) / 2.0).max(pad);
            let page = Rect::from_min_size(Pos2::new(left, rect.min.y + pad), page_px);
            // The content frame: its origin is the margin box corner, and content is drawn from it.
            let content = page.translate(frame_offset);
            match overlay.pages {
                Some(g) => {
                    // Page layout: the paper of each page, and the grid only inside its margins.
                    painter.rect_filled(page, 0.0, theme::PAPER);
                    if !overlay.hide_grid {
                        for index in 0..g.count {
                            let [l, t, r, b] = g.printable(index);
                            let area = Rect::from_min_max(
                                page.min + Vec2::new(l, t) * zoom,
                                page.min + Vec2::new(r, b) * zoom,
                            );
                            draw_grid(&painter, area, content.min, zoom, overlay.grid_pt);
                        }
                    }
                }
                None => {
                    // List layout: the grid covers the whole canvas, anchored at the frame's origin.
                    if !overlay.hide_grid {
                        draw_grid(&painter, rect, content.min, zoom, overlay.grid_pt);
                    }
                }
            }
            if let Some(g) = overlay.pages {
                // The header or footer being typed is drawn by the draft, not under it.
                let hide_furniture = overlay.draft.is_some_and(|d| d.furniture);
                draw_pages(
                    ui,
                    &painter,
                    (page, content.min),
                    zoom,
                    &g,
                    vm,
                    hide_furniture,
                );
            }
            // The collapse icons of the areas, for the click test below.
            let mut toggles: Vec<(u32, Rect)> = Vec::new();
            for item in &vm.items {
                if Some(item.region_id) == overlay.hidden {
                    continue;
                }
                // The selected image is drawn at its frame: its size changes while a handle is dragged.
                let previewed;
                let item = match overlay.image_frame {
                    Some((id, f))
                        if id == item.region_id
                            && matches!(item.body, Body::Image(_) | Body::Plot(_)) =>
                    {
                        previewed = Item {
                            rect: f.map(|v| v as f32),
                            ..item.clone()
                        };
                        &previewed
                    }
                    _ => item,
                };
                // A plot stops before the plot on its right, so that frame does not cover its axis.
                let narrowed;
                let item = match plot_room(vm, item) {
                    Some(w) => {
                        narrowed = Item {
                            rect: [item.rect[0], item.rect[1], w, item.rect[3]],
                            ..item.clone()
                        };
                        &narrowed
                    }
                    None => item,
                };
                let offset = moved_offset(overlay, item.region_id);
                let origin = content.min + (Vec2::new(item.rect[0], item.rect[1]) + offset) * zoom;
                if let Body::Area(view) = &item.body {
                    if !view.locked {
                        toggles.push((item.region_id, area_icon_rect(origin, zoom)));
                    }
                }
                draw_item(ui, &painter, item, origin, zoom, show_prime);
                if overlay.region_borders {
                    let size = Vec2::new(item.rect[2], item.rect[3]) * zoom;
                    painter.rect_stroke(
                        Rect::from_min_size(origin, size),
                        0.0,
                        Stroke::new(1.0, egui::Color32::from_gray(170)),
                        egui::StrokeKind::Outside,
                    );
                }
            }
            // The search hits are tinted, the current one more strongly. The region a step goes to
            // is scrolled into view. A region the draft replaces is not tinted (the draft shows it).
            for item in &vm.items {
                if Some(item.region_id) == overlay.hidden {
                    continue;
                }
                let hit = overlay
                    .search_hits
                    .iter()
                    .find(|(id, _)| *id == item.region_id);
                let scroll = overlay.scroll_to == Some(item.region_id);
                if hit.is_none() && !scroll {
                    continue;
                }
                let offset = moved_offset(overlay, item.region_id);
                let origin = content.min + (Vec2::new(item.rect[0], item.rect[1]) + offset) * zoom;
                let rect =
                    Rect::from_min_size(origin, Vec2::new(item.rect[2], item.rect[3]) * zoom);
                if scroll {
                    ui.scroll_to_rect(rect, Some(egui::Align::Center));
                }
                if let Some((_, current)) = hit {
                    let (fill, stroke) = if *current {
                        (
                            egui::Color32::from_rgba_unmultiplied(255, 170, 0, 90),
                            Stroke::new(2.0, egui::Color32::from_rgb(230, 120, 0)),
                        )
                    } else {
                        (
                            egui::Color32::from_rgba_unmultiplied(255, 230, 0, 60),
                            Stroke::new(1.0, egui::Color32::from_rgb(200, 180, 0)),
                        )
                    };
                    painter.rect(rect, 2.0, fill, stroke, egui::StrokeKind::Outside);
                }
            }
            let draft_area = draw_overlay(ui, &painter, vm, content, zoom, overlay);
            // The corner handles of the selected image, drawn on top of it.
            let handles = overlay.image_frame.map(|(_, f)| {
                let origin = content.min + Vec2::new(f[0] as f32, f[1] as f32) * zoom;
                let frame = Rect::from_min_size(origin, Vec2::new(f[2] as f32, f[3] as f32) * zoom);
                draw_handles(&painter, frame);
                handle_rects(frame)
            });
            let to_page = |p: Pos2| {
                let local = (p - content.min) / zoom;
                [f64::from(local.x), f64::from(local.y)]
            };
            // A click counts inside the printable area: the frame's extent, less the margins. A click
            // in a margin places nothing.
            let inside = |p: [f64; 2]| {
                let (limit, paged) = match overlay.pages {
                    Some(g) => {
                        let [top, right, bottom, left] = g.margins.map(f64::from);
                        let height = f64::from(g.paper[1]);
                        let width = f64::from(g.paper[0]) - left - right;
                        let total = height * g.count as f64 - top - bottom;
                        ([width, total], Some((height, height - top - bottom)))
                    }
                    None => ([f64::from(vm.page[0]), f64::from(vm.page[1])], None),
                };
                let on_page =
                    paged.is_none_or(|(height, printable)| p[1].rem_euclid(height) <= printable);
                p[0] >= 0.0 && p[1] >= 0.0 && p[0] <= limit[0] && p[1] <= limit[1] && on_page
            };
            // The character of the draft under a screen position, when the position is in the draft.
            let draft_index = |p: Pos2| -> Option<usize> {
                let (origin, galley) = draft_area.as_ref()?;
                let area = Rect::from_min_size(*origin, galley.size()).expand(4.0);
                let shown = galley.cursor_from_pos(p - *origin).index;
                let math_text = overlay.draft.filter(|d| d.math).map(|d| &d.text);
                area.contains(p).then(|| match math_text {
                    Some(text) => source_index(&draft_display(text).1, shown),
                    None => shown,
                })
            };
            let drag_key = ui.id().with("draft_drag");
            let press = response.interact_pointer_pos();
            if response.double_clicked() && press.and_then(draft_index).is_some() {
                draft_pointer = press.and_then(draft_index).map(DraftPointer::Word);
            } else if response.clicked() {
                let on_icon = press.and_then(|p| {
                    toggles
                        .iter()
                        .find(|(_, rect)| rect.expand(2.0).contains(p))
                        .map(|(id, _)| *id)
                });
                match press.and_then(draft_index) {
                    _ if on_icon.is_some() => area_toggle = on_icon,
                    Some(i) => draft_pointer = Some(DraftPointer::Press(i)),
                    None => {
                        if let Some(p) = press.map(to_page) {
                            if inside(p) {
                                click = Some(p);
                            }
                        }
                    }
                }
            }
            if response.double_clicked() && press.and_then(draft_index).is_none() {
                if let Some(p) = press.map(to_page).filter(|p| inside(*p)) {
                    // A double click on a region edits it and selects the word under the point.
                    // The word is selected a frame later, once the draft is drawn.
                    region_word = Some(p);
                    ui.ctx().request_repaint();
                }
                if let (Some(g), Some(p)) = (overlay.pages, press.map(to_page)) {
                    furniture_edit = margin_part(&g, p);
                }
            }
            // The word under the point of an earlier double click is selected in the draft.
            if let Some(p) = overlay.pending_word {
                let screen = content.min + Vec2::new(p[0] as f32, p[1] as f32) * zoom;
                if let Some(i) = draft_index(screen) {
                    draft_pointer = Some(DraftPointer::Word(i));
                }
            }
            if response.drag_started() {
                let origin = ui.input(|i| i.pointer.press_origin());
                let corner = origin.and_then(|p| {
                    handles.and_then(|hs| {
                        hs.iter()
                            .find(|(_, rect)| rect.expand(HANDLE_HIT_PT).contains(p))
                            .map(|(c, _)| *c)
                    })
                });
                if corner.is_some() {
                    resize_start = corner;
                } else if let Some(i) = origin.and_then(draft_index) {
                    // A drag that starts in the draft selects text; it does not move regions.
                    ui.data_mut(|d| d.insert_temp(drag_key, true));
                    draft_pointer = Some(DraftPointer::Press(i));
                } else if let Some(p) = origin.map(to_page) {
                    if inside(p) {
                        drag_start = Some(p);
                    }
                }
            }
            let selecting = ui.data(|d| d.get_temp::<bool>(drag_key)).unwrap_or(false);
            if response.dragged() && !response.drag_started() {
                if selecting {
                    let now = ui.input(|i| i.pointer.interact_pos()).and_then(draft_index);
                    draft_pointer = now.map(DraftPointer::Drag);
                } else if let Some(d) = response.total_drag_delta() {
                    drag = Some([f64::from(d.x / zoom), f64::from(d.y / zoom)]);
                }
            }
            if response.drag_stopped() {
                ui.data_mut(|d| d.remove::<bool>(drag_key));
            }
            drag_end = response.drag_stopped() && !selecting;
            // A right-click on an area opens its context menu, which locks or unlocks it.
            let menu_key = ui.id().with("area_menu");
            if response.secondary_clicked() {
                let target = response
                    .interact_pointer_pos()
                    .and_then(|p| area_at(vm, content, zoom, p));
                ui.data_mut(|d| d.insert_temp(menu_key, target));
            }
            let target = ui
                .data(|d| d.get_temp::<Option<(u32, bool)>>(menu_key))
                .flatten();
            response.context_menu(|ui| match target {
                Some((id, locked)) => {
                    let label = tr(if locked {
                        "edit.unlock_area"
                    } else {
                        "edit.lock_area"
                    });
                    if ui.button(label).clicked() {
                        area_lock = Some(id);
                        ui.close();
                    }
                }
                None => {
                    ws_menu = crate::chrome::worksheet_menu(ui, !overlay.hide_grid);
                }
            });
        });
    let centre = output.state.offset.y + output.inner_rect.height() / 2.0;
    let (total, index) = match overlay.pages {
        Some(g) => (
            g.count,
            (centre / (g.paper[1] * zoom)).floor().max(0.0) as usize,
        ),
        None => (
            ((vm.page[1] / PAGE_PT).ceil() as usize).max(1),
            (centre / (PAGE_PT * zoom)).floor().max(0.0) as usize,
        ),
    };
    Output {
        page: (index.min(total - 1) + 1, total),
        click,
        drag_start,
        drag,
        drag_end,
        draft: draft_pointer,
        area_toggle,
        furniture: furniture_edit,
        resize_start,
        area_lock,
        menu: ws_menu,
        region_word,
    }
}

/// Side of a corner handle of a selected image, in points at 100 %.
const HANDLE_PT: f32 = 8.0;
/// A press this far outside a handle still grabs it, in pixels.
const HANDLE_HIT_PT: f32 = 4.0;

/// The four corner handles of a frame, each with its corner.
fn handle_rects(frame: Rect) -> [(Corner, Rect); 4] {
    Corner::ALL.map(|corner| {
        let x = if corner.right() {
            frame.max.x
        } else {
            frame.min.x
        };
        let y = if corner.bottom() {
            frame.max.y
        } else {
            frame.min.y
        };
        (
            corner,
            Rect::from_center_size(Pos2::new(x, y), Vec2::splat(HANDLE_PT)),
        )
    })
}

/// Draws the frame of a selected image with its corner handles.
fn draw_handles(painter: &egui::Painter, frame: Rect) {
    let stroke = Stroke::new(1.0, theme::AREA_EDGE);
    painter.rect_stroke(frame, 0.0, stroke, egui::StrokeKind::Outside);
    for (_, rect) in handle_rects(frame) {
        painter.rect_filled(rect, 0.0, theme::PAPER);
        painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Outside);
    }
}

/// The innermost area under a screen position, as its id and whether it is locked. Areas nest
/// after their parents in `vm.items`, so the last one that contains the point is the innermost.
fn area_at(vm: &ViewModel, content: Rect, zoom: f32, p: Pos2) -> Option<(u32, bool)> {
    let page = (p - content.min) / zoom;
    vm.items
        .iter()
        .rfind(|item| {
            let [x, y, w, h] = item.rect;
            matches!(item.body, Body::Area(_))
                && page.x >= x
                && page.x <= x + w
                && page.y >= y
                && page.y <= y + h
        })
        .and_then(|item| match &item.body {
            Body::Area(view) => Some((item.region_id, view.locked)),
            _ => None,
        })
}

/// The collapse icon of an area: a square at the left of its header bar, `origin` being the
/// area's top-left corner.
fn area_icon_rect(origin: Pos2, zoom: f32) -> Rect {
    let side = (AREA_ICON_PT * zoom).max(6.0);
    Rect::from_min_size(
        origin + Vec2::splat(AREA_ICON_INSET_PT * zoom),
        Vec2::splat(side),
    )
}

/// How far a region is drawn from its stored position while it is dragged, in page points.
fn moved_offset(overlay: &Overlay<'_>, id: u32) -> Vec2 {
    match overlay.moving {
        Some((ids, d)) if ids.contains(&id) => Vec2::new(d[0] as f32, d[1] as f32),
        _ => Vec2::ZERO,
    }
}

/// Snaps an insertion point to the grid corner nearest to it, as Prime's crosshair does.
///
/// `point` is in the content frame: points measured from the margin box corner (the frame of
/// click positions and stored region positions). The grid lines are at multiples of the grid
/// pitch from that corner, where Prime's crosshair sits (screenshot 0003). The grid is in points,
/// so the snap holds at every zoom. The corner itself is a grid corner, and a point left of or
/// above the margin box snaps to it, so no region starts outside the printable area.
pub fn snap_to_grid(point: [f64; 2], grid_pt: f32) -> [f64; 2] {
    let grid = f64::from(grid_pt);
    let snap = |v: f64| (v / grid).round().max(0.0) * grid;
    [snap(point[0]), snap(point[1])]
}

/// Draws the grid lines that fall inside `area`. The lines are at multiples of the grid step from
/// `origin` (the content frame's origin), so they line up with the points that snap (see
/// `snap_to_grid`).
fn draw_grid(painter: &egui::Painter, area: Rect, origin: Pos2, zoom: f32, grid_pt: f32) {
    let step = grid_pt * zoom;
    if step < 2.0 {
        return;
    }
    let stroke = Stroke::new(1.0, theme::GRID);
    let first_x = area.min.x + (origin.x - area.min.x).rem_euclid(step);
    let mut x = first_x;
    while x <= area.max.x {
        painter.line_segment([Pos2::new(x, area.min.y), Pos2::new(x, area.max.y)], stroke);
        x += step;
    }
    let first_y = area.min.y + (origin.y - area.min.y).rem_euclid(step);
    let mut y = first_y;
    while y <= area.max.y {
        painter.line_segment([Pos2::new(area.min.x, y), Pos2::new(area.max.x, y)], stroke);
        y += step;
    }
}

/// A dashed rectangle: the outline of a region being typed and of a selected region.
fn dashed_rect(painter: &egui::Painter, rect: Rect, colour: egui::Color32) {
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    painter.extend(Shape::dashed_line(
        &corners,
        Stroke::new(1.0, colour),
        4.0,
        3.0,
    ));
}

/// The page layout: each page's outline, its header and footer, and a dashed line where a region
/// crosses a page boundary or where a page-break marker is. `page` is the stacked paper on screen,
/// and `frame` the screen point of the content frame's origin (page breaks are in frame points).
fn draw_pages(
    ui: &Ui,
    painter: &egui::Painter,
    (page, frame): (Rect, Pos2),
    zoom: f32,
    g: &Geometry,
    vm: &ViewModel,
    hide_furniture: bool,
) {
    let edge = Stroke::new(1.0, theme::PAGE_EDGE);
    let today = furniture::today();
    for index in 0..g.count {
        let top = page.min.y + g.page_top(index) * zoom;
        let rect = Rect::from_min_size(
            Pos2::new(page.min.x, top),
            Vec2::new(g.paper[0], g.paper[1]) * zoom,
        );
        painter.rect_stroke(rect, 0.0, edge, egui::StrokeKind::Outside);
        let fields = Fields {
            page: index + 1,
            pages: g.count,
            filename: &vm.title,
            path: if vm.path.is_empty() {
                &vm.title
            } else {
                &vm.path
            },
            date: &today,
        };
        for f in vm.furniture.iter().filter(|_| !hide_furniture) {
            let shown = furniture::expand(&f.text, &fields);
            let origin = Pos2::new(page.min.x + f.rect[0] * zoom, top + f.rect[1] * zoom);
            draw_rich_text(ui, painter, shown.rich(), origin, f.rect[2], zoom);
        }
    }
    for &y in &vm.page_breaks {
        let py = frame.y + y * zoom;
        let line = [Pos2::new(page.min.x, py), Pos2::new(page.max.x, py)];
        painter.extend(Shape::dashed_line(
            &line,
            Stroke::new(1.5, theme::UNSUPPORTED),
            6.0 * zoom,
            4.0 * zoom,
        ));
    }
    let spans: Vec<(f32, f32)> = vm
        .items
        .iter()
        .map(|item| (item.rect[1], item.rect[1] + item.rect[3]))
        .collect();
    for y in g.breaks(&spans) {
        let py = frame.y + y * zoom;
        // The line stops at a region that straddles the boundary, so the region is not cut by it
        // (QA finding F15).
        let covered: Vec<(f32, f32)> = vm
            .items
            .iter()
            .filter(|item| item.rect[1] < y && y < item.rect[1] + item.rect[3])
            .map(|item| {
                (
                    frame.x + item.rect[0] * zoom - 2.0,
                    frame.x + (item.rect[0] + item.rect[2]) * zoom + 2.0,
                )
            })
            .collect();
        for (from, to) in uncovered(page.min.x, page.max.x, covered) {
            let line = [Pos2::new(from, py), Pos2::new(to, py)];
            painter.extend(Shape::dashed_line(
                &line,
                Stroke::new(1.0, theme::CROSSHAIR),
                6.0 * zoom,
                4.0 * zoom,
            ));
        }
    }
}

/// The parts of the span `from..to` that the `covered` spans leave free, left to right.
fn uncovered(from: f32, to: f32, mut covered: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
    covered.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut x = from;
    for (a, b) in covered {
        if a > x {
            out.push((x, a.min(to)));
        }
        x = x.max(b);
    }
    if x < to {
        out.push((x, to));
    }
    out.retain(|(a, b)| b > a);
    out
}

/// The width a plot is drawn at, in points, when a plot starts inside its stored width on the
/// right (they overlap in some sheets). The width stops [`PLOT_GAP_PT`] before that plot, so the
/// right plot's frame does not cover the left plot's right axis. `None` keeps the stored width.
fn plot_room(vm: &ViewModel, item: &Item) -> Option<f32> {
    if !matches!(item.body, Body::Plot(_)) {
        return None;
    }
    let others = vm
        .items
        .iter()
        .filter(|o| o.region_id != item.region_id && matches!(o.body, Body::Plot(_)))
        .map(|o| o.rect);
    narrowed_width(item.rect, others)
}

/// The width `rect` is drawn at, given the rectangles of the other plots: it stops
/// [`PLOT_GAP_PT`] before the nearest plot that starts inside it on the right and shares some of
/// its rows. `None` when no such plot exists (the stored width stays).
fn narrowed_width(rect: [f32; 4], others: impl Iterator<Item = [f32; 4]>) -> Option<f32> {
    let [x, y, w, h] = rect;
    let right = x + w;
    let cut = others
        .filter(|[ox, oy, _, oh]| *oy < y + h && y < oy + oh && *ox > x && *ox < right)
        .map(|[ox, _, _, _]| ox)
        .reduce(f32::min)?;
    let width = (cut - PLOT_GAP_PT - x).max(0.0);
    (width < w).then_some(width)
}

/// A small cross at `c`, the mark of an insertion point.
fn cross_at(painter: &egui::Painter, c: Pos2, colour: egui::Color32) {
    let arm = 6.0;
    let stroke = Stroke::new(1.0, colour);
    painter.line_segment([c - Vec2::new(arm, 0.0), c + Vec2::new(arm, 0.0)], stroke);
    painter.line_segment([c - Vec2::new(0.0, arm), c + Vec2::new(0.0, arm)], stroke);
}

/// Selection outline, insertion point and draft. Positions are page points.
fn draw_overlay(
    ui: &mut Ui,
    painter: &egui::Painter,
    vm: &ViewModel,
    page: Rect,
    zoom: f32,
    overlay: &Overlay<'_>,
) -> Option<(Pos2, Arc<egui::Galley>)> {
    for &(id, strength) in overlay.flash {
        if let Some(item) = vm.items.iter().find(|i| i.region_id == id) {
            let [x, y, w, h] = item.rect;
            let rect =
                Rect::from_min_size(page.min + Vec2::new(x, y) * zoom, Vec2::new(w, h) * zoom)
                    .expand(3.0);
            let alpha = (strength.clamp(0.0, 1.0) * 70.0) as u8;
            painter.rect_filled(
                rect,
                2.0,
                egui::Color32::from_rgba_unmultiplied(40, 150, 90, alpha),
            );
        }
    }
    for &id in overlay.selected {
        if let Some(item) = vm.items.iter().find(|i| i.region_id == id) {
            let [x, y, w, h] = item.rect;
            let offset = moved_offset(overlay, id);
            let rect = Rect::from_min_size(
                page.min + (Vec2::new(x, y) + offset) * zoom,
                Vec2::new(w, h) * zoom,
            )
            .expand(3.0);
            painter.rect_filled(rect, 0.0, theme::SELECTION_FILL);
            dashed_rect(painter, rect, theme::CROSSHAIR);
        }
    }
    if let Some(at) = overlay.crosshair {
        // The insertion point is in the content frame (see `snap_to_grid`), like the regions. `page`
        // is that frame already (the caller passes the inset origin), so the point is not inset again:
        // it lands on the grid line where a region placed there starts.
        let p = page.min + Vec2::new(at[0] as f32, at[1] as f32) * zoom;
        cross_at(painter, p, theme::CROSSHAIR);
    }
    if let Some(r) = overlay.marquee {
        let a = page.min + Vec2::new(r[0] as f32, r[1] as f32) * zoom;
        let b = page.min + Vec2::new(r[2] as f32, r[3] as f32) * zoom;
        painter.rect_stroke(
            Rect::from_two_pos(a, b),
            0.0,
            Stroke::new(1.0, theme::CROSSHAIR),
            egui::StrokeKind::Outside,
        );
    }
    overlay
        .draft
        .map(|draft| draw_draft(ui, painter, draft, page, zoom))
}

/// The draft: its text in the region's place, with the caret. A math draft is shown as plain
/// text (see `edit/math.rs`); a text draft wraps like a text region.
fn draw_draft(
    ui: &mut Ui,
    painter: &egui::Painter,
    draft: &DraftView,
    page: Rect,
    zoom: f32,
) -> (Pos2, Arc<egui::Galley>) {
    let size = if draft.math { MATH_SIZE } else { TEXT_SIZE };
    let origin = page.min + Vec2::new(draft.at[0] as f32, draft.at[1] as f32) * zoom;
    if let Some(tree) = &draft.tree {
        return draw_typeset_draft(ui, painter, tree, origin, zoom);
    }
    let wrap = if draft.math {
        f32::INFINITY
    } else {
        360.0 * zoom
    };
    // A text draft is set in the sans of text regions, a math draft in the sheet's math face.
    let family = if draft.math {
        fonts::family(Face::Roman)
    } else {
        fonts::text_family(Face::Roman)
    };
    // A raw operator is shown as Prime shows it while typing (display only, the text is stored
    // as typed): `:=` as ≔, `*` as ·, `<=` ≤, `>=` ≥, `!=` ≠, `->` →.
    let (shown, map) = if draft.math {
        draft_display(&draft.text)
    } else {
        (
            draft.text.clone(),
            (0..=draft.text.chars().count()).collect(),
        )
    };
    let job = LayoutJob::simple(shown, FontId::new(size * zoom, family), theme::INK, wrap);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let rect = Rect::from_min_size(origin, galley.size()).expand(2.0);
    dashed_rect(painter, rect, theme::CROSSHAIR);
    if let Some((a, b)) = draft.selection {
        let range = (map[a.min(map.len() - 1)], map[b.min(map.len() - 1)]);
        for part in selection_rects(&galley, range) {
            painter.rect_filled(part.translate(origin.to_vec2()), 0.0, theme::SELECTION_FILL);
        }
    }
    let caret = galley.pos_from_cursor(CCursor::new(map[draft.caret.min(map.len() - 1)]));
    let x = origin.x + caret.min.x;
    let top = origin.y + caret.min.y;
    let bottom = origin.y + caret.max.y;
    painter.line_segment(
        [Pos2::new(x, top), Pos2::new(x, bottom)],
        Stroke::new(1.5, theme::INK),
    );
    let hit = (origin, galley.clone());
    painter.galley(origin, galley, theme::INK);
    hit
}

/// The text of a plain math draft as it is shown, and for each character of `text` (and the end)
/// the index of its first character in the shown text.
pub(crate) fn draft_display(text: &str) -> (String, Vec<usize>) {
    const PAIRS: [(&str, &str); 6] = [
        (":=", "\u{2254}"),
        ("<=", "\u{2264}"),
        (">=", "\u{2265}"),
        ("!=", "\u{2260}"),
        ("->", "\u{2192}"),
        ("==", "="),
    ];
    let chars: Vec<char> = text.chars().collect();
    let mut shown = String::new();
    let mut map = Vec::with_capacity(chars.len() + 1);
    let mut n = 0;
    let mut i = 0;
    while i < chars.len() {
        let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
        if let Some((from, to)) = PAIRS.iter().find(|(from, _)| *from == rest) {
            map.extend([n, n + 1]);
            shown.push_str(to);
            i += from.chars().count();
        } else {
            map.push(n);
            shown.push(match chars[i] {
                '*' => '\u{b7}',
                c => c,
            });
            i += 1;
        }
        n += 1;
    }
    map.push(n);
    (shown, map)
}

/// The character of the source text that the shown character `shown` stands for.
pub(crate) fn source_index(map: &[usize], shown: usize) -> usize {
    map.iter()
        .rposition(|&m| m <= shown)
        .unwrap_or(0)
        .min(map.len().saturating_sub(1))
}

/// A math draft with structure, typeset like an evaluated region: stacked fractions, roots and
/// operators with outlined placeholders for the empty slots, and the caret where the editor is.
/// The returned galley is empty, so a mouse press inside is not mapped to a character (the keys
/// move the caret in a typeset draft).
fn draw_typeset_draft(
    ui: &mut Ui,
    painter: &egui::Painter,
    tree: &crate::draftlayout::DraftTree,
    origin: Pos2,
    zoom: f32,
) -> (Pos2, Arc<egui::Galley>) {
    let math = tree.layout(MATH_SIZE);
    let rect = Rect::from_min_size(origin, Vec2::new(math.width, math.height()) * zoom).expand(2.0);
    dashed_rect(painter, rect, theme::CROSSHAIR);
    for prim in &math.prims {
        draw_prim(painter, prim, origin, math.ascent, zoom);
    }
    if let Some([cx, cy]) = math.caret {
        let x = origin.x + cx * zoom;
        let base = origin.y + (math.ascent + cy) * zoom;
        painter.line_segment(
            [
                Pos2::new(x, base - 0.8 * MATH_SIZE * zoom),
                Pos2::new(x, base + 0.25 * MATH_SIZE * zoom),
            ],
            Stroke::new(1.5, theme::INK),
        );
    }
    let galley = ui.fonts_mut(|f| {
        f.layout_job(LayoutJob::simple(
            String::new(),
            FontId::new(MATH_SIZE * zoom, fonts::family(Face::Roman)),
            theme::INK,
            f32::INFINITY,
        ))
    });
    (origin, galley)
}

/// Rectangles, in galley coordinates, that cover the characters `(start, end)` of a galley: one
/// per line the range touches. The galley has one glyph per character, except that a line ending
/// in a newline has the newline character left out of its glyphs.
fn selection_rects(galley: &egui::Galley, (start, end): (usize, usize)) -> Vec<Rect> {
    let mut rects = Vec::new();
    let mut index = 0;
    for row in &galley.rows {
        let len = row.glyphs.len();
        let (from, to) = (start.max(index), end.min(index + len));
        if from < to {
            let (first, last) = (&row.glyphs[from - index], &row.glyphs[to - index - 1]);
            let left = row.pos.x + first.pos.x;
            let right = row.pos.x + last.pos.x + last.advance_width;
            rects.push(Rect::from_min_max(
                Pos2::new(left, row.pos.y),
                Pos2::new(right, row.pos.y + row.size.y),
            ));
        }
        index += len + usize::from(row.ends_with_newline);
    }
    rects
}

/// Draws a text region: one galley per line, so each paragraph keeps its own alignment, wrapped
/// at the region's width. Runs are drawn with their size, colour, weight and slant (the real
/// bold and italic faces of the bundled font) and underline.
fn draw_rich_text(
    ui: &Ui,
    painter: &egui::Painter,
    rich: &RichText,
    origin: Pos2,
    width: f32,
    zoom: f32,
) {
    let mut y = 0.0;
    for line in &rich.lines {
        let mut job = LayoutJob::default();
        job.wrap.max_width = (width * zoom).max(10.0);
        job.halign = match line.align {
            TextAlign::Center => egui::Align::Center,
            TextAlign::Right => egui::Align::RIGHT,
            TextAlign::Left | TextAlign::Justify => egui::Align::LEFT,
        };
        if line.runs.iter().all(|run| run.text.is_empty()) {
            // An empty line still needs the height of the base font.
            job.append(" ", 0.0, run_format(&rich.base, zoom));
        }
        for run in &line.runs {
            job.append(&run.text, 0.0, run_format(&run.style, zoom));
        }
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let height = galley.size().y;
        painter.galley(origin + Vec2::new(0.0, y), galley, theme::INK);
        y += height;
    }
}

/// Colour of a hyperlink run that sets no colour of its own.
const LINK_BLUE: egui::Color32 = egui::Color32::from_rgb(0, 84, 204);

/// The text format of one run. Black text is drawn in the theme's ink colour.
fn run_format(style: &RunStyle, zoom: f32) -> TextFormat {
    let [r, g, b, a] = style.colour;
    let mut colour = if style.colour == RunStyle::default().colour {
        theme::INK
    } else {
        egui::Color32::from_rgba_unmultiplied(r, g, b, a)
    };
    // A hyperlink is drawn in link blue and underlined, unless the run sets its own colour.
    let link = style.link.is_some();
    if link && style.colour == RunStyle::default().colour {
        colour = LINK_BLUE;
    }
    TextFormat {
        font_id: FontId::new(
            style.size * zoom,
            fonts::text_family(Face::of(style.bold, style.italic)),
        ),
        color: colour,
        underline: if style.underline || link {
            Stroke::new(zoom.max(0.5), colour)
        } else {
            Stroke::NONE
        },
        ..Default::default()
    }
}

fn draw_item(
    ui: &mut Ui,
    painter: &egui::Painter,
    item: &Item,
    origin: Pos2,
    zoom: f32,
    show_prime: bool,
) {
    let [_, _, w, h] = item.rect;
    match &item.body {
        Body::Text(text) => draw_rich_text(ui, painter, text.rich(), origin, w, zoom),
        Body::Math { math, result } => {
            for prim in &math.prims {
                draw_prim(painter, prim, origin, math.ascent, zoom);
            }
            if let Some(line) = result {
                let at = ResultAt {
                    origin,
                    math_width: math.width,
                    math_height: math.height(),
                    ascent: math.ascent,
                    zoom,
                    region_id: item.region_id,
                    room: line.room,
                };
                draw_result(ui, painter, line, &at, show_prime);
            }
        }
        Body::Table(view) => draw_table(ui, painter, view, origin, w, zoom),
        Body::Plot(view) => {
            let rect = Rect::from_min_size(origin, Vec2::new(w, h) * zoom);
            draw_plot(painter, rect, view, zoom);
            ui.interact(rect, ui.id().with(("plot", item.region_id)), Sense::hover());
        }
        Body::Plot3d(view) => {
            let rect = Rect::from_min_size(origin, Vec2::new(w, h) * zoom);
            let id = ui.id().with(("plot3d", item.region_id));
            let response = ui.interact(rect, id, Sense::click_and_drag());
            // The view angles and the wireframe switch live in egui's memory, per region: a
            // drag turns the plot, a double click toggles wireframe and filled surfaces.
            let mut angles: Angles = ui.data(|d| d.get_temp(id)).unwrap_or_default();
            let wire_id = id.with("wire");
            let mut wire = ui.data(|d| d.get_temp::<bool>(wire_id)).unwrap_or(false);
            if response.dragged() {
                let delta = response.drag_delta();
                angles = angles.dragged(f64::from(delta.x), f64::from(delta.y));
                ui.data_mut(|d| d.insert_temp(id, angles));
            }
            if response.double_clicked() {
                wire = !wire;
                ui.data_mut(|d| d.insert_temp(wire_id, wire));
            }
            draw_plot3d(painter, rect, view, angles, wire, zoom);
        }
        Body::Area(view) => {
            let rect = Rect::from_min_size(origin, Vec2::new(w, h) * zoom);
            draw_area(painter, rect, view, zoom);
            ui.interact(rect, ui.id().with(("area", item.region_id)), Sense::hover());
        }
        Body::Image(view) => {
            let rect = Rect::from_min_size(origin, Vec2::new(w, h) * zoom);
            match image_texture(ui.ctx(), view) {
                Some(texture) => {
                    painter.image(
                        texture.id(),
                        rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                None => {
                    painter.rect(
                        rect,
                        2.0,
                        theme::PLACEHOLDER_FILL,
                        Stroke::new(1.0, theme::PLACEHOLDER_EDGE),
                        egui::StrokeKind::Inside,
                    );
                }
            }
            ui.interact(
                rect,
                ui.id().with(("image", item.region_id)),
                Sense::hover(),
            );
        }
        Body::Placeholder { label, detail } => {
            let rect = Rect::from_min_size(origin, Vec2::new(w, h) * zoom);
            painter.rect(
                rect,
                2.0,
                theme::PLACEHOLDER_FILL,
                Stroke::new(1.0, theme::PLACEHOLDER_EDGE),
                egui::StrokeKind::Inside,
            );
            let scale = zoom.max(0.5);
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(13.0 * scale),
                theme::UNSUPPORTED,
            );
            if !detail.is_empty() {
                painter.text(
                    rect.center() + Vec2::new(0.0, 14.0 * scale),
                    Align2::CENTER_CENTER,
                    detail,
                    FontId::proportional(10.0 * scale),
                    theme::UNSUPPORTED,
                );
            }
            ui.interact(rect, ui.id().with(("box", item.region_id)), Sense::hover());
        }
    }
}

/// Side of the collapse icon of an area, in points, and its inset from the area's corner.
const AREA_ICON_PT: f32 = 10.0;
const AREA_ICON_INSET_PT: f32 = 4.0;

/// Draws an area: its frame (unless the border is hidden), its header bar, and the collapse icon
/// (a minus when expanded, a plus when collapsed). A locked area shows a solid square instead.
fn draw_area(painter: &egui::Painter, rect: Rect, view: &AreaView, zoom: f32) {
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), AREA_HEADER_PT * zoom));
    if !view.hide_border {
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, theme::AREA_EDGE),
            egui::StrokeKind::Inside,
        );
    }
    painter.rect_filled(header, 0.0, theme::AREA_HEADER);
    let icon = area_icon_rect(rect.min, zoom);
    if view.locked {
        painter.rect_filled(icon, 1.0, theme::AREA_EDGE);
        return;
    }
    painter.rect_stroke(
        icon,
        1.0,
        Stroke::new(1.0, theme::AREA_EDGE),
        egui::StrokeKind::Inside,
    );
    let mid = icon.center();
    let half = icon.width() * 0.3;
    let stroke = Stroke::new(1.5, theme::AREA_EDGE);
    painter.line_segment(
        [mid - Vec2::new(half, 0.0), mid + Vec2::new(half, 0.0)],
        stroke,
    );
    if view.collapsed {
        painter.line_segment(
            [mid - Vec2::new(0.0, half), mid + Vec2::new(0.0, half)],
            stroke,
        );
    }
}

/// The texture of an image, decoded on first use and kept in egui's memory under its key. A
/// file that does not decode is remembered as `None`, so it is not decoded again each frame.
fn image_texture(ctx: &egui::Context, view: &ImageView) -> Option<egui::TextureHandle> {
    let id = egui::Id::new(("mathcraft-image", view.key));
    if let Some(known) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(id)) {
        return known;
    }
    // A texture larger than the driver's limit fails to upload, so the decoded image is scaled to
    // fit it (see `images::decode`).
    let max_side = u32::try_from(ctx.input(|i| i.max_texture_side)).unwrap_or(u32::MAX);
    let texture = crate::images::decode(&view.bytes, max_side, &view.budget).map(|pixels| {
        ctx.load_texture(
            format!("mathcraft-image-{:x}", view.key),
            pixels,
            egui::TextureOptions::LINEAR,
        )
    });
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    texture
}

/// Points beyond this many points from the origin are not drawn. They are off any page anyway,
/// and egui tessellation of huge coordinates is not useful.
const MAX_SCREEN_COORD: f64 = 1.0e6;

/// Draws a 3D plot inside `rect`: the box edges, then the surfaces farthest first (painter's
/// algorithm, see `plot3d::depth_order`), then the curves, then the tick labels on the front
/// edges. `wire` draws quad outlines only. Nothing is drawn when the rectangle is too small.
fn draw_plot3d(
    painter: &egui::Painter,
    rect: Rect,
    view: &Plot3dView,
    angles: Angles,
    wire: bool,
    zoom: f32,
) {
    painter.rect(
        rect,
        0.0,
        egui::Color32::WHITE,
        Stroke::new(1.0, theme::PLACEHOLDER_EDGE),
        egui::StrokeKind::Inside,
    );
    // Everything below stays inside the plot's frame, whatever the view angles.
    let clipped = painter.with_clip_rect(rect);
    let painter = &clipped;
    let size = rect.width().min(rect.height());
    if size < 40.0 * zoom {
        return;
    }
    let center = rect.center();
    // Half the cube's side, in points: the rotated cube spans about 1.7 of these.
    let half = size * 0.34;
    let m = plot3d::rotation(angles);
    let screen = |q: [f64; 3]| -> Pos2 {
        let [sx, sy] = plot3d::project(q);
        egui::pos2(center.x + half * sx as f32, center.y - half * sy as f32)
    };
    let ink = egui::Color32::from_rgb(120, 126, 136);
    let rotated_corner = |bits: u8| plot3d::apply(&m, plot3d::box_corner(bits));
    // The 12 edges of the box: corners that differ in one bit.
    for a in 0u8..8 {
        for bit in [1u8, 2, 4] {
            let b = a ^ bit;
            if b > a {
                let (p, q) = (rotated_corner(a), rotated_corner(b));
                painter.line_segment(
                    [screen(p), screen(q)],
                    Stroke::new(0.7, egui::Color32::from_rgb(200, 204, 212)),
                );
            }
        }
    }
    // Surfaces: every quad, sorted by its mean depth.
    let mut quads: Vec<(f64, Vec<Pos2>, egui::Color32)> = Vec::new();
    for trace in view.traces.iter().filter(|t| t.surface) {
        for quad in &trace.quads {
            let corners = quad.corners.map(|c| plot3d::apply(&m, c));
            let depth = corners.iter().map(|c| plot3d::depth(*c)).sum::<f64>() / 4.0;
            let shade = plot3d::shade(plot3d::apply(&m, quad.normal));
            let [r, g, b] = quad.color;
            let color = egui::Color32::from_rgb(
                (f64::from(r) * shade).round() as u8,
                (f64::from(g) * shade).round() as u8,
                (f64::from(b) * shade).round() as u8,
            );
            let points: Vec<Pos2> = corners.iter().map(|c| screen(*c)).collect();
            let flat: [[f64; 2]; 4] =
                std::array::from_fn(|i| [f64::from(points[i].x), f64::from(points[i].y)]);
            if plot3d::quad_is_drawable(&flat, 0.5) {
                quads.push((depth, points, color));
            }
        }
    }
    let depths: Vec<f64> = quads.iter().map(|q| q.0).collect();
    for index in plot3d::depth_order(&depths) {
        let (_, points, color) = &quads[index];
        if wire {
            let mut outline = points.clone();
            outline.push(points[0]);
            painter.add(Shape::line(outline, Stroke::new(0.6, *color)));
        } else {
            painter.add(Shape::convex_polygon(
                points.clone(),
                *color,
                Stroke::new(0.5, *color),
            ));
        }
    }
    // Curves, on top of the surfaces.
    for trace in view.traces.iter().filter(|t| !t.surface) {
        let [r, g, b, a] = trace.color;
        let stroke = Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(r, g, b, a));
        for line in &trace.polylines {
            let points: Vec<Pos2> = line.iter().map(|&p| screen(plot3d::apply(&m, p))).collect();
            painter.add(Shape::line(points, stroke));
        }
    }
    // Tick labels on the front edges: x along y = -1, z = -1; y along x = 1, z = -1; z along
    // x = -1, y = -1.
    let font = FontId::proportional((TICK_PT * zoom).max(8.0));
    let label = |axis: &plot3d::AxisView, unit: fn(f64) -> [f64; 3]| {
        for &tick in &axis.ticks {
            let u = plot3d::to_unit(tick, (axis.min, axis.max));
            if !u.is_finite() {
                continue;
            }
            let at = screen(plot3d::apply(&m, unit(u)));
            painter.text(
                at + Vec2::new(0.0, 6.0 * zoom),
                Align2::CENTER_TOP,
                plot::format_tick(tick, axis.decimals),
                font.clone(),
                ink,
            );
        }
    };
    label(&view.x, |u| [u, -1.0, -1.0]);
    label(&view.y, |u| [1.0, u, -1.0]);
    label(&view.z, |u| [-1.0, -1.0, u]);
    if wire {
        painter.text(
            rect.left_top() + Vec2::new(4.0, 2.0) * zoom,
            Align2::LEFT_TOP,
            "wireframe",
            font,
            ink,
        );
    }
}

/// Draws a 2D plot inside `rect` (the item's rectangle, already scaled by `zoom`). The axes, ticks
/// and grid take a margin on the left and the bottom; the traces are clipped to the plot area.
/// A rectangle too small for the plot draws only its frame.
/// A table region: the header names, the unit row in blue (as Prime shows it), the rule and the
/// values. The geometry is [`crate::table::table_geometry`]; this only paints it.
fn draw_table(
    ui: &Ui,
    painter: &egui::Painter,
    view: &TableView,
    origin: Pos2,
    width: f32,
    zoom: f32,
) {
    use crate::table::{table_geometry, CellKind};
    let geometry = table_geometry(view, width);
    let font = FontId::proportional(TEXT_SIZE * zoom);
    let ink = ui.visuals().text_color();
    let unit_ink = egui::Color32::from_rgb(0x1a, 0x2f, 0x9e);
    for cell in &geometry.cells {
        let colour = match cell.kind {
            CellKind::Unit => unit_ink,
            CellKind::Header | CellKind::Value => ink,
        };
        let at = origin + Vec2::new(cell.x, cell.y) * zoom;
        painter.text(at, Align2::CENTER_CENTER, &cell.text, font.clone(), colour);
    }
    for [x1, y1, x2, y2] in geometry.rules {
        let stroke = Stroke::new(1.0, ink);
        painter.line_segment(
            [
                origin + Vec2::new(x1, y1) * zoom,
                origin + Vec2::new(x2, y2) * zoom,
            ],
            stroke,
        );
    }
}

fn rgb(c: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

fn draw_plot(painter: &egui::Painter, rect: Rect, view: &PlotView, zoom: f32) {
    // Neighbouring plots must not touch: draw the card a little inside the item rectangle.
    let rect = rect.shrink2(Vec2::new(0.0, 2.0 * zoom));
    painter.rect(
        rect,
        0.0,
        rgb(plot::BACKGROUND),
        Stroke::new(1.0, rgb(plot::FRAME)),
        egui::StrokeKind::Inside,
    );
    let font = FontId::proportional((TICK_PT * zoom).max(8.0));
    let title_font = FontId::proportional((TITLE_PT * zoom).max(9.0));
    let text_w = |s: &str| {
        painter
            .layout_no_wrap(s.to_owned(), font.clone(), theme::INK)
            .size()
            .x
    };
    let y_cross = plot::axis_crossing(view.y.at_origin, &view.x);
    let x_cross = plot::axis_crossing(view.x.at_origin, &view.y);
    let y_at_left = y_cross <= view.x.min;
    let x_at_bottom = x_cross <= view.y.min;
    let widest_y = if view.y.labels && y_at_left {
        view.y
            .ticks
            .iter()
            .map(|t| text_w(&plot::axis_label(&view.y, *t)))
            .fold(0.0_f32, f32::max)
    } else {
        0.0
    };
    let last_x = if view.x.labels {
        view.x
            .ticks
            .last()
            .map_or(0.0, |t| text_w(&plot::axis_label(&view.x, *t)))
    } else {
        0.0
    };
    let label_h = painter
        .layout_no_wrap("0".to_owned(), font.clone(), theme::INK)
        .size()
        .y;
    // A note (see `plot.traces_not_drawn`) gets its own row above the frame, so it never sits on
    // the top tick label or the frame; a title goes below it.
    let note_row = if view.note.is_some() {
        (TICK_PT + 4.0) * zoom
    } else {
        0.0
    };
    let m = plot::plot_margins(
        widest_y,
        last_x,
        label_h,
        view.title.as_ref().map(|_| TITLE_PT * zoom * 1.3),
        zoom,
    );
    let top = m.top + note_row;
    // Labels of an axis through the interior sit inside the frame and need no margin; a caption
    // takes one text row on its side.
    let caption_row = label_h + 3.0 * zoom;
    let bottom_room = if x_at_bottom && view.x.labels {
        m.bottom
    } else {
        m.bottom - label_h
    } + if view.x.caption.is_some() {
        caption_row
    } else {
        0.0
    };
    let left_room = m.left
        + if view.y.caption.is_some() {
            caption_row
        } else {
            0.0
        };
    let plot_rect = Rect::from_min_max(
        egui::pos2(rect.left() + left_room, rect.top() + top),
        egui::pos2(rect.right() - m.right, rect.bottom() - bottom_room),
    );
    if plot_rect.width() < 8.0 || plot_rect.height() < 8.0 {
        return;
    }
    let sx = plot::scale_of(&view.x);
    let sy = plot::scale_of(&view.y);
    let frame = [
        f64::from(plot_rect.left()),
        f64::from(plot_rect.top()),
        f64::from(plot_rect.width()),
        f64::from(plot_rect.height()),
    ];
    let grid = rgb(plot::GRID_MAJOR);
    let minor_grid = rgb(plot::GRID_MINOR);
    let axis = rgb(plot::AXIS);
    // Where the axes lie on screen: the x axis at height `axis_y`, the y axis at `axis_x`.
    let axis_y = sy.fraction(x_cross).map_or(plot_rect.bottom(), |f| {
        plot_rect.bottom() - f as f32 * plot_rect.height()
    });
    let axis_x = sx.fraction(y_cross).map_or(plot_rect.left(), |f| {
        plot_rect.left() + f as f32 * plot_rect.width()
    });
    for &tick in &view.x.ticks {
        let Some(f) = sx.fraction(tick) else { continue };
        let x = plot_rect.left() + f as f32 * plot_rect.width();
        if view.grid {
            painter.line_segment(
                [
                    egui::pos2(x, plot_rect.top()),
                    egui::pos2(x, plot_rect.bottom()),
                ],
                Stroke::new(0.5, grid),
            );
        }
        let label = plot::axis_label(&view.x, tick);
        if view.x.tick_marks {
            painter.line_segment(
                [
                    egui::pos2(x, axis_y),
                    egui::pos2(x, axis_y + plot::TICK_LEN * zoom),
                ],
                Stroke::new(1.0, axis),
            );
        }
        if view.x.labels {
            painter.text(
                egui::pos2(x, axis_y + (plot::TICK_LEN + plot::LABEL_GAP) * zoom),
                Align2::CENTER_TOP,
                label,
                font.clone(),
                theme::INK,
            );
        }
    }
    for &tick in &view.y.ticks {
        let Some(f) = sy.fraction(tick) else { continue };
        let y = plot_rect.bottom() - f as f32 * plot_rect.height();
        if view.grid {
            painter.line_segment(
                [
                    egui::pos2(plot_rect.left(), y),
                    egui::pos2(plot_rect.right(), y),
                ],
                Stroke::new(0.5, grid),
            );
        }
        let label = plot::axis_label(&view.y, tick);
        if view.y.tick_marks {
            painter.line_segment(
                [
                    egui::pos2(axis_x - plot::TICK_LEN * zoom, y),
                    egui::pos2(axis_x, y),
                ],
                Stroke::new(1.0, axis),
            );
        }
        if view.y.labels {
            // The zero label of two crossing axes sits above the x axis, not on it.
            let on_x_axis = (y - axis_y).abs() < 0.5;
            painter.text(
                egui::pos2(axis_x - (plot::TICK_LEN + plot::LABEL_GAP) * zoom, y),
                if on_x_axis && !x_at_bottom {
                    Align2::RIGHT_BOTTOM
                } else {
                    Align2::RIGHT_CENTER
                },
                label,
                font.clone(),
                theme::INK,
            );
        }
    }
    if view.x.log && view.grid {
        for v in plot::log_minor_ticks(view.x.min, view.x.max) {
            if let Some(f) = sx.fraction(v) {
                let x = plot_rect.left() + f as f32 * plot_rect.width();
                painter.line_segment(
                    [
                        egui::pos2(x, plot_rect.top()),
                        egui::pos2(x, plot_rect.bottom()),
                    ],
                    Stroke::new(0.5, minor_grid),
                );
            }
        }
    }
    if view.y.log && view.grid {
        for v in plot::log_minor_ticks(view.y.min, view.y.max) {
            if let Some(f) = sy.fraction(v) {
                let y = plot_rect.bottom() - f as f32 * plot_rect.height();
                painter.line_segment(
                    [
                        egui::pos2(plot_rect.left(), y),
                        egui::pos2(plot_rect.right(), y),
                    ],
                    Stroke::new(0.5, minor_grid),
                );
            }
        }
    }
    if view.polar {
        // Circular grid: a ring at each positive x tick, spokes every 30 degrees.
        let centre = plot_rect.center();
        let (hw, hh) = (plot_rect.width() / 2.0, plot_rect.height() / 2.0);
        for &tick in view.x.ticks.iter().filter(|t| **t > 0.0) {
            let f = (tick / view.x.max.max(f64::MIN_POSITIVE)) as f32;
            let ring: Vec<Pos2> = (0..=72)
                .map(|i| {
                    let a = i as f32 * std::f32::consts::TAU / 72.0;
                    egui::pos2(centre.x + hw * f * a.cos(), centre.y + hh * f * a.sin())
                })
                .collect();
            painter.add(Shape::line(ring, Stroke::new(0.5, grid)));
        }
        for i in 0..12 {
            let a = i as f32 * std::f32::consts::TAU / 12.0;
            let end = egui::pos2(centre.x + hw * a.cos(), centre.y + hh * a.sin());
            painter.line_segment([centre, end], Stroke::new(0.5, grid));
            // The angle of the spoke (counter-clockwise from the right), just inside the ring.
            let deg = i * 30;
            let rad = (deg as f32).to_radians();
            let at = egui::pos2(
                centre.x + hw * 0.88 * rad.cos(),
                centre.y - hh * 0.88 * rad.sin(),
            );
            painter.text(
                at,
                egui::Align2::CENTER_CENTER,
                plot::polar_angle_label(deg),
                font.clone(),
                theme::INK,
            );
        }
    }
    let clipped = painter.with_clip_rect(plot_rect);
    for trace in &view.traces {
        let color = egui::Color32::from_rgba_unmultiplied(
            trace.color[0],
            trace.color[1],
            trace.color[2],
            trace.color[3],
        );
        let stroke = Stroke::new(trace.width * zoom, color);
        for segment in &trace.segments {
            let points: Vec<Pos2> = segment
                .iter()
                .filter_map(|p| plot::screen_point(sx, sy, frame, *p))
                .filter(|[x, y]| x.abs() < MAX_SCREEN_COORD && y.abs() < MAX_SCREEN_COORD)
                .map(|[x, y]| egui::pos2(x as f32, y as f32))
                .collect();
            if trace.points_only || points.len() == 1 {
                for p in &points {
                    clipped.circle_filled(*p, (trace.width * zoom).max(2.0), color);
                }
            } else if points.len() >= 2 && trace.dotted {
                clipped.extend(Shape::dashed_line(&points, stroke, 1.5 * zoom, 3.0 * zoom));
            } else if points.len() >= 2 && trace.dashed {
                clipped.extend(Shape::dashed_line(&points, stroke, 6.0 * zoom, 4.0 * zoom));
            } else if points.len() >= 2 {
                clipped.add(Shape::line(points.clone(), stroke));
            }
            if let Some(symbol) = trace.symbol {
                // One marker per sample, thinned so a dense trace stays readable.
                let step = (points.len() / 40).max(1);
                for p in points.iter().step_by(step) {
                    draw_symbol(&clipped, *p, symbol, 3.5 * zoom, color);
                }
            }
        }
    }
    for &(vertical, value) in &view.markers {
        let (scale, along) = if vertical {
            (sx, &plot_rect)
        } else {
            (sy, &plot_rect)
        };
        let Some(f) = scale.fraction(value) else {
            continue;
        };
        let ink = egui::Color32::BLACK;
        let (a, b, label_at) = if vertical {
            let x = along.left() + f as f32 * along.width();
            (
                egui::pos2(x, along.top()),
                egui::pos2(x, along.bottom()),
                egui::pos2(x, along.top()),
            )
        } else {
            let y = along.bottom() - f as f32 * along.height();
            (
                egui::pos2(along.left(), y),
                egui::pos2(along.right(), y),
                egui::pos2(along.right(), y),
            )
        };
        clipped.line_segment([a, b], Stroke::new(1.5 * zoom, ink));
        let text = plot::format_tick(value, plot::decimals_for(value.abs().max(1.0) / 10.0));
        let label = painter.text(
            label_at,
            if vertical {
                Align2::CENTER_BOTTOM
            } else {
                Align2::RIGHT_BOTTOM
            },
            text,
            font.clone(),
            ink,
        );
        painter.rect_stroke(
            label.expand(2.0),
            3.0,
            Stroke::new(1.0, ink),
            egui::StrokeKind::Outside,
        );
    }
    // The axes with arrowheads (up, right), through the origin or at the frame as stored.
    let axis_stroke = Stroke::new(1.0, axis);
    let y_axis_line = [
        egui::pos2(axis_x, plot_rect.bottom()),
        egui::pos2(axis_x, plot_rect.top()),
    ];
    let x_axis_line = [
        egui::pos2(plot_rect.left(), axis_y),
        egui::pos2(plot_rect.right(), axis_y),
    ];
    painter.line_segment(y_axis_line, axis_stroke);
    painter.line_segment(x_axis_line, axis_stroke);
    let head = 6.0 * zoom;
    for (tip, dir) in [(y_axis_line[1], [0.0, -1.0]), (x_axis_line[1], [1.0, 0.0])] {
        let pts = plot::arrow_head([tip.x, tip.y], dir, head)
            .iter()
            .map(|p| egui::pos2(p[0], p[1]))
            .collect();
        painter.add(Shape::convex_polygon(pts, axis, Stroke::NONE));
    }
    if let Some(caption) = &view.x.caption {
        painter.text(
            egui::pos2(plot_rect.center().x, rect.bottom() - 3.0 * zoom),
            Align2::CENTER_BOTTOM,
            caption,
            font.clone(),
            theme::INK,
        );
    }
    if let Some(caption) = &view.y.caption {
        let galley = painter.layout_no_wrap(caption.clone(), font.clone(), theme::INK);
        let at = egui::pos2(
            rect.left() + 3.0 * zoom,
            plot_rect.center().y + galley.size().x / 2.0,
        );
        painter.add(Shape::Text(
            egui::epaint::TextShape::new(at, galley, theme::INK)
                .with_angle(-std::f32::consts::FRAC_PI_2),
        ));
    }
    if let Some(title) = &view.title {
        painter.text(
            egui::pos2(rect.center().x, rect.top() + 4.0 * zoom + note_row),
            Align2::CENTER_TOP,
            title,
            title_font,
            theme::INK,
        );
    }
    let items = plot::legend_items(view);
    if !items.is_empty() {
        draw_legend(painter, plot_rect, &items, font.clone(), zoom);
    }
    if let Some(note) = &view.note {
        painter.text(
            egui::pos2(plot_rect.left(), rect.top() + 2.0 * zoom),
            Align2::LEFT_TOP,
            note,
            font,
            theme::UNSUPPORTED,
        );
    }
}

fn outline(p: Pos2, symbol: plot::Symbol, r: f32) -> Vec<Pos2> {
    plot::symbol_outline(symbol, r)
        .iter()
        .map(|o| p + egui::vec2(o[0], o[1]))
        .collect()
}

/// One trace symbol of radius `r` at `p`.
fn draw_symbol(
    painter: &egui::Painter,
    p: Pos2,
    symbol: plot::Symbol,
    r: f32,
    color: egui::Color32,
) {
    let stroke = Stroke::new(1.2, color);
    match symbol {
        plot::Symbol::Circle => {
            painter.circle_stroke(p, r, stroke);
        }
        plot::Symbol::Square => {
            painter.rect_stroke(
                Rect::from_center_size(p, egui::Vec2::splat(2.0 * r)),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        plot::Symbol::Diamond => {
            painter.add(Shape::closed_line(outline(p, symbol, r), stroke));
        }
        plot::Symbol::Triangle => {
            painter.add(Shape::closed_line(outline(p, symbol, r), stroke));
        }
        plot::Symbol::Cross => {
            painter.line_segment([p + egui::vec2(-r, -r), p + egui::vec2(r, r)], stroke);
            painter.line_segment([p + egui::vec2(-r, r), p + egui::vec2(r, -r)], stroke);
        }
        plot::Symbol::Star => {
            painter.line_segment([p + egui::vec2(-r, 0.0), p + egui::vec2(r, 0.0)], stroke);
            painter.line_segment([p + egui::vec2(0.0, -r), p + egui::vec2(0.0, r)], stroke);
            painter.line_segment([p + egui::vec2(-r, -r), p + egui::vec2(r, r)], stroke);
            painter.line_segment([p + egui::vec2(-r, r), p + egui::vec2(r, -r)], stroke);
        }
    }
}

/// The legend box in the top right corner of a plot's frame: for each named trace a sample of
/// its line style (colour, width, dash) and marker, then the trace's name.
fn draw_legend(
    painter: &egui::Painter,
    plot_rect: Rect,
    items: &[plot::LegendItem],
    font: FontId,
    zoom: f32,
) {
    let galleys: Vec<_> = items
        .iter()
        .map(|i| painter.layout_no_wrap(i.name.clone(), font.clone(), theme::INK))
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x).collect();
    let layout = plot::legend_layout(
        [
            plot_rect.left(),
            plot_rect.top(),
            plot_rect.right(),
            plot_rect.bottom(),
        ],
        &widths,
        (TICK_PT + 4.0) * zoom,
        4.0 * zoom,
        16.0 * zoom,
        4.0 * zoom,
    );
    let [l, t, r, b] = layout.rect;
    painter.rect(
        Rect::from_min_max(egui::pos2(l, t), egui::pos2(r, b)),
        0.0,
        egui::Color32::WHITE,
        Stroke::new(1.0, theme::PLACEHOLDER_EDGE),
        egui::StrokeKind::Inside,
    );
    for ((item, galley), y) in items.iter().zip(galleys).zip(&layout.row_y) {
        let [cr, cg, cb, ca] = item.color;
        let color = egui::Color32::from_rgba_unmultiplied(cr, cg, cb, ca);
        let [x0, x1] = layout.swatch_x;
        let stroke = Stroke::new((item.width * zoom).max(1.5), color);
        let line = [egui::pos2(x0, *y), egui::pos2(x1, *y)];
        // A points-only trace has no line sample: the marker alone, a dot without a symbol.
        if item.points_only {
        } else if item.dotted {
            painter.extend(Shape::dashed_line(&line, stroke, 1.5 * zoom, 3.0 * zoom));
        } else if item.dashed {
            painter.extend(Shape::dashed_line(&line, stroke, 4.0 * zoom, 3.0 * zoom));
        } else {
            painter.line_segment(line, stroke);
        }
        let mid = egui::pos2((x0 + x1) / 2.0, *y);
        match item.symbol {
            Some(symbol) => draw_symbol(painter, mid, symbol, 3.5 * zoom, color),
            None if item.points_only => {
                painter.circle_filled(mid, (item.width * zoom).max(2.0), color);
            }
            None => {}
        }
        painter.galley(
            egui::pos2(layout.text_x, y - galley.size().y / 2.0),
            galley,
            theme::INK,
        );
    }
}

/// Draws one primitive. `origin` is the item's top-left; the baseline is `ascent` below it.
fn draw_prim(painter: &egui::Painter, prim: &Prim, origin: Pos2, ascent: f32, zoom: f32) {
    let base = origin + Vec2::new(0.0, ascent * zoom);
    match prim {
        Prim::Text {
            x,
            y,
            text,
            size,
            face,
        } => {
            let pos = base + Vec2::new(*x, *y + DESCENT * size) * zoom;
            painter.text(
                pos,
                Align2::LEFT_BOTTOM,
                text,
                FontId::new(size * zoom, fonts::family(*face)),
                theme::INK,
            );
        }
        Prim::Rect { x, y, w, h } => {
            let rect =
                Rect::from_min_size(base + Vec2::new(*x, *y) * zoom, Vec2::new(*w, *h) * zoom);
            painter.rect_filled(rect, 0.0, theme::INK);
        }
        Prim::Line { points, width } => {
            let pts: Vec<Pos2> = points
                .iter()
                .map(|[px, py]| base + Vec2::new(*px, *py) * zoom)
                .collect();
            painter.add(Shape::line(
                pts,
                Stroke::new((width * zoom).max(1.0), theme::INK),
            ));
        }
    }
}

/// A math result (symbolic, or a matrix grid): drawn on the baseline of the region's own math, to
/// its right when `beside`, else on the line below the expression.
fn draw_result_math(
    ui: &mut Ui,
    painter: &egui::Painter,
    line: &ResultLine,
    math: &MathBox,
    at: &ResultAt,
    beside: bool,
) {
    let origin = if beside {
        Pos2::new(
            at.origin.x + (at.math_width + RESULT_GAP) * at.zoom,
            at.origin.y + (at.ascent - math.ascent) * at.zoom,
        )
    } else {
        // The top of the result's box, below the expression's box.
        Pos2::new(at.origin.x, at.origin.y + (at.math_height + 2.0) * at.zoom)
    };
    for prim in &math.prims {
        draw_prim(painter, prim, origin, math.ascent, at.zoom);
    }
    let rect = Rect::from_min_size(origin, Vec2::new(math.width, math.height()) * at.zoom);
    let id = ui.id().with(("result", at.region_id));
    ui.interact(rect, id, Sense::hover())
        .on_hover_text(line.tip.clone());
}

/// Where a math region's result goes.
struct ResultAt {
    origin: Pos2,
    math_width: f32,
    math_height: f32,
    ascent: f32,
    zoom: f32,
    region_id: u32,
    /// Free width right of the expression, in points (see `ResultLine::room`).
    room: f32,
}

/// The result after `=`: computed, or Prime's stored value when the toggle is on.
fn draw_result(
    ui: &mut Ui,
    painter: &egui::Painter,
    line: &ResultLine,
    at: &ResultAt,
    show_prime: bool,
) {
    if let (Some(math), false) = (&line.math, show_prime) {
        // Beside the expression when it fits the free width; else on the line below, from the
        // expression's left edge (so a wide matrix never runs into the next region).
        let beside = math.width + RESULT_GAP <= at.room;
        draw_result_math(ui, painter, line, math, at, beside);
        return;
    }
    let (text, colour) = if show_prime {
        match &line.stored {
            Some(stored) => (stored.clone(), theme::STORED),
            None => (tr("res.no_stored_short").to_string(), theme::STORED),
        }
    } else {
        let colour = match line.tone {
            Tone::Plain | Tone::Match => theme::INK,
            Tone::Problem => theme::PROBLEM,
            Tone::Unsupported => theme::UNSUPPORTED,
        };
        (line.computed.clone(), colour)
    };
    let font = FontId::new(MATH_SIZE * at.zoom, fonts::family(Face::Roman));
    // Beside the expression when the text fits there; else on the line below, from the
    // expression's left edge up to the next region, and cut with an ellipsis when still too long.
    let fits = |job: LayoutJob| ui.fonts_mut(|f| f.layout_job(job)).size().x <= at.room * at.zoom;
    let beside = LayoutJob::simple(text.clone(), font.clone(), colour, f32::INFINITY);
    let is_beside = fits(beside);
    let max_width = if is_beside {
        f32::INFINITY
    } else {
        (at.math_width + RESULT_GAP + at.room) * at.zoom
    };
    let mut job = LayoutJob::simple(text, font, colour, max_width);
    job.wrap.max_rows = 1;
    job.wrap.overflow_character = Some(ELLIPSIS);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let top_left = if is_beside {
        let baseline = at.origin.y + at.ascent * at.zoom;
        Pos2::new(
            at.origin.x + (at.math_width + RESULT_GAP) * at.zoom,
            baseline + DESCENT * MATH_SIZE * at.zoom - galley.size().y,
        )
    } else {
        Pos2::new(at.origin.x, at.origin.y + (at.math_height + 2.0) * at.zoom)
    };
    let rect = Rect::from_min_size(top_left, galley.size());
    if line.tone == Tone::Problem && !show_prime {
        // An error is red with a bar before it, so it reads as a problem and not as a result.
        let bar = Rect::from_min_size(
            top_left - Vec2::new(5.0 * at.zoom, 0.0),
            Vec2::new(2.5 * at.zoom, galley.size().y),
        );
        painter.rect_filled(bar, 0.0, theme::PROBLEM);
    }
    painter.galley(top_left, galley, colour);
    let id = ui.id().with(("result", at.region_id));
    ui.interact(rect, id, Sense::hover())
        .on_hover_text(line.tip.clone());
}

#[cfg(test)]
mod snap_tests {
    use super::*;

    /// The snap arithmetic does not depend on the grid in use, so these tests use a 24 pt grid.
    const TEST_GRID: f32 = 24.0;

    /// The frame origin is the margin box corner, so the grid corners are at k * 24 from it.
    #[test]
    fn a_point_snaps_to_the_nearest_grid_corner() {
        assert_eq!(snap_to_grid([40.0, 40.0], TEST_GRID), [48.0, 48.0]);
        assert_eq!(snap_to_grid([50.0, 25.0], TEST_GRID), [48.0, 24.0]);
        assert_eq!(snap_to_grid([12.0, 0.0], TEST_GRID), [24.0, 0.0]);
    }

    /// A point on a grid corner stays where it is, the margin box corner included.
    #[test]
    fn a_corner_is_a_fixed_point() {
        for k in 0..40 {
            let c = f64::from(k) * 24.0;
            assert_eq!(snap_to_grid([c, c], TEST_GRID), [c, c]);
        }
    }

    /// A point left of or above the margin box snaps to its corner: no region starts outside it.
    #[test]
    fn points_outside_the_margin_box_snap_to_its_corner() {
        assert_eq!(snap_to_grid([-5.0, 3.0], TEST_GRID), [0.0, 0.0]);
        assert_eq!(snap_to_grid([-40.0, -40.0], TEST_GRID), [0.0, 0.0]);
    }

    /// The snapped point is never farther than half a cell from the click on either axis.
    #[test]
    fn the_snap_moves_at_most_half_a_cell() {
        let mut x = 0.0;
        while x < 600.0 {
            let s = snap_to_grid([x, x * 0.7], TEST_GRID);
            assert!((s[0] - x).abs() <= 12.0 + 1e-9, "x={x} -> {}", s[0]);
            assert!(
                (s[1] - x * 0.7).abs() <= 12.0 + 1e-9,
                "y={} -> {}",
                x * 0.7,
                s[1]
            );
            x += 1.7;
        }
    }
}

#[cfg(test)]
mod plot_tests {
    use super::*;

    /// A plot that overlaps the plot on its right stops before it, with the gap.
    #[test]
    fn a_plot_stops_before_the_plot_that_overlaps_it_on_the_right() {
        let left = [0.0, 0.0, 323.0, 200.0];
        let right = [302.0, 0.0, 300.0, 200.0];
        assert_eq!(
            narrowed_width(left, [right].into_iter()),
            Some(302.0 - PLOT_GAP_PT)
        );
    }

    /// Plots side by side without overlap keep their stored width, and so do plots that share no
    /// rows, or that lie to the left.
    #[test]
    fn plots_that_do_not_overlap_keep_their_width() {
        let left = [0.0, 0.0, 290.0, 200.0];
        let right = [302.0, 0.0, 300.0, 200.0];
        assert_eq!(narrowed_width(left, [right].into_iter()), None);
        let below = [0.0, 300.0, 323.0, 200.0];
        assert_eq!(
            narrowed_width(left, [[302.0, 300.0, 300.0, 200.0]].into_iter()),
            None
        );
        assert_eq!(narrowed_width(right, [below].into_iter()), None);
    }

    /// The Fine grid is half the Standard one, and an unknown grid name is Standard.
    #[test]
    fn grid_size_names_give_the_grid_spacing() {
        let mut page = PageSettings::default();
        assert_eq!(grid_points(&page), GRID_PT);
        page.grid_size = "Fine".to_owned();
        assert_eq!(grid_points(&page), GRID_PT / 2.0);
        page.grid_size = "Something".to_owned();
        assert_eq!(grid_points(&page), GRID_PT);
    }

    /// Snapping follows the grid in use: on the Fine grid a point snaps to a 12 pt corner.
    #[test]
    fn snapping_uses_the_grid_in_use() {
        assert_eq!(snap_to_grid([25.0, 25.0], 12.0), [24.0, 24.0]);
        assert_eq!(snap_to_grid([25.0, 25.0], 24.0), [24.0, 24.0]);
    }
}

#[cfg(test)]
mod qa_tests {
    use super::*;

    #[test]
    fn a_draft_shows_raw_operators_as_prime_does_and_maps_the_caret() {
        let (shown, map) = draft_display("a:=b*2<=c>=d!=e->f");
        assert_eq!(
            shown,
            "a\u{2254}b\u{b7}2\u{2264}c\u{2265}d\u{2260}e\u{2192}f"
        );
        assert_eq!(map.len(), "a:=b*2<=c>=d!=e->f".chars().count() + 1);
        // The caret after `a:=` (3 characters) is after the shown ≔ (2 characters).
        assert_eq!(map[3], 2);
        assert_eq!(*map.last().unwrap(), shown.chars().count());
        assert_eq!(source_index(&map, 1), 1);
        assert_eq!(draft_display("x==1").0, "x=1");
    }

    #[test]
    fn the_page_break_line_skips_the_regions_it_crosses() {
        assert_eq!(uncovered(0.0, 100.0, vec![]), vec![(0.0, 100.0)]);
        assert_eq!(
            uncovered(0.0, 100.0, vec![(60.0, 80.0), (10.0, 20.0)]),
            vec![(0.0, 10.0), (20.0, 60.0), (80.0, 100.0)]
        );
        assert!(uncovered(0.0, 100.0, vec![(-5.0, 105.0)]).is_empty());
    }
}
