//! File roots. A worksheet path from a client is only used when it lies inside a configured root
//! after every symlink and `..` is resolved. Only `.mcdx` files can be opened or written, so a
//! root such as the Documents folder does not expose other files.

use std::path::{Component, Path, PathBuf};

use crate::error::ToolError;

#[derive(Debug, Clone)]
pub struct Roots {
    /// Canonical directories.
    roots: Vec<PathBuf>,
}

impl Roots {
    /// Builds the root list. A directory that does not exist is skipped; an empty list is an error.
    pub fn new(dirs: &[PathBuf]) -> Result<Self, String> {
        let roots: Vec<PathBuf> = dirs
            .iter()
            .filter_map(|d| std::fs::canonicalize(d).ok())
            .filter(|d| d.is_dir())
            .collect();
        if roots.is_empty() {
            return Err("no usable root directory".into());
        }
        Ok(Self { roots })
    }

    /// The default roots: the current directory and `~/Documents` when it exists.
    pub fn default_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            dirs.push(cwd);
        }
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        if let Some(home) = home {
            dirs.push(PathBuf::from(home).join("Documents"));
        }
        dirs
    }

    pub fn list(&self) -> &[PathBuf] {
        &self.roots
    }

    fn contains(&self, canonical: &Path) -> bool {
        self.roots.iter().any(|r| canonical.starts_with(r))
    }

    /// The textual checks shared by reading and writing. Returns the path to resolve: a relative
    /// path is taken relative to the first root.
    fn prepare(&self, text: &str) -> Result<PathBuf, ToolError> {
        self.prepare_as(
            text,
            &["mcdx"],
            "only .mcdx worksheets can be opened or saved",
        )
    }

    /// As [`Roots::prepare`], for the files with one of the extensions `exts` (lower case).
    fn prepare_as(&self, text: &str, exts: &[&str], refusal: &str) -> Result<PathBuf, ToolError> {
        if text.is_empty() || text.contains('\0') {
            return Err(ToolError::PathDenied("empty or invalid path".into()));
        }
        let path = Path::new(text);
        if path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(ToolError::PathDenied(
                "`..` is not allowed in a path".into(),
            ));
        }
        for name in self.below_root(path) {
            check_name(&name)?;
        }
        let ext_ok = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| exts.iter().any(|x| e.eq_ignore_ascii_case(x)));
        if !ext_ok {
            return Err(ToolError::PathDenied(refusal.into()));
        }
        Ok(if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.roots[0].join(path)
        })
    }

    /// The components of an absolute path that lie below the configured root it is in. Only these
    /// are checked by `check_name`: the root and the folders above it are whatever the user chose
    /// (`/Users/x/Proj: 2024/a.mcdx` with the root `/Users/x/Proj: 2024`, or an ancestor named
    /// `aux`, are fine). A path that spells the root another way (a symlink, a Windows verbatim
    /// prefix) is split at the nearest existing ancestor that resolves inside a root. A relative
    /// path is relative to the first root, so all of it is below the root.
    fn below_root(&self, path: &Path) -> Vec<String> {
        let names = |p: &Path| -> Vec<String> {
            p.components()
                .filter_map(|c| match c {
                    Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect()
        };
        if !path.is_absolute() {
            return names(path);
        }
        if let Some(root) = self.roots.iter().find(|r| path.starts_with(r)) {
            return names(path.strip_prefix(root).unwrap_or(Path::new("")));
        }
        let mut current = path;
        while let Some(parent) = current.parent() {
            if std::fs::canonicalize(parent).is_ok_and(|c| self.contains(&c)) {
                return names(path.strip_prefix(parent).unwrap_or(Path::new("")));
            }
            current = parent;
        }
        names(path)
    }

    fn outside(&self) -> ToolError {
        ToolError::PathDenied(format!(
            "the path is outside the allowed roots ({})",
            self.roots
                .iter()
                .map(|r| r.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    /// An existing file to read. Symlinks are resolved first, so a link that points out of a root
    /// is refused.
    pub fn resolve_existing(&self, text: &str) -> Result<PathBuf, ToolError> {
        let path = self.prepare(text)?;
        self.existing(text, &path)
    }

    /// An existing image file to insert (`.png`, `.jpg` or `.jpeg`), with the same root checks as
    /// [`Roots::resolve_existing`].
    pub fn resolve_existing_image(&self, text: &str) -> Result<PathBuf, ToolError> {
        let path = self.prepare_as(
            text,
            &["png", "jpg", "jpeg"],
            "only .png, .jpg or .jpeg images can be inserted",
        )?;
        self.existing(text, &path)
    }

    fn existing(&self, text: &str, path: &Path) -> Result<PathBuf, ToolError> {
        let canonical = std::fs::canonicalize(path)
            .map_err(|e| ToolError::Doc(format!("cannot open {text}: {e}")))?;
        if !self.contains(&canonical) {
            return Err(self.outside());
        }
        Ok(canonical)
    }

    /// A file to write, which may not exist yet. The directory must exist and lie inside a root;
    /// an existing target is resolved through symlinks and must lie inside a root too.
    pub fn resolve_for_write(&self, text: &str) -> Result<PathBuf, ToolError> {
        let path = self.prepare(text)?;
        let name = path
            .file_name()
            .ok_or_else(|| ToolError::PathDenied("the path has no file name".into()))?
            .to_owned();
        let parent = path.parent().unwrap_or(Path::new("."));
        let parent = std::fs::canonicalize(parent)
            .map_err(|e| ToolError::PathDenied(format!("the directory is not usable: {e}")))?;
        if !self.contains(&parent) {
            return Err(self.outside());
        }
        let target = parent.join(name);
        // `symlink_metadata` sees a dangling or escaping link that `exists` would call absent.
        if std::fs::symlink_metadata(&target).is_ok() {
            let real = std::fs::canonicalize(&target)
                .map_err(|e| ToolError::PathDenied(format!("the file is not usable: {e}")))?;
            if !self.contains(&real) {
                return Err(self.outside());
            }
            return Ok(real);
        }
        Ok(target)
    }

    /// Clears the way for a `.bak` copy of `bak`'s sibling target. The backup's directory must lie
    /// inside a root. A leftover regular file is removed so the copy can be created exclusively; a
    /// symlink or any other kind of entry is refused and never followed.
    pub fn prepare_backup(&self, bak: &Path) -> Result<(), ToolError> {
        let parent = bak.parent().unwrap_or(Path::new("."));
        let parent = std::fs::canonicalize(parent).map_err(|e| {
            ToolError::PathDenied(format!("the backup directory is not usable: {e}"))
        })?;
        if !self.contains(&parent) {
            return Err(self.outside());
        }
        match std::fs::symlink_metadata(bak) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => Err(
                ToolError::PathDenied("backup path is a symlink or not a regular file".into()),
            ),
            Ok(_) => std::fs::remove_file(bak)
                .map_err(|e| ToolError::Doc(format!("cannot write the backup: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(ToolError::Doc(format!("cannot write the backup: {e}"))),
        }
    }
}

/// Windows device names, which open a device instead of a file whatever the extension. The
/// superscript digits are the legacy `COM¹`-`COM³` and `LPT¹`-`LPT³` names.
const RESERVED: &[&str] = &[
    "CON",
    "PRN",
    "AUX",
    "NUL",
    "CONIN$",
    "CONOUT$",
    "COM1",
    "COM2",
    "COM3",
    "COM4",
    "COM5",
    "COM6",
    "COM7",
    "COM8",
    "COM9",
    "COM\u{b9}",
    "COM\u{b2}",
    "COM\u{b3}",
    "LPT1",
    "LPT2",
    "LPT3",
    "LPT4",
    "LPT5",
    "LPT6",
    "LPT7",
    "LPT8",
    "LPT9",
    "LPT\u{b9}",
    "LPT\u{b2}",
    "LPT\u{b3}",
];

/// Refuses a path component that means something else on another platform: a backslash or colon
/// (a separator, a drive or an NTFS alternate data stream on Windows), wildcard and control
/// characters, device names such as `CON` or `NUL.mcdx`, and a trailing dot or space (Windows
/// drops them, so `x.mcdx.` would name `x.mcdx`). Applied on every platform so a path behaves the
/// same everywhere and a worksheet saved on Linux opens on Windows.
fn check_name(name: &str) -> Result<(), ToolError> {
    let bad_char = |c: char| c.is_control() || "\\:<>\"|?*".contains(c);
    if name.chars().any(bad_char) || name.ends_with('.') && name != ".." && name != "." {
        return Err(ToolError::PathDenied(
            "the path has a character that is not allowed in a file name".into(),
        ));
    }
    if name.ends_with(' ') {
        return Err(ToolError::PathDenied(
            "a file name must not end in a space".into(),
        ));
    }
    let stem = name.split('.').next().unwrap_or("").trim_end();
    if RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
        return Err(ToolError::PathDenied(
            "the file name is a reserved device name".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mathcraft-paths-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::canonicalize(&dir).unwrap()
    }

    fn denied(result: Result<PathBuf, ToolError>) -> bool {
        matches!(result, Err(ToolError::PathDenied(_)))
    }

    #[test]
    fn names_above_the_root_are_not_checked() {
        let base = scratch("ancestors");
        let root = base.join("aux").join("Proj: 2024");
        std::fs::create_dir_all(&root).unwrap();
        let roots = Roots::new(std::slice::from_ref(&root)).unwrap();
        // The colon and the device name `aux` are in folders above the root.
        assert!(roots
            .resolve_for_write(&root.join("a.mcdx").to_string_lossy())
            .is_ok());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn names_below_the_root_are_checked() {
        let base = scratch("below");
        let roots = Roots::new(std::slice::from_ref(&base)).unwrap();
        for name in [
            "a:b.mcdx",
            "CON.mcdx",
            "x.mcdx::$DATA",
            "aux/x.mcdx",
            "Proj: 2024/a.mcdx",
        ] {
            let text = base.join(name).to_string_lossy().into_owned();
            assert!(
                denied(roots.resolve_for_write(&text)),
                "{name} must be refused"
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn relative_paths_are_checked_in_full() {
        let base = scratch("relative");
        let roots = Roots::new(std::slice::from_ref(&base)).unwrap();
        assert!(denied(roots.resolve_for_write("aux/x.mcdx")));
        assert!(denied(roots.resolve_for_write("a:b.mcdx")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn reserved_names_are_refused_with_and_without_extension() {
        for name in [
            "CON",
            "con.mcdx",
            "CONIN$",
            "conin$.mcdx",
            "CONOUT$",
            "Conout$.MCDX",
            "NUL.mcdx",
            "COM1",
            "com2.mcdx",
            "COM\u{b9}",
            "COM\u{b2}.mcdx",
            "COM\u{b3}",
            "LPT\u{b9}.mcdx",
            "lpt\u{b2}",
            "LPT\u{b3}.mcdx",
            "LPT9.mcdx",
        ] {
            assert!(check_name(name).is_err(), "{name} must be refused");
        }
        for name in ["CONSOLE.mcdx", "COM\u{b4}.mcdx", "auxiliary.mcdx"] {
            assert!(check_name(name).is_ok(), "{name} must be accepted");
        }
    }
}
