//! The loop body the UI thread runs for each queued job. The window (or a fake one in tests)
//! implements [`LiveHost`]; everything else is here, so the live path is tested without a window.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::Value;

use super::backend::{Effects, LentDoc, LiveBackend};
use super::ui::{Job, LiveShared, UiQueue};
use crate::paths::Roots;
use crate::server::{PendingCall, Server, Step};
use crate::tools::{default_table, Limits};

/// The documents a host lends for one job.
pub struct Lend {
    pub docs: Vec<LentDoc>,
    /// The id of the tab in front.
    pub front: Option<String>,
}

/// What the UI thread offers. Both methods run on the UI thread, between two frames.
pub trait LiveHost {
    /// Takes the documents out of the window. A tab with a draft open is listed with `doc: None`
    /// (and stays whole).
    fn lend(&mut self) -> Lend;
    /// Puts them back, then applies the effects (new tabs, closed tabs, saved files, the region to
    /// select, the regions to flash).
    fn reclaim(&mut self, docs: Vec<LentDoc>, effects: Effects);
}

/// The tool table on the live backend.
pub struct LiveCore {
    backend: Arc<LiveBackend>,
    server: Server,
    shared: Arc<LiveShared>,
    /// Calls whose result is still being computed (a render), with the job to answer. They are
    /// polled once per frame, so the window never waits for them.
    waiting: Mutex<Vec<(Job, PendingCall)>>,
}

/// The most jobs one frame runs: a job can take long (calculate, render), so one per frame keeps
/// the window drawing between them.
const JOBS_PER_FRAME: usize = 1;

impl LiveCore {
    pub fn new(
        roots: Roots,
        max_documents: usize,
        limits: Limits,
        shared: Arc<LiveShared>,
    ) -> Self {
        let backend = Arc::new(LiveBackend::new(roots, max_documents, shared.clone()));
        let server = Server::new(backend.clone(), default_table(), limits);
        Self {
            backend,
            server,
            shared,
            waiting: Mutex::new(Vec::new()),
        }
    }

    /// Runs one message with the host's documents lent, waiting for a deferred result. The
    /// documents always go back, also when the tool panicked.
    pub fn run(&self, host: &mut dyn LiveHost, message: Value) -> Option<Value> {
        match self.run_step(host, message) {
            Step::Reply(reply) => reply,
            Step::Pending(call) => Some(self.server.wait(call)),
        }
    }

    /// As [`LiveCore::run`], but a call still running (a render) is returned pending.
    fn run_step(&self, host: &mut dyn LiveHost, message: Value) -> Step {
        let lend = host.lend();
        self.backend.begin(lend.docs, lend.front);
        let step = catch_unwind(AssertUnwindSafe(|| self.server.step(message)));
        let (docs, effects) = self.backend.end();
        host.reclaim(docs, effects);
        step.unwrap_or_else(|_| {
            Step::Reply(Some(serde_json::json!({
                "jsonrpc": "2.0", "id": null,
                "error": {"code": -32603, "message": "internal error"},
            })))
        })
    }

    /// Answers the waiting calls whose result is ready (or whose deadline has passed).
    fn settle(&self) {
        let mut waiting = self.waiting.lock().unwrap_or_else(PoisonError::into_inner);
        let mut i = 0;
        while i < waiting.len() {
            match waiting[i].1.poll() {
                Some(result) => {
                    let (job, call) = waiting.swap_remove(i);
                    let reply = self.server.complete(call, result);
                    job.finish(Some(reply));
                }
                None => i += 1,
            }
        }
    }

    /// True while a deferred result (a render) is still running. The window must keep polling
    /// for it even when no job is queued: nothing else wakes it when the result arrives.
    pub fn has_waiting(&self) -> bool {
        !self
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    }

    /// Runs the queued jobs (at most a few). Returns true when more jobs are queued, so the caller
    /// asks for another frame at once. A pending deferred result is not counted here: see
    /// [`LiveCore::has_waiting`].
    pub fn pump(&self, host: &mut dyn LiveHost, queue: &UiQueue) -> bool {
        self.settle();
        for _ in 0..JOBS_PER_FRAME {
            let Some(job) = queue.try_next() else {
                return false;
            };
            if !job.wanted() {
                continue;
            }
            // Paused after the job was queued: it answers `paused` instead of running.
            let method = job.message.get("method").and_then(Value::as_str);
            if self.shared.paused() && matches!(method, Some("tools/call" | "resources/read")) {
                let reply = job.message.get("id").map(|id| {
                    super::ui::refuse(
                        method.unwrap_or_default(),
                        id,
                        &crate::error::ToolError::Paused(
                            "the user paused the AI connection in Mathcraft".into(),
                        ),
                    )
                });
                job.finish(reply);
                continue;
            }
            match self.run_step(host, job.message.clone()) {
                Step::Reply(reply) => job.finish(reply),
                Step::Pending(call) => self
                    .waiting
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((job, call)),
            }
        }
        true
    }
}
