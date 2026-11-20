//! Tests of the Tab key while a draft is open: the draft takes Tab only when no text box, dialog
//! or load has the keyboard. The window runs on a bare context (no window, no GPU).

use eframe::egui;

use crate::app::App;

fn app() -> (egui::Context, App) {
    let ctx = egui::Context::default();
    let app = App::with_context(&ctx, None);
    (ctx, app)
}

fn tab(shift: bool) -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Tab,
        physical_key: Some(egui::Key::Tab),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            shift,
            ..Default::default()
        },
    }
}

fn tab_input(shift: bool) -> egui::RawInput {
    egui::RawInput {
        events: vec![tab(shift)],
        ..Default::default()
    }
}

/// Runs one frame in which a single-line text box is drawn and asked for the focus.
fn focus_a_text_box(ctx: &egui::Context) {
    let mut text = String::new();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.text_edit_singleline(&mut text).request_focus();
        });
    });
}

#[test]
fn a_tab_with_a_focused_text_field_is_not_consumed_or_queued() {
    let (ctx, mut app) = app();
    app.type_into_active("x");
    focus_a_text_box(&ctx);
    assert!(ctx.wants_keyboard_input(), "the text box has the keyboard");
    let mut input = tab_input(false);
    app.take_draft_tabs(&ctx, &mut input);
    assert_eq!(input.events.len(), 1, "the Tab stays in the input for egui");
    assert_eq!(
        app.queued_draft_tabs(),
        0,
        "and is not replayed into the draft"
    );
}

#[test]
fn a_tab_with_an_open_draft_and_no_focus_moves_between_slots() {
    let (ctx, mut app) = app();
    app.type_into_active("x");
    let mut input = tab_input(true);
    app.take_draft_tabs(&ctx, &mut input);
    assert!(input.events.is_empty(), "the draft takes the Tab");
    assert_eq!(app.queued_draft_tabs(), 1);
    // The next frame replays the queued Tab into the draft, which leaves the queue empty.
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.ui(ctx));
    assert_eq!(app.queued_draft_tabs(), 0);
}

#[test]
fn a_queued_tab_is_dropped_when_the_keys_go_elsewhere() {
    let (ctx, mut app) = app();
    app.type_into_active("x");
    let mut input = tab_input(false);
    app.take_draft_tabs(&ctx, &mut input);
    assert_eq!(app.queued_draft_tabs(), 1);
    focus_a_text_box(&ctx);
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.ui(ctx));
    assert_eq!(
        app.queued_draft_tabs(),
        0,
        "the stale Tab is not replayed later"
    );
}
