//! Saving and the file state of the open document: which file it is stored in, whether it has
//! unsaved edits, and what the window does after the user is asked about them.
//!
//! Saving is two steps. The writer builds the complete `.mcdx` bytes in memory first, so a writer
//! error never touches the file. Then the bytes go to a temporary file in the same folder, which is
//! flushed and renamed over the target, so a crash or a full disk leaves the old file intact.

use crate::i18n::{tr, trf};
use crate::temp_file::create_new_in;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mathcraft_import::writer::write_package_with_report;

use super::document::Document;

/// Writes a document to a file.
pub trait DocumentWriter {
    /// Writes `doc` to `path`. An `Err` is a message for the user; the file is then unchanged. An
    /// `Ok` carries the writer's warnings (unverified file formats written), empty when there are none.
    fn write(&self, doc: &Document, path: &Path) -> Result<Vec<String>, String>;
}

/// Writes `.mcdx` packages with the package writer, copying the source parts of the document.
#[derive(Debug, Default, Clone, Copy)]
pub struct McdxWriter;

impl DocumentWriter for McdxWriter {
    fn write(&self, doc: &Document, path: &Path) -> Result<Vec<String>, String> {
        let (bytes, mut warnings) = write_package_with_report(&doc.worksheet(), doc.source())
            .map_err(|e| {
                trf(
                    "save.cannot_save",
                    &[&path.display().to_string(), &e.to_string()],
                )
            })?;
        // UNVERIFIED: Prime's file format of a disabled region is not known (see
        // `documentation/UNVERIFIED_FORMATS.md`), so the flag is not saved.
        let off = doc.disabled_ids().count();
        if off > 0 {
            warnings.push(trf("save.disabled_lost", &[&off.to_string()]));
        }
        write_atomic(path, &bytes)?;
        Ok(warnings)
    }
}

/// Replaces the file at `path` with `bytes`: the bytes go to a new temporary file next to it, which
/// is flushed and then renamed over `path`. The temporary file is created with `create_new`, so an
/// existing name (or a symbolic link planted at it) is never written through; the next number is
/// tried instead. When `path` exists, its permission bits are kept. On any error the temporary file
/// is removed and `path` keeps its old contents.
///
/// # Errors
/// A message naming `path` when the temporary file cannot be written or renamed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} is not a file path", path.display()))?
        .to_string_lossy()
        .into_owned();
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let pid = std::process::id();
    let cannot_save = |e: std::io::Error| {
        trf(
            "save.cannot_save",
            &[&path.display().to_string(), &e.to_string()],
        )
    };
    let (temp, mut file) =
        create_new_in(dir, |seq| format!(".{name}.{pid}.{seq}.tmp")).map_err(cannot_save)?;
    let result = (|| -> std::io::Result<()> {
        file.write_all(bytes)?;
        if let Ok(meta) = fs::metadata(path) {
            file.set_permissions(meta.permissions())?;
        }
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    result.map_err(|e| {
        let _ = fs::remove_file(&temp);
        cannot_save(e)
    })
}

/// Where the open document is stored, and the revision it was last saved at. The document is
/// dirty when its revision differs from the saved one, or when a draft is being typed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileState {
    path: Option<PathBuf>,
    saved_at: u64,
}

impl FileState {
    /// The state of a document just opened from `path` (or just created, when `path` is `None`).
    /// Both are clean: the file is what the document shows.
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path, saved_at: 0 }
    }

    /// The file the document is stored in, if it has one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// True when the document has edits that are not in its file. `revision` is the session's
    /// revision and `draft_open` tells whether text is being typed (not yet committed).
    pub fn is_dirty(&self, revision: u64, draft_open: bool) -> bool {
        draft_open || revision != self.saved_at
    }

    /// Records a successful save of the document at `revision` to `path`.
    pub fn mark_saved(&mut self, path: PathBuf, revision: u64) {
        self.path = Some(path);
        self.saved_at = revision;
    }
}

/// An action that waits for the user's answer to "save the unsaved edits?".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    Open(PathBuf),
    New,
    /// Close the active worksheet (its tab).
    CloseDoc,
    /// Close the window.
    Close,
}

impl Pending {
    /// What the action does, for the dialog text.
    pub fn describe(&self) -> String {
        match self {
            Pending::Open(path) => trf("save.open_path", &[&path.display().to_string()]),
            Pending::New => tr("save.new").to_string(),
            Pending::CloseDoc => tr("save.close_doc").to_string(),
            Pending::Close => tr("save.close").to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use mathcraft_import::regions::{
        ImportReport, PlotInfo, RegionKind, RegionModel, Settings, Worksheet,
    };
    use mathcraft_import::results::ResultTable;
    use std::collections::BTreeMap;

    use super::*;

    /// A fresh scratch folder for one test. Removed at the end of the test by the caller.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mathcraft-save-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch folder");
        dir
    }

    #[test]
    fn a_new_or_opened_document_is_clean_until_it_changes() {
        let state = FileState::new(Some(PathBuf::from("a.mcdx")));
        assert!(!state.is_dirty(0, false));
        assert!(state.is_dirty(1, false), "an edit makes it dirty");
        assert!(
            state.is_dirty(0, true),
            "a draft being typed is an unsaved edit"
        );
    }

    #[test]
    fn saving_makes_the_document_clean_at_that_revision() {
        let mut state = FileState::new(None);
        assert_eq!(state.path(), None);
        state.mark_saved(PathBuf::from("b.mcdx"), 5);
        assert_eq!(state.path(), Some(Path::new("b.mcdx")));
        assert!(!state.is_dirty(5, false));
        assert!(state.is_dirty(6, false));
    }

    #[test]
    fn the_pending_action_names_itself() {
        assert_eq!(Pending::New.describe(), tr("save.new"));
        assert!(Pending::Open(PathBuf::from("x.mcdx"))
            .describe()
            .contains("x.mcdx"));
    }

    #[test]
    fn atomic_write_replaces_the_file_and_leaves_no_temporary_file() {
        let dir = scratch("replace");
        let target = dir.join("sheet.mcdx");
        fs::write(&target, b"old").expect("old file");
        write_atomic(&target, b"new contents").expect("write");
        assert_eq!(fs::read(&target).expect("read"), b"new contents");
        let names: Vec<_> = fs::read_dir(&dir)
            .expect("list")
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .collect();
        assert_eq!(names.len(), 1, "only the target is left: {names:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_atomic_write_keeps_the_old_file_and_removes_the_temporary_file() {
        let dir = scratch("failed");
        // A directory where the file should be: the rename cannot replace it.
        let target = dir.join("sheet.mcdx");
        fs::create_dir_all(&target).expect("directory in the way");
        assert!(write_atomic(&target, b"data").is_err());
        let names: Vec<_> = fs::read_dir(&dir)
            .expect("list")
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .collect();
        assert_eq!(names.len(), 1, "no temporary file is left: {names:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_writer_error_never_touches_the_target_file() {
        let dir = scratch("writer-error");
        let target = dir.join("sheet.mcdx");
        fs::write(&target, b"keep me").expect("old file");
        // A plot has no source to copy from, and the writer cannot generate one: it refuses.
        let plot = RegionModel {
            id: 1,
            pos: Some((0.0, 0.0)),
            size: None,
            result_ref: None,
            kind: RegionKind::Plot(PlotInfo {
                kind: String::new(),
                traces: Vec::new(),
                x_range: None,
                y_range: None,
                plot: None,
                plot3d: None,
            }),
            solve_category: None,
            children: Vec::new(),
            result_format: None,
            matrix: Default::default(),
        };
        let ws = Worksheet {
            regions: vec![plot],
            settings: Settings::default(),
            page: Default::default(),
            report: ImportReport::default(),
            header: Vec::new(),
            footer: Vec::new(),
        };
        let doc = Document::from_worksheet(
            "t",
            ws,
            ResultTable {
                by_id: BTreeMap::new(),
            },
        );
        assert!(McdxWriter.write(&doc, &target).is_err());
        assert_eq!(fs::read(&target).expect("read"), b"keep me");
        let _ = fs::remove_dir_all(&dir);
    }
}
