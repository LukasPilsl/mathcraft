//! Tests of the tab management and the menu actions (no window: the app runs on a bare context).

use eframe::egui;

use crate::app::App;
use crate::edit::save::Pending;
use crate::menus::MenuAction;

fn app() -> (egui::Context, App) {
    let ctx = egui::Context::default();
    let app = App::with_context(&ctx, None);
    (ctx, app)
}

fn names(app: &App) -> Vec<String> {
    app.tab_entries().into_iter().map(|(n, _)| n).collect()
}

#[test]
fn the_app_starts_with_one_untitled_tab() {
    let (_ctx, app) = app();
    assert_eq!(app.tab_entries().len(), 1);
    assert!(!app.tab_entries()[0].1, "a new worksheet is clean");
}

#[test]
fn new_adds_numbered_tabs_and_activates_the_newest() {
    let (ctx, mut app) = app();
    app.menu_action(&ctx, MenuAction::New);
    app.menu_action(&ctx, MenuAction::New);
    let list = names(&app);
    assert_eq!(list.len(), 3);
    assert_eq!(list[1], format!("{}-1", list[0]));
    assert_eq!(list[2], format!("{}-2", list[0]));
    assert_eq!(app.active_tab(), 2);
}

#[test]
fn selecting_a_tab_keeps_each_worksheets_own_state() {
    let (ctx, mut app) = app();
    app.menu_action(&ctx, MenuAction::New);
    app.type_into_active("x:=1");
    assert!(
        app.tab_entries()[1].1,
        "the typed draft makes tab 2 unsaved"
    );
    app.select_tab(0);
    assert_eq!(app.active_tab(), 0);
    assert!(!app.tab_entries()[0].1);
    assert!(app.tab_entries()[1].1, "tab 2 keeps its unsaved edits");
}

#[test]
fn closing_a_clean_tab_activates_the_neighbour() {
    let (ctx, mut app) = app();
    app.menu_action(&ctx, MenuAction::New);
    app.menu_action(&ctx, MenuAction::New);
    app.select_tab(1);
    app.menu_action(&ctx, MenuAction::CloseDocument);
    assert_eq!(app.tab_entries().len(), 2);
    assert_eq!(app.active_tab(), 1);
}

#[test]
fn closing_an_unsaved_tab_asks_first() {
    let (ctx, mut app) = app();
    app.type_into_active("y:=2");
    app.menu_action(&ctx, MenuAction::CloseDocument);
    assert_eq!(app.waiting(), Some(Pending::CloseDoc));
    assert_eq!(app.tab_entries().len(), 1, "nothing closed yet");
}

#[test]
fn closing_the_last_tab_leaves_an_empty_window() {
    let (ctx, mut app) = app();
    app.menu_action(&ctx, MenuAction::CloseDocument);
    assert!(app.tab_entries().is_empty());
    // Close with nothing open does nothing, and New works again.
    app.menu_action(&ctx, MenuAction::CloseDocument);
    app.menu_action(&ctx, MenuAction::New);
    assert_eq!(app.tab_entries().len(), 1);
}

#[test]
fn the_title_has_the_version_and_the_name_without_extension() {
    let (_ctx, app) = app();
    let title = app.title_text();
    assert!(title.starts_with("Mathcraft "), "{title}");
    assert!(title.contains(env!("CARGO_PKG_VERSION")));
    assert!(!title.contains(".mcdx"));
    assert!(!title.contains('*'));
}

#[test]
fn options_and_about_open_their_windows() {
    let (ctx, mut app) = app();
    app.menu_action(&ctx, MenuAction::Options);
    assert!(app.options_open());
    app.menu_action(&ctx, MenuAction::About);
    assert!(app.about_open());
}

#[test]
fn undo_and_redo_from_the_toolbar_reach_the_session() {
    let (ctx, mut app) = app();
    // Nothing to undo: the actions are harmless.
    app.menu_action(&ctx, MenuAction::Undo);
    app.menu_action(&ctx, MenuAction::Redo);
    assert_eq!(app.tab_entries().len(), 1);
}
