//! The headless backend: the server owns the documents in memory, loads and saves `.mcdx` files
//! inside the configured roots.
//!
//! Locking: the map of documents is locked only to find or add an entry. Each document has its
//! own lock, so a slow calculation on one document does not block the others (work package S-07).
//! A second call on the same document waits for the first. Lock order, to rule out deadlocks:
//! document, then `info` or `backed_up`, then the map. Nothing waits for a document while it
//! holds the map.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mathcraft_doc::Doc;
use serde_json::Value;

use crate::backend::{DocAccess, DocBackend, DocSummary, SaveOutcome};
use crate::error::ToolError;
use crate::paths::Roots;

/// What `list` and `summary` report. Kept beside the document so they never wait for its lock.
#[derive(Clone)]
struct Info {
    title: String,
    path: Option<String>,
    dirty: bool,
    region_count: usize,
}

impl Info {
    fn of(doc: &Doc) -> Self {
        Self {
            title: doc.title.clone(),
            path: doc.path().map(|p| p.display().to_string()),
            dirty: doc.is_dirty(),
            region_count: doc.regions().len(),
        }
    }
}

struct Entry {
    doc: Mutex<Doc>,
    /// Number of calls queued on or running against the document.
    busy: AtomicUsize,
    /// Set under the document lock by `close`. A call that locked the document after that
    /// reports the document as unknown, so its edits are never lost silently.
    closed: AtomicBool,
    info: Mutex<Info>,
}

impl Entry {
    fn new(doc: Doc) -> Self {
        let info = Info::of(&doc);
        Self {
            doc: Mutex::new(doc),
            busy: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
            info: Mutex::new(info),
        }
    }

    fn refresh(&self, doc: &Doc) {
        *lock(&self.info) = Info::of(doc);
    }

    fn summary(&self, id: &str) -> DocSummary {
        let info = lock(&self.info).clone();
        DocSummary {
            busy: self.busy.load(Ordering::SeqCst) > 0,
            id: id.to_string(),
            title: info.title,
            path: info.path,
            dirty: info.dirty,
            live: false,
            region_count: info.region_count,
        }
    }
}

/// Counts one call on a document while it lives: queued or running. Decrements on drop, also
/// when the call unwinds.
struct BusyGuard<'a>(&'a AtomicUsize);

impl<'a> BusyGuard<'a> {
    fn set(counter: &'a AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(counter)
    }
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A poisoned lock (a panic in an earlier call) must not lock the server out of its documents.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Inner {
    docs: BTreeMap<String, Arc<Entry>>,
    next_id: u64,
}

pub struct HeadlessBackend {
    roots: Roots,
    max_documents: usize,
    inner: Mutex<Inner>,
    /// Files that already got their `.bak` copy in this session.
    backed_up: Mutex<HashSet<PathBuf>>,
}

impl HeadlessBackend {
    pub fn new(roots: Roots, max_documents: usize) -> Self {
        Self {
            roots,
            max_documents,
            inner: Mutex::new(Inner {
                docs: BTreeMap::new(),
                next_id: 1,
            }),
            backed_up: Mutex::new(HashSet::new()),
        }
    }

    fn check_capacity(&self, inner: &Inner) -> Result<(), ToolError> {
        if inner.docs.len() >= self.max_documents {
            return Err(ToolError::LimitReached(format!(
                "{} documents are open, the limit; close one first",
                inner.docs.len()
            )));
        }
        Ok(())
    }

    fn add(&self, doc: Doc) -> Result<DocSummary, ToolError> {
        let mut inner = lock(&self.inner);
        self.check_capacity(&inner)?;
        let id = format!("doc{}", inner.next_id);
        inner.next_id += 1;
        let entry = Arc::new(Entry::new(doc));
        let summary = entry.summary(&id);
        inner.docs.insert(id, entry);
        Ok(summary)
    }

    /// The entry for `id`. The map lock is released before the caller touches the document.
    fn entry(&self, id: &str) -> Result<Arc<Entry>, ToolError> {
        lock(&self.inner)
            .docs
            .get(id)
            .cloned()
            .ok_or_else(|| unknown(id))
    }
}

fn unknown(id: &str) -> ToolError {
    ToolError::UnknownDocument(format!(
        "no open document `{id}`; use document_list to see the open ones"
    ))
}

/// Locks the document of `entry` for a call. A call that got the entry before `close` removed
/// it, and locks it after, gets the same error as an unknown id.
fn lock_open<'a>(entry: &'a Entry, id: &str) -> Result<MutexGuard<'a, Doc>, ToolError> {
    let doc = lock(&entry.doc);
    if entry.closed.load(Ordering::SeqCst) {
        return Err(unknown(id));
    }
    Ok(doc)
}

impl DocBackend for HeadlessBackend {
    fn mode(&self) -> &'static str {
        "headless"
    }

    fn roots(&self) -> Vec<String> {
        self.roots
            .list()
            .iter()
            .map(|r| r.display().to_string())
            .collect()
    }

    fn max_documents(&self) -> usize {
        self.max_documents
    }

    fn new_document(&self, title: Option<&str>) -> Result<DocSummary, ToolError> {
        let doc = Doc::new(title.unwrap_or("Untitled"));
        self.add(doc)
    }

    fn resolve_image(&self, path: &str) -> Result<std::path::PathBuf, ToolError> {
        self.roots.resolve_existing_image(path)
    }

    fn open(&self, path: &str) -> Result<DocSummary, ToolError> {
        let resolved = self.roots.resolve_existing(path)?;
        {
            let inner = lock(&self.inner);
            self.check_capacity(&inner)?;
        }
        let doc = Doc::open_verified(&resolved, &resolved)?;
        self.add(doc)
    }

    fn save(
        &self,
        id: &str,
        path: Option<&str>,
        overwrite: bool,
    ) -> Result<SaveOutcome, ToolError> {
        let entry = self.entry(id)?;
        let _busy = BusyGuard::set(&entry.busy);
        let mut doc = lock_open(&entry, id)?;
        let default = doc.path().map(Path::to_path_buf);
        let mut backed_up = lock(&self.backed_up);
        let result = save_doc(
            &self.roots,
            &mut backed_up,
            &mut doc,
            default,
            path,
            overwrite,
        );
        entry.refresh(&doc);
        result
    }

    fn close(&self, id: &str, discard: bool) -> Result<(), ToolError> {
        let entry = self.entry(id)?;
        let doc = lock_open(&entry, id)?;
        if doc.is_dirty() && !discard {
            return Err(ToolError::Unsaved(format!(
                "`{id}` has unsaved changes; save it or pass discard: true"
            )));
        }
        entry.closed.store(true, Ordering::SeqCst);
        lock(&self.inner).docs.remove(id);
        Ok(())
    }

    fn list(&self) -> Vec<DocSummary> {
        let inner = lock(&self.inner);
        inner.docs.iter().map(|(id, e)| e.summary(id)).collect()
    }

    fn summary(&self, id: &str) -> Result<DocSummary, ToolError> {
        Ok(self.entry(id)?.summary(id))
    }

    fn with_doc(&self, id: &str, f: DocAccess<'_>) -> Result<Value, ToolError> {
        let entry = self.entry(id)?;
        let _busy = BusyGuard::set(&entry.busy);
        let mut doc = lock_open(&entry, id)?;
        let result = f(&mut doc);
        entry.refresh(&doc);
        result
    }
}

/// Copies `from` to `to`, which must not exist. The `create_new` open fails rather than follow a
/// symlink that appeared at `to`. The source permissions are kept.
fn copy_exclusive(from: &Path, to: &Path) -> std::io::Result<()> {
    // `from` is the resolved target of a save: a FIFO or device there is refused, not read.
    let mut src = mathcraft_core::regular_file::open_regular_file(from, from)?;
    let perms = src.metadata()?.permissions();
    let mut dst = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    std::io::copy(&mut src, &mut dst)?;
    dst.set_permissions(perms)
}

/// `sheet.mcdx` -> `sheet.mcdx.bak`.
fn backup_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    target.with_file_name(name)
}

/// Saves `doc` under the rules of both modes: the target lies inside a root, an existing file
/// needs `overwrite`, and the first overwrite of a file in this session keeps a `.bak` copy.
/// `default` is the file the document came from, used when `path` is `None`.
pub(crate) fn save_doc(
    roots: &Roots,
    backed_up: &mut HashSet<PathBuf>,
    doc: &mut Doc,
    default: Option<PathBuf>,
    path: Option<&str>,
    overwrite: bool,
) -> Result<SaveOutcome, ToolError> {
    let target = match (path, default) {
        (Some(p), _) => roots.resolve_for_write(p)?,
        (None, Some(p)) => roots.resolve_for_write(&p.to_string_lossy())?,
        (None, None) => {
            return Err(ToolError::InvalidParams(
                "the document has no file yet; pass `path`".into(),
            ))
        }
    };
    let exists = target.exists();
    if exists && !overwrite {
        return Err(ToolError::Exists(format!(
            "{} already exists; pass overwrite: true to replace it (a .bak copy is kept)",
            target.display()
        )));
    }
    // Refuse before any file work: a refused save must leave no `.bak` and the file untouched.
    doc.check_saveable()?;
    let mut backup = None;
    if exists && !backed_up.contains(&target) {
        let bak = backup_path(&target);
        roots.prepare_backup(&bak)?;
        copy_exclusive(&target, &bak)
            .map_err(|e| ToolError::Doc(format!("cannot write the backup: {e}")))?;
        backed_up.insert(target.clone());
        backup = Some(bak.display().to_string());
    }
    // `doc.save` writes a `create_new` temporary file in the target's folder and renames it over
    // the target. A symlink swapped in at the final component after the check above is replaced
    // by the rename, never written through. Left open: a swap of a parent directory for a link
    // between the check and the write (needs openat with a directory fd); the attacker needs write
    // access inside a root.
    let warnings = doc.save_with_report(Some(&target))?;
    Ok(SaveOutcome {
        path: target.display().to_string(),
        backup,
        overwrote: exists,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn backend() -> HeadlessBackend {
        let dir = std::env::temp_dir().join(format!("mathcraft-headless-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let roots = Roots::new(&[dir]).unwrap();
        HeadlessBackend::new(roots, 8)
    }

    /// A FIFO named like a worksheet, in a fresh root. Unix only.
    #[cfg(unix)]
    fn fifo_root(name: &str, file: &str) -> (PathBuf, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("mathcraft-fifo-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = std::fs::canonicalize(&dir).unwrap();
        let fifo = dir.join(file);
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        (dir, fifo)
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_worksheet_is_refused_without_hanging() {
        let (dir, _fifo) = fifo_root("open", "pipe.mcdx");
        let (tx, rx) = mpsc::channel();
        let root = dir.clone();
        std::thread::spawn(move || {
            let backend = HeadlessBackend::new(Roots::new(&[root]).unwrap(), 4);
            let result = backend.open("pipe.mcdx").map(|_| ());
            let _ = tx.send(result);
        });
        let result = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("document_open must not block on a FIFO");
        assert!(result.is_err(), "a FIFO must not open as a worksheet");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_backup_source_is_refused_without_hanging() {
        let (dir, fifo) = fifo_root("backup", "pipe.mcdx");
        let bak = dir.join("pipe.mcdx.bak");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(copy_exclusive(&fifo, &bak));
        });
        let result = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the backup copy must not block on a FIFO");
        assert!(result.is_err(), "a FIFO must not be copied as a backup");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_slow_calculation_does_not_block_other_documents() {
        let backend = backend();
        let a = backend.new_document(Some("A")).unwrap().id;
        let b = backend.new_document(Some("B")).unwrap().id;
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let backend = &backend;
        std::thread::scope(|scope| {
            let slow_id = a.clone();
            let slow = scope.spawn(move || {
                backend.with_doc(&slow_id, &mut |_doc: &mut Doc| {
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(serde_json::json!("done"))
                })
            });
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap();

            // While A is busy, the list answers and reports A as busy.
            let list = backend.list();
            assert_eq!(list.len(), 2);
            assert!(list.iter().any(|d| d.id == a && d.busy));
            assert!(list.iter().any(|d| d.id == b && !d.busy));

            // A read of B completes while A is still running.
            let (done_tx, done_rx) = mpsc::channel();
            let read_id = b.clone();
            scope.spawn(move || {
                let read = backend.with_doc(&read_id, &mut |doc: &mut Doc| {
                    Ok(serde_json::json!(doc.title.clone()))
                });
                done_tx.send(read).unwrap();
            });
            let read = done_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("document B must not wait for document A");
            assert_eq!(read.unwrap(), serde_json::json!("B"));

            release_tx.send(()).unwrap();
            assert_eq!(slow.join().unwrap().unwrap(), serde_json::json!("done"));
        });
        assert!(backend.list().iter().all(|d| !d.busy));
    }

    #[test]
    fn busy_stays_set_until_every_queued_call_ends() {
        let backend = backend();
        let id = backend.new_document(Some("A")).unwrap().id;
        let entry = backend.entry(&id).unwrap();
        let (started1_tx, started1_rx) = mpsc::channel();
        let (release1_tx, release1_rx) = mpsc::channel::<()>();
        let (started2_tx, started2_rx) = mpsc::channel();
        let (release2_tx, release2_rx) = mpsc::channel::<()>();
        let backend = &backend;
        std::thread::scope(|scope| {
            let first_id = id.clone();
            let first = scope.spawn(move || {
                backend.with_doc(&first_id, &mut |_doc: &mut Doc| {
                    started1_tx.send(()).unwrap();
                    release1_rx.recv().unwrap();
                    Ok(serde_json::json!(1))
                })
            });
            started1_rx.recv_timeout(Duration::from_secs(5)).unwrap();

            let second_id = id.clone();
            let second = scope.spawn(move || {
                backend.with_doc(&second_id, &mut |_doc: &mut Doc| {
                    started2_tx.send(()).unwrap();
                    release2_rx.recv().unwrap();
                    Ok(serde_json::json!(2))
                })
            });
            // Wait until the second call has marked the document busy and queued on its lock.
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while entry.busy.load(Ordering::SeqCst) < 2 {
                assert!(
                    std::time::Instant::now() < deadline,
                    "second call never queued"
                );
                std::thread::sleep(Duration::from_millis(1));
            }

            release1_tx.send(()).unwrap();
            assert_eq!(first.join().unwrap().unwrap(), serde_json::json!(1));
            // The first call is over, the second is still queued: the document stays busy.
            assert!(backend.summary(&id).unwrap().busy);

            started2_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(backend.list().iter().any(|d| d.id == id && d.busy));
            release2_tx.send(()).unwrap();
            assert_eq!(second.join().unwrap().unwrap(), serde_json::json!(2));
        });
        assert!(!backend.summary(&id).unwrap().busy);
    }

    #[test]
    fn a_call_queued_behind_close_reports_unknown_document() {
        let backend = backend();
        let id = backend.new_document(Some("A")).unwrap().id;
        // A call that already holds the entry, as one queued on the document lock does.
        let queued = backend.entry(&id).unwrap();
        backend.close(&id, true).unwrap();

        // Once it gets the lock, it must not edit the removed document.
        match lock_open(&queued, &id) {
            Err(ToolError::UnknownDocument(_)) => {}
            _ => panic!("a call queued behind close must report an unknown document"),
        }
        // A call made after the close fails the same way, for with_doc and save.
        assert!(matches!(
            backend.with_doc(&id, &mut |_doc: &mut Doc| Ok(serde_json::json!("edited"))),
            Err(ToolError::UnknownDocument(_))
        ));
        assert!(matches!(
            backend.save(&id, None, false),
            Err(ToolError::UnknownDocument(_))
        ));
        assert!(matches!(
            backend.close(&id, true),
            Err(ToolError::UnknownDocument(_))
        ));
    }

    #[test]
    fn a_refused_overwrite_leaves_no_backup_and_the_file_unchanged() {
        let dir = std::env::temp_dir().join(format!("mathcraft-refused-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let backend = HeadlessBackend::new(Roots::new(std::slice::from_ref(&dir)).unwrap(), 8);
        let target = dir.join("sheet.mcdx");
        let backup = dir.join("sheet.mcdx.bak");
        let _ = std::fs::remove_file(&backup);
        let mut plain = Doc::new("sheet");
        plain
            .insert_math("x := 2", mathcraft_doc::Placement::Auto)
            .unwrap();
        plain.save(Some(&target)).unwrap();
        let before = std::fs::read(&target).unwrap();

        let id = backend.open(&target.to_string_lossy()).unwrap().id;
        backend
            .with_doc(&id, &mut |doc: &mut Doc| {
                let lines = vec!["return n".to_owned()];
                doc.insert_program(
                    "id1",
                    &["n".to_owned()],
                    &lines,
                    mathcraft_doc::Placement::Auto,
                )
                .map_err(|e| ToolError::Doc(e.to_string()))?;
                Ok(Value::Null)
            })
            .unwrap();
        match backend.save(&id, None, true) {
            Err(ToolError::Doc(m)) => assert!(m.contains("programming regions"), "{m}"),
            other => panic!(
                "a program sheet must be refused, got {:?}",
                other.map(|o| o.path)
            ),
        }
        assert!(!backup.exists(), "a refused save must not create a .bak");
        assert_eq!(
            std::fs::read(&target).unwrap(),
            before,
            "the file must be unchanged"
        );
    }
}
