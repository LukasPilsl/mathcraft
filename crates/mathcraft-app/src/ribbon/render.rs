//! Draws a [`TabSpec`] in one row of fixed height. Groups that do not fit first lose the text of
//! their small buttons (compact density), then move into a chevron dropdown at the right end.

use eframe::egui::{
    self, Align, Color32, Id, Layout, Popup, PopupCloseBehavior, PopupKind, Rect, Sense, Ui,
};

use crate::i18n::tr;
use crate::ribbon::spec::{
    function_names, Button, Col, Group, Item, Kind, Menu, Section, Status, TabSpec,
};
use crate::ribbon::{text_format, Action, RibbonState, TextFormat};
use crate::ribbon_icons::{self, Icon};

/// Height of the ribbon body, in points. Never changes with the window width.
pub const RIBBON_H: f32 = 94.0;
const CAPTION_H: f32 = 16.0;
const BODY_H: f32 = RIBBON_H - CAPTION_H;
const GROUP_PAD: f32 = 5.0;
const GROUP_GAP: f32 = 4.0;
const COL_GAP: f32 = 3.0;
const LARGE_ICON: f32 = 32.0;
const SMALL_H: f32 = 24.0;
const SMALL_ICON: f32 = 16.0;
const ARROW_W: f32 = 12.0;
const CHEVRON_W: f32 = 26.0;

/// How much of the labels is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Density {
    Full,
    /// Small buttons show their icon only; their label is the tooltip.
    Compact,
}

/// Which groups are drawn in the row and which moved into the chevron dropdown.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub density: Density,
    /// Number of groups drawn in the row; the rest are in the chevron dropdown.
    pub visible: usize,
}

/// The plan for `widths` (full density) and `compact` widths in `avail` points.
pub fn plan(full: &[f32], compact: &[f32], avail: f32) -> Plan {
    let total = |w: &[f32]| w.iter().sum::<f32>() + GROUP_GAP * w.len().saturating_sub(1) as f32;
    if total(full) <= avail {
        return Plan {
            density: Density::Full,
            visible: full.len(),
        };
    }
    if total(compact) <= avail {
        return Plan {
            density: Density::Compact,
            visible: compact.len(),
        };
    }
    let mut x = 0.0;
    let mut visible = 0;
    for w in compact {
        if x + w + GROUP_GAP + CHEVRON_W > avail {
            break;
        }
        x += w + GROUP_GAP;
        visible += 1;
    }
    Plan {
        density: Density::Compact,
        visible,
    }
}

fn text_width(ui: &Ui, text: &str, size: f32) -> f32 {
    let job = egui::text::LayoutJob::simple(
        text.to_owned(),
        egui::FontId::proportional(size),
        Color32::BLACK,
        f32::INFINITY,
    );
    ui.fonts_mut(|f| f.layout_job(job)).size().x
}

/// The one or two lines of a large button's label and the width of the wider one.
fn split_lines(ui: &Ui, label: &str, size: f32) -> (Vec<String>, f32) {
    let words: Vec<&str> = label.split_whitespace().collect();
    let whole = text_width(ui, label, size);
    if words.len() < 2 || whole <= 74.0 {
        return (vec![label.to_string()], whole);
    }
    let mut best: Option<(f32, usize)> = None;
    for at in 1..words.len() {
        let a = text_width(ui, &words[..at].join(" "), size);
        let b = text_width(ui, &words[at..].join(" "), size);
        let w = a.max(b);
        if best.is_none_or(|(bw, _)| w < bw) {
            best = Some((w, at));
        }
    }
    let (w, at) = best.unwrap_or((whole, words.len()));
    (vec![words[..at].join(" "), words[at..].join(" ")], w)
}

fn has_arrow(b: &Button) -> bool {
    matches!(b.kind, Kind::Split(_) | Kind::Dropdown(_))
}

fn large_width(ui: &Ui, b: &Button, density: Density) -> f32 {
    let size = if density == Density::Full { 12.0 } else { 11.0 };
    let (_, w) = split_lines(ui, b.label.get(), size);
    let arrow = if has_arrow(b) && !b.arrow_below {
        ARROW_W
    } else {
        0.0
    };
    (w + arrow + 10.0).max(40.0)
}

fn small_width(ui: &Ui, b: &Button, density: Density) -> f32 {
    let arrow = if has_arrow(b) { ARROW_W + 2.0 } else { 0.0 };
    let text = match b.kind {
        Kind::Number(v) => {
            text_width(ui, b.label.get(), 11.5) + 8.0 + text_width(ui, v, 11.5) + 12.0
        }
        Kind::Combo(e) => {
            let widest = e
                .iter()
                .map(|t| text_width(ui, t, 11.5))
                .fold(0.0, f32::max);
            text_width(ui, b.label.get(), 11.5) + 8.0 + widest + 22.0
        }
        Kind::Field(..) => {
            text_width(ui, b.label.get(), 11.5) + 8.0 + text_width(ui, "10⁻³", 11.5) + 22.0
        }
        Kind::Check(_) => 18.0 + text_width(ui, b.label.get(), 11.5) + 6.0,
        _ if density == Density::Full => text_width(ui, b.label.get(), 11.5) + 6.0,
        _ => 0.0,
    };
    let icon = if matches!(
        b.kind,
        Kind::Check(_) | Kind::Number(_) | Kind::Combo(_) | Kind::Field(..)
    ) {
        0.0
    } else {
        SMALL_ICON + 4.0
    };
    4.0 + icon + text + arrow
}

fn col_width(ui: &Ui, col: &Col, density: Density) -> f32 {
    match col {
        Col::Large(b) => large_width(ui, b, density),
        Col::Stack(bs) => bs
            .iter()
            .map(|b| small_width(ui, b, density))
            .fold(20.0, f32::max),
    }
}

/// Width of a group in the row.
pub fn group_width(ui: &Ui, g: &Group, density: Density, expanded: bool) -> f32 {
    if g.collapsed.is_some() && !expanded {
        let (_, w) = split_lines(ui, g.caption.get(), 12.0);
        return (w + 14.0).max(46.0);
    }
    let cols: f32 = g
        .cols
        .iter()
        .map(|c| col_width(ui, c, density))
        .sum::<f32>()
        + COL_GAP * g.cols.len().saturating_sub(1) as f32;
    let caption = text_width(ui, g.caption.get(), 11.0) + 10.0;
    (cols + 2.0 * GROUP_PAD).max(caption)
}

/// What every drawing function needs.
struct Cx<'a> {
    tab: &'static str,
    salt: &'static str,
    state: &'a RibbonState,
    density: Density,
}

/// The ribbon body of `spec`: returns the action of the button pressed.
pub fn tab_body(ui: &mut Ui, spec: &TabSpec, state: &RibbonState) -> Option<Action> {
    let avail = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(avail, RIBBON_H), Sense::hover());
    let widths = |d: Density| -> Vec<f32> {
        spec.groups
            .iter()
            .map(|g| group_width(ui, g, d, false))
            .collect()
    };
    let (full, compact) = (widths(Density::Full), widths(Density::Compact));
    let plan = plan(&full, &compact, avail);
    let widths = if plan.density == Density::Full {
        full
    } else {
        compact
    };
    let mut picked = None;
    let mut x = rect.left();
    for (i, g) in spec.groups.iter().take(plan.visible).enumerate() {
        let r = Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(widths[i], RIBBON_H));
        let cx = Cx {
            tab: spec.key,
            salt: "row",
            state,
            density: plan.density,
        };
        if let Some(a) = draw_group(ui, r, i, g, &cx, false) {
            picked = Some(a);
        }
        x += widths[i] + GROUP_GAP;
    }
    if plan.visible < spec.groups.len() {
        let r = Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(CHEVRON_W, BODY_H));
        let resp = ui.interact(r, ui.id().with((spec.key, "overflow")), Sense::click());
        if resp.hovered() {
            ui.painter()
                .rect_filled(r, 3.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            "»",
            egui::FontId::proportional(18.0),
            ui.visuals().text_color(),
        );
        let resp = resp.on_hover_text(tr("tip.more_groups"));
        let pid = resp.id.with("popup");
        if resp.clicked() {
            Popup::toggle_id(ui.ctx(), pid);
        }
        let rest = &spec.groups[plan.visible..];
        let inner = Popup::from_response(&resp)
            .id(pid)
            .open_memory(None)
            .kind(PopupKind::Menu)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .align(egui::RectAlign::BOTTOM_END)
            .gap(0.0)
            .show(|ui| {
                let mut out = None;
                ui.horizontal(|ui| {
                    for (i, g) in rest.iter().enumerate() {
                        let w = group_width(ui, g, Density::Compact, true);
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(w, RIBBON_H), Sense::hover());
                        let cx = Cx {
                            tab: spec.key,
                            salt: "overflow",
                            state,
                            density: Density::Compact,
                        };
                        if let Some(a) = draw_group(ui, r, plan.visible + i, g, &cx, true) {
                            out = Some(a);
                        }
                    }
                });
                out
            });
        if let Some(a) = inner.and_then(|i| i.inner) {
            picked = Some(a);
            Popup::close_id(ui.ctx(), pid);
        }
    }
    picked
}

fn draw_group(
    ui: &mut Ui,
    rect: Rect,
    index: usize,
    g: &Group,
    cx: &Cx,
    expanded: bool,
) -> Option<Action> {
    let visuals = ui.visuals().clone();
    if let (Some(icon), false) = (g.collapsed, expanded) {
        let b = Button::new(g.caption.0, g.caption.0, g.caption.1, icon).below();
        let body = Rect::from_min_size(rect.min, egui::vec2(rect.width(), BODY_H));
        return draw_collapsed(ui, body, index, g, &b, cx);
    }
    let frame = Rect::from_min_size(rect.min, egui::vec2(rect.width(), RIBBON_H));
    ui.painter().rect_stroke(
        frame.shrink(0.5),
        3.0,
        egui::Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        egui::pos2(frame.center().x, frame.bottom() - CAPTION_H / 2.0 - 1.0),
        egui::Align2::CENTER_CENTER,
        g.caption.get(),
        egui::FontId::proportional(11.0),
        visuals.weak_text_color(),
    );
    let mut picked = None;
    let mut x = rect.left() + GROUP_PAD;
    for (ci, col) in g.cols.iter().enumerate() {
        let w = col_width(ui, col, cx.density);
        match col {
            Col::Large(b) => {
                let r = Rect::from_min_size(
                    egui::pos2(x, rect.top() + 2.0),
                    egui::vec2(w, BODY_H - 2.0),
                );
                if let Some(a) = draw_button(ui, r, (index, ci, 0), b, true, cx) {
                    picked = Some(a);
                }
            }
            Col::Stack(bs) => {
                let top = rect.top() + (BODY_H - SMALL_H * bs.len() as f32) / 2.0;
                for (bi, b) in bs.iter().enumerate() {
                    let r = Rect::from_min_size(
                        egui::pos2(x, top + SMALL_H * bi as f32),
                        egui::vec2(w, SMALL_H),
                    );
                    if let Some(a) = draw_button(ui, r, (index, ci, bi), b, false, cx) {
                        picked = Some(a);
                    }
                }
            }
        }
        x += w + COL_GAP;
    }
    picked
}

/// A group shown as one dropdown button; its content opens as a panel.
fn draw_collapsed(
    ui: &mut Ui,
    rect: Rect,
    index: usize,
    g: &Group,
    b: &Button,
    cx: &Cx,
) -> Option<Action> {
    let id = ui.id().with((cx.tab, cx.salt, "collapsed", index));
    let resp = ui.interact(rect, id, Sense::click());
    paint_large(
        ui,
        rect,
        b,
        Status::Ready,
        resp.hovered(),
        false,
        cx.density,
    );
    let resp = resp.on_hover_text(g.caption.get());
    let pid = id.with("popup");
    if resp.clicked() {
        Popup::toggle_id(ui.ctx(), pid);
    }
    let inner = Popup::from_response(&resp)
        .id(pid)
        .open_memory(None)
        .kind(PopupKind::Menu)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .gap(0.0)
        .show(|ui| {
            let w = group_width(ui, g, Density::Full, true);
            let (r, _) = ui.allocate_exact_size(egui::vec2(w, RIBBON_H), Sense::hover());
            let panel = Cx {
                tab: cx.tab,
                salt: "panel",
                state: cx.state,
                density: Density::Full,
            };
            draw_group(ui, r, index, g, &panel, true)
        });
    let picked = inner.and_then(|i| i.inner);
    if picked.is_some() {
        Popup::close_id(ui.ctx(), pid);
    }
    picked
}

fn paint_arrow(ui: &Ui, centre: egui::Pos2, colour: Color32) {
    let s = 3.0;
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            centre + egui::vec2(-s, -s * 0.6),
            centre + egui::vec2(s, -s * 0.6),
            centre + egui::vec2(0.0, s * 0.7),
        ],
        colour,
        egui::Stroke::NONE,
    ));
}

fn ink(ui: &Ui, status: Status) -> Color32 {
    if status == Status::Ready {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    }
}

fn paint_large(
    ui: &Ui,
    rect: Rect,
    b: &Button,
    status: Status,
    hovered: bool,
    active: bool,
    density: Density,
) {
    if active || (hovered && status == Status::Ready) {
        let fill = if active {
            ui.visuals().selection.bg_fill
        } else {
            ui.visuals().widgets.hovered.weak_bg_fill
        };
        ui.painter().rect_filled(rect, 3.0, fill);
    }
    let colour = ink(ui, status);
    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 4.0 + LARGE_ICON / 2.0),
        egui::Vec2::splat(LARGE_ICON),
    );
    ribbon_icons::paint(ui.painter(), icon_rect, b.icon, colour, 1.6);
    let size = if density == Density::Full { 12.0 } else { 11.0 };
    let (lines, _) = split_lines(ui, b.label.get(), size);
    let arrow_side = has_arrow(b) && !b.arrow_below;
    let shift = if arrow_side {
        -ARROW_W / 2.0 + 2.0
    } else {
        0.0
    };
    let mut y = rect.top() + 4.0 + LARGE_ICON + 4.0;
    let mut last = egui::pos2(rect.center().x, y);
    for line in &lines {
        let galley =
            ui.painter()
                .layout_no_wrap(line.clone(), egui::FontId::proportional(size), colour);
        let w = galley.size().x;
        let at = egui::pos2(rect.center().x + shift - w / 2.0, y);
        ui.painter().galley(at, galley, colour);
        last = egui::pos2(at.x + w + 7.0, y + 7.0);
        y += 13.0;
    }
    if has_arrow(b) {
        if arrow_side {
            paint_arrow(ui, last, colour);
        } else {
            paint_arrow(ui, egui::pos2(rect.center().x, y + 4.0), colour);
        }
    }
}

fn tooltip(b: &Button, status: Status, cx: &Cx) -> String {
    let mut text = b.label.get().to_string();
    if !b.tip.is_empty() {
        text.push('\n');
        text.push_str(b.tip.get());
    }
    match status {
        Status::NotAvailable => {
            text.push('\n');
            text.push_str(tr("tip.not_available"));
        }
        Status::Disabled if !cx.state.enabled => {
            text.push('\n');
            text.push_str(tr("tip.no_worksheet"));
        }
        _ => {}
    }
    text
}

fn draw_button(
    ui: &mut Ui,
    rect: Rect,
    at: (usize, usize, usize),
    b: &Button,
    large: bool,
    cx: &Cx,
) -> Option<Action> {
    let status = b.status(cx.state);
    let ready = status == Status::Ready;
    let id = ui.id().with((cx.tab, cx.salt, at, b.id));
    let sense = if ready {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(rect, id, sense);
    let active = match b.kind {
        Kind::Toggle(f) => f.on(cx.state),
        _ => false,
    };
    let mut arrow_rect = None;
    if large {
        paint_large(ui, rect, b, status, resp.hovered(), active, cx.density);
        if matches!(b.kind, Kind::Split(_)) {
            let strip = Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 22.0), rect.max);
            arrow_rect = Some(strip);
        }
    } else {
        paint_small(ui, rect, b, status, resp.hovered(), active, cx);
        if matches!(b.kind, Kind::Split(_)) {
            arrow_rect = Some(Rect::from_min_max(
                egui::pos2(rect.right() - 16.0, rect.top()),
                rect.max,
            ));
        }
    }
    let arrow_resp = arrow_rect
        .filter(|_| ready)
        .map(|r| ui.interact(r, id.with("arrow"), Sense::click()));
    let resp = resp.on_hover_text(tooltip(b, status, cx));
    let mut picked = None;
    let pid = id.with("popup");
    if ui
        .ctx()
        .data(|d| d.get_temp::<String>(Id::new("stage-popup")))
        .is_some_and(|name| name == b.id)
    {
        Popup::open_id(ui.ctx(), pid);
    }
    let arrow_clicked = arrow_resp.as_ref().is_some_and(|r| r.clicked());
    let opens = match b.kind {
        Kind::Dropdown(_) | Kind::Field(..) => resp.clicked(),
        Kind::Split(_) => arrow_clicked,
        _ => false,
    };
    if opens {
        Popup::toggle_id(ui.ctx(), pid);
    } else if resp.clicked() && !arrow_clicked {
        picked = b.action;
    }
    if let Some(menu) = b.menu_of() {
        let inner = Popup::from_response(&resp)
            .id(pid)
            .open_memory(None)
            .kind(PopupKind::Menu)
            .layout(Layout::top_down_justified(Align::Min))
            .close_behavior(PopupCloseBehavior::CloseOnClick)
            .gap(0.0)
            .show(|ui| menu_contents(ui, menu, cx.state));
        if let Some(a) = inner.and_then(|i| i.inner) {
            picked = Some(a);
        }
    }
    if picked.is_some() && ready {
        picked
    } else {
        None
    }
}

fn paint_small(
    ui: &Ui,
    rect: Rect,
    b: &Button,
    status: Status,
    hovered: bool,
    active: bool,
    cx: &Cx,
) {
    let colour = ink(ui, status);
    if active || (hovered && status == Status::Ready) {
        let fill = if active {
            ui.visuals().selection.bg_fill
        } else {
            ui.visuals().widgets.hovered.weak_bg_fill
        };
        ui.painter().rect_filled(rect, 3.0, fill);
    }
    let font = egui::FontId::proportional(11.5);
    let mut x = rect.left() + 4.0;
    let mid = rect.center().y;
    let label = b.label.get();
    match b.kind {
        Kind::Check(flag) => {
            let boxr = Rect::from_center_size(egui::pos2(x + 6.0, mid), egui::Vec2::splat(12.0));
            ui.painter().rect_stroke(
                boxr,
                2.0,
                egui::Stroke::new(1.0, colour),
                egui::StrokeKind::Inside,
            );
            if flag.on(cx.state) {
                ui.painter().text(
                    boxr.center(),
                    egui::Align2::CENTER_CENTER,
                    "✓",
                    egui::FontId::proportional(11.0),
                    colour,
                );
            }
            ui.painter().text(
                egui::pos2(x + 18.0, mid),
                egui::Align2::LEFT_CENTER,
                label,
                font,
                colour,
            );
        }
        Kind::Number(_) | Kind::Combo(_) | Kind::Field(..) => {
            let value: String = match b.kind {
                Kind::Number(v) | Kind::Combo(&[v, ..]) => v.to_owned(),
                Kind::Field(v, _) => v.text(&cx.state.doc),
                _ => String::new(),
            };
            ui.painter().text(
                egui::pos2(x, mid),
                egui::Align2::LEFT_CENTER,
                label,
                font.clone(),
                colour,
            );
            let lw = text_width(ui, label, 11.5) + 8.0;
            let boxr = Rect::from_min_max(
                egui::pos2(x + lw, rect.top() + 3.0),
                egui::pos2(rect.right() - 2.0, rect.bottom() - 3.0),
            );
            ui.painter().rect_stroke(
                boxr,
                2.0,
                egui::Stroke::new(1.0, colour),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                egui::pos2(boxr.left() + 4.0, mid),
                egui::Align2::LEFT_CENTER,
                value,
                font,
                colour,
            );
            if matches!(b.kind, Kind::Combo(_) | Kind::Field(..)) {
                paint_arrow(ui, egui::pos2(boxr.right() - 8.0, mid), colour);
            }
        }
        _ => {
            let icon_rect = Rect::from_center_size(
                egui::pos2(x + SMALL_ICON / 2.0, mid),
                egui::Vec2::splat(SMALL_ICON),
            );
            ribbon_icons::paint(ui.painter(), icon_rect, b.icon, colour, 1.3);
            x += SMALL_ICON + 4.0;
            if cx.density == Density::Full {
                ui.painter().text(
                    egui::pos2(x, mid),
                    egui::Align2::LEFT_CENTER,
                    label,
                    font,
                    colour,
                );
            }
            if has_arrow(b) {
                paint_arrow(ui, egui::pos2(rect.right() - 8.0, mid), colour);
            }
        }
    }
}

/// The content of a dropdown; returns the action chosen.
fn menu_contents(ui: &mut Ui, menu: Menu, state: &RibbonState) -> Option<Action> {
    match menu {
        Menu::Items(items) => items_menu(ui, items, state),
        Menu::Palette(sections) => palette_menu(ui, sections, state),
        Menu::Functions(_) | Menu::Category(_) => function_menu(ui, menu, state),
        Menu::TextColours => text_colour_menu(ui),
        Menu::SizeGrid(title) => size_grid_menu(ui, title, state),
    }
}

/// The Textfarbe palette: swatches that set the colour of the selected text, then "Mehr Farben",
/// a colour picker that applies its colour with "Übernehmen" (Apply).
fn text_colour_menu(ui: &mut Ui) -> Option<Action> {
    let mut picked = None;
    let de = crate::i18n::language() == crate::i18n::Lang::German;
    for row in text_format::colour_rows() {
        ui.horizontal(|ui| {
            for rgb in row {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
                let fill = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                ui.painter().rect_filled(rect, 1.0, fill);
                if resp.hovered() {
                    ui.painter().rect_stroke(
                        rect,
                        1.0,
                        egui::Stroke::new(1.5, Color32::GRAY),
                        egui::StrokeKind::Outside,
                    );
                }
                if resp.clicked() {
                    picked = Some(Action::TextFormat(TextFormat::Colour([
                        rgb[0], rgb[1], rgb[2], 255,
                    ])));
                }
            }
        });
    }
    ui.separator();
    let id = ui.id().with("more_colours");
    let mut more: [u8; 3] = ui.data(|d| d.get_temp(id)).unwrap_or([0, 0, 0]);
    let title = if de { "Mehr Farben" } else { "More colours" };
    ui.collapsing(title, |ui| {
        ui.color_edit_button_srgb(&mut more);
        let apply = if de { "Übernehmen" } else { "Apply" };
        if ui.button(apply).clicked() {
            picked = Some(Action::TextFormat(TextFormat::Colour([
                more[0], more[1], more[2], 255,
            ])));
        }
    });
    ui.data_mut(|d| d.insert_temp(id, more));
    picked
}

/// Prime's size grid (0098): 12 by 12 cells, the hovered block from the top left is highlighted,
/// a click on a cell inserts a matrix of that many rows and columns.
fn size_grid_menu(
    ui: &mut Ui,
    title: crate::ribbon::spec::L,
    state: &RibbonState,
) -> Option<Action> {
    const N: usize = 12;
    const CELL: f32 = 14.0;
    const GAP: f32 = 4.5;
    let step = CELL + GAP;
    let mut picked = None;
    let hover_id = ui.id().with("size-grid-hover");
    let staged: Option<(usize, usize)> = ui.ctx().data(|d| d.get_temp(Id::new("stage-grid-hover")));
    let hovered: Option<(usize, usize)> = staged.or_else(|| ui.data(|d| d.get_temp(hover_id)));
    let label = match hovered {
        Some((r, c)) => format!("{} ({} x {})", title.get(), r + 1, c + 1),
        None => title.get().to_owned(),
    };
    ui.label(egui::RichText::new(label).size(12.0));
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(N as f32 * step, N as f32 * step), Sense::hover());
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let mut now = None;
    let ink = ui.visuals().text_color();
    let accent = ui.visuals().selection.bg_fill;
    for r in 0..N {
        for c in 0..N {
            let min = rect.min + egui::vec2(c as f32 * step, r as f32 * step);
            let cell = Rect::from_min_size(min, egui::Vec2::splat(CELL));
            let hit = Rect::from_min_size(min, egui::Vec2::splat(step));
            if pointer.is_some_and(|p| hit.contains(p)) {
                now = Some((r, c));
            }
            let lit = hovered.is_some_and(|(hr, hc)| r <= hr && c <= hc);
            if lit {
                ui.painter().rect_filled(cell, 0.0, accent);
            }
            ui.painter().rect_stroke(
                cell,
                0.0,
                egui::Stroke::new(1.0, ink),
                egui::StrokeKind::Inside,
            );
        }
    }
    ui.data_mut(|d| match now {
        Some(v) => d.insert_temp(hover_id, v),
        None => d.remove::<(usize, usize)>(hover_id),
    });
    if let Some((r, c)) = now {
        if state.enabled && ui.input(|i| i.pointer.any_click()) {
            picked = Some(Action::InsertMatrix(r + 1, c + 1));
        }
    }
    picked
}

fn items_menu(ui: &mut Ui, items: &'static [Item], state: &RibbonState) -> Option<Action> {
    let mut picked = None;
    ui.set_min_width(180.0);
    for item in items {
        if let Some(sub) = item.sub {
            ui.menu_button(item.label.get(), |ui| {
                if let Some(a) = items_menu(ui, sub, state) {
                    picked = Some(a);
                    ui.close();
                }
            });
            continue;
        }
        let ready = item.action.is_some() && item.rule.holds(state);
        let label = if item.mark.on(state) {
            format!("✓ {}", item.label.get())
        } else {
            item.label.get().to_owned()
        };
        let r = ui.add_enabled(ready, egui::Button::new(label));
        let r = if item.action.is_none() {
            let mut tip = tr("tip.not_available").to_owned();
            if !item.tip.is_empty() {
                tip = format!("{}\n{tip}", item.tip.get());
            }
            r.on_disabled_hover_text(tip)
        } else if !item.tip.is_empty() {
            r.on_hover_text(item.tip.get())
        } else {
            r
        };
        if r.clicked() {
            picked = item.action;
        }
    }
    picked
}

fn palette_menu(ui: &mut Ui, sections: &'static [Section], state: &RibbonState) -> Option<Action> {
    let mut picked = None;
    egui::ScrollArea::vertical()
        .max_height(520.0)
        .show(ui, |ui| {
            for s in sections {
                ui.label(egui::RichText::new(s.title.get()).strong());
                egui::Grid::new(("palette", s.title.0))
                    .spacing([2.0, 2.0])
                    .show(ui, |ui| {
                        for (i, cell) in s.cells.iter().enumerate() {
                            let ready = cell.action.is_some() && state.enabled;
                            let text = egui::RichText::new(cell.glyph).size(15.0);
                            let r = ui.add_enabled(
                                ready,
                                egui::Button::new(text).min_size(egui::vec2(50.0, 26.0)),
                            );
                            let r = if cell.action.is_none() {
                                r.on_disabled_hover_text(format!(
                                    "{}\n{}",
                                    cell.tip.get(),
                                    tr("tip.not_available")
                                ))
                            } else {
                                r.on_hover_text(cell.tip.get())
                            };
                            if r.clicked() {
                                picked = cell.action;
                            }
                            if (i + 1) % s.cols.max(1) == 0 {
                                ui.end_row();
                            }
                        }
                    });
                ui.add_space(2.0);
            }
        });
    picked
}

fn function_menu(ui: &mut Ui, menu: Menu, state: &RibbonState) -> Option<Action> {
    let mut picked = None;
    let names = function_names(menu);
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .show(ui, |ui| {
            egui::Grid::new(("functions", names.len()))
                .spacing([6.0, 2.0])
                .show(ui, |ui| {
                    for (i, name) in names.iter().enumerate() {
                        let entry = crate::functions_table::FUNCTIONS
                            .iter()
                            .find(|f| f.name == *name);
                        let ready = entry.is_some() && state.enabled;
                        let r = ui.add_enabled(
                            ready,
                            egui::Button::new(*name).min_size(egui::vec2(70.0, 22.0)),
                        );
                        let r = match entry {
                            Some(f) => r.on_hover_text(f.description),
                            None => r.on_disabled_hover_text(tr("tip.not_available")),
                        };
                        if r.clicked() {
                            picked = Some(Action::InsertFunction(name));
                        }
                        if (i + 1) % 5 == 0 {
                            ui.end_row();
                        }
                    }
                });
        });
    picked
}

/// The icon used for tests of the framework.
pub fn blank() -> Icon {
    Icon::Blank
}

/// Screenshot harness: opens the dropdown of the button `id` on the next frame it is drawn.
pub fn stage_popup(ctx: &egui::Context, id: &str) {
    ctx.data_mut(|d| d.insert_temp(Id::new("stage-popup"), id.to_owned()));
}

/// Screenshot harness: draws the size grid with the cell (`row`, `col`) hovered.
pub fn stage_grid_hover(ctx: &egui::Context, row: usize, col: usize) {
    ctx.data_mut(|d| d.insert_temp(Id::new("stage-grid-hover"), (row, col)));
}
