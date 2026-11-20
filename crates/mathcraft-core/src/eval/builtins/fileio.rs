//! Safe file access for the file functions: the path policy, the file-system trait the evaluator
//! is given, and the delimited text (CSV) reader and writer.
//!
//! Help: "Functions > Reading and Writing Files" (READCSV/WRITECSV, READTEXT/WRITETEXT). Not built
//! here: READFILE/WRITEFILE, READPRN, Excel, READBIN, the image readers (READBMP, READRGB, READ_IMAGE
//! and the HLS/HSV readers) and READWAV/GETWAVINFO. Those need their own format parsers.
//!
//! Switches: file access is off by default. [`FileSwitches`] turn reading and writing on for one
//! evaluation; the viewer sets them per session and per worksheet open (see the app's file access
//! state). A worksheet never turns them on by itself.
//!
//! Policy (checked by [`check_name`]):
//! - A file name is relative to a base directory (the worksheet's). `..`, `.`, empty components,
//!   backslashes, colons, control characters and names over 1024 bytes are refused.
//! - A component that starts with `.` (a hidden file or directory) is refused.
//! - Windows device names (CON, PRN, AUX, NUL, COM0-9, LPT0-9, with or without an extension) and
//!   components that end with a dot or a space are refused, so the policy means the same on every
//!   platform.
//! - Only the extensions in [`ALLOWED_EXTENSIONS`] are accepted. A `.mcdx` worksheet (and its
//!   `.mcdx.bak`) is therefore never read or written by these functions.
//! - An absolute name (leading `/`) is refused unless [`FilePolicy::allow_absolute`] is set.
//!
//! Links and file types ([`RealFs`]):
//! - The final component is opened with `O_NOFOLLOW | O_NONBLOCK` (Windows: the reparse-point flag),
//!   then checked with `fstat`: it must be a regular file. A symbolic link, a FIFO, a device
//!   or a socket is refused without blocking on it. Because the check is made on the open file, there
//!   is no check-then-open gap for the final component.
//! - A write refuses an existing file with other hard links (`nlink > 1`), so a link planted in the
//!   worksheet's folder cannot be used to reach a file elsewhere on the disk.
//! - Parent directories are walked with `symlink_metadata` before the open. This leaves a residual
//!   race: a parent directory replaced by a link between the walk and the open is followed. Closing
//!   it needs `openat`-style resolution (for example the `cap-std` crate); it is not done here. The
//!   base directory itself may be a link, since the user chose it.
//! - Writes are atomic: the bytes go to a new hidden temporary file in the same directory, which is
//!   synced and renamed over the target. A failure removes the temporary file and leaves the target
//!   as it was.
//! - The network is never used: there is no URL form.
//!
//! Budget: a read or write charges the statement's work budget per cell, and each evaluation has a
//! total cell and byte quota ([`FileOptions::reserve`]). The evaluator gives each file operation a
//! fresh [`FileOptions`] per evaluation (see `evaluate_worksheet_with`).
//!
//! The evaluator reaches files only through [`FileAccess`]. Tests use [`MemFs`]; the viewer passes
//! a [`RealFs`]. Parsers and writers take a `charge` callback that is called as they work, so the
//! caller can charge the work budget; an error from it stops the parse.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Which file operations are switched on for an evaluation. Both are off by default, and a
/// worksheet is never given them by its own content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileSwitches {
    pub read: bool,
    pub write: bool,
}

/// Cells that all file reads of one evaluation may produce together.
pub const EVAL_MAX_CELLS: u64 = 4_000_000;

/// Bytes that all file reads of one evaluation may take together.
pub const EVAL_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// The per-evaluation quota, shared by the clones of one [`FileOptions`].
#[derive(Debug)]
struct Quota {
    bytes: AtomicU64,
    cells: AtomicU64,
}

impl Default for Quota {
    fn default() -> Self {
        Self {
            bytes: AtomicU64::new(EVAL_MAX_BYTES),
            cells: AtomicU64::new(EVAL_MAX_CELLS),
        }
    }
}

/// The file access a worksheet's evaluation is given: an optional [`FileAccess`], the policy it
/// runs under, the switches, and the evaluation's quota. The default has no access at all, so
/// every file function fails until the caller opts in (see `Session::with_file_options`).
#[derive(Clone, Default)]
pub struct FileOptions {
    access: Option<Arc<dyn FileAccess + Send + Sync>>,
    policy: FilePolicy,
    switches: FileSwitches,
    quota: Arc<Quota>,
}

impl FileOptions {
    /// No file access (the default).
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Access through `access` under `policy`. Both switches start off.
    pub fn with_access(access: Arc<dyn FileAccess + Send + Sync>, policy: FilePolicy) -> Self {
        Self {
            access: Some(access),
            policy,
            ..Self::default()
        }
    }

    /// Real files under `base` (the worksheet's directory), with the default policy and both
    /// switches off. Fails when `base` is not an existing directory.
    pub fn real_under(base: &Path) -> FileResult<Self> {
        Ok(Self::with_access(
            Arc::new(RealFs::new(base)?),
            FilePolicy::default(),
        ))
    }

    /// The same access under another policy.
    pub fn with_policy(mut self, policy: FilePolicy) -> Self {
        self.policy = policy;
        self
    }

    /// The same access with the given switches.
    pub fn with_switches(mut self, switches: FileSwitches) -> Self {
        self.switches = switches;
        self
    }

    /// The access, or `None` when file access is disabled.
    pub fn access(&self) -> Option<&(dyn FileAccess + Send + Sync)> {
        self.access.as_deref()
    }

    pub fn policy(&self) -> FilePolicy {
        self.policy
    }

    pub fn switches(&self) -> FileSwitches {
        self.switches
    }

    /// Takes `bytes` and `cells` from the evaluation's quota. Fails with [`FileError::Quota`] when
    /// either is not left; nothing is taken then.
    pub fn reserve(&self, bytes: u64, cells: u64) -> FileResult<()> {
        let take = |counter: &AtomicU64, n: u64| {
            counter
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(n)
                })
                .map(|_| ())
                .map_err(|_| FileError::Quota)
        };
        let bytes_left = self.quota.bytes.load(Ordering::SeqCst);
        if bytes_left < bytes || self.quota.cells.load(Ordering::SeqCst) < cells {
            return Err(FileError::Quota);
        }
        take(&self.quota.bytes, bytes)?;
        take(&self.quota.cells, cells)
    }
}

/// Largest file (read or written), in bytes.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Longest accepted file name, in bytes.
const MAX_NAME_BYTES: usize = 1024;

/// The extensions (lower case, without the dot) that the file functions read and write.
pub const ALLOWED_EXTENSIONS: [&str; 6] = ["csv", "txt", "dat", "prn", "bmp", "wav"];

/// What the caller allows. The default allows only names relative to the base directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FilePolicy {
    /// Accept names starting with `/`. Off unless the user turns it on.
    pub allow_absolute: bool,
}

/// Why a file operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    /// The name or the file was refused by the policy (path tricks, links, directories).
    Refused(String),
    /// The file does not exist.
    NotFound(String),
    /// The file is larger than the limit (`size` is the known size or the limit reached).
    TooLarge { size: u64, limit: u64 },
    /// The content is not what the reader expects (not UTF-8, not a number, ragged rows).
    Format(String),
    /// The operating system refused the operation.
    Io(String),
    /// The work budget ran out (the `charge` callback failed).
    Budget,
    /// The evaluation's file quota (cells or bytes) is used up.
    Quota,
}

pub type FileResult<T> = Result<T, FileError>;

/// The file system the evaluator is given.
pub trait FileAccess {
    /// Reads the whole file `name`. Fails with `TooLarge` before reading when the file is over
    /// `limit` bytes.
    fn read(&self, name: &str, policy: FilePolicy, limit: u64) -> FileResult<Vec<u8>>;

    /// Creates or replaces the file `name` with `bytes`. The parent directory must exist.
    fn write(&self, name: &str, policy: FilePolicy, bytes: &[u8]) -> FileResult<()>;
}

/// Whether `component` is a Windows device name (`CON`, `PRN`, `AUX`, `NUL`, `COM0`-`COM9`,
/// `LPT0`-`LPT9`), in any case and with or without an extension. Such a name is refused on every
/// platform, so a worksheet cannot mean a device on Windows.
pub fn is_reserved_device_name(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        s => {
            s.len() == 4
                && (s.starts_with("COM") || s.starts_with("LPT"))
                && s.as_bytes()[3].is_ascii_digit()
        }
    }
}

/// Checks one path component: no hidden (leading dot) names, no names of only spaces, no trailing
/// dot or space, no Windows device name.
pub fn check_component(part: &str) -> FileResult<()> {
    if part.starts_with('.') {
        return Err(FileError::Refused(
            "hidden names (starting with a dot) are refused".to_string(),
        ));
    }
    if part.chars().all(|c| c == ' ') {
        return Err(FileError::Refused(
            "file names may not be only spaces".to_string(),
        ));
    }
    if part.ends_with('.') || part.ends_with(' ') {
        return Err(FileError::Refused(
            "file names may not end with a dot or a space".to_string(),
        ));
    }
    if is_reserved_device_name(part) {
        return Err(FileError::Refused(
            "device names such as CON, NUL or COM1 are refused".to_string(),
        ));
    }
    Ok(())
}

/// Checks the extension of the last component against [`ALLOWED_EXTENSIONS`].
pub fn check_extension(leaf: &str) -> FileResult<()> {
    match leaf.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && ALLOWED_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) =>
        {
            Ok(())
        }
        _ => Err(FileError::Refused(
            "only .csv, .txt, .dat, .prn, .bmp and .wav files can be read or written".to_string(),
        )),
    }
}

/// Splits and checks a file name. The result is whether it is absolute, and its components. An
/// absolute name (only when allowed) starts with `/` in the caller, which is why the leading slash
/// is not returned here: the caller joins the components to the root it chooses.
pub fn check_name(name: &str, policy: FilePolicy) -> FileResult<(bool, Vec<String>)> {
    if name.is_empty() || name.len() > MAX_NAME_BYTES {
        return Err(FileError::Refused(
            "file name is empty or too long".to_string(),
        ));
    }
    if name
        .chars()
        .any(|c| c.is_control() || c == '\\' || c == ':')
    {
        return Err(FileError::Refused(
            "file names may not contain control characters, backslashes or colons".to_string(),
        ));
    }
    let absolute = name.starts_with('/');
    if absolute && !policy.allow_absolute {
        return Err(FileError::Refused(
            "absolute file names are not allowed".to_string(),
        ));
    }
    let body = if absolute { &name[1..] } else { name };
    let mut parts = Vec::new();
    for part in body.split('/') {
        match part {
            "" => {
                return Err(FileError::Refused(
                    "file names may not have empty components".to_string(),
                ))
            }
            "." | ".." => {
                return Err(FileError::Refused(
                    "file names may not contain . or .. components".to_string(),
                ))
            }
            _ => {
                check_component(part)?;
                parts.push(part.to_string());
            }
        }
    }
    match parts.last() {
        Some(leaf) => check_extension(leaf)?,
        None => {
            return Err(FileError::Refused(
                "file names may not have empty components".to_string(),
            ))
        }
    }
    Ok((absolute, parts))
}

/// A checked file name: whether it is absolute, and its components.
type FileKey = (bool, Vec<String>);

/// In-memory file system for tests. Files are keyed by their checked components.
#[derive(Debug, Default)]
pub struct MemFs {
    files: Mutex<HashMap<FileKey, Vec<u8>>>,
}

impl MemFs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Places a file directly, for a test that needs content the evaluator did not write.
    pub fn put(&self, name: &str, policy: FilePolicy, bytes: &[u8]) -> FileResult<()> {
        let key = check_name(name, policy)?;
        self.files
            .lock()
            .map_err(|_| FileError::Io("file store poisoned".to_string()))?
            .insert(key, bytes.to_vec());
        Ok(())
    }
}

impl FileAccess for MemFs {
    fn read(&self, name: &str, policy: FilePolicy, limit: u64) -> FileResult<Vec<u8>> {
        let key = check_name(name, policy)?;
        let files = self
            .files
            .lock()
            .map_err(|_| FileError::Io("file store poisoned".to_string()))?;
        let bytes = files
            .get(&key)
            .ok_or_else(|| FileError::NotFound(name.to_string()))?;
        if bytes.len() as u64 > limit {
            return Err(FileError::TooLarge {
                size: bytes.len() as u64,
                limit,
            });
        }
        Ok(bytes.clone())
    }

    fn write(&self, name: &str, policy: FilePolicy, bytes: &[u8]) -> FileResult<()> {
        let key = check_name(name, policy)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(FileError::TooLarge {
                size: bytes.len() as u64,
                limit: MAX_FILE_BYTES,
            });
        }
        self.files
            .lock()
            .map_err(|_| FileError::Io("file store poisoned".to_string()))?
            .insert(key, bytes.to_vec());
        Ok(())
    }
}

/// The real file system under one base directory.
#[derive(Debug, Clone)]
pub struct RealFs {
    base: PathBuf,
}

impl RealFs {
    /// A file system rooted at `base`, which must be an existing directory.
    pub fn new(base: &Path) -> FileResult<Self> {
        let base = base
            .canonicalize()
            .map_err(|e| FileError::Io(e.to_string()))?;
        if !base.is_dir() {
            return Err(FileError::Refused(
                "the base is not a directory".to_string(),
            ));
        }
        Ok(Self { base })
    }

    /// The path of a checked name. Every existing parent directory must be a real directory, never
    /// a symbolic link (the residual race is in the module docs). The leaf is not checked here: it
    /// is opened with [`open_checked`].
    fn locate(&self, name: &str, policy: FilePolicy) -> FileResult<PathBuf> {
        let (absolute, parts) = check_name(name, policy)?;
        let mut path = if absolute {
            PathBuf::from("/")
        } else {
            self.base.clone()
        };
        let Some((leaf, parents)) = parts.split_last() else {
            return Err(FileError::Refused("file name is empty".to_string()));
        };
        for part in parents {
            path.push(part);
            let meta = fs::symlink_metadata(&path).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    FileError::NotFound(name.to_string())
                } else {
                    FileError::Io(e.to_string())
                }
            })?;
            if meta.file_type().is_symlink() {
                return Err(FileError::Refused("symbolic links are refused".to_string()));
            }
            if !meta.is_dir() {
                return Err(FileError::Refused(
                    "a parent of the file is not a directory".to_string(),
                ));
            }
        }
        path.push(leaf);
        Ok(path)
    }
}

/// Opens the leaf without following a link and without blocking on a FIFO. Unix: `O_NOFOLLOW |
/// O_NONBLOCK`. Windows: `FILE_FLAG_OPEN_REPARSE_POINT`, so the link itself is opened (it is then
/// refused by [`regular_file`]). A missing file is `NotFound`, a link is `Refused`.
fn open_checked(path: &Path, name: &str, write: bool) -> FileResult<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(!write).write(write);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT (winbase.h): open a symbolic link itself, not its target.
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            FileError::NotFound(name.to_string())
        } else if is_link_error(&e) {
            FileError::Refused("symbolic links are refused".to_string())
        } else {
            FileError::Io(e.to_string())
        }
    })
}

/// Whether an open failed because the leaf is a symbolic link (`ELOOP` with `O_NOFOLLOW`).
fn is_link_error(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(libc::ELOOP)
    }
    #[cfg(not(unix))]
    {
        let _ = e;
        false
    }
}

/// The metadata of an open file, which must be a regular file (not a link, FIFO, device or
/// directory).
fn regular_file(file: &fs::File) -> FileResult<fs::Metadata> {
    let meta = file.metadata().map_err(|e| FileError::Io(e.to_string()))?;
    if meta.file_type().is_symlink() {
        return Err(FileError::Refused("symbolic links are refused".to_string()));
    }
    if !meta.is_file() {
        return Err(FileError::Refused("not a regular file".to_string()));
    }
    Ok(meta)
}

/// Refuses a file with other hard links: writing it would change the other names too.
fn refuse_hard_links(meta: &fs::Metadata) -> FileResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() > 1 {
            return Err(FileError::Refused(
                "the file has other hard links, so it is not overwritten".to_string(),
            ));
        }
    }
    #[cfg(not(unix))]
    {
        // The hard-link count is not stable in std on Windows; the link check is unix-only.
        let _ = meta;
    }
    Ok(())
}

/// Counts the temporary files made, so each one has its own name.
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Writes `bytes` to a new hidden file in `dir`, syncs it, and renames it over `target`. On any
/// failure the temporary file is removed and `target` is left as it was. `fail_before_rename` is a
/// test hook that injects a failure after the bytes are written.
fn write_atomic(
    dir: &Path,
    target: &Path,
    bytes: &[u8],
    fail_before_rename: bool,
) -> FileResult<()> {
    let seq = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = dir.join(format!(".mathcraft-{}-{seq}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| FileError::Io(e.to_string()))?;
    let written = file
        .write_all(bytes)
        .and_then(|()| {
            if fail_before_rename {
                Err(std::io::Error::other("injected failure"))
            } else {
                Ok(())
            }
        })
        .and_then(|()| file.sync_all());
    drop(file);
    let result = written.and_then(|()| fs::rename(&temp, target));
    if let Err(e) = result {
        let _ = fs::remove_file(&temp);
        return Err(FileError::Io(e.to_string()));
    }
    Ok(())
}

impl FileAccess for RealFs {
    fn read(&self, name: &str, policy: FilePolicy, limit: u64) -> FileResult<Vec<u8>> {
        let path = self.locate(name, policy)?;
        let file = open_checked(&path, name, false)?;
        let meta = regular_file(&file)?;
        if meta.len() > limit {
            return Err(FileError::TooLarge {
                size: meta.len(),
                limit,
            });
        }
        // Read one byte past the limit, in case the file grew after the size check.
        let mut bytes = Vec::with_capacity(meta.len().min(limit) as usize);
        file.take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| FileError::Io(e.to_string()))?;
        if bytes.len() as u64 > limit {
            return Err(FileError::TooLarge {
                size: bytes.len() as u64,
                limit,
            });
        }
        Ok(bytes)
    }

    fn write(&self, name: &str, policy: FilePolicy, bytes: &[u8]) -> FileResult<()> {
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(FileError::TooLarge {
                size: bytes.len() as u64,
                limit: MAX_FILE_BYTES,
            });
        }
        let target = self.locate(name, policy)?;
        match open_checked(&target, name, false) {
            Ok(file) => refuse_hard_links(&regular_file(&file)?)?,
            Err(FileError::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
        let dir = target
            .parent()
            .ok_or_else(|| FileError::Refused("file name has no directory".to_string()))?;
        write_atomic(dir, &target, bytes, false)
    }
}

/// The text of a delimited file as a flat grid: `rows * cols` numbers, row by row.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f64>,
}

/// Parses delimited text (CSV with `,`) into a grid of numbers. A leading byte order mark is
/// skipped. Blank lines are skipped. Every field must be a finite real number; every row must have
/// the same number of fields; the content must be UTF-8; the total is capped at `max_cells`.
/// `charge` is called once per cell, so a large file runs out of the work budget.
pub fn parse_delimited(
    bytes: &[u8],
    delimiter: char,
    max_cells: usize,
    charge: &mut dyn FnMut(u64) -> FileResult<()>,
) -> FileResult<Grid> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| FileError::Format("the file is not UTF-8 text".to_string()))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut data: Vec<f64> = Vec::new();
    let mut rows = 0usize;
    let mut width: Option<usize> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut row_len = 0usize;
        // A space as the delimiter means runs of whitespace (space or tab).
        let fields: Box<dyn Iterator<Item = &str>> = if delimiter == ' ' {
            Box::new(line.split_whitespace())
        } else {
            Box::new(line.split(delimiter))
        };
        for field in fields {
            charge(1)?;
            let field = field.trim();
            let x: f64 = field
                .parse()
                .map_err(|_| FileError::Format(format!("not a number: {field:?}")))?;
            if !x.is_finite() {
                return Err(FileError::Format("values must be finite".to_string()));
            }
            if data.len() >= max_cells {
                return Err(FileError::TooLarge {
                    size: data.len() as u64 + 1,
                    limit: max_cells as u64,
                });
            }
            data.push(x);
            row_len += 1;
        }
        match width {
            None => width = Some(row_len),
            Some(w) if w != row_len => {
                return Err(FileError::Format(
                    "every row must have the same number of fields".to_string(),
                ))
            }
            Some(_) => {}
        }
        rows += 1;
    }
    let cols = width.unwrap_or(0);
    if rows == 0 || cols == 0 {
        return Err(FileError::Format("the file has no numbers".to_string()));
    }
    Ok(Grid { rows, cols, data })
}

/// Formats rows of finite numbers as delimited text, one row per line, with `\n` line ends. The
/// output is capped at [`MAX_FILE_BYTES`]. Numbers use the shortest form that reads back exactly.
/// `charge` is called once per cell.
pub fn format_delimited(
    rows: &[Vec<f64>],
    delimiter: char,
    charge: &mut dyn FnMut(u64) -> FileResult<()>,
) -> FileResult<Vec<u8>> {
    let mut out = String::new();
    for row in rows {
        for (k, x) in row.iter().enumerate() {
            charge(1)?;
            if !x.is_finite() {
                return Err(FileError::Format(
                    "cannot write NaN or infinity".to_string(),
                ));
            }
            if k > 0 {
                out.push(delimiter);
            }
            out.push_str(&format!("{x}"));
            if out.len() as u64 > MAX_FILE_BYTES {
                return Err(FileError::TooLarge {
                    size: out.len() as u64,
                    limit: MAX_FILE_BYTES,
                });
            }
        }
        out.push('\n');
    }
    Ok(out.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn free(_: u64) -> FileResult<()> {
        Ok(())
    }

    fn rel() -> FilePolicy {
        FilePolicy::default()
    }

    /// A fresh directory for one test, named after it, so parallel tests do not share files.
    fn scratch(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mathcraft-fileio-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn names_relative_to_the_base_are_accepted() {
        assert_eq!(
            check_name("data/run1.csv", rel()),
            Ok((false, vec!["data".to_string(), "run1.csv".to_string()]))
        );
    }

    #[test]
    fn path_tricks_are_refused() {
        for bad in [
            "../secret.csv",
            "a/../../secret.csv",
            "./a.csv",
            "a//b.csv",
            "a/",
            "a\\..\\b.csv",
            "C:x.csv",
            "a/b:c.csv",
            "a\u{0}b.csv",
            "a\nb.csv",
            "",
        ] {
            assert!(
                matches!(check_name(bad, rel()), Err(FileError::Refused(_))),
                "{bad:?} should be refused"
            );
        }
    }

    #[test]
    fn absolute_names_need_the_flag() {
        assert!(matches!(
            check_name("/etc/passwd", rel()),
            Err(FileError::Refused(_))
        ));
        let allow = FilePolicy {
            allow_absolute: true,
        };
        assert_eq!(
            check_name("/tmp/a.csv", allow),
            Ok((true, vec!["tmp".to_string(), "a.csv".to_string()]))
        );
        // The flag does not lift the other rules.
        assert!(matches!(
            check_name("/tmp/../a.csv", allow),
            Err(FileError::Refused(_))
        ));
    }

    #[test]
    fn overlong_names_are_refused() {
        let long = "a".repeat(MAX_NAME_BYTES + 1);
        assert!(matches!(
            check_name(&long, rel()),
            Err(FileError::Refused(_))
        ));
    }

    #[test]
    fn hidden_names_are_refused_in_any_component() {
        for bad in [".bashrc.csv", "a/.hidden.csv", ".ssh/x.csv", "a/.b/c.txt"] {
            assert!(
                matches!(check_name(bad, rel()), Err(FileError::Refused(_))),
                "{bad:?} should be refused"
            );
        }
    }

    #[test]
    fn worksheets_and_other_extensions_are_refused() {
        for bad in [
            "sheet.mcdx",
            "sheet.mcdx.bak",
            "sheet.MCDX",
            "run.exe",
            "noext",
            ".csv",
        ] {
            assert!(
                matches!(check_name(bad, rel()), Err(FileError::Refused(_))),
                "{bad:?} should be refused"
            );
        }
        assert!(check_name("RUN.CSV", rel()).is_ok());
        assert!(check_name("a.b.dat", rel()).is_ok());
    }

    #[test]
    fn windows_device_names_are_refused_on_every_platform() {
        for bad in [
            "CON",
            "con.txt",
            "NUL.csv",
            "aux",
            "PRN.dat",
            "COM1.csv",
            "lpt9",
            "COM0.txt",
            "a/CON.csv",
            "x.",
            "x ",
            "...",
            " ",
        ] {
            assert!(check_name(bad, rel()).is_err(), "{bad:?} should be refused");
        }
        for ok in [
            "CONSOLE.csv",
            "COM10.csv",
            "console.txt",
            "lpt.csv",
            "NULL.csv",
        ] {
            assert!(check_name(ok, rel()).is_ok(), "{ok:?} should be accepted");
        }
        assert!(is_reserved_device_name("Con"));
        assert!(!is_reserved_device_name("COM10"));
    }

    #[test]
    fn mem_fs_round_trips_a_csv() {
        let fs = MemFs::new();
        let rows = vec![vec![1.0, 2.5], vec![-3.0, 1e-7]];
        let bytes = format_delimited(&rows, ',', &mut free).unwrap();
        fs.write("out.csv", rel(), &bytes).unwrap();
        let back = fs.read("out.csv", rel(), MAX_FILE_BYTES).unwrap();
        let parsed = parse_delimited(&back, ',', 1000, &mut free).unwrap();
        assert_eq!(parsed.data, vec![1.0, 2.5, -3.0, 1e-7]);
        assert_eq!((parsed.rows, parsed.cols), (2, 2));
    }

    #[test]
    fn mem_fs_refuses_missing_and_oversized_reads() {
        let fs = MemFs::new();
        assert!(matches!(
            fs.read("nope.csv", rel(), 10),
            Err(FileError::NotFound(_))
        ));
        fs.put("big.txt", rel(), &[b'1'; 100]).unwrap();
        assert_eq!(
            fs.read("big.txt", rel(), 50),
            Err(FileError::TooLarge {
                size: 100,
                limit: 50
            })
        );
        assert!(fs.read("../big.txt", rel(), 500).is_err());
    }

    #[test]
    fn default_limit_is_64_mib() {
        assert_eq!(MAX_FILE_BYTES, 64 * 1024 * 1024);
    }

    #[test]
    fn binary_garbage_is_refused_as_text() {
        let garbage = [0xff, 0xfe, 0x00, 0x81, 0x12];
        assert!(matches!(
            parse_delimited(&garbage, ',', 100, &mut free),
            Err(FileError::Format(_))
        ));
    }

    #[test]
    fn csv_rejects_ragged_rows_nan_and_empty_files() {
        assert!(parse_delimited(b"1,2\n3\n", ',', 100, &mut free).is_err());
        assert!(parse_delimited(b"1,nan\n", ',', 100, &mut free).is_err());
        assert!(parse_delimited(b"1,inf\n", ',', 100, &mut free).is_err());
        assert!(parse_delimited(b"1,,2\n", ',', 100, &mut free).is_err());
        assert!(parse_delimited(b"\n  \n", ',', 100, &mut free).is_err());
    }

    #[test]
    fn csv_skips_blank_lines_and_trims_fields() {
        let got = parse_delimited(b" 1 , 2 \r\n\r\n3,4\r\n", ',', 100, &mut free).unwrap();
        assert_eq!(got.data, vec![1.0, 2.0, 3.0, 4.0]);
        assert_eq!((got.rows, got.cols), (2, 2));
    }

    #[test]
    fn a_utf8_byte_order_mark_is_skipped() {
        let got = parse_delimited(b"\xEF\xBB\xBF1,2\n3,4\n", ',', 100, &mut free).unwrap();
        assert_eq!(got.data, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn csv_cell_cap_and_budget_stop_the_parse() {
        assert!(matches!(
            parse_delimited(b"1,2,3\n4,5,6\n", ',', 4, &mut free),
            Err(FileError::TooLarge { .. })
        ));
        let mut out_of_budget = |_: u64| -> FileResult<()> { Err(FileError::Budget) };
        assert_eq!(
            parse_delimited(b"1,2\n", ',', 100, &mut out_of_budget),
            Err(FileError::Budget)
        );
    }

    #[test]
    fn four_million_line_file_is_refused_quickly() {
        // 4,000,000 one-digit lines (8 MB): the cell cap stops the parse after one million cells.
        let bytes = b"1\n".repeat(4_000_000);
        let started = std::time::Instant::now();
        let got = parse_delimited(&bytes, ',', 1_000_000, &mut free);
        assert!(matches!(got, Err(FileError::TooLarge { .. })), "{got:?}");
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn the_budget_is_charged_per_cell() {
        let mut seen = 0u64;
        let mut count = |units: u64| -> FileResult<()> {
            seen += units;
            Ok(())
        };
        parse_delimited(b"1,2,3\n4,5,6\n", ',', 100, &mut count).unwrap();
        assert_eq!(seen, 6);
    }

    #[test]
    fn the_evaluation_quota_is_shared_and_runs_out() {
        let base = FileOptions::with_access(Arc::new(MemFs::new()), rel());
        base.reserve(10, EVAL_MAX_CELLS - 5).unwrap();
        let clone = base.clone();
        assert_eq!(clone.reserve(0, 6), Err(FileError::Quota));
        assert!(clone.reserve(0, 5).is_ok());
        assert_eq!(base.reserve(0, 1), Err(FileError::Quota));
    }

    #[test]
    fn a_base_directory_alone_starts_with_both_switches_off() {
        let dir = scratch("switches");
        let opts = FileOptions::real_under(&dir).unwrap();
        assert_eq!(opts.switches(), FileSwitches::default());
        assert!(opts.access().is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn writer_refuses_non_finite_values() {
        assert!(matches!(
            format_delimited(&[vec![f64::NAN]], ',', &mut free),
            Err(FileError::Format(_))
        ));
    }

    #[test]
    fn real_fs_refuses_traversal_and_symlinks() {
        let dir = scratch("traversal");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/ok.csv"), b"1,2\n").unwrap();
        let outside = dir.join("outside.csv");
        fs::write(&outside, b"9\n").unwrap();
        let base = dir.join("sub");
        let real = RealFs::new(&base).unwrap();
        assert_eq!(real.read("ok.csv", rel(), 100).unwrap(), b"1,2\n");
        assert!(matches!(
            real.read("../ok.csv", rel(), 100),
            Err(FileError::Refused(_))
        ));
        assert!(matches!(
            real.read("missing.csv", rel(), 100),
            Err(FileError::NotFound(_))
        ));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, base.join("link.csv")).unwrap();
            assert!(matches!(
                real.read("link.csv", rel(), 100),
                Err(FileError::Refused(_))
            ));
            assert!(matches!(
                real.write("link.csv", rel(), b"x"),
                Err(FileError::Refused(_))
            ));
            assert_eq!(fs::read(&outside).unwrap(), b"9\n");
        }
        real.write("new.csv", rel(), b"5,6\n").unwrap();
        assert_eq!(fs::read(base.join("new.csv")).unwrap(), b"5,6\n");
        assert!(matches!(
            real.read("ok.csv", rel(), 1),
            Err(FileError::TooLarge { .. })
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_hard_link_is_not_overwritten() {
        let dir = scratch("hardlink");
        let base = dir.join("sub");
        fs::create_dir_all(&base).unwrap();
        // A file outside the base, hard-linked into it: writing the link would change the original.
        let outside = dir.join("secret.txt");
        fs::write(&outside, b"keep me\n").unwrap();
        fs::hard_link(&outside, base.join("link.csv")).unwrap();
        let real = RealFs::new(&base).unwrap();
        assert!(matches!(
            real.write("link.csv", rel(), b"overwritten\n"),
            Err(FileError::Refused(_))
        ));
        assert_eq!(fs::read(&outside).unwrap(), b"keep me\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_refused_without_hanging() {
        let dir = scratch("fifo");
        let fifo = dir.join("pipe.csv");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs");
        assert!(status.success());
        let real = RealFs::new(&dir).unwrap();
        // Both calls return at once: the open is non-blocking and the file type is checked.
        assert!(matches!(
            real.read("pipe.csv", rel(), 100),
            Err(FileError::Refused(_))
        ));
        assert!(matches!(
            real.write("pipe.csv", rel(), b"1\n"),
            Err(FileError::Refused(_))
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dotfile_and_a_worksheet_are_not_written() {
        let dir = scratch("dotfile");
        fs::write(dir.join("sheet.mcdx"), b"zip bytes").unwrap();
        let real = RealFs::new(&dir).unwrap();
        assert!(matches!(
            real.write(".profile.csv", rel(), b"1\n"),
            Err(FileError::Refused(_))
        ));
        assert!(matches!(
            real.write("sheet.mcdx", rel(), b"1\n"),
            Err(FileError::Refused(_))
        ));
        assert_eq!(fs::read(dir.join("sheet.mcdx")).unwrap(), b"zip bytes");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_atomic_write_leaves_the_original_intact() {
        let dir = scratch("atomic");
        let target = dir.join("data.csv");
        fs::write(&target, b"original\n").unwrap();
        assert!(write_atomic(&dir, &target, b"new\n", true).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"original\n");
        let leftovers = fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            })
            .count();
        assert_eq!(leftovers, 0, "the temporary file is removed on failure");
        write_atomic(&dir, &target, b"new\n", false).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_output_is_refused_before_writing() {
        let dir = scratch("oversized");
        let real = RealFs::new(&dir).unwrap();
        let big = vec![0u8; MAX_FILE_BYTES as usize + 1];
        assert!(matches!(
            real.write("big.csv", rel(), &big),
            Err(FileError::TooLarge { .. })
        ));
        assert!(!dir.join("big.csv").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
