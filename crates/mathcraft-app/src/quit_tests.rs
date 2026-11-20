//! Tests of the quit flow: closing the window with unsaved worksheets asks about each one, and
//! Discard or Cancel leads to the right next step. Runs on a bare context (no window).

use eframe::egui;

use crate::app::{App, Choice};
use crate::edit::save::Pending;
use crate::menus::MenuAction;

fn app() -> (egui::Context, App) {
    let ctx = egui::Context::default();
    let app = App::with_context(&ctx, None);
    (ctx, app)
}

/// Makes the active worksheet unsaved.
fn dirty(app: &mut App) {
    app.type_into_active("x:=1");
}

#[test]
fn a_clean_window_closes_at_once() {
    let (ctx, mut app) = app();
    assert!(
        app.on_close_request(&ctx, true),
        "the close is not cancelled"
    );
    assert_eq!(app.waiting(), None);
}

#[test]
fn quit_from_the_menu_closes_a_clean_window_at_once() {
    let (ctx, mut app) = app();
    app.menu_action(&ctx, MenuAction::Quit);
    assert!(app.closing());
    assert_eq!(app.waiting(), None);
}

#[test]
fn a_close_request_without_a_request_does_nothing() {
    let (ctx, mut app) = app();
    dirty(&mut app);
    app.on_close_request(&ctx, false);
    assert!(!app.closing());
    assert_eq!(app.waiting(), None);
}

#[test]
fn closing_with_one_unsaved_worksheet_asks_then_discard_closes() {
    let (ctx, mut app) = app();
    dirty(&mut app);
    assert!(!app.on_close_request(&ctx, true), "the close is cancelled");
    assert_eq!(app.waiting(), Some(Pending::Close));
    assert!(
        !app.closing(),
        "the window stays open while the question is open"
    );

    app.answer_confirm(&ctx, Choice::Discard);
    assert_eq!(app.waiting(), None);
    assert!(
        app.tab_entries().is_empty(),
        "the discarded worksheet is gone"
    );
    assert!(app.closing(), "Don't save must really close the window");
}

#[test]
fn quit_with_one_unsaved_worksheet_asks_then_discard_closes() {
    let (ctx, mut app) = app();
    dirty(&mut app);
    app.menu_action(&ctx, MenuAction::Quit);
    assert_eq!(app.waiting(), Some(Pending::Close));
    app.answer_confirm(&ctx, Choice::Discard);
    assert!(app.closing());
}

#[test]
fn two_unsaved_worksheets_are_both_asked_about_before_closing() {
    let (ctx, mut app) = app();
    dirty(&mut app);
    app.menu_action(&ctx, MenuAction::New);
    dirty(&mut app);
    assert_eq!(app.tab_entries().len(), 2);

    app.on_close_request(&ctx, true);
    assert_eq!(app.waiting(), Some(Pending::Close));
    app.answer_confirm(&ctx, Choice::Discard);
    assert!(
        !app.closing(),
        "the second unsaved worksheet is still asked about"
    );
    assert_eq!(app.waiting(), Some(Pending::Close));
    assert_eq!(app.tab_entries().len(), 1);
    assert!(app.tab_entries()[0].1, "the remaining worksheet is unsaved");

    app.answer_confirm(&ctx, Choice::Discard);
    assert!(app.closing());
    assert_eq!(app.waiting(), None);
}

#[test]
fn a_clean_worksheet_is_not_asked_about() {
    let (ctx, mut app) = app();
    app.menu_action(&ctx, MenuAction::New);
    dirty(&mut app);
    // Tab 0 is clean; the question is about the unsaved tab 1 only.
    app.select_tab(0);
    app.on_close_request(&ctx, true);
    assert_eq!(app.waiting(), Some(Pending::Close));
    assert_eq!(
        app.active_tab(),
        1,
        "the question is about the unsaved worksheet"
    );
    app.answer_confirm(&ctx, Choice::Discard);
    assert!(app.closing());
}

#[test]
fn cancel_keeps_the_window_open_and_a_later_quit_asks_again() {
    let (ctx, mut app) = app();
    dirty(&mut app);
    app.on_close_request(&ctx, true);
    app.answer_confirm(&ctx, Choice::Cancel);
    assert!(!app.closing());
    assert_eq!(app.waiting(), None);
    assert_eq!(app.tab_entries().len(), 1, "nothing was discarded");
    assert!(app.tab_entries()[0].1, "the edits are kept");

    // A later quit starts the flow again and works through to the end.
    app.menu_action(&ctx, MenuAction::Quit);
    assert_eq!(app.waiting(), Some(Pending::Close));
    app.answer_confirm(&ctx, Choice::Discard);
    assert!(app.closing());
}

#[test]
fn a_close_request_during_a_tab_question_asks_the_quit_question() {
    let (ctx, mut app) = app();
    dirty(&mut app);
    app.menu_action(&ctx, MenuAction::CloseDocument);
    assert_eq!(app.waiting(), Some(Pending::CloseDoc));
    // The window is closed while the tab question is open: the quit question replaces it.
    app.on_close_request(&ctx, true);
    assert_eq!(app.waiting(), Some(Pending::Close));
    app.answer_confirm(&ctx, Choice::Discard);
    assert!(app.closing());
}
