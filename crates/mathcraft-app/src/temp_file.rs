//! Creating new files under names that cannot be guessed or reused.
//!
//! A file is always opened with `create_new`, which fails when the name exists, and a symbolic link
//! at the name is never followed. A name that is taken is simply not used: the next one is tried.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// How many names are tried before giving up.
const ATTEMPTS: u32 = 100;

/// Counts the unique suffixes made in this process.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Creates a new file in `dir`. `name` gives the file name for each attempt, starting at 0; an
/// attempt whose name exists fails with `AlreadyExists` and the next one is tried.
///
/// # Errors
/// The first error other than `AlreadyExists`, or the last `AlreadyExists` after the last attempt.
pub fn create_new_in(
    dir: &Path,
    mut name: impl FnMut(u32) -> String,
) -> io::Result<(PathBuf, File)> {
    let mut taken = None;
    for attempt in 0..ATTEMPTS {
        let path = dir.join(name(attempt));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => taken = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(taken.unwrap_or_else(|| io::Error::other("no free file name")))
}

/// A suffix that is unique to this process and call: the process ID, the time in nanoseconds and a
/// counter. Two calls never return the same suffix.
pub fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{nanos}-{count}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mathcraft-temp-{}-{}-{name}",
            std::process::id(),
            unique_suffix()
        ));
        fs::create_dir_all(&dir).expect("scratch folder");
        dir
    }

    #[test]
    fn unique_suffixes_differ() {
        let a = unique_suffix();
        let b = unique_suffix();
        assert_ne!(a, b);
        assert!(a.starts_with(&format!("{}-", std::process::id())));
    }

    #[test]
    fn a_taken_name_is_skipped_for_the_next_one() {
        let dir = scratch("taken");
        fs::write(dir.join("a.tmp"), b"someone else's").expect("existing file");
        let (path, _file) = create_new_in(&dir, |n| {
            format!(
                "a{}.tmp",
                if n == 0 { String::new() } else { n.to_string() }
            )
        })
        .expect("second name is free");
        assert_eq!(path.file_name().expect("name"), "a1.tmp");
        assert_eq!(
            fs::read(dir.join("a.tmp")).expect("read"),
            b"someone else's"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_existing_symbolic_link_is_never_followed() {
        let dir = scratch("symlink");
        let victim = dir.join("victim.txt");
        fs::write(&victim, b"keep").expect("victim");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&victim, dir.join("link.tmp")).expect("symlink");
            let (path, _file) = create_new_in(&dir, |n| {
                if n == 0 {
                    "link.tmp".to_string()
                } else {
                    format!("link{n}.tmp")
                }
            })
            .expect("a new name");
            assert_eq!(path.file_name().expect("name"), "link1.tmp");
            assert_eq!(fs::read(&victim).expect("read"), b"keep");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_attempt_taken_is_an_error_and_writes_nothing() {
        let dir = scratch("all-taken");
        fs::write(dir.join("x"), b"").expect("existing file");
        let err = create_new_in(&dir, |_| "x".to_string()).expect_err("all names taken");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(dir.join("x")).expect("read"), b"");
        let _ = fs::remove_dir_all(&dir);
    }
}
