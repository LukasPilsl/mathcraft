//! Loading a worksheet: open the package, import the sheet, read the stored results, evaluate
//! every region with the shared evaluation, and build the view model. Every failure is returned
//! as text for the window to show; nothing here panics on file data.

use crate::i18n::trf;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mathcraft_core::eval::builtins::fileio::FileOptions;
use mathcraft_import::evaluate::{
    evaluate_worksheet_in_limited, evaluate_worksheet_limited, load_results, on_big_stack,
    EvalLimits, EVALUATION_DEADLINE,
};
use mathcraft_import::package::Package;
use mathcraft_import::regions::Worksheet;

use crate::edit::document::Document;
use crate::loader::CancelFlag;
use crate::viewmodel::{build, ViewModel};

/// Largest `.mcdx` file the window opens. The whole file is kept in memory as the source of the
/// writer, so a larger file is refused rather than read.
pub const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;

/// A worksheet opened for editing: the document, and its evaluated view.
pub struct Loaded {
    pub doc: Document,
    pub vm: ViewModel,
}

/// Opens and evaluates the `.mcdx` file at `path`. The evaluation runs on a thread with the
/// engine's stack size, so this may be called from any thread.
///
/// # Errors
/// A message for the window when the file cannot be opened, imported or its results read.
#[cfg(test)]
pub fn load_file(path: &Path) -> Result<ViewModel, String> {
    open_file(path, None).map(|loaded| loaded.vm)
}

/// Like [`load_file`], but keeps the document for editing. The document keeps the bytes of the
/// file as its source, so a save copies the parts it does not model from them. `cancel`, when
/// given, stops the evaluation between regions (see [`evaluation_limits`]).
///
/// # Errors
/// The same as [`load_file`], and a message when the file is larger than [`MAX_FILE_BYTES`].
pub fn open_file(path: &Path, cancel: Option<CancelFlag>) -> Result<Loaded, String> {
    let owned = path.to_path_buf();
    on_big_stack(move || {
        let title = file_name(&owned);
        let len = std::fs::metadata(&owned)
            .map_err(|e| trf("load.cannot_open", &[&title, &e.to_string()]))?
            .len();
        if len > MAX_FILE_BYTES {
            return Err(trf(
                "load.too_big",
                &[&title, &len.to_string(), &MAX_FILE_BYTES.to_string()],
            ));
        }
        let bytes = std::fs::read(&owned)
            .map_err(|e| trf("load.cannot_open", &[&title, &e.to_string()]))?;
        // Opening never gives the file functions access: the opening evaluation has none, and the
        // viewer grants reading or writing per session (see `file_access`).
        open_bytes_in(&title, bytes, None, cancel)
    })
}

/// The limits of one evaluation that starts now: the wall-clock deadline, and the cancel flag
/// when there is one.
pub fn evaluation_limits(cancel: Option<CancelFlag>) -> EvalLimits {
    let limits = EvalLimits::new(EVALUATION_DEADLINE);
    match cancel {
        Some(flag) => limits.with_cancel(flag),
        None => limits,
    }
}

/// The directory the file functions read from for a worksheet at `path`: its parent, or the
/// current directory for a bare file name.
pub fn worksheet_dir(path: &Path) -> PathBuf {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Imports and evaluates a package given as its bytes, and keeps the bytes as the document's
/// source. Must run on a thread with the engine's stack size. The file functions have no access.
///
/// # Errors
/// The same as [`open_package`], and a message when the bytes are not a readable package.
#[cfg(test)]
pub fn open_bytes(title: &str, bytes: Vec<u8>) -> Result<Loaded, String> {
    open_bytes_in(title, bytes, None, None)
}

/// [`open_bytes`] with the file functions reading under `base` (the worksheet's directory) and the
/// evaluation stopped by `cancel` when it is set.
///
/// # Errors
/// The same as [`open_bytes`].
pub fn open_bytes_in(
    title: &str,
    bytes: Vec<u8>,
    base: Option<&Path>,
    cancel: Option<CancelFlag>,
) -> Result<Loaded, String> {
    let pkg = Package::from_bytes(&bytes)
        .map_err(|e| trf("load.cannot_open", &[title, &e.to_string()]))?;
    let loaded = open_package_in(title, &pkg, base, cancel)?;
    Ok(Loaded {
        doc: loaded.doc.with_source(Arc::from(bytes)),
        vm: loaded.vm,
    })
}

/// Imports and evaluates an opened package. Used by [`load_file`] and by tests on in-memory
/// packages. Must run on a thread with the engine's stack size (see
/// `mathcraft_import::evaluate::STACK_SIZE`).
///
/// # Errors
/// A message naming the file and the step that failed (open, import or stored results).
#[cfg(test)]
pub fn load_package(title: &str, pkg: &Package) -> Result<ViewModel, String> {
    open_package(title, pkg).map(|loaded| loaded.vm)
}

/// Imports the package into a document and evaluates it. Same thread rule as [`load_package`].
///
/// # Errors
/// A message naming the file and the step that failed (import or stored results).
#[cfg(test)]
pub fn open_package(title: &str, pkg: &Package) -> Result<Loaded, String> {
    open_package_in(title, pkg, None, None)
}

/// [`open_package`] with the file functions reading under `base` (the worksheet's directory) and
/// the evaluation stopped by `cancel` when it is set.
///
/// # Errors
/// The same as [`open_package`].
pub fn open_package_in(
    title: &str,
    pkg: &Package,
    base: Option<&Path>,
    cancel: Option<CancelFlag>,
) -> Result<Loaded, String> {
    let ws = Worksheet::from_package(pkg)
        .map_err(|e| trf("load.cannot_read_worksheet", &[title, &e.to_string()]))?;
    let table =
        load_results(pkg).map_err(|e| trf("load.cannot_read_results", &[title, &e.to_string()]))?;
    let doc = Document::from_worksheet(title, ws, table);
    let vm = evaluate_document_in(&doc, base, cancel);
    Ok(Loaded { doc, vm })
}

/// Evaluates a document as it is now, with no file access. Must run on a thread with the engine's
/// stack size.
pub fn evaluate_document(doc: &Document) -> ViewModel {
    evaluate_document_in(doc, None, None)
}

/// [`evaluate_document`] with the file functions reading under `base`, and the evaluation stopped
/// by `cancel` when it is set. Must run on a thread with the engine's stack size.
pub fn evaluate_document_in(
    doc: &Document,
    base: Option<&Path>,
    cancel: Option<CancelFlag>,
) -> ViewModel {
    let ws = doc.worksheet();
    let evaluation = evaluate_worksheet_in_limited(
        &doc.eval_worksheet(),
        doc.results(),
        base,
        &evaluation_limits(cancel),
    );
    build(&doc.title, &ws, &evaluation)
}

/// [`evaluate_document`] with explicit file options (the session's switches, see `file_access`),
/// and the evaluation stopped by `cancel` when it is set. Must run on a thread with the engine's
/// stack size.
pub fn evaluate_document_with(
    doc: &Document,
    files: FileOptions,
    cancel: Option<CancelFlag>,
) -> ViewModel {
    let ws = doc.worksheet();
    let evaluation = evaluate_worksheet_limited(
        &doc.eval_worksheet(),
        doc.results(),
        files,
        &evaluation_limits(cancel),
    );
    build(&doc.title, &ws, &evaluation)
}

/// The file name of `path`, or the whole path when it has none.
pub fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The path typed in the path box or the path a drop gives. A leading `~/` (or `~\`) is replaced
/// by the home directory, from `HOME` or, where that is not set, `USERPROFILE`. Other text is
/// returned as it is.
pub fn expand_home(text: &str) -> PathBuf {
    expand_home_in(text, home_dir())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()))
        .map(PathBuf::from)
}

/// [`expand_home`] with the home directory given, so it can be tested without the environment.
fn expand_home_in(text: &str, home: Option<PathBuf>) -> PathBuf {
    match (
        text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")),
        home,
    ) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_tilde_slash_is_the_home_directory() {
        let home = PathBuf::from("/home/someone");
        assert_eq!(
            expand_home_in("~/work/sheet.mcdx", Some(home.clone())),
            home.join("work/sheet.mcdx")
        );
    }

    #[test]
    fn without_a_home_directory_the_text_is_kept() {
        assert_eq!(
            expand_home_in("~/work/sheet.mcdx", None),
            PathBuf::from("~/work/sheet.mcdx")
        );
    }

    #[test]
    fn other_paths_are_kept() {
        let home = Some(PathBuf::from("/home/someone"));
        assert_eq!(
            expand_home_in("/tmp/a.mcdx", home.clone()),
            PathBuf::from("/tmp/a.mcdx")
        );
        assert_eq!(
            expand_home_in("~user/a.mcdx", home.clone()),
            PathBuf::from("~user/a.mcdx")
        );
        assert_eq!(
            expand_home_in("a/~/b.mcdx", home),
            PathBuf::from("a/~/b.mcdx")
        );
    }
}
