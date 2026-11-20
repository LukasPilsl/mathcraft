//! The live [`DocBackend`]: the documents of the open window, lent to it for the duration of one
//! call.
//!
//! The window owns the documents (each tab's edit session holds a `Doc`). For one job the UI thread
//! *lends* them (`LiveCore::run` takes each `Doc` out of its session), the tool table runs on this
//! backend, and the documents go back afterwards together with the [`Effects`] of the call: tabs
//! to create or close, saves to record, the region to select, the regions that changed. Nothing
//! else touches a lent document, so MCP and GUI share one document and one undo history.
//!
//! A document with an open draft (the user is typing) is not lent: tools on it answer `user_busy`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mathcraft_doc::Doc;
use mathcraft_import::regions::RegionModel;
use serde_json::{json, Value};

use super::ui::{shared_json, LiveShared};
use super::{doc_id, next_doc_number};
use crate::backend::{DocAccess, DocBackend, DocSummary, LiveControl, SaveOutcome};
use crate::error::ToolError;
use crate::headless::save_doc;
use crate::paths::Roots;

/// One document of the window for the length of a call.
#[derive(Debug)]
pub struct LentDoc {
    pub id: String,
    pub title: String,
    pub path: Option<PathBuf>,
    /// Unsaved edits, as the window sees them (file state and draft).
    pub dirty: bool,
    pub region_count: usize,
    /// The document's revision when it was lent (the host compares it on return).
    pub revision: u64,
    /// `None` when the user is typing in it: it stays in the window and tools answer `user_busy`.
    pub doc: Option<Doc>,
}

/// A document a tool created (`document_new`, or `document_open` with a file that was not open).
/// The window gives it a tab.
#[derive(Debug)]
pub struct NewDoc {
    pub id: String,
    pub doc: Doc,
    pub path: Option<PathBuf>,
}

/// What the window must do after the call.
#[derive(Debug, Default)]
pub struct Effects {
    pub new_docs: Vec<NewDoc>,
    /// Ids of documents the tool closed: their tabs go.
    pub closed: Vec<String>,
    /// `(id, path, revision)` of saves: the window records the file and marks the tab clean.
    pub saved: Vec<(String, PathBuf, u64)>,
    /// A tab to bring to the front.
    pub focus: Option<String>,
    /// A region to select (its tab is brought to the front too).
    pub select: Option<(String, u32)>,
    /// Regions a mutating call added or changed, per document: the window flashes them.
    pub changed: Vec<(String, Vec<u32>)>,
}

/// What a mutating call needs to be compared with afterwards.
struct Snapshot {
    revision: u64,
    steps: u64,
    regions: Vec<RegionModel>,
}

struct Entry {
    lent: LentDoc,
    /// The document revision when it was lent (to tell if a call changed it).
    rev_at_lend: u64,
    /// Created during this call.
    new: bool,
    snapshot: Option<Snapshot>,
}

#[derive(Default)]
struct State {
    entries: Vec<Entry>,
    effects: Effects,
    front: Option<String>,
    /// The tool being run: name and whether it changes documents.
    tool: Option<(String, bool)>,
}

pub struct LiveBackend {
    roots: Roots,
    max_documents: usize,
    shared: Arc<LiveShared>,
    state: Mutex<State>,
    backed_up: Mutex<HashSet<PathBuf>>,
}

impl LiveBackend {
    pub fn new(roots: Roots, max_documents: usize, shared: Arc<LiveShared>) -> Self {
        Self {
            roots,
            max_documents,
            shared,
            state: Mutex::new(State::default()),
            backed_up: Mutex::new(HashSet::new()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts a call: the lent documents are what the tools see.
    pub fn begin(&self, docs: Vec<LentDoc>, front: Option<String>) {
        let mut state = self.lock();
        *state = State {
            entries: docs
                .into_iter()
                .map(|lent| Entry {
                    rev_at_lend: lent.doc.as_ref().map_or(0, Doc::revision),
                    lent,
                    new: false,
                    snapshot: None,
                })
                .collect(),
            effects: Effects::default(),
            front,
            tool: None,
        };
    }

    /// Ends a call: the documents go back to the window, with what the call caused.
    pub fn end(&self) -> (Vec<LentDoc>, Effects) {
        let mut state = self.lock();
        let mut effects = std::mem::take(&mut state.effects);
        let mut back = Vec::new();
        for entry in std::mem::take(&mut state.entries) {
            if entry.new {
                if let Some(doc) = entry.lent.doc {
                    effects.new_docs.push(NewDoc {
                        id: entry.lent.id,
                        doc,
                        path: entry.lent.path,
                    });
                }
            } else {
                back.push(entry.lent);
            }
        }
        (back, effects)
    }
}

fn summarize(entry: &Entry) -> DocSummary {
    let l = &entry.lent;
    let (dirty, count) = match &l.doc {
        Some(doc) => (
            l.dirty || doc.revision() != entry.rev_at_lend,
            doc.regions().len(),
        ),
        None => (l.dirty, l.region_count),
    };
    DocSummary {
        id: l.id.clone(),
        title: l.title.clone(),
        path: l.path.as_ref().map(|p| p.display().to_string()),
        dirty,
        live: true,
        region_count: count,
        busy: l.doc.is_none(),
    }
}

fn unknown(id: &str) -> ToolError {
    ToolError::UnknownDocument(format!(
        "no open document `{id}`; use document_list to see the open ones"
    ))
}

fn busy(id: &str) -> ToolError {
    ToolError::UserBusy(format!(
        "the user is typing in `{id}` right now; try again in a moment, or work in another document"
    ))
}

/// `AI: <tool>`, as a `'static` string (the undo history stores static labels). One leaked string
/// per distinct tool name, and the names come from the fixed tool table.
fn ai_label(tool: &str) -> &'static str {
    static LABELS: Mutex<Vec<(String, &'static str)>> = Mutex::new(Vec::new());
    let mut labels = LABELS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, label)) = labels.iter().find(|(name, _)| name == tool) {
        return label;
    }
    let label: &'static str = Box::leak(format!("AI: {tool}").into_boxed_str());
    labels.push((tool.to_string(), label));
    label
}

/// Same file, as far as the file system can tell.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

impl DocBackend for LiveBackend {
    fn mode(&self) -> &'static str {
        "live"
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
        let mut state = self.lock();
        if state.entries.len() >= self.max_documents {
            return Err(ToolError::LimitReached(format!(
                "{} documents are open, the limit; close one first",
                state.entries.len()
            )));
        }
        let doc = Doc::new(title.unwrap_or("Untitled"));
        let entry = Entry {
            lent: LentDoc {
                id: doc_id(next_doc_number()),
                title: doc.title.clone(),
                path: None,
                dirty: false,
                region_count: 0,
                revision: 0,
                doc: Some(doc),
            },
            rev_at_lend: 0,
            new: true,
            snapshot: None,
        };
        let summary = summarize(&entry);
        state.entries.push(entry);
        Ok(summary)
    }

    fn resolve_image(&self, path: &str) -> Result<std::path::PathBuf, ToolError> {
        self.roots.resolve_existing_image(path)
    }

    fn open(&self, path: &str) -> Result<DocSummary, ToolError> {
        let resolved = self.roots.resolve_existing(path)?;
        let mut state = self.lock();
        // A file that is open in the window is attached to, not opened twice.
        if let Some(entry) = state.entries.iter().find(|e| {
            e.lent
                .path
                .as_deref()
                .is_some_and(|p| same_file(p, &resolved))
        }) {
            return Ok(summarize(entry));
        }
        if state.entries.len() >= self.max_documents {
            return Err(ToolError::LimitReached(format!(
                "{} documents are open, the limit; close one first",
                state.entries.len()
            )));
        }
        let doc = Doc::open_verified(&resolved, &resolved)?;
        let entry = Entry {
            rev_at_lend: doc.revision(),
            lent: LentDoc {
                id: doc_id(next_doc_number()),
                title: doc.title.clone(),
                path: Some(resolved),
                dirty: false,
                region_count: doc.regions().len(),
                revision: doc.revision(),
                doc: Some(doc),
            },
            new: true,
            snapshot: None,
        };
        let summary = summarize(&entry);
        state.entries.push(entry);
        Ok(summary)
    }

    fn save(
        &self,
        id: &str,
        path: Option<&str>,
        overwrite: bool,
    ) -> Result<SaveOutcome, ToolError> {
        let mut guard = self.lock();
        let state = &mut *guard;
        let entry = state
            .entries
            .iter_mut()
            .find(|e| e.lent.id == id)
            .ok_or_else(|| unknown(id))?;
        let default = entry.lent.path.clone();
        let doc = entry.lent.doc.as_mut().ok_or_else(|| busy(id))?;
        let mut backed_up = self
            .backed_up
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let outcome = save_doc(&self.roots, &mut backed_up, doc, default, path, overwrite)?;
        let target = PathBuf::from(&outcome.path);
        entry.rev_at_lend = doc.revision();
        entry.lent.dirty = false;
        entry.lent.path = Some(target.clone());
        let revision = doc.revision();
        state.effects.saved.push((id.to_string(), target, revision));
        Ok(outcome)
    }

    fn close(&self, id: &str, discard: bool) -> Result<(), ToolError> {
        let mut state = self.lock();
        let at = state
            .entries
            .iter()
            .position(|e| e.lent.id == id)
            .ok_or_else(|| unknown(id))?;
        let entry = &state.entries[at];
        if entry.lent.doc.is_none() {
            return Err(busy(id));
        }
        if summarize(entry).dirty && !discard {
            return Err(ToolError::Unsaved(format!(
                "`{id}` has unsaved changes; save it or pass discard: true"
            )));
        }
        let removed = state.entries.remove(at);
        if !removed.new {
            state.effects.closed.push(id.to_string());
        }
        Ok(())
    }

    fn list(&self) -> Vec<DocSummary> {
        self.lock().entries.iter().map(summarize).collect()
    }

    fn summary(&self, id: &str) -> Result<DocSummary, ToolError> {
        let state = self.lock();
        state
            .entries
            .iter()
            .find(|e| e.lent.id == id)
            .map(summarize)
            .ok_or_else(|| unknown(id))
    }

    fn with_doc(&self, id: &str, f: DocAccess<'_>) -> Result<Value, ToolError> {
        let mut state = self.lock();
        let entry = state
            .entries
            .iter_mut()
            .find(|e| e.lent.id == id)
            .ok_or_else(|| unknown(id))?;
        let doc = entry.lent.doc.as_mut().ok_or_else(|| busy(id))?;
        f(doc)
    }

    fn call_started(&self, tool: &str, mutating: bool) {
        let mut state = self.lock();
        state.tool = Some((tool.to_string(), mutating));
        if !mutating {
            return;
        }
        for entry in &mut state.entries {
            if let Some(doc) = &entry.lent.doc {
                entry.snapshot = Some(Snapshot {
                    revision: doc.revision(),
                    steps: doc.steps_executed(),
                    regions: doc.regions().to_vec(),
                });
            }
        }
    }

    /// Makes the call one undo step labelled `AI: <tool>` and notes the regions it changed.
    fn call_finished(&self) {
        let mut guard = self.lock();
        let state = &mut *guard;
        let Some((tool, mutating)) = state.tool.take() else {
            return;
        };
        if !mutating {
            return;
        }
        let history_tool = matches!(tool.as_str(), "document_undo" | "document_redo");
        for entry in &mut state.entries {
            let (Some(snap), Some(doc)) = (entry.snapshot.take(), entry.lent.doc.as_mut()) else {
                continue;
            };
            if doc.revision() == snap.revision {
                continue;
            }
            if !history_tool {
                let taken = doc.steps_executed().saturating_sub(snap.steps);
                let label = ai_label(&tool);
                // A tool that labelled its own step ("AI: batch") has collapsed it already; the newest
                // step is on top, so an "AI:" label there means this call made it.
                let already = doc.undo_label().is_some_and(|l| l.starts_with("AI:"));
                if taken >= 1 && !already {
                    doc.collapse_steps(taken, label);
                }
            }
            let changed: Vec<u32> = doc
                .regions()
                .iter()
                .filter(|r| snap.regions.iter().find(|b| b.id == r.id) != Some(*r))
                .map(|r| r.id)
                .collect();
            if !changed.is_empty() {
                state.effects.changed.push((entry.lent.id.clone(), changed));
            }
        }
    }

    fn live(&self) -> Option<&dyn LiveControl> {
        Some(self)
    }
}

impl LiveControl for LiveBackend {
    fn status(&self) -> Value {
        let state = self.lock();
        let docs: Vec<Value> = state
            .entries
            .iter()
            .map(|e| summarize(e).to_json())
            .collect();
        json!({
            "connected": true,
            "protocol": super::PROTOCOL,
            "front_document": state.front,
            "documents": docs,
            "connection": shared_json(&self.shared),
        })
    }

    fn focus(&self, id: &str) -> Result<(), ToolError> {
        let mut state = self.lock();
        if !state.entries.iter().any(|e| e.lent.id == id) {
            return Err(unknown(id));
        }
        state.effects.focus = Some(id.to_string());
        Ok(())
    }

    fn select_region(&self, id: &str, region: u32) -> Result<(), ToolError> {
        let mut state = self.lock();
        let entry = state
            .entries
            .iter()
            .find(|e| e.lent.id == id)
            .ok_or_else(|| unknown(id))?;
        let doc = entry.lent.doc.as_ref().ok_or_else(|| busy(id))?;
        if doc.region(region).is_none() {
            return Err(crate::error::no_region(region));
        }
        state.effects.select = Some((id.to_string(), region));
        Ok(())
    }
}
