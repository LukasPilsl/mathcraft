//! Colours of the window. Plain values, so the palette is changed in one place.

use eframe::egui::Color32;

/// Paper of the worksheet page.
pub const PAPER: Color32 = Color32::from_rgb(245, 250, 244);
/// Edge of a page in the page layout (Prime draws a one-pixel dark edge against the desk).
pub const PAGE_EDGE: Color32 = Color32::from_rgb(84, 89, 85);
/// Light blue grid on the paper.
pub const GRID: Color32 = Color32::from_rgb(196, 220, 240);
/// Tint over a selected region (drawn under its outline, so the content stays readable).
pub const SELECTION_FILL: Color32 = Color32::from_rgba_premultiplied(6, 19, 33, 40);
/// Crosshair at the start of the page.
pub const CROSSHAIR: Color32 = Color32::from_rgb(40, 120, 210);
/// Desk around the page in the page layout (Prime's dark grey, from screenshot 0003).
pub const DESK: Color32 = Color32::from_rgb(87, 89, 88);
/// Ink of text and math.
pub const INK: Color32 = Color32::from_rgb(24, 24, 28);
/// A computed result that matches Prime's stored result.
pub const MATCH: Color32 = Color32::from_rgb(0, 122, 74);
/// A mismatch or a failed evaluation.
pub const PROBLEM: Color32 = Color32::from_rgb(200, 30, 30);
/// Unsupported regions and their reasons.
pub const UNSUPPORTED: Color32 = Color32::from_rgb(128, 128, 134);
/// Prime's stored results (shown with the toggle, and in tooltips).
pub const STORED: Color32 = Color32::from_rgb(110, 110, 120);
/// Placeholder boxes (plots, unsupported elements).
pub const PLACEHOLDER_FILL: Color32 = Color32::from_rgb(236, 238, 242);
pub const PLACEHOLDER_EDGE: Color32 = Color32::from_rgb(150, 156, 166);
/// Accent of the ribbon's selected tab.
pub const ACCENT: Color32 = Color32::from_rgb(34, 98, 168);

/// Frame and collapse icon of an area.
pub const AREA_EDGE: Color32 = Color32::from_rgb(70, 110, 160);
/// Header bar of an area.
pub const AREA_HEADER: Color32 = Color32::from_rgb(222, 232, 244);
/// Green status dot of the calculation mode.
pub const STATUS_OK: Color32 = Color32::from_rgb(40, 170, 80);
