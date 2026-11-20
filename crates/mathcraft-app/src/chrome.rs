//! The window chrome around the ribbon, drawn like Prime's (screenshots 0003 to 0015, 0103 to
//! 0105, 0162 to 0169, 0205 to 0207, 0213): the round application button with its menu, the quick
//! access toolbar, the document tab strip and the worksheet's context menu. All icons are our own
//! vector drawings. Decisions that need no window live in `menus.rs`.

use std::ops::Range;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Align2, Color32, FontId, Id, Pos2, Rect, Sense, Stroke, Ui};

use crate::i18n::tr;
use crate::load::file_name;
use crate::menus::{
    recent_heading, submenu_entries, MenuAction, Submenu, WsAction, MENU_ROWS, WS_ROWS,
};

/// The icons of the chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    New,
    Open,
    Save,
    SaveAs,
    Print,
    Close,
    Options,
    Info,
    Quit,
    Undo,
    Redo,
}

/// Paints `glyph` inside `rect`. `enabled` greys it out.
pub fn paint_glyph(painter: &egui::Painter, rect: Rect, glyph: Glyph, enabled: bool) {
    let at = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + x * rect.width(),
            rect.top() + y * rect.height(),
        )
    };
    let ink = if enabled {
        Color32::from_rgb(60, 70, 85)
    } else {
        Color32::from_rgb(170, 175, 182)
    };
    let stroke = Stroke::new(1.3, ink);
    let folder = if enabled {
        Color32::from_rgb(240, 205, 80)
    } else {
        Color32::from_rgb(220, 218, 205)
    };
    let blue = if enabled {
        Color32::from_rgb(52, 110, 175)
    } else {
        Color32::from_rgb(185, 195, 208)
    };
    match glyph {
        Glyph::New => {
            let pts = vec![
                at(0.2, 0.05),
                at(0.62, 0.05),
                at(0.82, 0.25),
                at(0.82, 0.95),
                at(0.2, 0.95),
            ];
            painter.add(egui::Shape::closed_line(pts, stroke));
            painter.line_segment([at(0.62, 0.05), at(0.62, 0.25)], stroke);
            painter.line_segment([at(0.62, 0.25), at(0.82, 0.25)], stroke);
        }
        Glyph::Open | Glyph::Close => {
            painter.rect_filled(
                Rect::from_min_max(at(0.08, 0.32), at(0.92, 0.88)),
                1.5,
                folder,
            );
            painter.rect_stroke(
                Rect::from_min_max(at(0.08, 0.32), at(0.92, 0.88)),
                1.5,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.line_segment([at(0.08, 0.32), at(0.08, 0.2)], stroke);
            painter.line_segment([at(0.08, 0.2), at(0.4, 0.2)], stroke);
            painter.line_segment([at(0.4, 0.2), at(0.5, 0.32)], stroke);
        }
        Glyph::Save | Glyph::SaveAs => {
            let body = Rect::from_min_max(at(0.12, 0.1), at(0.88, 0.9));
            painter.rect_filled(body, 2.0, blue);
            painter.rect_filled(
                Rect::from_min_max(at(0.28, 0.1), at(0.72, 0.4)),
                0.0,
                Color32::WHITE,
            );
            painter.rect_filled(
                Rect::from_min_max(at(0.26, 0.56), at(0.74, 0.9)),
                0.0,
                Color32::from_rgb(235, 238, 242),
            );
        }
        Glyph::Print => {
            painter.rect_filled(Rect::from_min_max(at(0.1, 0.38), at(0.9, 0.78)), 2.0, blue);
            painter.rect_filled(
                Rect::from_min_max(at(0.28, 0.1), at(0.72, 0.42)),
                0.0,
                Color32::WHITE,
            );
            painter.rect_stroke(
                Rect::from_min_max(at(0.28, 0.1), at(0.72, 0.42)),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.rect_filled(
                Rect::from_min_max(at(0.28, 0.62), at(0.72, 0.95)),
                0.0,
                Color32::WHITE,
            );
        }
        Glyph::Options => {
            painter.rect_stroke(
                Rect::from_min_max(at(0.2, 0.05), at(0.8, 0.95)),
                1.5,
                stroke,
                egui::StrokeKind::Middle,
            );
            for k in 0..4 {
                let y = 0.25 + 0.17 * k as f32;
                painter.line_segment([at(0.32, y), at(0.68, y)], Stroke::new(1.0, ink));
            }
        }
        Glyph::Info => {
            painter.rect_filled(
                Rect::from_min_max(at(0.08, 0.08), at(0.92, 0.92)),
                2.0,
                Color32::from_rgb(35, 70, 150),
            );
            painter.text(
                at(0.5, 0.52),
                Align2::CENTER_CENTER,
                "i",
                FontId::proportional(rect.height() * 0.8),
                Color32::WHITE,
            );
        }
        Glyph::Quit => {
            painter.rect_filled(
                Rect::from_min_max(at(0.08, 0.08), at(0.92, 0.92)),
                2.0,
                Color32::from_rgb(220, 60, 55),
            );
            let w = Stroke::new(2.2, Color32::WHITE);
            painter.line_segment([at(0.3, 0.3), at(0.7, 0.7)], w);
            painter.line_segment([at(0.7, 0.3), at(0.3, 0.7)], w);
        }
        Glyph::Undo | Glyph::Redo => {
            let flip = glyph == Glyph::Redo;
            let x = |v: f32| if flip { 1.0 - v } else { v };
            let pts = vec![
                at(x(0.2), 0.45),
                at(x(0.55), 0.3),
                at(x(0.82), 0.45),
                at(x(0.82), 0.7),
                at(x(0.6), 0.85),
            ];
            painter.add(egui::Shape::line(pts, Stroke::new(1.8, ink)));
            painter.add(egui::Shape::convex_polygon(
                vec![at(x(0.1), 0.45), at(x(0.34), 0.22), at(x(0.34), 0.62)],
                ink,
                Stroke::NONE,
            ));
        }
    }
}

/// Small symbols the fonts may lack, drawn as shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbol {
    ArrowLeft,
    ArrowRight,
    Plus,
    Minus,
    CaretDown,
    CaretRight,
}

/// Paints `symbol` centred in `rect`.
pub fn paint_symbol(painter: &egui::Painter, rect: Rect, symbol: Symbol, colour: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    let stroke = Stroke::new(1.4, colour);
    let p = |dx: f32, dy: f32| c + egui::vec2(dx * r, dy * r);
    match symbol {
        Symbol::ArrowLeft | Symbol::ArrowRight => {
            let s = if symbol == Symbol::ArrowLeft {
                -1.0
            } else {
                1.0
            };
            painter.line_segment([p(-0.7 * s, 0.0), p(0.7 * s, 0.0)], stroke);
            painter.add(egui::Shape::convex_polygon(
                vec![p(0.9 * s, 0.0), p(0.2 * s, -0.6), p(0.2 * s, 0.6)],
                colour,
                Stroke::NONE,
            ));
        }
        Symbol::Plus | Symbol::Minus => {
            painter.circle_stroke(c, r * 0.9, stroke);
            painter.line_segment([p(-0.5, 0.0), p(0.5, 0.0)], stroke);
            if symbol == Symbol::Plus {
                painter.line_segment([p(0.0, -0.5), p(0.0, 0.5)], stroke);
            }
        }
        Symbol::CaretDown => {
            painter.add(egui::Shape::convex_polygon(
                vec![p(-0.55, -0.25), p(0.55, -0.25), p(0.0, 0.45)],
                colour,
                Stroke::NONE,
            ));
        }
        Symbol::CaretRight => {
            painter.add(egui::Shape::convex_polygon(
                vec![p(-0.25, -0.55), p(-0.25, 0.55), p(0.45, 0.0)],
                colour,
                Stroke::NONE,
            ));
        }
    }
}

/// A clickable symbol, greyed out when `enabled` is false.
pub fn symbol_button(ui: &mut Ui, symbol: Symbol, enabled: bool, tip: &str) -> egui::Response {
    // A greyed symbol takes no clicks and no keyboard focus, so Tab skips it.
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), sense);
    let colour = if !enabled {
        Color32::from_rgb(175, 180, 188)
    } else if response.hovered() {
        Color32::from_rgb(30, 100, 190)
    } else {
        Color32::from_rgb(70, 80, 95)
    };
    paint_symbol(ui.painter(), rect.shrink(4.0), symbol, colour);
    if tip.is_empty() {
        response
    } else {
        response.on_hover_text(tip)
    }
}

/// The application button: the sigma with the squared exponent of the app icon, drawn straight on
/// the ribbon (no ball): a dark blue sigma and a bright blue exponent. A soft rounded plate shows
/// under the pointer and while the menu is open.
pub fn orb(ui: &mut Ui, open: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), Sense::click());
    let p = ui.painter();
    if open || response.hovered() {
        p.rect_filled(
            rect.shrink(1.0),
            8.0,
            Color32::from_rgba_unmultiplied(34, 86, 146, if open { 40 } else { 24 }),
        );
    }
    // The egui default face is always there (the math faces are installed after the first frame).
    let family = egui::FontFamily::Proportional;
    let c = rect.center();
    // Drawn twice, half a point apart, so the thin default face reads as semi-bold.
    for dx in [-0.35_f32, 0.35] {
        p.text(
            c + egui::vec2(-3.5 + dx, 1.5),
            Align2::CENTER_CENTER,
            "\u{03A3}",
            FontId::new(27.0, family.clone()),
            Color32::from_rgb(28, 52, 128),
        );
    }
    for dx in [-0.25_f32, 0.25] {
        p.text(
            c + egui::vec2(10.5 + dx, -6.5),
            Align2::CENTER_CENTER,
            "2",
            FontId::new(14.0, family.clone()),
            Color32::from_rgb(38, 150, 230),
        );
    }
    response.on_hover_text(tr("menu.app_button"))
}

/// The quick access toolbar: New, Open, Save, Undo, Redo. Returns the action clicked.
pub fn quick_access(
    ui: &mut Ui,
    can_save: bool,
    can_undo: bool,
    can_redo: bool,
) -> Option<MenuAction> {
    let mut out = None;
    let items = [
        (Glyph::New, true, MenuAction::New, "btn.new"),
        (Glyph::Open, true, MenuAction::Open, "btn.open"),
        (Glyph::Save, can_save, MenuAction::Save, "btn.save"),
        (Glyph::Undo, can_undo, MenuAction::Undo, "qat.undo"),
        (Glyph::Redo, can_redo, MenuAction::Redo, "qat.redo"),
    ];
    for (glyph, enabled, action, tip) in items {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), Sense::click());
        if response.hovered() && enabled {
            ui.painter()
                .rect_filled(rect, 3.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        paint_glyph(ui.painter(), rect.shrink(4.0), glyph, enabled);
        if response.on_hover_text(tr(tip)).clicked() && enabled {
            out = Some(action);
        }
    }
    out
}

/// What the application menu asks for.
#[derive(Debug, Default)]
pub struct AppMenu {
    pub open: bool,
    pub submenu: Option<Submenu>,
}

/// Draws the application menu under the orb at `anchor` (screenshots 0004 and 0007): the left
/// column of actions, the right column with a submenu or the recent worksheets. Returns the action
/// picked; the menu closes then, on Escape and on a click outside.
pub fn app_menu(
    ctx: &egui::Context,
    anchor: Pos2,
    orb_rect: Rect,
    menu: &mut AppMenu,
    recent: &[PathBuf],
) -> Option<MenuAction> {
    if !menu.open {
        return None;
    }
    let mut picked = None;
    let area = egui::Area::new(Id::new("mathcraft-app-menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(anchor)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .inner_margin(0.0)
                .fill(Color32::from_rgb(244, 248, 251))
                .show(ui, |ui| {
                    ui.set_min_size(egui::vec2(440.0, 380.0));
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                        ui.vertical(|ui| {
                            ui.set_width(180.0);
                            for row in MENU_ROWS {
                                if row.separator_above {
                                    ui.add_space(4.0);
                                    ui.separator();
                                    ui.add_space(4.0);
                                }
                                let (rect, response) =
                                    ui.allocate_exact_size(egui::vec2(180.0, 46.0), Sense::click());
                                if response.hovered() {
                                    ui.painter().rect_filled(
                                        rect,
                                        0.0,
                                        Color32::from_rgb(214, 232, 248),
                                    );
                                    menu.submenu = row.submenu;
                                }
                                let glyph = match row.action {
                                    crate::menus::MenuAction0::New => Glyph::New,
                                    crate::menus::MenuAction0::Open => Glyph::Open,
                                    crate::menus::MenuAction0::Save => Glyph::Save,
                                    crate::menus::MenuAction0::SaveAs => Glyph::SaveAs,
                                    crate::menus::MenuAction0::Print => Glyph::Print,
                                    crate::menus::MenuAction0::CloseDocument => Glyph::Close,
                                    crate::menus::MenuAction0::Options => Glyph::Options,
                                    crate::menus::MenuAction0::About => Glyph::Info,
                                    crate::menus::MenuAction0::Quit => Glyph::Quit,
                                };
                                paint_glyph(
                                    ui.painter(),
                                    Rect::from_min_size(
                                        rect.left_center() + egui::vec2(12.0, -14.0),
                                        egui::vec2(28.0, 28.0),
                                    ),
                                    glyph,
                                    true,
                                );
                                ui.painter().text(
                                    rect.left_center() + egui::vec2(52.0, 0.0),
                                    Align2::LEFT_CENTER,
                                    tr(row.label),
                                    FontId::proportional(14.0),
                                    Color32::from_rgb(30, 35, 45),
                                );
                                if row.submenu.is_some() {
                                    paint_symbol(
                                        ui.painter(),
                                        Rect::from_center_size(
                                            rect.right_center() - egui::vec2(14.0, 0.0),
                                            egui::vec2(10.0, 10.0),
                                        ),
                                        Symbol::CaretRight,
                                        Color32::from_rgb(80, 85, 95),
                                    );
                                }
                                if response.clicked() {
                                    picked = Some(row.action.action());
                                }
                            }
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                            ui.set_width(250.0);
                            ui.add_space(8.0);
                            if let Some(sub) = menu.submenu {
                                for entry in submenu_entries(sub) {
                                    let enabled = entry.action.is_some();
                                    let response = ui.add_enabled(
                                        enabled,
                                        egui::Button::new(tr(entry.label))
                                            .frame(false)
                                            .min_size(egui::vec2(240.0, 28.0)),
                                    );
                                    if response.clicked() {
                                        picked = entry.action.map(|a| a.action());
                                    }
                                }
                                ui.add_space(6.0);
                            }
                            if matches!(menu.submenu, None | Some(Submenu::Open)) {
                                ui.label(
                                    egui::RichText::new(recent_heading(menu.submenu)).strong(),
                                );
                                ui.separator();
                                if recent.is_empty() {
                                    ui.weak(tr("menu.no_recent"));
                                }
                                for (i, path) in recent.iter().enumerate() {
                                    let label = format!("{}. {}", i + 1, file_name(path));
                                    let response = ui
                                        .add(
                                            egui::Button::new(label)
                                                .frame(false)
                                                .min_size(egui::vec2(240.0, 22.0)),
                                        )
                                        .on_hover_text(path.display().to_string());
                                    if response.clicked() {
                                        picked = Some(MenuAction::OpenRecent(path.clone()));
                                    }
                                }
                            }
                        });
                    });
                });
        });
    let outside = ctx.input(|i| i.pointer.any_click())
        && ctx
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|pos| !area.response.rect.contains(pos) && !orb_rect.contains(pos));
    if picked.is_some() || outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        menu.open = false;
        menu.submenu = None;
    }
    picked
}

/// What happened in the tab strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabEvent {
    Select(usize),
    Close(usize),
}

/// The tabs to draw: from a first one such that the active tab fits in `avail`, as many as fit.
#[must_use]
pub fn visible_tabs(widths: &[f32], active: usize, avail: f32) -> Range<usize> {
    if widths.is_empty() {
        return 0..0;
    }
    let active = active.min(widths.len() - 1);
    let mut start = 0;
    while start < active && widths[start..=active].iter().sum::<f32>() > avail {
        start += 1;
    }
    let mut end = start;
    let mut used = 0.0;
    while end < widths.len() && (used + widths[end] <= avail || end <= active) {
        used += widths[end];
        end += 1;
    }
    start..end
}

/// The document tab strip with the overflow list on the right (0213). `tabs` are the names (with
/// the unsaved marker already added by the caller through `dirty`).
pub fn tab_strip(ui: &mut Ui, tabs: &[(String, bool)], active: usize) -> Option<TabEvent> {
    let mut event = None;
    let font = FontId::proportional(14.0);
    let widths: Vec<f32> = tabs
        .iter()
        .map(|(name, dirty)| {
            let text = if *dirty {
                format!("{name} *")
            } else {
                name.clone()
            };
            let galley = ui
                .painter()
                .layout_no_wrap(text, font.clone(), Color32::BLACK);
            galley.size().x + 40.0
        })
        .collect();
    let (strip, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), Sense::hover());
    let painter = ui.painter_at(strip);
    painter.rect_filled(strip, 0.0, Color32::from_rgb(208, 222, 236));
    painter.line_segment(
        [strip.left_bottom(), strip.right_bottom()],
        Stroke::new(1.0, Color32::from_rgb(150, 170, 190)),
    );
    let arrow = Rect::from_min_size(
        strip.right_top() - egui::vec2(22.0, 0.0),
        egui::vec2(22.0, strip.height()),
    );
    let range = visible_tabs(&widths, active, strip.width() - 26.0);
    let mut x = strip.left() + 2.0;
    for i in range {
        let rect = Rect::from_min_size(
            Pos2::new(x, strip.top() + 2.0),
            egui::vec2(widths[i], strip.height() - 2.0),
        );
        let id = ui.id().with(("doc-tab", i));
        let response = ui.interact(rect, id, Sense::click());
        let selected = i == active;
        let fill = if selected {
            Color32::from_rgb(248, 251, 253)
        } else if response.hovered() {
            Color32::from_rgb(226, 236, 246)
        } else {
            Color32::from_rgb(190, 208, 226)
        };
        painter.rect_filled(
            rect,
            egui::CornerRadius {
                nw: 6,
                ne: 6,
                sw: 0,
                se: 0,
            },
            fill,
        );
        painter.rect_stroke(
            rect,
            egui::CornerRadius {
                nw: 6,
                ne: 6,
                sw: 0,
                se: 0,
            },
            Stroke::new(1.0, Color32::from_rgb(140, 160, 182)),
            egui::StrokeKind::Inside,
        );
        let (name, dirty) = &tabs[i];
        let text = if *dirty {
            format!("{name} *")
        } else {
            name.clone()
        };
        painter.text(
            rect.left_center() + egui::vec2(8.0, 0.0),
            Align2::LEFT_CENTER,
            text,
            font.clone(),
            Color32::from_rgb(25, 35, 50),
        );
        let close = Rect::from_center_size(
            rect.right_center() - egui::vec2(14.0, 0.0),
            egui::vec2(16.0, 16.0),
        );
        let close_response = ui.interact(close, id.with("close"), Sense::click());
        let cc = close.center();
        let col = if close_response.hovered() {
            Color32::from_rgb(200, 40, 40)
        } else {
            Color32::from_rgb(60, 70, 85)
        };
        painter.line_segment(
            [cc + egui::vec2(-4.0, -4.0), cc + egui::vec2(4.0, 4.0)],
            Stroke::new(1.5, col),
        );
        painter.line_segment(
            [cc + egui::vec2(4.0, -4.0), cc + egui::vec2(-4.0, 4.0)],
            Stroke::new(1.5, col),
        );
        if close_response.on_hover_text(tr("tab.close")).clicked() {
            event = Some(TabEvent::Close(i));
        } else if response.clicked() {
            event = Some(TabEvent::Select(i));
        }
        x += widths[i] + 2.0;
    }
    // The overflow arrow lists every open worksheet with a check mark on the current one.
    let arrow_response = ui.interact(arrow, ui.id().with("tab-overflow"), Sense::click());
    paint_symbol(
        &painter,
        Rect::from_center_size(arrow.center(), egui::vec2(12.0, 12.0)),
        Symbol::CaretDown,
        Color32::from_rgb(40, 50, 65),
    );
    egui::Popup::menu(&arrow_response).show(|ui| {
        for (i, (name, dirty)) in tabs.iter().enumerate() {
            let mark = if i == active { "\u{2713} " } else { "    " };
            let text = if *dirty {
                format!("{mark}{name} *")
            } else {
                format!("{mark}{name}")
            };
            if ui.button(text).clicked() {
                event = Some(TabEvent::Select(i));
            }
        }
    });
    event
}

/// The worksheet's context menu body (0162). Returns the supported action picked. `grid_on`
/// chooses between "Hide grid" and "Show grid".
pub fn worksheet_menu(ui: &mut Ui, grid_on: bool) -> Option<WsAction> {
    let mut out = None;
    ui.set_min_width(300.0);
    for row in WS_ROWS {
        if row.separator_above {
            ui.separator();
        }
        let key = if row.action == WsAction::ToggleGrid {
            crate::menus::grid_row_label(grid_on)
        } else {
            row.label
        };
        let text = if row.shortcut.is_empty() {
            tr(key).to_string()
        } else {
            format!("{}    {}", tr(key), row.shortcut)
        };
        if ui
            .add_enabled(row.supported, egui::Button::new(text).frame(false))
            .clicked()
        {
            out = Some(row.action);
            ui.close();
        }
    }
    out
}

/// True when `path` is listed as open already (used to switch to its tab instead of reopening).
#[must_use]
pub fn same_file(a: &Path, b: &Path) -> bool {
    a == b || (a.canonicalize().ok().is_some() && a.canonicalize().ok() == b.canonicalize().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tabs_show_when_they_fit() {
        assert_eq!(visible_tabs(&[100.0, 100.0, 100.0], 2, 400.0), 0..3);
    }

    #[test]
    fn the_active_tab_stays_visible_when_they_do_not_fit() {
        let r = visible_tabs(&[100.0; 6], 5, 250.0);
        assert!(r.contains(&5), "{r:?}");
        assert!(r.len() <= 3);
        let r = visible_tabs(&[100.0; 6], 0, 250.0);
        assert_eq!(r.start, 0);
    }

    #[test]
    fn no_tabs_gives_an_empty_range() {
        assert_eq!(visible_tabs(&[], 0, 100.0), 0..0);
    }
}
