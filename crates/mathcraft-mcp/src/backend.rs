//! Where documents live. Tools never touch a document directly; they go through [`DocBackend`].
//! Today the only implementation is [`crate::headless::HeadlessBackend`] (documents in this
//! process). A live backend, which forwards to the open Mathcraft window, implements the same
//! trait later, so every tool works in both modes unchanged.

use mathcraft_doc::Doc;
use serde_json::{json, Value};

use crate::error::ToolError;

/// What `document_list` and friends report about one open document.
#[derive(Debug, Clone, PartialEq)]
pub struct DocSummary {
    pub id: String,
    pub title: String,
    pub path: Option<String>,
    pub dirty: bool,
    /// True when the document is open in the running app (live mode).
    pub live: bool,
    pub region_count: usize,
    /// Live mode: the user is typing in this document, so tools on it answer `user_busy`.
    pub busy: bool,
}

impl DocSummary {
    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "document_id": self.id,
            "title": self.title,
            "path": self.path,
            "dirty": self.dirty,
            "live": self.live,
            "region_count": self.region_count,
        });
        if self.busy {
            v["busy"] = json!(true);
        }
        v
    }
}

/// What a save did.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveOutcome {
    pub path: String,
    /// The `.bak` copy made of the file that was replaced, when this was its first overwrite in
    /// this session.
    pub backup: Option<String>,
    pub overwrote: bool,
    /// Warnings from the writer: unverified file formats the file was written with (see
    /// `documentation/UNVERIFIED_FORMATS.md`). Empty when the file holds only verified formats.
    pub warnings: Vec<String>,
}

/// The closure a tool runs on a document. `FnMut` and a trait object keep the trait object-safe.
pub type DocAccess<'a> = &'a mut dyn FnMut(&mut Doc) -> Result<Value, ToolError>;

/// What only a running app can do (live mode). `None` from [`DocBackend::live`] in headless mode.
pub trait LiveControl: Send + Sync {
    /// Connection state, documents and the document in front, as JSON.
    fn status(&self) -> Value;
    /// Brings the document's tab to the front.
    ///
    /// # Errors
    /// `unknown_document`.
    fn focus(&self, id: &str) -> Result<(), ToolError>;
    /// Selects a region of the document in the window (and brings the tab to the front).
    ///
    /// # Errors
    /// `unknown_document`, `not_found`.
    fn select_region(&self, id: &str, region: u32) -> Result<(), ToolError>;
}

pub trait DocBackend: Send + Sync {
    /// `headless` or `live`.
    fn mode(&self) -> &'static str;
    /// The directories files may be read from and written to.
    fn roots(&self) -> Vec<String>;
    /// The most documents that can be open at once.
    fn max_documents(&self) -> usize;

    fn new_document(&self, title: Option<&str>) -> Result<DocSummary, ToolError>;
    fn open(&self, path: &str) -> Result<DocSummary, ToolError>;
    /// An existing PNG or JPEG file to insert as an image, inside a root (the same checks as
    /// [`DocBackend::open`]).
    ///
    /// # Errors
    /// `path_denied` for a path outside the roots or a file that is not an image path, `doc` when
    /// the file cannot be read.
    fn resolve_image(&self, path: &str) -> Result<std::path::PathBuf, ToolError>;
    /// Saves to `path`, or to the file the document came from. An existing file is only replaced
    /// when `overwrite` is true.
    fn save(&self, id: &str, path: Option<&str>, overwrite: bool)
        -> Result<SaveOutcome, ToolError>;
    /// Closes the document. Unsaved changes need `discard`.
    fn close(&self, id: &str, discard: bool) -> Result<(), ToolError>;
    fn list(&self) -> Vec<DocSummary>;
    fn summary(&self, id: &str) -> Result<DocSummary, ToolError>;
    /// Runs `f` on the document. Read and write tools use this one entry point.
    fn with_doc(&self, id: &str, f: DocAccess<'_>) -> Result<Value, ToolError>;

    /// The server calls this before a tool runs and [`DocBackend::call_finished`] after it. The
    /// live backend uses the pair to make one tool call one undo step and to note what changed.
    fn call_started(&self, _tool: &str, _mutating: bool) {}
    fn call_finished(&self) {}

    /// The live-only controls, when this backend is the running app.
    fn live(&self) -> Option<&dyn LiveControl> {
        None
    }
}
