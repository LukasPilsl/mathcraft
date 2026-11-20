//! Application state and the frame loop: starts loads, forwards keys, clicks, drags and drops to
//! the editing session, runs the file actions, and lays out the window. A load and a re-evaluation
//! run on background threads (see `loader.rs`), so the window stays responsive. All editing
//! decisions are in `edit/`; all drawing decisions are in the modules below this one.
//!
//! # Manual test (for the shareholder; nobody has run the window yet)
//!
//! Start with `cargo run -p mathcraft-app -- documentation/examples/Mechanik_1_massen_ziehen.mcdx`
//! (or a copy of it: saving overwrites the file it was opened from).
//! 1. Click a region: it gets an outline. Click empty page: an insertion point (a cross) appears.
//! 2. Type `zz:=3` at an insertion point, press Enter: a new region appears. Type `zz*2=`, Enter:
//!    the result 6 shows after `=`. The title gets `*` after the file name.
//! 3. Drag a selected region with the mouse: it moves; release. Ctrl+Z moves it back.
//! 4. Click a region, press Delete: it is removed. Ctrl+Z restores it, Ctrl+Shift+Z (or Ctrl+Y)
//!    removes it again.
//! 5. Ctrl+A selects every region; Arrow keys move them all; Escape clears the selection.
//! 6. Ribbon Math tab: Fraction, Power, Square root, Subscript, `:=`, `=`, `->`, Matrix 2x2 insert
//!    into the math region being typed. Summation and Integral are greyed out (not built yet).
//! 7. Ctrl+S (or Save) writes the file; the `*` goes away. Save As with a path typed in the File
//!    box (a copy, for example `/tmp/copy.mcdx`) writes there; with the box empty it opens a save
//!    dialog. The original stays unchanged. Ctrl+O and Open work the same way with a file dialog.
//! 8. Ctrl+I (Ribbon Insert, Image): click where the image goes, pick a PNG or JPEG. Select it:
//!    drag a corner handle to resize (aspect ratio kept). Ctrl+Shift+A inserts an empty area;
//!    select regions and use Area around selection to put them in one. Right-click an area to lock.
//! 9. Ctrl+N (or New) starts an empty worksheet. Opening or closing with unsaved edits asks
//!    Save / Discard / Cancel first.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align2, Event, Key, Modifiers, Rect, RichText, ViewportCommand};
use mathcraft_core::units::UnitSystem;
use mathcraft_import::evaluate::on_big_stack;
use mathcraft_import::page::{known_paper_codes, Margins, PageSettings};
use mathcraft_import::regions::{RegionKind, Settings};

use crate::canvas::{self, Overlay};
use crate::chrome::{self, AppMenu, TabEvent};
use crate::chrome_dialogs::{self, OptionsTab};
use crate::config;
use crate::dialogs::{Dialog, Dialogs, Insertion};
use crate::edit::document::Document;
use crate::edit::math::Template;
use crate::edit::save::{DocumentWriter, FileState, McdxWriter, Pending};
use crate::edit::session::{EditKey, EditSession};
use crate::edit::shortcut::{shortcut, Shortcut};
use crate::export;
use crate::file_picker;
use crate::i18n::{self, term, tr, trf, Lang};
use crate::menus::{self, MenuAction, WsAction};
use crate::statusbar::{self, SearchAction, SearchState};
use mathcraft_core::eval::builtins::fileio::FileOptions;
use mathcraft_doc::search::step_index;
use mathcraft_doc::MatchKind;

use crate::file_access::{FileAccess, Switch};
use crate::link_window::{self, LinkDraft};
use crate::load::{
    evaluate_document, evaluate_document_with, expand_home, file_name, open_file, worksheet_dir,
    Loaded,
};
use crate::loader::Loader;
use crate::messages::localize_error;
use crate::pages::Geometry;
use crate::palette::GreekChord;
use crate::plot_dialog::{PlotChoice, PlotDialog};
use crate::ribbon::{self, Action, FileAction, RibbonState, TextFormat, ZoomStep};
use crate::settings_window::{self, Choice as WindowChoice, OptionsDraft, ResultDialog};
use crate::theme;
use crate::units_dialog::{self, UnitsDialog};
use crate::view_state::{
    needs_refresh, zoom_by_factor, zoom_in, zoom_out, CalcMode, DisplayFlags, ZOOM_RESET,
};
use crate::viewmodel::{Body, Tone, ViewModel};

mod live_host;

/// Space added by the Insert lines button (points): one line of text.
const INSERT_LINES_POINTS: f64 = 12.0;
/// The state of one open worksheet while it is not the active one. The active worksheet lives in
/// the fields of [`App`]; its slot in `App::tabs` is an empty placeholder (see [`App::swap_slot`]).
struct DocSlot {
    name: Option<String>,
    file: FileState,
    session: Option<EditSession>,
    view: Option<ViewModel>,
    view_revision: u64,
    error: Option<String>,
    file_access: FileAccess,
    file_access_open: bool,
}

impl DocSlot {
    fn empty() -> Self {
        Self {
            name: None,
            file: FileState::default(),
            session: None,
            view: None,
            view_revision: 0,
            error: None,
            file_access: FileAccess::new(),
            file_access_open: false,
        }
    }

    fn is_dirty(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|s| self.file.is_dirty(s.revision(), s.has_draft()))
    }
}

/// The answer in the unsaved-changes dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Choice {
    Save,
    Discard,
    Cancel,
}

pub struct App {
    /// The other open worksheets and a placeholder at `active` (see [`DocSlot`]).
    tabs: Vec<DocSlot>,
    /// Index of the active worksheet in `tabs`.
    active: usize,
    app_menu: AppMenu,
    options_tab: Option<OptionsTab>,
    search: SearchState,
    recent: Vec<PathBuf>,
    /// The context-menu entry picked on the worksheet this frame.
    pending_ws: Option<WsAction>,
    /// Screenshot harness: a ribbon dropdown to open and a size-grid cell to show hovered, handed
    /// to the ribbon on the next frame (see [`Staged`]).
    staged_popup: Option<String>,
    staged_hover: Option<(usize, usize)>,
    /// The file name of the open document, for the title. `None` before anything is open.
    name: Option<String>,
    /// Where the open document is stored, and whether it has unsaved edits.
    file: FileState,
    /// The path of the load in progress, which becomes the file of the document when it arrives.
    loading: Option<PathBuf>,
    /// The open document under edit. Kept on screen, under a spinner, while another one loads.
    session: Option<EditSession>,
    /// The evaluated view of the document. Refreshed in the background after each change.
    view: Option<ViewModel>,
    /// The revision of the session that `view` (or the refresh in progress) belongs to.
    view_revision: u64,
    loader: Loader<Loaded>,
    reeval: Loader<ViewModel>,
    /// Writes `.mcdx` files: builds the bytes first, then replaces the file atomically.
    writer: McdxWriter,
    /// A load error: the window shows it instead of the page.
    error: Option<String>,
    /// A note about the last action: a save, a failed save, a failed refresh.
    notice: Option<String>,
    /// An action that waits for the answer in the unsaved-changes dialog.
    confirm: Option<Pending>,
    /// Set when the window is being closed on purpose, so the close is not asked again.
    allow_close: bool,
    tab: usize,
    /// Tab presses taken out of the input while a draft is open (egui would move the focus to a
    /// button with them); `true` is Shift+Tab. Replayed as editing keys.
    draft_tabs: Vec<bool>,
    zoom_pct: f32,
    show_prime: bool,
    page: (usize, usize),
    title_sent: String,
    /// The Insert and Help dialogs.
    dialogs: Dialogs,
    /// Ctrl+G, then a letter: a Greek letter.
    chord: GreekChord,
    /// Automatic or manual calculation.
    calc_mode: CalcMode,
    /// Set by F9 or the Calculate button: evaluate the worksheet on the next frame.
    recalc_requested: bool,
    /// Grid and region outlines on the page.
    display: DisplayFlags,
    /// The page setup window's draft, while it is open. Applied with OK.
    page_setup: Option<PageSettings>,
    /// The Results format window: the draft and the region it was opened for.
    result_dialog: Option<ResultDialog>,
    /// The Link window of the Textformatierung tab, while it is open.
    link_window: Option<LinkDraft>,
    /// The Worksheet options window.
    options_dialog: Option<OptionsDraft>,
    /// The Einheitensystem window.
    units_dialog: Option<UnitsDialog>,
    /// The Resources button whose target the small window shows.
    resource_dialog: Option<&'static ribbon::resources::Resource>,
    /// The file-access switches of the open worksheet (session only, off on open).
    file_access: FileAccess,
    /// Whether the File access window is shown.
    file_access_open: bool,
    /// The plot window, while it is open.
    plot_dialog: Option<PlotDialog>,
    /// The AI endpoint (live mode), when it is on.
    live: Option<crate::live::LiveRuntime>,
    /// The Settings switch of the AI endpoint.
    live_enabled: bool,
    /// Regions the AI changed during the last pump, handed to the runtime after it.
    pending_flashes: Vec<crate::live::Flash>,
    stage_live: bool,
}

impl App {
    /// Creates the app and opens `initial` when one is given.
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        Self::with_context(&cc.egui_ctx, initial)
    }

    /// Creates the app on `ctx` (fonts and options are set on it) and opens `initial` when one is
    /// given. The window and the headless screenshot harness both come through here.
    pub fn with_context(ctx: &egui::Context, initial: Option<PathBuf>) -> Self {
        // Ctrl+0 and Ctrl+scroll belong to the worksheet zoom, not to the window's UI zoom.
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        crate::fonts::install(ctx);
        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            app_menu: AppMenu::default(),
            options_tab: None,
            search: SearchState::default(),
            recent: menus::load_recent(),
            pending_ws: None,
            staged_popup: None,
            staged_hover: None,
            name: None,
            file: FileState::default(),
            loading: None,
            session: None,
            view: None,
            view_revision: 0,
            loader: Loader::new(),
            reeval: Loader::new(),
            writer: McdxWriter,
            error: None,
            notice: None,
            confirm: None,
            allow_close: false,
            tab: 0,
            draft_tabs: Vec::new(),
            zoom_pct: 100.0,
            show_prime: false,
            page: (1, 1),
            title_sent: String::new(),
            dialogs: Dialogs::default(),
            chord: GreekChord::default(),
            calc_mode: CalcMode::Auto,
            recalc_requested: false,
            display: DisplayFlags::default(),
            page_setup: None,
            result_dialog: None,
            link_window: None,
            options_dialog: None,
            units_dialog: None,
            resource_dialog: None,
            file_access: FileAccess::new(),
            file_access_open: false,
            plot_dialog: None,
            live: None,
            live_enabled: false,
            pending_flashes: Vec::new(),
            stage_live: false,
        };
        match initial {
            Some(path) => app.open(ctx, &path),
            None => app.new_document(),
        }
        app
    }

    /// True while a load or a re-evaluation runs on a background thread.
    pub fn is_busy(&self) -> bool {
        self.loader.busy() || self.reeval.busy()
    }

    /// Stages a tab, a dialog or a window to show on the next frames. Used by the screenshot
    /// harness (`examples/screenshot.rs`); the window never calls it.
    pub fn stage(&mut self, staged: Staged) {
        match staged {
            Staged::Live => self.stage_live = true,
            Staged::Tab(index) => self.tab = index,
            Staged::Dialog(dialog) => self.dialogs.open(dialog),
            Staged::ResultFormat => self.open_result_format(),
            Staged::PageSetup => self.open_page_setup(),
            Staged::WorksheetOptions => self.open_worksheet_options(),
            Staged::FileAccess => self.file_access_open = true,
            Staged::PageLayout => self.display.page_layout = true,
            Staged::AppMenu(sub) => {
                self.app_menu.open = true;
                self.app_menu.submenu = sub;
            }
            Staged::AppOptions => self.options_tab = Some(OptionsTab::Options),
            Staged::SelectPlot => {
                if let Some(session) = self.session.as_mut() {
                    session.select_first(true);
                }
            }
            Staged::SelectMath => {
                if let Some(session) = self.session.as_mut() {
                    session.select_first(false);
                }
            }
            Staged::Popup(id) => self.staged_popup = Some(id),
            Staged::GridHover(r, c) => self.staged_hover = Some((r, c)),
            Staged::UnitsDialog => self.open_units_dialog(),
            Staged::Resource(id) => self.resource_dialog = ribbon::resources::find(&id),
            Staged::Zoom(pct) => self.zoom_pct = pct,
            Staged::Tabs(n) => {
                for _ in 0..n {
                    self.new_document();
                }
                self.select_tab(0);
            }
        }
    }

    /// True when the open document has edits that are not in its file.
    fn is_dirty(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|s| self.file.is_dirty(s.revision(), s.has_draft()))
    }

    /// Asks for `action`. With unsaved edits the dialog asks first; otherwise it runs at once.
    fn request(&mut self, ctx: &egui::Context, action: Pending) {
        // Opening and creating use a new tab, so nothing is lost and nothing is asked.
        let asks = matches!(action, Pending::Close | Pending::CloseDoc);
        if asks && self.is_dirty() {
            self.confirm = Some(action);
        } else {
            self.run(ctx, action);
        }
    }

    /// Runs an action that has been confirmed (or needed no confirmation).
    fn run(&mut self, ctx: &egui::Context, action: Pending) {
        match action {
            Pending::Open(path) => self.open(ctx, &path),
            Pending::New => self.new_document(),
            Pending::CloseDoc => self.close_active(),
            Pending::Close => {
                // Every unsaved worksheet is asked about in turn before the window closes.
                match self.first_dirty() {
                    Some(i) => {
                        self.select_tab(i);
                        // The dialog always asks about the active worksheet. While a load runs the
                        // tab cannot be switched to yet, so the question waits for the next request.
                        if self.active == i {
                            self.confirm = Some(Pending::Close);
                        }
                    }
                    None => {
                        self.allow_close = true;
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                }
            }
        }
    }

    /// The first worksheet with unsaved edits, the active one included.
    fn first_dirty(&self) -> Option<usize> {
        (0..self.tabs.len()).find(|&i| {
            if i == self.active {
                self.is_dirty()
            } else {
                self.tabs[i].is_dirty()
            }
        })
    }

    /// Exchanges the active worksheet's fields with `slot`.
    fn swap_slot(&mut self, slot: &mut DocSlot) {
        std::mem::swap(&mut self.name, &mut slot.name);
        std::mem::swap(&mut self.file, &mut slot.file);
        std::mem::swap(&mut self.session, &mut slot.session);
        std::mem::swap(&mut self.view, &mut slot.view);
        std::mem::swap(&mut self.view_revision, &mut slot.view_revision);
        std::mem::swap(&mut self.error, &mut slot.error);
        std::mem::swap(&mut self.file_access, &mut slot.file_access);
        std::mem::swap(&mut self.file_access_open, &mut slot.file_access_open);
    }

    /// Parks the active worksheet in its slot and makes a fresh empty tab the active one.
    fn add_tab(&mut self) {
        // Work in progress for the parked worksheet is stale; its view is made again on return.
        self.reeval = Loader::new();
        if !self.tabs.is_empty() {
            let mut parked = DocSlot::empty();
            self.swap_slot(&mut parked);
            parked.view_revision = 0;
            self.tabs[self.active] = parked;
        }
        self.tabs.push(DocSlot::empty());
        self.active = self.tabs.len() - 1;
        self.notice = None;
    }

    /// Makes worksheet `index` the active one. Ignored while a load runs.
    pub(crate) fn select_tab(&mut self, index: usize) {
        if index == self.active || index >= self.tabs.len() || self.loader.busy() {
            return;
        }
        self.reeval = Loader::new();
        let mut parked = DocSlot::empty();
        self.swap_slot(&mut parked);
        parked.view_revision = 0;
        self.tabs[self.active] = parked;
        let mut incoming = std::mem::replace(&mut self.tabs[index], DocSlot::empty());
        self.swap_slot(&mut incoming);
        self.active = index;
        self.notice = None;
    }

    /// Closes the active worksheet without asking and activates its neighbour.
    fn close_active(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        self.loader = Loader::new();
        self.loading = None;
        self.reeval = Loader::new();
        self.swap_slot(&mut DocSlot::empty());
        let closed = self.active;
        let next = menus::active_after_close(self.tabs.len(), closed, closed);
        self.tabs.remove(closed);
        if let Some(next) = next {
            let mut incoming = std::mem::replace(&mut self.tabs[next], DocSlot::empty());
            self.swap_slot(&mut incoming);
            self.active = next;
        } else {
            self.active = 0;
        }
        self.notice = None;
    }

    /// The tab strip's entries: the display name and whether the worksheet has unsaved edits.
    pub(crate) fn tab_entries(&self) -> Vec<(String, bool)> {
        (0..self.tabs.len())
            .map(|i| {
                if i == self.active {
                    let name = self.name.as_deref().unwrap_or(tr("app.untitled"));
                    (menus::display_name(name), self.is_dirty())
                } else {
                    let slot = &self.tabs[i];
                    let name = slot.name.as_deref().unwrap_or(tr("app.untitled"));
                    (menus::display_name(name), slot.is_dirty())
                }
            })
            .collect()
    }

    /// The tab that already shows `path`, if any.
    fn tab_of(&self, path: &Path) -> Option<usize> {
        (0..self.tabs.len()).find(|&i| {
            let file = if i == self.active {
                self.file.path()
            } else {
                self.tabs[i].file.path()
            };
            file.is_some_and(|f| chrome::same_file(f, path))
        })
    }

    /// The file names of every open worksheet, for numbering new ones.
    fn open_names(&self) -> Vec<String> {
        (0..self.tabs.len())
            .filter_map(|i| {
                if i == self.active {
                    self.name.clone()
                } else {
                    self.tabs[i].name.clone()
                }
            })
            .collect()
    }

    /// Runs a command of the application menu, the quick access toolbar or a shortcut.
    pub(crate) fn menu_action(&mut self, ctx: &egui::Context, action: MenuAction) {
        match action {
            MenuAction::New => self.request(ctx, Pending::New),
            MenuAction::Open => self.file_action(ctx, FileAction::BrowseOpen),
            MenuAction::OpenRecent(path) => self.request(ctx, Pending::Open(path)),
            MenuAction::Save => self.save_or_ask(),
            MenuAction::SaveAs => self.file_action(ctx, FileAction::BrowseSaveAs),
            MenuAction::ExportPdf => self.file_action(ctx, FileAction::ExportPdf),
            MenuAction::Print => self.file_action(ctx, FileAction::Print),
            MenuAction::PageSetup => self.file_action(ctx, FileAction::PageSetup),
            MenuAction::CloseDocument => {
                if self.session.is_some() || self.error.is_some() {
                    self.request(ctx, Pending::CloseDoc);
                }
            }
            MenuAction::Options => self.options_tab = Some(OptionsTab::Options),
            MenuAction::About => self.dialogs.open(Dialog::About),
            MenuAction::Quit => self.request(ctx, Pending::Close),
            MenuAction::Undo => {
                if let Some(session) = self.session.as_mut() {
                    session.undo();
                }
            }
            MenuAction::Redo => {
                if let Some(session) = self.session.as_mut() {
                    session.redo();
                }
            }
        }
    }

    /// Saves to the file of the worksheet, or asks for a file when it has none (Prime asks too).
    fn save_or_ask(&mut self) {
        if self.session.is_none() {
            return;
        }
        if self.file.path().is_some() {
            self.save_document(None);
        } else if let Some(path) = file_picker::save_worksheet(self.file.path()) {
            self.save_document(Some(path));
        }
    }

    /// Remembers `path` in the recent list and writes the list.
    fn remember(&mut self, path: &Path) {
        self.recent = menus::push_recent(&self.recent, path);
        menus::save_recent(&self.recent);
    }

    /// Starts loading `path`. The window shows a spinner until the result arrives, over the
    /// previous document if there is one; a result of an earlier open that is still running is
    /// dropped.
    fn open(&mut self, ctx: &egui::Context, path: &Path) {
        // A worksheet that is open already is shown, not opened twice.
        if let Some(index) = self.tab_of(path) {
            self.select_tab(index);
            return;
        }
        if self.loader.busy() {
            self.notice = Some(tr("app.busy_loading").to_string());
            return;
        }
        self.add_tab();
        self.loading = Some(path.to_path_buf());
        self.error = None;
        self.notice = None;
        let owned = path.to_path_buf();
        let wake_ctx = ctx.clone();
        let started = self.loader.start(
            move |cancel| open_file(&owned, Some(cancel)),
            move || wake_ctx.request_repaint(),
        );
        if let Err(message) = started {
            self.loading = None;
            self.error = Some(message);
            self.drop_failed_tab();
        }
    }

    /// Replaces the document with an empty, untitled one.
    fn new_document(&mut self) {
        // A load in progress is stale now.
        self.loader = Loader::new();
        self.loading = None;
        let name = menus::next_untitled(&self.open_names(), tr("app.untitled"));
        self.add_tab();
        let doc = Document::untitled(&name);
        let for_eval = doc.clone();
        let vm = on_big_stack(move || evaluate_document(&for_eval));
        let session = EditSession::new(doc);
        self.view_revision = session.revision();
        self.session = Some(session);
        self.view = Some(vm);
        self.file = FileState::default();
        self.file_access.reset();
        self.file_access_open = false;
        self.name = Some(name);
        self.error = None;
        self.notice = None;
        self.reeval = Loader::new();
    }

    /// After a load failed: the empty tab it made goes away when other worksheets are open; the
    /// error is then shown as a note over the worksheet that is back.
    fn drop_failed_tab(&mut self) {
        if self.tabs.len() > 1 {
            let message = self.error.take();
            self.close_active();
            self.notice = message.map(|m| trf("app.cannot_open_file", &[&localize_error(&m)]));
        }
    }

    /// Takes a finished load, if there is one.
    fn collect_load(&mut self) {
        if let Some(result) = self.loader.poll() {
            match result {
                Ok(loaded) => {
                    let session = EditSession::new(loaded.doc);
                    self.view_revision = session.revision();
                    self.session = Some(session);
                    self.view = Some(loaded.vm);
                    let path = self.loading.take();
                    self.name = path.as_deref().map(file_name);
                    if let Some(p) = path.as_deref() {
                        self.remember(p);
                    }
                    self.file = FileState::new(path);
                    // A newly opened worksheet starts with file access off, every time.
                    self.file_access.reset();
                    self.file_access_open = false;
                    self.error = None;
                    // A refresh of the previous document is now stale.
                    self.reeval = Loader::new();
                }
                Err(message) => {
                    self.loading = None;
                    self.session = None;
                    self.view = None;
                    self.name = None;
                    self.file = FileState::default();
                    self.error = Some(message);
                    self.drop_failed_tab();
                }
            }
        }
    }

    /// Takes a finished re-evaluation, if there is one.
    fn collect_refresh(&mut self) {
        if let Some(result) = self.reeval.poll() {
            match result {
                Ok(vm) => self.view = Some(vm),
                Err(message) => {
                    // Revision 0 never matches a changed session, so the next frame retries.
                    self.view_revision = 0;
                    self.notice = Some(message);
                }
            }
        }
    }

    /// Starts a re-evaluation when the document changed since the view was made. Runs on a
    /// background thread, so typing and clicking go on while it runs.
    fn refresh_view(&mut self, ctx: &egui::Context) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let revision = session.revision();
        let forced = std::mem::take(&mut self.recalc_requested);
        let changed = revision != self.view_revision;
        if !needs_refresh(self.calc_mode, forced, self.view.is_some(), changed) {
            return;
        }
        if self.reeval.busy() {
            // Ask again on the next frame, once the evaluation in progress has finished.
            self.recalc_requested = forced;
            return;
        }
        self.view_revision = revision;
        let doc = session.document().clone();
        // The file functions read beside the worksheet's file, when it has one, and only with the
        // switches the user turned on. Writing needs an explicit calculate (`forced`).
        let base = self.file.path().map(worksheet_dir);
        let files = base
            .as_deref()
            .and_then(|dir| FileOptions::real_under(dir).ok())
            .unwrap_or_else(FileOptions::disabled)
            .with_switches(self.file_access.switches(forced));
        let waits = self.file_access.writes_wait_for_calculate(forced);
        let wake_ctx = ctx.clone();
        let started = self.reeval.start(
            move |cancel| {
                Ok(on_big_stack(move || {
                    evaluate_document_with(&doc, files, Some(cancel))
                }))
            },
            move || wake_ctx.request_repaint(),
        );
        match started {
            Err(message) => self.notice = Some(message),
            Ok(()) if waits => self.notice = Some(tr("file.waits").to_string()),
            Ok(()) => {}
        }
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some((first, rest)) = dropped.split_first() {
            if !rest.is_empty() {
                let names: Vec<String> = rest.iter().map(|p| file_name(p)).collect();
                self.notice = Some(trf(
                    "app.drop_multi",
                    &[
                        &file_name(first),
                        &rest.len().to_string(),
                        &names.join(", "),
                    ],
                ));
            }
            self.request(ctx, Pending::Open(first.clone()));
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Num0)) {
            self.zoom_pct = ZOOM_RESET;
        }
        // egui reports Ctrl+wheel and pinches as a zoom factor.
        let pinch = ctx.input(|i| i.zoom_delta());
        if (pinch - 1.0).abs() > 1e-4 {
            self.zoom_pct = zoom_by_factor(self.zoom_pct, pinch);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F9)) {
            self.recalc_requested = true;
        }
        for action in take_shortcuts(ctx) {
            self.apply_shortcut(ctx, action);
        }
    }

    /// Runs a keyboard shortcut.
    fn apply_shortcut(&mut self, ctx: &egui::Context, action: Shortcut) {
        match action {
            Shortcut::Undo => {
                if let Some(session) = self.session.as_mut() {
                    session.undo();
                }
            }
            Shortcut::Redo => {
                if let Some(session) = self.session.as_mut() {
                    session.redo();
                }
            }
            Shortcut::Save => self.save_or_ask(),
            Shortcut::SaveAs => self.file_action(ctx, FileAction::BrowseSaveAs),
            Shortcut::Open => self.file_action(ctx, FileAction::BrowseOpen),
            Shortcut::InsertImage => self.insert_image_dialog(),
            Shortcut::InsertArea => match self.session.as_mut() {
                Some(session) => {
                    session.insert_area_at_crosshair();
                }
                None => self.notice = Some(tr("tip.no_worksheet").to_string()),
            },
            Shortcut::InsertXyPlot | Shortcut::InsertPolarPlot => match self.session.as_mut() {
                Some(session) => {
                    session.insert_plot_at_crosshair(action == Shortcut::InsertPolarPlot);
                }
                None => self.notice = Some(tr("tip.no_worksheet").to_string()),
            },
            Shortcut::New => self.request(ctx, Pending::New),
            Shortcut::SelectAll => {
                if let Some(session) = self.session.as_mut() {
                    session.select_all();
                }
            }
            Shortcut::SquareRoot => {
                if let Some(session) = self.session.as_mut() {
                    session.insert_template(Template::SquareRoot);
                }
            }
            Shortcut::InsertFunction => self.dialogs.open(Dialog::Function(None)),
            Shortcut::InsertUnit => self.dialogs.open(Dialog::Unit),
            Shortcut::GreekPrefix => self.chord.arm(),
            Shortcut::Matrix => {
                if let Some(session) = self.session.as_mut() {
                    session.insert_matrix(1, 1);
                }
            }
            Shortcut::SymbolicEvaluate => {
                if let Some(session) = self.session.as_mut() {
                    session.insert_template(Template::Symbolic);
                }
            }
            Shortcut::MathTemplate(template) => {
                if let Some(session) = self.session.as_mut() {
                    session.insert_template(template);
                }
            }
            Shortcut::Prime => {
                if let Some(session) = self.session.as_mut() {
                    session.type_char('\'');
                }
            }
        }
    }

    /// Typed text and editing keys go to the session. Ignored while the path box has the keyboard,
    /// a load runs, or the unsaved-changes dialog is open. Text that comes with Ctrl or Command is
    /// a shortcut, not text.
    fn handle_edit_keys(&mut self, ctx: &egui::Context) {
        if self.keys_go_elsewhere(ctx) {
            // A Tab queued for the draft is lost to the widget that has the keyboard.
            self.draft_tabs.clear();
            return;
        }
        let Some(session) = self.session.as_mut() else {
            return;
        };
        for back in std::mem::take(&mut self.draft_tabs) {
            self.chord.disarm();
            session.key(if back { EditKey::BackTab } else { EditKey::Tab });
        }
        let (events, shortcut_held) =
            ctx.input(|i| (i.events.clone(), i.modifiers.command || i.modifiers.ctrl));
        for event in events {
            match event {
                Event::Text(text) if !shortcut_held => {
                    for c in text.chars() {
                        // After Ctrl+G, a letter types its Greek letter.
                        session.type_char(self.chord.type_for(c));
                    }
                }
                // egui turns Ctrl+C, Ctrl+X and Ctrl+V into these events; the text goes to the
                // system clipboard, where another Mathcraft window can read it back.
                Event::Copy => {
                    if let Some(text) = session.copy_selection() {
                        ctx.copy_text(text);
                    }
                }
                Event::Cut => {
                    if let Some(text) = session.cut_selection() {
                        ctx.copy_text(text);
                    }
                }
                Event::Paste(text) => session.paste(&text),
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if let Some(key) = edit_key_with_modifiers(key, modifiers.shift, modifiers.ctrl)
                    {
                        // Editing keys end the Ctrl+G chord; letters come with their Text event.
                        self.chord.disarm();
                        session.key(key);
                    }
                }
                _ => {}
            }
        }
    }

    /// Saves the document to `target`, or to its own file when `target` is `None`. A draft is
    /// committed first. Returns true when the file was written. A failure keeps the document as
    /// it is and says why in the notice; the file on disk is not touched.
    fn save_document(&mut self, target: Option<PathBuf>) -> bool {
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        if let Err(message) = session.commit() {
            self.notice = Some(message);
            return false;
        }
        let Some(path) = target.or_else(|| self.file.path().map(Path::to_path_buf)) else {
            self.notice = Some(tr("app.no_file_save").to_string());
            return false;
        };
        match self.writer.write(session.document(), &path) {
            Ok(warnings) => {
                let revision = session.revision();
                let name = file_name(&path);
                self.remember(&path);
                self.file.mark_saved(path, revision);
                self.name = Some(name.clone());
                let mut notice = trf("app.saved", &[&name]);
                // The first warning is shown; the rest are counted, so a large sheet keeps the line short.
                if let Some(first) = warnings.first() {
                    notice = format!("{notice} {}", trf("app.save_warning", &[first]));
                    if warnings.len() > 1 {
                        notice = format!(
                            "{notice} {}",
                            trf(
                                "app.save_warning_more",
                                &[&(warnings.len() - 1).to_string()]
                            )
                        );
                    }
                }
                self.notice = Some(notice);
                true
            }
            Err(message) => {
                self.notice = Some(message);
                false
            }
        }
    }

    /// Runs a file button or a path box action.
    fn file_action(&mut self, ctx: &egui::Context, action: FileAction) {
        match action {
            FileAction::New => self.request(ctx, Pending::New),
            FileAction::Save => self.save_or_ask(),
            FileAction::Open(text) => self.request(ctx, Pending::Open(expand_home(&text))),
            FileAction::SaveAs(text) => {
                self.save_document(Some(expand_home(&text)));
            }
            FileAction::BrowseOpen => {
                if let Some(path) = file_picker::pick_worksheet() {
                    self.request(ctx, Pending::Open(path));
                }
            }
            FileAction::BrowseSaveAs => {
                if let Some(path) = file_picker::save_worksheet(self.file.path()) {
                    self.save_document(Some(path));
                }
            }
            FileAction::ExportPdf => {
                if let Some(path) = file_picker::save_pdf(self.file.path()) {
                    self.export_pdf(&path);
                }
            }
            FileAction::Print => self.print(),
            FileAction::PageSetup => self.open_page_setup(),
        }
    }

    /// The page settings of the open document, or the default when none is open.
    fn page_settings(&self) -> PageSettings {
        self.session
            .as_ref()
            .map(|s| s.document().page().clone())
            .unwrap_or_default()
    }

    /// The page layout for the view, when the page layout is on.
    fn page_geometry(&self, vm: &ViewModel) -> Option<Geometry> {
        page_geometry_for(self.display.page_layout, &self.page_settings(), vm)
    }

    /// Writes the worksheet's pages to `path` as a PDF and reports the result.
    fn export_pdf(&mut self, path: &Path) {
        let Some(vm) = self.view.as_ref() else {
            self.notice = Some(tr("app.open_before_export").to_string());
            return;
        };
        let title = self
            .name
            .clone()
            .unwrap_or_else(|| tr("app.worksheet").to_string());
        match export::export(vm, &self.page_settings(), self.show_prime, &title) {
            Ok(result) => match crate::edit::save::write_atomic(path, &result.pdf) {
                Ok(()) => {
                    let mut note = trf(
                        "app.exported",
                        &[&result.pages.to_string(), &path.display().to_string()],
                    );
                    if result.capped {
                        note.push_str(tr("app.page_limit"));
                    }
                    for skipped in &result.skipped {
                        note.push_str(&format!(" {skipped}."));
                    }
                    self.notice = Some(note);
                }
                Err(message) => self.notice = Some(message),
            },
            Err(message) => {
                self.notice = Some(trf("app.export_failed", &[&localize_error(&message)]))
            }
        }
    }

    /// Exports to a temporary PDF and opens it with the system's default viewer, which prints it.
    /// The temporary file gets a unique name (see [`export::write_print_file`]).
    fn print(&mut self) {
        let raw_stem = self
            .name
            .as_deref()
            .and_then(|n| Path::new(n).file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("worksheet")
            .to_string();
        let Some(vm) = self.view.as_ref() else {
            self.notice = Some(tr("app.open_before_print").to_string());
            return;
        };
        let result = match export::export(vm, &self.page_settings(), self.show_prime, &raw_stem) {
            Ok(result) => result,
            Err(message) => {
                self.notice = Some(trf("app.print_failed", &[&localize_error(&message)]));
                return;
            }
        };
        let path = match export::write_print_file(&export::safe_stem(&raw_stem), &result.pdf) {
            Ok(path) => path,
            Err(err) => {
                self.notice = Some(trf(
                    "app.cannot_write",
                    &[&std::env::temp_dir().display().to_string(), &err],
                ));
                return;
            }
        };
        self.notice = Some(match export::open_with_system_viewer(&path) {
            Ok(()) => trf("app.opened_viewer", &[&result.pages.to_string()]),
            Err(err) => trf(
                "app.print_needs_viewer",
                &[&err.to_string(), &path.display().to_string()],
            ),
        });
    }

    /// Opens the page setup window with the document's current settings.
    fn open_page_setup(&mut self) {
        if self.session.is_none() {
            self.notice = Some(tr("tip.no_worksheet").to_string());
            return;
        }
        self.page_setup = Some(self.page_settings());
    }

    /// Opens the Link window (screenshot 0222) with the selected text as its text.
    fn open_link_window(&mut self) {
        let Some(view) = self.session.as_ref().and_then(|s| s.draft_view()) else {
            return;
        };
        if view.math {
            return;
        }
        let text = match view.selection {
            Some((start, end)) => view.text.chars().skip(start).take(end - start).collect(),
            None => String::new(),
        };
        self.link_window = Some(LinkDraft {
            text,
            address: String::new(),
        });
    }

    /// The Link window: OK makes the hyperlink run in the text being typed (display only).
    fn link_dialog(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.link_window.as_mut() else {
            return;
        };
        match link_window::show(ctx, draft) {
            None => {}
            Some(false) => self.link_window = None,
            Some(true) => {
                if let Some(draft) = self.link_window.take() {
                    if let Some(session) = self.session.as_mut() {
                        session.insert_link(&draft.text, &draft.address);
                    }
                }
            }
        }
    }

    /// Opens the Results format window for the worksheet, or for the selected region when one is
    /// selected. The draft starts from the format the target shows now.
    fn open_result_format(&mut self) {
        let Some(session) = self.session.as_ref() else {
            self.notice = Some(tr("tip.no_worksheet").to_string());
            return;
        };
        let doc = session.document();
        let global = doc.settings().result_format.clone().unwrap_or_default();
        // The region is the selected one, else the region being edited in the draft (a result
        // format applies to a region that is typed in as well as one that is selected).
        let region = session
            .selection()
            .or_else(|| session.draft_view().and_then(|d| d.target));
        let found = region.and_then(|id| doc.region(id));
        let math = found.is_some_and(|r| matches!(r.kind, RegionKind::Math(_)));
        let draft = found
            .and_then(|r| r.result_format.clone())
            .unwrap_or_else(|| global.clone());
        let matrix = found.map(|r| r.matrix).unwrap_or_default();
        self.result_dialog = Some(ResultDialog {
            region,
            math,
            draft,
            base: global,
            matrix,
            base_matrix: matrix,
        });
    }

    /// Changes the worksheet's calculation settings (ORIGIN, TOL, CTOL). Marks it modified.
    fn change_settings(&mut self, change: impl FnOnce(&mut Settings)) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let mut settings = session.document().settings().clone();
        change(&mut settings);
        if &settings != session.document().settings() {
            session.set_settings(settings);
        }
    }

    /// Changes the worksheet's page setup (paper, orientation, margins, grid). Marks it modified.
    fn change_page(&mut self, change: impl FnOnce(&mut PageSettings)) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let mut page = session.document().page().clone();
        change(&mut page);
        if &page != session.document().page() {
            session.set_page(page);
        }
    }

    /// The ids of the regions whose result is an error, in sheet order (top to bottom).
    fn error_regions(&self) -> Vec<u32> {
        let Some(vm) = self.view.as_ref() else {
            return Vec::new();
        };
        let mut found: Vec<(f32, f32, u32)> = vm
            .items
            .iter()
            .filter(|item| {
                matches!(
                    &item.body,
                    Body::Math { result: Some(r), .. } if r.tone == Tone::Problem
                )
            })
            .map(|item| (item.rect[1], item.rect[0], item.region_id))
            .collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        found.into_iter().map(|(_, _, id)| id).collect()
    }

    /// Makes the search matches again from the worksheet (after a change or a new query).
    fn refresh_search(&mut self) {
        match self.session.as_ref() {
            Some(session) => self.search.refresh_with(session.revision(), |query, opts| {
                session.find_matches(query, opts)
            }),
            None => {
                self.search.matches.clear();
                self.search.current = None;
            }
        }
    }

    /// Carries out a search action of the status bar.
    fn search_action(&mut self, action: SearchAction) {
        match action {
            SearchAction::Next => self.search_step(true),
            SearchAction::Previous => self.search_step(false),
            SearchAction::ReplaceOne => self.search_replace_one(),
            SearchAction::ReplaceAll => self.search_replace_all(),
        }
    }

    /// Goes to the next or previous match, wrapping at the ends.
    fn search_step(&mut self, forward: bool) {
        let len = self.search.matches.len();
        if let Some(index) = step_index(len, self.search.current, forward) {
            self.search_goto(index);
        }
    }

    /// Shows match `index`: a text match is selected in its text, any other region is selected,
    /// and the region is scrolled into view.
    fn search_goto(&mut self, index: usize) {
        let Some(hit) = self.search.matches.get(index).cloned() else {
            return;
        };
        self.search.current = Some(index);
        let range = (hit.kind == MatchKind::Text).then(|| hit.range.clone());
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if session.reveal_match(hit.region_id, range) {
            self.search.scroll_to = Some(hit.region_id);
        }
    }

    /// Replaces the current match and goes on to the next one. Without a current match this is a
    /// step to the first.
    fn search_replace_one(&mut self) {
        let Some(index) = self.search.current else {
            self.search_step(true);
            return;
        };
        let Some(hit) = self.search.matches.get(index).cloned() else {
            return;
        };
        let replacement = self.search.replace.clone();
        let Some(session) = self.session.as_mut() else {
            return;
        };
        // A refused replacement leaves its message on the status line (the session's message).
        if session.replace_match(&hit, &replacement).is_err() {
            return;
        }
        self.refresh_search();
        let len = self.search.matches.len();
        if len > 0 {
            // The replaced match is gone, so the next one has its index; past the end it wraps.
            self.search_goto(if index < len { index } else { 0 });
        } else {
            self.search.current = None;
        }
    }

    /// Replaces every match as one undo step and reports how many, and how many regions were left.
    fn search_replace_all(&mut self) {
        let query = self.search.find.clone();
        let replacement = self.search.replace.clone();
        let opts = self.search.doc_options();
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let text = match session.replace_all_matches(&query, &replacement, opts, "Replace all") {
            Ok(report) => {
                let mut text = trf("status.replaced_count", &[&report.replaced.to_string()]);
                if !report.refused.is_empty() {
                    text.push(' ');
                    text.push_str(&trf(
                        "status.refused_count",
                        &[&report.refused.len().to_string()],
                    ));
                }
                text
            }
            Err(text) => text,
        };
        self.notice = Some(text);
        self.search.current = None;
        self.refresh_search();
    }

    /// The error trace: selects the first, previous, next or last region that shows an error.
    fn error_trace(&mut self, step: ribbon::Trace) {
        let errors = self.error_regions();
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if errors.is_empty() {
            self.notice = Some(tr("app.no_errors").to_string());
            return;
        }
        let current = session
            .selection()
            .and_then(|id| errors.iter().position(|e| *e == id));
        let index = match (step, current) {
            (ribbon::Trace::First, _) => 0,
            (ribbon::Trace::Last, _) => errors.len() - 1,
            (ribbon::Trace::Next, Some(i)) => (i + 1).min(errors.len() - 1),
            (ribbon::Trace::Next, None) => 0,
            (ribbon::Trace::Previous, Some(i)) => i.saturating_sub(1),
            (ribbon::Trace::Previous, None) => errors.len() - 1,
        };
        session.select_region(errors[index]);
    }

    /// Opens the Einheitensystem window with the worksheet's unit system.
    fn open_units_dialog(&mut self) {
        let Some(session) = self.session.as_ref() else {
            self.notice = Some(tr("tip.no_worksheet").to_string());
            return;
        };
        let system = session
            .document()
            .settings()
            .unit_system
            .as_deref()
            .and_then(UnitSystem::from_attr)
            .unwrap_or_default();
        self.units_dialog = Some(UnitsDialog::new(system));
    }

    /// The Einheitensystem window. OK stores the system. Cancel stores nothing but, like Prime
    /// (screenshot 0184), still marks the worksheet as modified.
    fn units_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.units_dialog.as_mut() else {
            return;
        };
        let outcome = units_dialog::show(ctx, dialog);
        let Some(session) = self.session.as_mut() else {
            self.units_dialog = None;
            return;
        };
        match outcome {
            units_dialog::Outcome::Open => {}
            units_dialog::Outcome::Ok(system) => {
                let mut settings = session.document().settings().clone();
                settings.unit_system = Some(system.attr().to_owned());
                session.set_settings(settings);
                self.units_dialog = None;
            }
            units_dialog::Outcome::Cancel => {
                let settings = session.document().settings().clone();
                session.set_settings(settings);
                self.units_dialog = None;
            }
        }
    }

    /// The window of a Resources button: the web address or the help topic, and a copy button.
    /// Mathcraft never opens a browser.
    fn resource_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(resource) = self.resource_dialog else {
            return;
        };
        let mut close = false;
        egui::Window::new(tr("dlg.resource"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(RichText::new(resource.title.get()).strong());
                let target = match resource.target {
                    ribbon::resources::Target::Web(url) => {
                        ui.label(tr("res.target_web"));
                        ui.label(RichText::new(url).monospace());
                        ui.label(RichText::new(tr("res.unverified")).small().weak());
                        url.to_owned()
                    }
                    ribbon::resources::Target::Help(topic) => {
                        ui.label(tr("res.target_help"));
                        ui.label(RichText::new(topic.get()).monospace());
                        topic.get().to_owned()
                    }
                };
                ui.horizontal(|ui| {
                    if ui.button(tr("btn.copy_address")).clicked() {
                        ctx.copy_text(target);
                    }
                    if ui.button(tr("btn.close")).clicked() {
                        close = true;
                    }
                });
            });
        if close {
            self.resource_dialog = None;
        }
    }

    /// Opens the Worksheet options window with the document's settings.
    fn open_worksheet_options(&mut self) {
        let Some(session) = self.session.as_ref() else {
            self.notice = Some(tr("tip.no_worksheet").to_string());
            return;
        };
        self.options_dialog = Some(OptionsDraft::from_settings(session.document().settings()));
    }

    /// The Results format window. Apply stores the draft (refused with a notice when the file
    /// cannot hold it); the window closes on success.
    fn result_format_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.result_dialog.as_mut() else {
            return;
        };
        let choice = settings_window::result_format_window(
            ctx,
            &mut dialog.draft,
            &mut dialog.matrix,
            dialog.region.is_some(),
            dialog.math,
        );
        let draft = dialog.draft.clone();
        let matrix = dialog.matrix;
        let base_matrix = dialog.base_matrix;
        let region = dialog.region;
        let base = dialog.base.clone();
        let Some(session) = self.session.as_mut() else {
            self.result_dialog = None;
            return;
        };
        match choice {
            WindowChoice::Open => {}
            WindowChoice::Cancel => self.result_dialog = None,
            WindowChoice::ApplyWorksheet => {
                // The matrix display is stored per region, so a worksheet apply cannot keep it.
                if matrix != base_matrix {
                    self.notice = Some(crate::i18n::tr("res.matrix_needs_region").to_string());
                    return;
                }
                if let Some(reason) = settings_window::unstorable(&draft, &base) {
                    self.notice = Some(reason.to_string());
                    return;
                }
                let mut settings = session.document().settings().clone();
                settings.result_format = Some(draft);
                session.set_settings(settings);
                self.result_dialog = None;
            }
            WindowChoice::ApplyRegion => {
                let Some(id) = region else {
                    return;
                };
                let current = session
                    .document()
                    .region(id)
                    .and_then(|r| r.result_format.clone())
                    .unwrap_or_else(|| base.clone());
                if let Some(reason) = settings_window::unstorable(&draft, &current) {
                    self.notice = Some(reason.to_string());
                    return;
                }
                session.set_region_format(id, Some(draft));
                if matrix != base_matrix {
                    session.set_region_matrix(id, matrix);
                }
                self.result_dialog = None;
            }
        }
    }

    /// The Worksheet options window. OK writes the changed values; a refused value keeps the
    /// window open with a notice.
    fn options_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.options_dialog.as_mut() else {
            return;
        };
        match settings_window::worksheet_options_window(ctx, draft) {
            WindowChoice::Open => {}
            WindowChoice::Cancel => self.options_dialog = None,
            WindowChoice::ApplyWorksheet | WindowChoice::ApplyRegion => {
                let draft = draft.clone();
                let Some(session) = self.session.as_mut() else {
                    self.options_dialog = None;
                    return;
                };
                match draft.applied_to(session.document().settings()) {
                    Ok(settings) => {
                        session.set_settings(settings);
                        self.options_dialog = None;
                    }
                    Err(reason) => self.notice = Some(reason.to_string()),
                }
            }
        }
    }

    /// The File access window: the two switches, and the confirmation the first time each one is
    /// enabled in this session. The state is in [`FileAccess`]; this only draws it.
    fn file_access_window(&mut self, ctx: &egui::Context) {
        if !self.file_access_open {
            return;
        }
        let mut read = self.file_access.is_on(Switch::Read);
        let mut write = self.file_access.is_on(Switch::Write);
        let mut requests: Vec<(Switch, bool)> = Vec::new();
        let mut close = false;
        egui::Window::new(tr("dlg.file_access"))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(tr("file.intro"));
                if ui.checkbox(&mut read, tr("file.allow_read")).changed() {
                    requests.push((Switch::Read, read));
                }
                if ui.checkbox(&mut write, tr("file.allow_write")).changed() {
                    requests.push((Switch::Write, write));
                }
                if self.file_access.writes_wait_for_calculate(false) {
                    ui.label(tr("file.waits"));
                }
                ui.separator();
                if ui.button(tr("btn.close")).clicked() {
                    close = true;
                }
            });
        for (switch, on) in requests {
            self.file_access.request(switch, on);
        }
        if close {
            self.file_access_open = false;
        }
        let Some(pending) = self.file_access.pending() else {
            return;
        };
        let (title, body) = match pending {
            Switch::Read => ("file.confirm_read_title", "file.confirm_read_body"),
            Switch::Write => ("file.confirm_write_title", "file.confirm_write_body"),
        };
        egui::Window::new(tr(title))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(tr(body));
                ui.horizontal(|ui| {
                    if ui.button(tr("btn.ok")).clicked() {
                        self.file_access.confirm();
                    }
                    if ui.button(tr("btn.cancel")).clicked() {
                        self.file_access.cancel();
                    }
                });
            });
    }

    /// The page setup window: paper, orientation and margin type. OK applies them to the document.
    fn page_setup_window(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.page_setup.as_mut() else {
            return;
        };
        let mut apply = false;
        let mut close = false;
        egui::Window::new(tr("dlg.page_setup"))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("page_setup_grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label(tr("page.paper"));
                        egui::ComboBox::from_id_salt("page_setup_paper")
                            .selected_text(draft.paper_code.clone())
                            .show_ui(ui, |ui| {
                                for code in known_paper_codes() {
                                    ui.selectable_value(
                                        &mut draft.paper_code,
                                        code.to_owned(),
                                        code,
                                    );
                                }
                            });
                        ui.end_row();
                        ui.label(tr("page.orientation"));
                        ui.horizontal(|ui| {
                            ui.radio_value(
                                &mut draft.orientation,
                                "Portrait".to_owned(),
                                tr("page.portrait"),
                            );
                            ui.radio_value(
                                &mut draft.orientation,
                                "Landscape".to_owned(),
                                tr("page.landscape"),
                            );
                        });
                        ui.end_row();
                        ui.label(tr("page.margins"));
                        egui::ComboBox::from_id_salt("page_setup_margins")
                            .selected_text(draft.margin_type.clone())
                            .show_ui(ui, |ui| {
                                for name in ["Narrow", "Standard", "Legacy", "Wide"] {
                                    ui.selectable_value(
                                        &mut draft.margin_type,
                                        name.to_owned(),
                                        term(name),
                                    );
                                }
                            });
                        ui.end_row();
                        ui.label(tr("page.grid"));
                        egui::ComboBox::from_id_salt("page_setup_grid")
                            .selected_text(grid_label(&draft.grid_size))
                            .show_ui(ui, |ui| {
                                for name in ["Standard", "Fine"] {
                                    ui.selectable_value(
                                        &mut draft.grid_size,
                                        name.to_owned(),
                                        grid_label(name),
                                    );
                                }
                            });
                        ui.end_row();
                    });
                let m = Margins::preset(&draft.margin_type);
                ui.label(
                    RichText::new(trf(
                        "page.margins_note",
                        &[&(m.top * 25.4 / 72.0).round().to_string()],
                    ))
                    .small(),
                );
                ui.horizontal(|ui| {
                    if ui.button(tr("btn.ok")).clicked() {
                        apply = true;
                    }
                    if ui.button(tr("btn.cancel")).clicked() {
                        close = true;
                    }
                });
            });
        if apply {
            let page = draft.clone();
            if let Some(session) = self.session.as_mut() {
                session.set_page(page);
            }
            self.page_setup = None;
        } else if close {
            self.page_setup = None;
        }
    }

    /// Opens the plot window for the plot region `id`.
    fn open_plot_dialog(&mut self, id: u32) {
        self.plot_dialog = self
            .session
            .as_ref()
            .and_then(|s| s.plot_spec(id))
            .map(|spec| PlotDialog::new(id, spec));
    }

    /// Draws the plot window and applies its choice to the session.
    fn plot_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.plot_dialog.as_mut() else {
            return;
        };
        match dialog.show(ctx) {
            Some(PlotChoice::Cancel) => self.plot_dialog = None,
            Some(PlotChoice::Apply) => {
                let (id, spec) = (dialog.id, dialog.spec.clone());
                let result = match self.session.as_mut() {
                    Some(session) => session.set_plot_spec(id, &spec),
                    None => Ok(()),
                };
                match result {
                    Ok(()) => self.plot_dialog = None,
                    Err(message) => dialog.error = Some(message),
                }
            }
            None => {}
        }
    }

    /// Switches the display language and saves the choice. A save error is reported in the
    /// notice; the language change itself holds for this session.
    fn set_language(&mut self, lang: Lang) {
        i18n::set_language(lang);
        // Results are text built with the separators of the language: evaluate again to show them.
        self.recalc_requested = true;
        if let Err(err) = config::save(&config::Config {
            language: Some(lang),
            live: config::load().live,
        }) {
            self.notice = Some(trf("app.config_error", &[&err.to_string()]));
        }
    }

    /// Runs a ribbon button on the session.
    fn ribbon_action(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Dialog(dialog) => {
                self.dialogs.open(dialog);
                return;
            }
            Action::InsertImage => {
                self.insert_image_dialog();
                return;
            }
            Action::EditPlot => {
                if let Some(session) = self.session.as_mut() {
                    if let Some(id) = session.selected_plot() {
                        self.open_plot_dialog(id);
                    }
                }
                return;
            }
            Action::Calculate => {
                self.recalc_requested = true;
                return;
            }
            Action::FileAccess => {
                self.file_access_open = true;
                return;
            }
            Action::TextLink => {
                self.open_link_window();
                return;
            }
            Action::ToggleCalcMode => {
                self.calc_mode = self.calc_mode.toggled();
                self.notice = Some(trf("app.calc_mode_notice", &[self.calc_mode.label()]));
                return;
            }
            Action::ToggleGrid => {
                self.display.grid = !self.display.grid;
                return;
            }
            Action::ToggleRegionBorders => {
                self.display.region_borders = !self.display.region_borders;
                return;
            }
            Action::TogglePageLayout => {
                self.display.page_layout = !self.display.page_layout;
                return;
            }
            Action::Zoom(step) => {
                self.zoom_pct = match step {
                    ZoomStep::In => zoom_in(self.zoom_pct),
                    ZoomStep::Out => zoom_out(self.zoom_pct),
                    ZoomStep::Reset => ZOOM_RESET,
                };
                return;
            }
            Action::SetPageView(page) => {
                self.display.page_layout = page;
                return;
            }
            Action::Paste => {
                // The system answers with a paste event, which the key handler applies.
                ctx.send_viewport_cmd(ViewportCommand::RequestPaste);
                return;
            }
            Action::StopCalc => {
                if self.reeval.busy() {
                    self.reeval.cancel();
                    // A cancelled refresh leaves the view stale; ask again on the next change.
                    self.view_revision = 0;
                    self.notice = Some(tr("app.calc_stopped").to_string());
                } else {
                    self.notice = Some(tr("app.no_calc_running").to_string());
                }
                return;
            }
            Action::Resource(id) => {
                self.resource_dialog = ribbon::resources::find(id);
                return;
            }
            Action::ErrorTrace(step) => {
                self.error_trace(step);
                return;
            }
            Action::UnitSystem => {
                self.open_units_dialog();
                return;
            }
            Action::PageSetup => {
                self.open_page_setup();
                return;
            }
            _ => {}
        }
        let Some(session) = self.session.as_mut() else {
            self.notice = Some(tr("tip.no_worksheet").to_string());
            return;
        };
        match action {
            Action::InsertMath => {
                session.start_math();
            }
            Action::InsertText | Action::InsertTextField => {
                session.start_text();
            }
            Action::InsertSolveBlock => {
                session.insert_solve_block_at_crosshair();
            }
            Action::ToggleDisabled => {
                if let Some(off) = session.toggle_disabled() {
                    self.notice = Some(
                        tr(if off {
                            "app.region_disabled"
                        } else {
                            "app.region_enabled"
                        })
                        .to_string(),
                    );
                }
            }
            Action::EditFurniture(footer) => {
                session.start_furniture(footer);
            }
            Action::InsertField(field) => {
                // Outside a header or footer, a field goes into the footer (opened on the way).
                if !session.draft_view().is_some_and(|d| d.furniture) {
                    session.start_furniture(true);
                }
                session.insert_field(field);
            }
            Action::RemoveField(prefix) => session.remove_field(prefix),
            Action::SetOrigin(v) => self.change_settings(|s| s.array_origin = Some(v)),
            Action::SetTol(exp) => {
                self.change_settings(|s| s.convergence_tolerance = Some(10f64.powi(exp)));
            }
            Action::SetCtol(exp) => {
                self.change_settings(|s| s.constraint_tolerance = Some(10f64.powi(exp)));
            }
            Action::SetPaper(code) => self.change_page(|p| p.paper_code = code.to_owned()),
            Action::SetLandscape(on) => self.change_page(|p| {
                p.orientation = if on { "Landscape" } else { "Portrait" }.to_owned();
            }),
            Action::SetMargins(kind) => self.change_page(|p| p.margin_type = kind.to_owned()),
            Action::SetFineGrid(fine) => {
                self.change_page(|p| {
                    p.grid_size = if fine { "Fine" } else { "Standard" }.to_owned()
                });
            }
            Action::SeparateRegions(vertical) => session.separate_selected(vertical),
            Action::InsertBlankPage => session.insert_blank_page(),
            Action::RemoveLines => session.remove_lines(INSERT_LINES_POINTS),
            Action::RemoveEmptyArea => session.remove_lines(f64::INFINITY),
            Action::InsertPageBreak => session.insert_page_break(),
            Action::DeletePageBreak => session.delete_page_break(),
            Action::InsertArea => {
                session.insert_area_at_crosshair();
            }
            Action::InsertPlot(polar) => {
                session.insert_plot_at_crosshair(polar);
            }
            Action::AreaAroundSelection => {
                session.area_around_selection();
            }
            Action::Template(template) => session.insert_template(template),
            Action::TypeText(text) => session.insert_text(text),
            Action::InsertFunction(name) => session.insert_text(&format!("{name}(")),
            Action::DeleteSelected => session.delete_selected(),
            Action::Cut => {
                if let Some(text) = session.cut_selection() {
                    ctx.copy_text(text);
                }
            }
            Action::TextFormat(format) => apply_text_format(session, format),
            // Handled before the session is borrowed (it opens the Link window).
            Action::TextLink => {}
            Action::Copy => {
                if let Some(text) = session.copy_selection() {
                    ctx.copy_text(text);
                }
            }
            Action::MatrixOp(op) => session.matrix_op(op),
            Action::InsertMatrix(rows, cols) => session.insert_matrix(rows, cols),
            Action::ToggleMatrixIndices => {
                session.toggle_matrix_flag(true);
            }
            Action::ToggleMatrixNested => {
                session.toggle_matrix_flag(false);
            }
            Action::PlotOp(op) => {
                session.plot_op(op);
            }
            Action::Arrange(how) => session.arrange(how),
            Action::InsertLines => session.insert_lines(INSERT_LINES_POINTS),
            Action::ResultFormat => self.open_result_format(),
            Action::SetLanguage(lang) => self.set_language(lang),
            Action::WorksheetOptions => self.open_worksheet_options(),
            // Handled above.
            Action::Dialog(_)
            | Action::EditPlot
            | Action::InsertImage
            | Action::Calculate
            | Action::FileAccess
            | Action::ToggleCalcMode
            | Action::ToggleGrid
            | Action::ToggleRegionBorders
            | Action::TogglePageLayout
            | Action::SetPageView(_)
            | Action::Paste
            | Action::StopCalc
            | Action::Resource(_)
            | Action::ErrorTrace(_)
            | Action::UnitSystem
            | Action::PageSetup
            | Action::Zoom(_) => {}
        }
    }

    /// Asks for a PNG or JPEG file and inserts it at the insertion point. The insertion point is
    /// checked first, so no file is picked when there is nowhere to put it.
    fn insert_image_dialog(&mut self) {
        let Some(session) = self.session.as_ref() else {
            self.notice = Some(tr("tip.no_worksheet").to_string());
            return;
        };
        if session.crosshair().is_none() {
            self.notice = Some(tr("edit.click_first").to_string());
            return;
        }
        let Some(path) = file_picker::pick_image() else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.notice = Some(trf("app.image_read_failed", &[&err.to_string()]));
                return;
            }
        };
        if let Some(session) = self.session.as_mut() {
            if let Err(message) = session.insert_image_at_crosshair(bytes) {
                self.notice = Some(message);
            }
        }
    }

    /// Puts what a dialog picked into the math draft.
    fn apply_insertion(&mut self, insertion: Insertion) {
        let Some(session) = self.session.as_mut() else {
            self.notice = Some(tr("tip.no_worksheet").to_string());
            return;
        };
        match insertion {
            Insertion::Text(text) => session.insert_text(&text),
            Insertion::Template(template) => session.insert_template(template),
            Insertion::Matrix { rows, cols } => session.insert_matrix(rows, cols),
        }
    }

    /// Asks before closing the window while there are unsaved edits. The close is cancelled and
    /// the dialog asks; the dialog closes the window when the user chooses Save or Discard.
    fn guard_close(&mut self, ctx: &egui::Context) {
        let requested = ctx.input(|i| i.viewport().close_requested());
        self.on_close_request(ctx, requested);
    }

    /// Handles a close request of the window (`requested`). Returns true when the close goes ahead.
    /// Clean worksheets close at once; with unsaved ones the close is cancelled and the quit flow
    /// starts (or restarts) from the top.
    pub(crate) fn on_close_request(&mut self, ctx: &egui::Context, requested: bool) -> bool {
        if !requested || self.allow_close || self.first_dirty().is_none() {
            return true;
        }
        ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        self.run(ctx, Pending::Close);
        false
    }

    /// The unsaved-changes dialog, shown while an action waits for the user's answer.
    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.confirm.clone() else {
            return;
        };
        let name = self
            .name
            .clone()
            .unwrap_or_else(|| tr("app.worksheet").to_string());
        let mut choice = None;
        egui::Window::new(tr("dlg.unsaved"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(trf("save.prompt", &[&name, &action.describe()]));
                if let Some(note) = &self.notice {
                    ui.label(RichText::new(note).color(theme::PROBLEM));
                }
                ui.horizontal(|ui| {
                    if ui.button(tr("btn.save")).clicked() {
                        choice = Some(Choice::Save);
                    }
                    if ui.button(tr("btn.discard")).clicked() {
                        choice = Some(Choice::Discard);
                    }
                    if ui.button(tr("btn.cancel")).clicked() {
                        choice = Some(Choice::Cancel);
                    }
                });
            });
        if let Some(choice) = choice {
            self.answer_confirm(ctx, choice);
        }
    }

    /// Applies the answer to the open question. Save saves first and only goes on when the save
    /// worked. Discard and Save both finish the asked worksheet, so for a window close it leaves
    /// the tab strip before the next unsaved worksheet is asked about.
    pub(crate) fn answer_confirm(&mut self, ctx: &egui::Context, choice: Choice) {
        let Some(action) = self.confirm.clone() else {
            return;
        };
        match choice {
            Choice::Save => {
                let saved = if self.file.path().is_some() {
                    self.save_document(None)
                } else {
                    match file_picker::save_worksheet(None) {
                        Some(path) => self.save_document(Some(path)),
                        None => false,
                    }
                };
                if saved {
                    self.confirm = None;
                    self.finish_confirmed(ctx, action);
                }
            }
            Choice::Discard => {
                self.confirm = None;
                self.finish_confirmed(ctx, action);
            }
            Choice::Cancel => {
                // The quit is abandoned; the next quit or close request starts it again.
                self.confirm = None;
                self.allow_close = false;
            }
        }
    }

    /// Runs the action after its confirmation. A window close always closes the asked worksheet
    /// first (it is resolved now, saved or discarded), even when it is the only one, so the next
    /// step looks at the remaining worksheets only.
    fn finish_confirmed(&mut self, ctx: &egui::Context, action: Pending) {
        if action == Pending::Close {
            self.close_active();
        }
        self.run(ctx, action);
    }

    /// The window title: the program, its version and the active worksheet. Prime puts the unsaved
    /// marker on the tab, not in the title.
    fn window_title(&self) -> String {
        let version = env!("CARGO_PKG_VERSION");
        match &self.name {
            Some(name) => trf("app.title", &[version, &menus::display_name(name)]),
            None => format!("Mathcraft {version}"),
        }
    }

    fn sync_title(&mut self, ctx: &egui::Context) {
        let title = self.window_title();
        if title != self.title_sent {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title_sent = title;
        }
    }

    /// The name of the file being loaded, for the spinner text.
    fn loading_name(&self) -> String {
        self.loading
            .as_deref()
            .map_or_else(|| tr("app.the_file").to_string(), file_name)
    }

    /// The spinner, its text and a Cancel button, floating over the page while a load runs.
    fn loading_overlay(&self, ctx: &egui::Context) {
        let name = self.loading_name();
        egui::Area::new(egui::Id::new("mathcraft-loading"))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 40.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(trf("app.loading", &[&name]));
                        if ui.button(tr("btn.cancel")).clicked() {
                            self.loader.cancel();
                        }
                    });
                });
            });
    }

    /// The spinner and Cancel button of a re-evaluation that runs over the page.
    fn evaluating_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(tr("app.evaluating"));
            if ui.button(tr("btn.cancel")).clicked() {
                self.reeval.cancel();
            }
        });
    }

    fn central(&mut self, ui: &mut egui::Ui) {
        let zoom = self.zoom_pct / 100.0;
        if self.reeval.busy() {
            self.evaluating_bar(ui);
        }
        if self.loader.busy() {
            match &self.view {
                Some(vm) => {
                    let overlay = Overlay {
                        hide_grid: !self.display.grid,
                        grid_pt: canvas::grid_points(&self.page_settings()),
                        region_borders: self.display.region_borders,
                        pages: self.page_geometry(vm),
                        ..Overlay::default()
                    };
                    self.page = canvas::show(ui, vm, zoom, self.show_prime, &overlay).page;
                    self.loading_overlay(ui.ctx());
                }
                None => {
                    let name = self.loading_name();
                    ui.add_space(24.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(trf("app.loading", &[&name]));
                        if ui.button(tr("btn.cancel")).clicked() {
                            self.loader.cancel();
                        }
                    });
                }
            }
            return;
        }
        if let Some(message) = &self.error {
            ui.add_space(12.0);
            ui.colored_label(
                theme::PROBLEM,
                trf("app.cannot_open_file", &[&localize_error(message)]),
            );
            ui.label(tr("app.choose_other_file"));
            return;
        }
        let page_layout = self.display.page_layout;
        let page_model = self.page_settings();
        let (Some(vm), Some(session)) = (self.view.as_ref(), self.session.as_mut()) else {
            ui.add_space(24.0);
            ui.heading("Mathcraft");
            ui.label(tr("app.open_hint"));
            ui.label(tr("app.drop_hint"));
            return;
        };
        // The session keeps stored sheet positions; the canvas draws the laid-out sheet, where the
        // page breaks have moved the content below them.
        let draft = session.draft_view().map(|mut d| {
            d.at[1] = f64::from(vm.to_display(d.at[1] as f32));
            d
        });
        let crosshair = session
            .crosshair()
            .map(|p| [p[0], f64::from(vm.to_display(p[1] as f32))]);
        // The word a double click chose is selected in the draft drawn this frame.
        let pending_word = session
            .take_word_point()
            .map(|p| [p[0], f64::from(vm.to_display(p[1] as f32))]);
        let (flash, flashing) = match (self.live.as_mut(), session.live_number()) {
            (Some(rt), n) => rt.flashing(n),
            _ => (Vec::new(), false),
        };
        if flashing {
            ui.ctx().request_repaint();
        }
        // The search hits are outlined; the current one stronger. A step asks for one hit in view.
        let hits: Vec<(u32, bool)> = self
            .search
            .matches
            .iter()
            .enumerate()
            .map(|(i, m)| (m.region_id, self.search.current == Some(i)))
            .collect();
        let scroll_to = self.search.scroll_to.take();
        let overlay = Overlay {
            flash: &flash,
            selected: session.selected(),
            search_hits: &hits,
            scroll_to,
            hidden: draft.as_ref().and_then(|d| d.target),
            crosshair,
            draft: draft.as_ref(),
            moving: session.drag_preview(),
            marquee: session.marquee_rect(),
            hide_grid: !self.display.grid,
            grid_pt: canvas::grid_points(&page_model),
            region_borders: self.display.region_borders,
            pages: page_geometry_for(page_layout, &page_model, vm),
            image_frame: session.image_frame(),
            pending_word,
        };
        let output = canvas::show(ui, vm, zoom, self.show_prime, &overlay);
        self.page = output.page;
        // A press, drag or double click inside the draft edits its text (see `canvas::show`).
        if let Some(pointer) = output.draft {
            session.draft_pointer(pointer);
        }
        // The collapse icon of an area expands or collapses it.
        if let Some(id) = output.area_toggle {
            session.toggle_area(id);
        }
        // A double click in the top or bottom margin of a page edits the header or footer.
        if let Some(footer) = output.furniture {
            session.start_furniture(footer);
        }
        // A corner handle of the selected image starts a resize (the drag then moves it).
        if let Some(corner) = output.resize_start {
            session.begin_resize(corner);
        }
        // The area context menu locks or unlocks an area.
        if let Some(id) = output.area_lock {
            session.toggle_area_lock(id);
        }
        // Shift+click adds to the selection; Ctrl (or Command) with a drag copies the regions.
        // Hit tests use the laid-out sheet; the session gets stored positions.
        let (shift, command) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
        let stored = |p: [f64; 2]| [p[0], f64::from(vm.to_stored(p[1] as f32))];
        if let Some(point) = output.click {
            let hit = vm.hit_test(point[0], point[1]);
            if shift {
                session.toggle_selection(hit);
            } else {
                // A click on empty page snaps to the grid; the new region's box sits in the
                // row above that point (its bottom-left corner).
                let grid = canvas::grid_points(&page_model);
                session.click(stored(canvas::snap_to_grid(point, grid)), hit);
            }
        }
        // A double click on a region starts editing it (after the click that selected it).
        if let Some(point) = output.region_word {
            let hit = vm.hit_test(point[0], point[1]);
            let plot = hit.filter(|id| session.plot_spec(*id).is_some());
            match plot {
                // A plot has no text draft: its expressions are edited in the plot window.
                Some(id) => {
                    session.click(stored(point), Some(id));
                    self.plot_dialog = session.plot_spec(id).map(|spec| PlotDialog::new(id, spec));
                }
                None => session.edit_region_word(stored(point), hit),
            }
        }
        if let Some(start) = output.drag_start {
            let hit = vm.hit_test(start[0], start[1]);
            if command {
                session.begin_copy_drag(hit);
            } else {
                session.begin_press(stored(start), hit);
            }
        }
        if let Some(delta) = output.drag {
            session.drag_to(delta);
        }
        if output.drag_end {
            session.end_drag();
        }
        self.pending_ws = output.menu;
    }

    /// Test helpers.
    #[cfg(test)]
    pub(crate) fn active_tab(&self) -> usize {
        self.active
    }
    #[cfg(test)]
    pub(crate) fn type_into_active(&mut self, text: &str) {
        let session = self.session.as_mut().expect("a worksheet is open");
        session.click([100.0, 100.0], None);
        session.start_math();
        for c in text.chars() {
            session.type_char(c);
        }
    }
    #[cfg(test)]
    pub(crate) fn queued_draft_tabs(&self) -> usize {
        self.draft_tabs.len()
    }
    #[cfg(test)]
    pub(crate) fn waiting(&self) -> Option<Pending> {
        self.confirm.clone()
    }
    /// True once the window has been told to close (no further close request is asked).
    #[cfg(test)]
    pub(crate) fn closing(&self) -> bool {
        self.allow_close
    }
    #[cfg(test)]
    pub(crate) fn title_text(&self) -> String {
        self.window_title()
    }
    #[cfg(test)]
    pub(crate) fn options_open(&self) -> bool {
        self.options_tab.is_some()
    }
    #[cfg(test)]
    pub(crate) fn about_open(&self) -> bool {
        self.dialogs.is_open()
    }

    /// Runs an entry of the worksheet's context menu.
    pub(crate) fn ws_action(&mut self, ctx: &egui::Context, action: WsAction) {
        match action {
            WsAction::InsertImage => self.insert_image_dialog(),
            WsAction::ToggleGrid => self.display.grid = !self.display.grid,
            WsAction::InsertText => self.ribbon_action(ctx, Action::InsertText),
            WsAction::InsertSolveBlock => self.ribbon_action(ctx, Action::InsertSolveBlock),
            WsAction::InsertXyPlot => self.ribbon_action(ctx, Action::InsertPlot(false)),
            WsAction::AddSpace => self.ribbon_action(ctx, Action::InsertLines),
            WsAction::AddPageBreak => self.ribbon_action(ctx, Action::InsertPageBreak),
            // The other entries are shown greyed out and never reach here.
            _ => {}
        }
    }
}

/// Takes the shortcut key presses out of the input, so no other handler sees them, and returns
/// what they mean. Nothing is taken while a text box has the keyboard (Ctrl+Z there is the text
/// box's own undo).
fn take_shortcuts(ctx: &egui::Context) -> Vec<Shortcut> {
    if ctx.wants_keyboard_input() {
        return Vec::new();
    }
    ctx.input_mut(|i| {
        let mut found = Vec::new();
        i.events.retain(|event| {
            if let Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                if modifiers.command || modifiers.ctrl {
                    let meaning = key_letter(*key, modifiers.shift)
                        .and_then(|c| shortcut(c, modifiers.shift));
                    if let Some(action) = meaning {
                        found.push(action);
                        return false;
                    }
                }
            }
            true
        });
        found
    })
}

/// The lower-case letter of a letter key that shortcuts use.
fn key_letter(key: Key, shift: bool) -> Option<char> {
    Some(match key {
        Key::A => 'a',
        Key::D => 'd',
        Key::E => 'e',
        Key::I => 'i',
        Key::G => 'g',
        Key::M => 'm',
        Key::N => 'n',
        Key::Num2 => '2',
        // Ctrl+4 is the polar plot; Ctrl+Shift+4 and Ctrl+Shift+3 give the summation and product
        // keys' symbols.
        Key::Num4 if !shift => '4',
        Key::Num4 => '$',
        Key::Num3 if shift => '#',
        Key::Period => '.',
        Key::O => 'o',
        Key::R => 'r',
        Key::S => 's',
        Key::U => 'u',
        Key::Y => 'y',
        Key::Z => 'z',
        Key::Quote => '\'',
        Key::L => 'l',
        _ => return None,
    })
}

/// The editing key for a key press, with Shift and Ctrl: Shift+arrows select, Shift+Tab goes
/// back, Ctrl+arrows move by a term, Shift+Enter starts a matrix row.
fn edit_key_with_modifiers(key: Key, shift: bool, ctrl: bool) -> Option<EditKey> {
    match (key, shift, ctrl) {
        (Key::ArrowLeft, true, _) => Some(EditKey::SelectLeft),
        (Key::ArrowRight, true, _) => Some(EditKey::SelectRight),
        (Key::ArrowLeft, false, true) => Some(EditKey::TermLeft),
        (Key::ArrowRight, false, true) => Some(EditKey::TermRight),
        (Key::Tab, true, _) => Some(EditKey::BackTab),
        (Key::Tab, false, _) => Some(EditKey::Tab),
        (Key::Enter, true, _) => Some(EditKey::ShiftEnter),
        (key, _, _) => edit_key(key),
    }
}

/// The editing key an egui key stands for, if it is one the session uses.
fn edit_key(key: Key) -> Option<EditKey> {
    Some(match key {
        Key::Enter => EditKey::Enter,
        Key::Escape => EditKey::Escape,
        Key::Backspace => EditKey::Backspace,
        Key::Delete => EditKey::Delete,
        Key::ArrowLeft => EditKey::Left,
        Key::ArrowRight => EditKey::Right,
        Key::ArrowUp => EditKey::Up,
        Key::ArrowDown => EditKey::Down,
        Key::Home => EditKey::Home,
        Key::End => EditKey::End,
        _ => return None,
    })
}

/// A tab, dialog or window that the screenshot harness can show (see [`App::stage`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Staged {
    /// The AI endpoint on, one client connected, the first region flashing (screenshot harness).
    Live,
    /// A ribbon tab, by index into `ribbon::TABS`.
    Tab(usize),
    Dialog(Dialog),
    ResultFormat,
    PageSetup,
    WorksheetOptions,
    FileAccess,
    /// The page layout instead of the draft view.
    PageLayout,
    /// The application menu, with a submenu showing.
    AppMenu(Option<menus::Submenu>),
    /// The application Options window.
    AppOptions,
    /// Open this many more untitled worksheets as tabs, then go back to the first.
    Tabs(usize),
    /// Select the first plot of the sheet.
    SelectPlot,
    /// Select the first math region of the sheet.
    SelectMath,
    /// Open the dropdown of the ribbon button with this id (`ribbon/*.rs`).
    Popup(String),
    /// Draw the size grid with the cell (row, column), counted from 0, as hovered.
    GridHover(usize, usize),
    /// The Einheitensystem (units) window.
    UnitsDialog,
    /// The window of the Resources button with this id (`ribbon/resources.rs`).
    Resource(String),
    /// Worksheet zoom in percent.
    Zoom(f32),
}

impl eframe::App for App {
    /// Tab belongs to the draft being edited (next slot), not to egui's focus traversal: the
    /// press is taken out of the input before egui sees it and replayed by `handle_edit_keys`.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.take_draft_tabs(ctx, raw_input);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
    }
}

impl App {
    /// True while the keys belong to a widget or a window rather than the worksheet: a text box
    /// has the keyboard, a load runs, or a dialog is open. Shared by the key handling and the Tab
    /// hook, so a Tab is taken only when the draft would also get the other keys.
    fn keys_go_elsewhere(&self, ctx: &egui::Context) -> bool {
        ctx.wants_keyboard_input()
            || self.loader.busy()
            || self.confirm.is_some()
            || self.dialogs.is_open()
    }

    /// See `eframe::App::raw_input_hook`. Public for the headless harness, which has no eframe.
    /// Leaves the input alone when the keys go elsewhere, so the focused widget gets its Tab.
    pub fn take_draft_tabs(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if !self.session.as_ref().is_some_and(|s| s.has_draft()) || self.keys_go_elsewhere(ctx) {
            return;
        }
        let tabs = &mut self.draft_tabs;
        raw_input.events.retain(|event| match event {
            Event::Key {
                key: Key::Tab,
                pressed,
                modifiers,
                ..
            } => {
                if *pressed {
                    tabs.push(modifiers.shift);
                }
                false
            }
            _ => true,
        });
    }

    /// One frame of the window: runs the background results, handles input and draws everything.
    /// `eframe::App::update` calls this; the headless harness calls it directly.
    pub fn ui(&mut self, ctx: &egui::Context) {
        if let Some(id) = self.staged_popup.clone() {
            ribbon::render::stage_popup(ctx, &id);
        }
        if let Some((r, c)) = self.staged_hover {
            ribbon::render::stage_grid_hover(ctx, r, c);
        }
        self.collect_load();
        self.collect_refresh();
        self.pump_live(ctx);
        self.handle_input(ctx);
        self.handle_edit_keys(ctx);
        self.refresh_view(ctx);
        self.sync_title(ctx);
        self.guard_close(ctx);
        let mut picked_action = None;
        let mut menu_pick = None;
        let (can_undo, can_redo) = self
            .session
            .as_ref()
            .map_or((false, false), |s| (s.can_undo(), s.can_redo()));
        let can_save = self.session.is_some();
        let mut orb_rect = Rect::NOTHING;
        egui::TopBottomPanel::top("quick-access").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let response = chrome::orb(ui, self.app_menu.open);
                orb_rect = response.rect;
                if response.clicked() {
                    self.app_menu.open = !self.app_menu.open;
                    self.app_menu.submenu = None;
                }
                ui.add_space(6.0);
                menu_pick = chrome::quick_access(ui, can_save, can_undo, can_redo);
            });
        });
        // The `{path}` header and footer field follows the file the worksheet is stored in.
        let path_text = self
            .file
            .path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        if let Some(vm) = self.view.as_mut().filter(|vm| vm.path != path_text) {
            vm.path = path_text;
        }
        let state = RibbonState::new(self.session.as_ref(), self.calc_mode, self.display)
            .with_errors(self.error_regions().len());
        let tab = self.tab;
        egui::TopBottomPanel::top("ribbon").show(ctx, |ui| {
            self.tab = ribbon::tab_row(ui, self.tab);
            picked_action = ribbon::tab_body(ui, tab, &state);
        });
        let entries = self.tab_entries();
        let mut tab_event = None;
        egui::TopBottomPanel::top("documents")
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                tab_event = chrome::tab_strip(ui, &entries, self.active);
            });
        match tab_event {
            Some(TabEvent::Select(i)) => self.select_tab(i),
            Some(TabEvent::Close(i)) => {
                self.select_tab(i);
                self.menu_action(ctx, MenuAction::CloseDocument);
            }
            None => {}
        }
        if let Some(action) = picked_action {
            self.ribbon_action(ctx, action);
        }
        let anchor = orb_rect.left_bottom() + egui::vec2(0.0, 2.0);
        if let Some(action) =
            chrome::app_menu(ctx, anchor, orb_rect, &mut self.app_menu, &self.recent)
        {
            menu_pick = Some(action);
        }
        if let Some(action) = menu_pick {
            self.menu_action(ctx, action);
        }
        // The search matches follow the worksheet: they are made again after every change.
        self.refresh_search();
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            // The message line: a problem with the last action, or the session's own message. It is
            // shown here, not above the sheet, so the paper does not move when it appears.
            let message = self
                .session
                .as_ref()
                .and_then(|s| s.message())
                .map(localize_error)
                .or_else(|| self.notice.clone());
            let events = statusbar::status_bar(
                ui,
                self.page,
                self.calc_mode,
                &mut self.zoom_pct,
                self.display.page_layout,
                &mut self.search,
                message.as_deref(),
                self.live.as_ref().map(crate::live::LiveRuntime::state),
            );
            if events.toggle_ai_pause {
                if let Some(rt) = self.live.as_ref() {
                    rt.shared.set_paused(!rt.shared.paused());
                }
            }
            if events.toggle_calc {
                self.ribbon_action(ctx, Action::ToggleCalcMode);
            }
            if events.zoom_in {
                self.zoom_pct = zoom_in(self.zoom_pct);
            }
            if events.zoom_out {
                self.zoom_pct = zoom_out(self.zoom_pct);
            }
            if let Some(page_view) = events.page_view {
                self.display.page_layout = page_view;
            }
            if let Some(action) = events.search {
                self.search_action(action);
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| self.central(ui));
        if let Some(action) = self.pending_ws.take() {
            self.ws_action(ctx, action);
        }
        self.page_setup_window(ctx);
        self.result_format_dialog(ctx);
        self.link_dialog(ctx);
        self.options_dialog_window(ctx);
        self.units_dialog_window(ctx);
        self.resource_dialog_window(ctx);
        self.file_access_window(ctx);
        self.plot_dialog_window(ctx);
        self.confirm_dialog(ctx);
        if let Some(mut tab) = self.options_tab {
            let before = self.live_enabled;
            let mut live_on = before;
            let keep =
                chrome_dialogs::options_window(ctx, &mut tab, &mut self.show_prime, &mut live_on);
            self.options_tab = keep.then_some(tab);
            if live_on != before {
                self.set_live(ctx, live_on, config::config_dir());
                let mut saved = config::load();
                saved.live = Some(self.live_enabled);
                let _ = config::save(&saved);
            }
        }
        if let Some(insertion) = self.dialogs.show(ctx) {
            self.apply_insertion(insertion);
        }
    }
}

/// The page layout of `vm` with `page` settings, when the page layout is on.
/// The name of a grid size in the page setup dialog (`Fine` or `Standard`).
fn grid_label(name: &str) -> String {
    if name.eq_ignore_ascii_case("Fine") {
        tr("page.grid_fine").to_string()
    } else {
        tr("page.grid_standard").to_string()
    }
}

pub(crate) fn page_geometry_for(on: bool, page: &PageSettings, vm: &ViewModel) -> Option<Geometry> {
    if !on {
        return None;
    }
    let bottom = vm
        .items
        .iter()
        .map(|item| item.rect[1] + item.rect[3])
        .fold(0.0_f32, f32::max);
    Some(Geometry::new(page, bottom))
}

/// Applies a Textformatierung button to the selected text of the text being typed. Toggles read
/// the selection first, so a bold selection goes plain again.
fn apply_text_format(session: &mut EditSession, format: TextFormat) {
    match format {
        TextFormat::Align(align) => {
            session.align_text(align);
        }
        _ => {
            let styles = session.selected_text_styles();
            if let Some(change) = ribbon::text_format::style_change(format, &styles) {
                session.format_text_selection(&change);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letter_keys_used_by_shortcuts_map_to_letters() {
        assert_eq!(key_letter(Key::Z, false), Some('z'));
        assert_eq!(key_letter(Key::Y, false), Some('y'));
        assert_eq!(key_letter(Key::Num0, false), None);
        assert_eq!(key_letter(Key::Enter, false), None);
        // Ctrl+4 is the polar plot, Ctrl+Shift+4 the summation template; Ctrl+3 alone is nothing.
        assert_eq!(key_letter(Key::Num4, false), Some('4'));
        assert_eq!(key_letter(Key::Num4, true), Some('$'));
        assert_eq!(key_letter(Key::Num3, true), Some('#'));
        assert_eq!(key_letter(Key::Num3, false), None);
    }

    #[test]
    fn edit_keys_map_the_navigation_keys() {
        assert_eq!(edit_key(Key::ArrowLeft), Some(EditKey::Left));
        assert_eq!(edit_key(Key::Delete), Some(EditKey::Delete));
        assert_eq!(edit_key(Key::A), None);
    }
}
