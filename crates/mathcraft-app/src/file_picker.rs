//! Native file dialogs (open, save, export and image pick) through `rfd`. Each call blocks until
//! the user picks a file or cancels; a cancel gives `None`. The dialogs only choose a path: the
//! callers read or write the file.

use std::path::{Path, PathBuf};

use crate::i18n::tr;

/// Extensions offered when picking an image.
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg"];

/// The file to open: a `.mcdx` worksheet.
pub fn pick_worksheet() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(tr("dlg.open_title"))
        .add_filter(tr("dlg.filter_worksheets"), &["mcdx"])
        .pick_file()
}

/// Where to save a worksheet. `current` is the file it was opened from, which gives the folder
/// and the name the dialog starts with. The result always ends in `.mcdx`.
pub fn save_worksheet(current: Option<&Path>) -> Option<PathBuf> {
    let dialog = rfd::FileDialog::new()
        .set_title(tr("dlg.save_title"))
        .add_filter(tr("dlg.filter_worksheets"), &["mcdx"]);
    with_location(dialog, current)
        .save_file()
        .map(|path| with_extension(path, "mcdx"))
}

/// Where to export the pages as a PDF. The result always ends in `.pdf`.
pub fn save_pdf(current: Option<&Path>) -> Option<PathBuf> {
    let dialog = rfd::FileDialog::new()
        .set_title(tr("dlg.export_title"))
        .add_filter(tr("dlg.filter_pdf"), &["pdf"]);
    let suggested = current.map(|p| p.with_extension("pdf"));
    with_location(dialog, suggested.as_deref())
        .save_file()
        .map(|path| with_extension(path, "pdf"))
}

/// The image to insert: a PNG or JPEG file.
pub fn pick_image() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(tr("dlg.image_title"))
        .add_filter(tr("dlg.filter_images"), IMAGE_EXTENSIONS)
        .pick_file()
}

/// Starts the dialog in the folder of `current`, with its file name.
fn with_location(dialog: rfd::FileDialog, current: Option<&Path>) -> rfd::FileDialog {
    let Some(path) = current else {
        return dialog;
    };
    let dialog = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dialog.set_directory(dir),
        _ => dialog,
    };
    match path.file_name() {
        Some(name) => dialog.set_file_name(name.to_string_lossy()),
        None => dialog,
    }
}

/// `path` with `ext` as its extension, added when the dialog returned a name without one.
fn with_extension(path: PathBuf, ext: &str) -> PathBuf {
    let has = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(ext));
    if has {
        path
    } else {
        path.with_extension(ext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_extension_is_added_and_a_right_one_is_kept() {
        assert_eq!(
            with_extension(PathBuf::from("/tmp/work"), "mcdx"),
            PathBuf::from("/tmp/work.mcdx")
        );
        assert_eq!(
            with_extension(PathBuf::from("/tmp/work.MCDX"), "mcdx"),
            PathBuf::from("/tmp/work.MCDX")
        );
    }
}
