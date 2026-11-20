//! The decisions behind the window chrome that need no window: what the application menu and the
//! worksheet's context menu offer, the recent-worksheet list, the names of new worksheets and the
//! tab bookkeeping. `chrome.rs` draws these; `app.rs` runs the actions. Keeping them free of egui
//! lets the unit tests check them directly.

use std::path::{Path, PathBuf};

use crate::i18n::tr;

/// Most worksheets the recent list keeps (Prime shows about fifteen).
pub const RECENT_MAX: usize = 15;

/// What the user picked in the application menu, the quick access toolbar or the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    New,
    Open,
    OpenRecent(PathBuf),
    Save,
    SaveAs,
    ExportPdf,
    Print,
    PageSetup,
    CloseDocument,
    Options,
    About,
    Quit,
    Undo,
    Redo,
}

/// The submenu the application menu shows in its right column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submenu {
    New,
    Open,
    SaveAs,
    Print,
}

/// One entry of the application menu's left column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuRow {
    /// The i18n key of the label.
    pub label: &'static str,
    pub action: MenuAction0,
    /// The submenu that opens on hover, if any.
    pub submenu: Option<Submenu>,
    /// A separator is drawn above this row.
    pub separator_above: bool,
}

/// [`MenuAction`] without the path payload, so that the row table can be `const`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction0 {
    New,
    Open,
    Save,
    SaveAs,
    Print,
    CloseDocument,
    Options,
    About,
    Quit,
}

impl MenuAction0 {
    /// The action this row runs when its label is clicked.
    #[must_use]
    pub fn action(self) -> MenuAction {
        match self {
            MenuAction0::New => MenuAction::New,
            MenuAction0::Open => MenuAction::Open,
            MenuAction0::Save => MenuAction::Save,
            MenuAction0::SaveAs => MenuAction::SaveAs,
            MenuAction0::Print => MenuAction::Print,
            MenuAction0::CloseDocument => MenuAction::CloseDocument,
            MenuAction0::Options => MenuAction::Options,
            MenuAction0::About => MenuAction::About,
            MenuAction0::Quit => MenuAction::Quit,
        }
    }
}

/// The left column of the application menu, in Prime's order (screenshot 0004).
pub const MENU_ROWS: [MenuRow; 9] = [
    MenuRow {
        label: "menu.new",
        action: MenuAction0::New,
        submenu: Some(Submenu::New),
        separator_above: false,
    },
    MenuRow {
        label: "menu.open",
        action: MenuAction0::Open,
        submenu: Some(Submenu::Open),
        separator_above: false,
    },
    MenuRow {
        label: "menu.save",
        action: MenuAction0::Save,
        submenu: None,
        separator_above: false,
    },
    MenuRow {
        label: "menu.save_as",
        action: MenuAction0::SaveAs,
        submenu: Some(Submenu::SaveAs),
        separator_above: false,
    },
    MenuRow {
        label: "menu.print",
        action: MenuAction0::Print,
        submenu: Some(Submenu::Print),
        separator_above: false,
    },
    MenuRow {
        label: "menu.close",
        action: MenuAction0::CloseDocument,
        submenu: None,
        separator_above: false,
    },
    MenuRow {
        label: "menu.options",
        action: MenuAction0::Options,
        submenu: None,
        separator_above: true,
    },
    MenuRow {
        label: "menu.about",
        action: MenuAction0::About,
        submenu: None,
        separator_above: false,
    },
    MenuRow {
        label: "menu.quit",
        action: MenuAction0::Quit,
        submenu: None,
        separator_above: true,
    },
];

/// One entry of a submenu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubEntry {
    pub label: &'static str,
    /// `None` for an entry we do not support (shown greyed out).
    pub action: Option<MenuAction0Sub>,
}

/// The actions the submenus offer beyond [`MenuAction0`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction0Sub {
    New,
    Open,
    SaveAs,
    ExportPdf,
    Print,
    PageSetup,
}

impl MenuAction0Sub {
    #[must_use]
    pub fn action(self) -> MenuAction {
        match self {
            MenuAction0Sub::New => MenuAction::New,
            MenuAction0Sub::Open => MenuAction::Open,
            MenuAction0Sub::SaveAs => MenuAction::SaveAs,
            MenuAction0Sub::ExportPdf => MenuAction::ExportPdf,
            MenuAction0Sub::Print => MenuAction::Print,
            MenuAction0Sub::PageSetup => MenuAction::PageSetup,
        }
    }
}

/// The entries of `submenu`, above the recent list. Template entries are greyed out: Mathcraft has
/// no template files.
#[must_use]
pub fn submenu_entries(submenu: Submenu) -> &'static [SubEntry] {
    match submenu {
        Submenu::New => &[
            SubEntry {
                label: "menu.new_blank",
                action: Some(MenuAction0Sub::New),
            },
            SubEntry {
                label: "menu.from_my_templates",
                action: None,
            },
            SubEntry {
                label: "menu.from_shared_templates",
                action: None,
            },
        ],
        Submenu::Open => &[
            SubEntry {
                label: "menu.open",
                action: Some(MenuAction0Sub::Open),
            },
            SubEntry {
                label: "menu.from_my_templates",
                action: None,
            },
            SubEntry {
                label: "menu.from_shared_templates",
                action: None,
            },
        ],
        Submenu::SaveAs => &[
            SubEntry {
                label: "menu.save_as_worksheet",
                action: Some(MenuAction0Sub::SaveAs),
            },
            SubEntry {
                label: "menu.save_as_template",
                action: None,
            },
            SubEntry {
                label: "menu.save_as_pdf",
                action: Some(MenuAction0Sub::ExportPdf),
            },
        ],
        Submenu::Print => &[
            SubEntry {
                label: "menu.print",
                action: Some(MenuAction0Sub::Print),
            },
            SubEntry {
                label: "btn.page_setup",
                action: Some(MenuAction0Sub::PageSetup),
            },
        ],
    }
}

/// The heading above the recent list: Prime says "worksheets" with no submenu open and
/// "documents" under Open.
#[must_use]
pub fn recent_heading(submenu: Option<Submenu>) -> &'static str {
    match submenu {
        Some(Submenu::Open) => tr("menu.recent_documents"),
        _ => tr("menu.recent_worksheets"),
    }
}

/// The recent list after opening or saving `path`: the path first, no duplicate, at most
/// [`RECENT_MAX`] entries.
#[must_use]
pub fn push_recent(list: &[PathBuf], path: &Path) -> Vec<PathBuf> {
    let mut out = vec![path.to_path_buf()];
    out.extend(list.iter().filter(|p| p.as_path() != path).cloned());
    out.truncate(RECENT_MAX);
    out
}

/// The text of the recent list file: one path per line.
#[must_use]
pub fn render_recent(list: &[PathBuf]) -> String {
    list.iter()
        .map(|p| format!("{}\n", p.display()))
        .collect::<String>()
}

/// The recent list from the text of its file. Blank lines are skipped; the list is cut at
/// [`RECENT_MAX`].
#[must_use]
pub fn parse_recent(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(RECENT_MAX)
        .map(PathBuf::from)
        .collect()
}

/// The file of the recent list, beside the settings file.
fn recent_file() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("recent.txt"))
}

/// Reads the recent list from the config folder (empty when there is none).
#[must_use]
pub fn load_recent() -> Vec<PathBuf> {
    recent_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|t| parse_recent(&t))
        .unwrap_or_default()
}

/// Writes the recent list to the config folder. A failure is ignored: the list is a convenience.
pub fn save_recent(list: &[PathBuf]) {
    if let Some(file) = recent_file() {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, render_recent(list));
    }
}

/// The name shown on a tab and in the title: the file name without the `.mcdx` extension.
#[must_use]
pub fn display_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".mcdx") {
        name[..name.len() - 5].to_string()
    } else {
        name.to_string()
    }
}

/// The file name of the next new worksheet: `Untitled.mcdx`, then `Untitled-1.mcdx`, ... the first
/// that no open worksheet uses (as Prime numbers `Unbenannt`, `Unbenannt-1`).
#[must_use]
pub fn next_untitled(taken: &[String], base: &str) -> String {
    let stem = display_name(base);
    let used = |candidate: &str| {
        taken
            .iter()
            .any(|t| display_name(t).eq_ignore_ascii_case(candidate))
    };
    if !used(&stem) {
        return format!("{stem}.mcdx");
    }
    (1u32..)
        .map(|n| format!("{stem}-{n}"))
        .find(|c| !used(c))
        .map(|c| format!("{c}.mcdx"))
        .unwrap_or_else(|| format!("{stem}.mcdx"))
}

/// The tab that becomes active when tab `closed` is closed in a list of `len` tabs (before the
/// removal) while `active` was active: the one that moves into its place, or the new last one.
/// `None` when no tab is left.
#[must_use]
pub fn active_after_close(len: usize, closed: usize, active: usize) -> Option<usize> {
    if len <= 1 {
        return None;
    }
    let remaining = len - 1;
    let next = if closed < active {
        active - 1
    } else if closed == active {
        closed
    } else {
        active
    };
    Some(next.min(remaining - 1))
}

/// An entry of the worksheet's context menu (screenshot 0162).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsAction {
    Paste,
    InsertText,
    InsertTextBox,
    InsertImage,
    InsertComponent,
    InsertXyPlot,
    InsertSolveBlock,
    InsertTable,
    AddSpace,
    RemoveSpace,
    AddPage,
    AddPageBreak,
    ToggleGrid,
    SplitVertical,
    SplitHorizontal,
}

/// A row of the context menu: the label key, the shortcut text, the action, and whether we
/// support it. A separator is drawn above rows that start a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WsRow {
    pub label: &'static str,
    pub shortcut: &'static str,
    pub action: WsAction,
    pub separator_above: bool,
    pub supported: bool,
}

/// The rows of the context menu, in Prime's order.
pub const WS_ROWS: [WsRow; 15] = [
    ws(WsAction::Paste, "ws.paste", "Ctrl+V", false, false),
    ws(WsAction::InsertText, "ws.insert_text", "", true, true),
    ws(
        WsAction::InsertTextBox,
        "ws.insert_textbox",
        "",
        false,
        false,
    ),
    ws(WsAction::InsertImage, "ws.insert_image", "", false, true),
    ws(
        WsAction::InsertComponent,
        "ws.insert_component",
        "",
        true,
        false,
    ),
    ws(WsAction::InsertXyPlot, "ws.insert_xy", "", false, true),
    ws(
        WsAction::InsertSolveBlock,
        "ws.insert_solve",
        "",
        false,
        true,
    ),
    ws(WsAction::InsertTable, "ws.insert_table", "", false, false),
    ws(WsAction::AddSpace, "ws.add_space", "", true, true),
    ws(WsAction::RemoveSpace, "ws.remove_space", "", false, false),
    ws(WsAction::AddPage, "ws.add_page", "", false, false),
    ws(WsAction::AddPageBreak, "ws.add_break", "", false, true),
    ws(WsAction::ToggleGrid, "ws.hide_grid", "", true, true),
    ws(WsAction::SplitVertical, "ws.split_v", "", true, false),
    ws(WsAction::SplitHorizontal, "ws.split_h", "", false, false),
];

const fn ws(
    action: WsAction,
    label: &'static str,
    shortcut: &'static str,
    separator_above: bool,
    supported: bool,
) -> WsRow {
    WsRow {
        label,
        shortcut,
        action,
        separator_above,
        supported,
    }
}

/// The label key of the grid row: it reads "Hide grid" while the grid shows and "Show grid"
/// otherwise.
#[must_use]
pub fn grid_row_label(grid_on: bool) -> &'static str {
    if grid_on {
        "ws.hide_grid"
    } else {
        "ws.show_grid"
    }
}

/// The search options of the status bar (screenshot 0159).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOptions {
    pub in_equations: bool,
    pub in_text: bool,
    pub match_case: bool,
    pub whole_words: bool,
}

impl Default for SearchOptions {
    /// Prime starts with equations and text searched, case ignored, parts of words allowed.
    fn default() -> Self {
        Self {
            in_equations: true,
            in_text: true,
            match_case: false,
            whole_words: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_has_prime_rows_in_order() {
        let labels: Vec<_> = MENU_ROWS.iter().map(|r| r.label).collect();
        assert_eq!(
            labels,
            [
                "menu.new",
                "menu.open",
                "menu.save",
                "menu.save_as",
                "menu.print",
                "menu.close",
                "menu.options",
                "menu.about",
                "menu.quit"
            ]
        );
        assert_eq!(MENU_ROWS[0].action.action(), MenuAction::New);
        assert_eq!(MENU_ROWS[8].action.action(), MenuAction::Quit);
    }

    #[test]
    fn template_entries_are_unsupported_and_the_rest_run_actions() {
        let open = submenu_entries(Submenu::Open);
        assert_eq!(
            open[0].action.map(MenuAction0Sub::action),
            Some(MenuAction::Open)
        );
        assert!(open[1].action.is_none() && open[2].action.is_none());
        let print = submenu_entries(Submenu::Print);
        assert_eq!(
            print[1].action.map(MenuAction0Sub::action),
            Some(MenuAction::PageSetup)
        );
    }

    #[test]
    fn recent_puts_the_newest_first_without_duplicates_and_is_capped() {
        let a = PathBuf::from("a.mcdx");
        let b = PathBuf::from("b.mcdx");
        let list = push_recent(&push_recent(&[], &a), &b);
        assert_eq!(list, vec![b.clone(), a.clone()]);
        assert_eq!(push_recent(&list, &a), vec![a.clone(), b]);
        let mut long = Vec::new();
        for i in 0..40 {
            long = push_recent(&long, &PathBuf::from(format!("{i}.mcdx")));
        }
        assert_eq!(long.len(), RECENT_MAX);
        assert_eq!(long[0], PathBuf::from("39.mcdx"));
    }

    #[test]
    fn the_recent_file_round_trips() {
        let list = vec![PathBuf::from("/x/a.mcdx"), PathBuf::from("/y/b.mcdx")];
        assert_eq!(parse_recent(&render_recent(&list)), list);
        assert!(parse_recent("\n  \n").is_empty());
    }

    #[test]
    fn new_worksheets_are_numbered_like_prime() {
        let mut taken: Vec<String> = Vec::new();
        let first = next_untitled(&taken, "Untitled.mcdx");
        assert_eq!(first, "Untitled.mcdx");
        taken.push(first);
        let second = next_untitled(&taken, "Untitled.mcdx");
        assert_eq!(second, "Untitled-1.mcdx");
        taken.push(second);
        taken.push("Untitled-3.mcdx".to_string());
        assert_eq!(next_untitled(&taken, "Untitled.mcdx"), "Untitled-2.mcdx");
    }

    #[test]
    fn display_names_drop_the_extension() {
        assert_eq!(display_name("sheet.mcdx"), "sheet");
        assert_eq!(display_name("Sheet.MCDX"), "Sheet");
        assert_eq!(display_name("notes"), "notes");
    }

    #[test]
    fn closing_a_tab_picks_the_neighbour() {
        assert_eq!(active_after_close(1, 0, 0), None);
        // Closing the active middle tab activates the one that slides into its place.
        assert_eq!(active_after_close(3, 1, 1), Some(1));
        // Closing the last active tab activates the new last one.
        assert_eq!(active_after_close(3, 2, 2), Some(1));
        // Closing a tab before the active one keeps the same document active.
        assert_eq!(active_after_close(4, 0, 2), Some(1));
        // Closing a tab after the active one changes nothing.
        assert_eq!(active_after_close(4, 3, 1), Some(1));
    }

    #[test]
    fn the_context_menu_has_prime_rows() {
        assert_eq!(WS_ROWS.len(), 15);
        assert_eq!(WS_ROWS[0].action, WsAction::Paste);
        assert_eq!(WS_ROWS[0].shortcut, "Ctrl+V");
        assert!(WS_ROWS.iter().any(|r| !r.supported));
        assert_eq!(grid_row_label(true), "ws.hide_grid");
        assert_eq!(grid_row_label(false), "ws.show_grid");
    }

    #[test]
    fn search_options_default_like_prime() {
        let o = SearchOptions::default();
        assert!(o.in_equations && o.in_text && !o.match_case && !o.whole_words);
    }
}
