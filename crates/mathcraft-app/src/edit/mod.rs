//! Editing of a worksheet: the document model, undoable commands, the math and text editors,
//! the session that turns keys and clicks into edits, the shortcuts, and saving. Nothing here
//! depends on egui; `app.rs` and `canvas.rs` only forward input and draw what the session reports.
//!
//! Manual checklist for the shareholder (the agents never ran the window). Start with
//! `cargo run -p mathcraft-app` and Ctrl+N, or open an .mcdx file:
//! - Click the page, type `a/b`: the caret is in the denominator. Tab goes to the next empty place.
//! - Type `|x|`, `(a+b)`, `x^2`. Shift+Left selects the last item; typing `^` then raises it.
//! - Spacebar with a selection grows it (item, then whole expression). Ctrl+C copies the selection.
//! - Ctrl+M makes a one-element matrix, Ctrl+. the `->` arrow. Matrices tab: Matrix 2x2, then Tab
//!   through the cells; Tab from the last cell adds a row; Up and Down move between rows.
//! - Drag from empty page: a rectangle appears and the regions it touches are selected.
//! - Shift+click adds regions. Format tab: Align left, Distribute horizontally; Ctrl+Z undoes.
//! - Ctrl+drag a selected region: a copy appears where the mouse is released; Ctrl+Z removes it.
//! - Ctrl+X on regions, click the page, Ctrl+V: the regions come back. Paste the same text into
//!   a text editor to see the Mathcad text (one line per region).
//! - Format tab, Insert lines after a click on the page: regions below the click move down.
//! - Check that Tab moves between places (egui may use Tab for focus; report it if it does not).

pub mod clipboard;
pub mod command;
pub mod document;
pub mod layout;
pub mod math;
pub mod plot;
pub mod resize;
pub mod save;
pub mod session;
pub mod shortcut;
pub mod text;

#[cfg(test)]
mod area_tests;
#[cfg(test)]
mod furniture_tests;
#[cfg(test)]
mod german_input_tests;
#[cfg(test)]
mod interaction_tests;
#[cfg(test)]
mod math_keys_tests;
#[cfg(test)]
mod operators_tests;
#[cfg(test)]
mod parity2_tests;
#[cfg(test)]
mod parity_tests;
#[cfg(test)]
mod plot_tests;
#[cfg(test)]
mod region_tests;
#[cfg(test)]
mod ribbon_tests;
#[cfg(test)]
mod roundtrip_tests;
