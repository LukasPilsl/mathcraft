//! Reads a file that a user named, without hanging on a FIFO or device and without being redirected
//! by a link that appears after the path was checked.
//!
//! The caller first resolves the path (canonical, inside its roots) and passes that canonical path
//! as `expected`. The file is then opened once, checked to be a regular file, and checked again to
//! still sit at `expected` with the same identity. Every read goes through the open handle.

use std::fs::{File, Metadata};
use std::io::{self, Read};
use std::path::Path;

/// Why a regular file could not be opened or read.
#[derive(Debug, thiserror::Error)]
pub enum RegularFileError {
    /// A FIFO, device, socket or directory.
    #[error("not a regular file")]
    NotRegular,
    /// Larger than the limit, or it grew past the limit while being read.
    #[error("the file is larger than the limit of {limit} bytes")]
    TooLarge { limit: u64 },
    /// The name no longer leads to the expected file (it was replaced, moved or re-linked).
    #[error("the file changed while it was being opened")]
    Changed,
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl From<RegularFileError> for io::Error {
    fn from(e: RegularFileError) -> Self {
        match e {
            RegularFileError::Io(e) => e,
            other => io::Error::new(io::ErrorKind::InvalidInput, other.to_string()),
        }
    }
}

/// Opens `path` for reading, once, and returns the handle if it is a regular file that still
/// resolves to `expected`.
///
/// Unix: the open uses `O_NONBLOCK` (a FIFO open returns at once instead of waiting for a writer)
/// and `O_NOFOLLOW` (a link at the final name fails the open). The metadata of the handle must be
/// a regular file, and `path` must still canonicalize to `expected` and name the same inode.
/// Other platforms: the same checks without the open flags.
///
/// # Errors
/// [`RegularFileError::NotRegular`] for a FIFO, device, socket or directory,
/// [`RegularFileError::Changed`] when the name no longer leads to `expected`, and
/// [`RegularFileError::Io`] for any other failure.
pub fn open_regular_file(path: &Path, expected: &Path) -> Result<File, RegularFileError> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(RegularFileError::NotRegular);
    }
    // The name must still lead to the place the caller checked, and to this very file.
    let now = std::fs::canonicalize(path)?;
    if now != expected {
        return Err(RegularFileError::Changed);
    }
    if !same_file(&meta, &std::fs::metadata(&now)?) {
        return Err(RegularFileError::Changed);
    }
    Ok(file)
}

/// Reads at most `limit` bytes of the regular file that [`open_regular_file`] accepts.
///
/// # Errors
/// As [`open_regular_file`], plus [`RegularFileError::TooLarge`] when the file is over `limit`.
pub fn read_regular_file(
    path: &Path,
    expected: &Path,
    limit: u64,
) -> Result<Vec<u8>, RegularFileError> {
    let file = open_regular_file(path, expected)?;
    if file.metadata()?.len() > limit {
        return Err(RegularFileError::TooLarge { limit });
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(RegularFileError::TooLarge { limit });
    }
    Ok(bytes)
}

#[cfg(unix)]
fn same_file(a: &Metadata, b: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(not(unix))]
fn same_file(_a: &Metadata, _b: &Metadata) -> bool {
    // The canonical path check is the identity check here.
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mathcraft-regular-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::canonicalize(&dir).unwrap()
    }

    #[test]
    fn reads_a_regular_file_up_to_the_limit() {
        let dir = scratch("reads");
        let file = dir.join("a.bin");
        std::fs::write(&file, b"hello").unwrap();
        assert_eq!(read_regular_file(&file, &file, 5).unwrap(), b"hello");
        assert!(matches!(
            read_regular_file(&file, &file, 4),
            Err(RegularFileError::TooLarge { limit: 4 })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_different_expected_path_is_changed() {
        let dir = scratch("changed");
        let file = dir.join("a.bin");
        let other = dir.join("b.bin");
        std::fs::write(&file, b"x").unwrap();
        std::fs::write(&other, b"y").unwrap();
        assert!(matches!(
            open_regular_file(&file, &other),
            Err(RegularFileError::Changed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_refused_without_blocking() {
        let dir = scratch("fifo");
        let fifo = dir.join("pipe.mcdx");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        let (tx, rx) = std::sync::mpsc::channel();
        let target = fifo.clone();
        std::thread::spawn(move || {
            let _ = tx.send(read_regular_file(&target, &target, 1024).map(|_| ()));
        });
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("opening a FIFO must not block");
        assert!(matches!(result, Err(RegularFileError::NotRegular)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
