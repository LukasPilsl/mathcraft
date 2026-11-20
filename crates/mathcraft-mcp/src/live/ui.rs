//! The hand-over from the endpoint threads to the UI thread. An endpoint thread gives the message
//! to [`UiDispatcher::handle`], which refuses it while paused or over the rate limit, and
//! otherwise queues a job, wakes the window (`repaint`) and waits for the answer. The UI thread
//! takes jobs from the [`UiQueue`] once per frame, so every edit is serialised with user input.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::endpoint::Handler;
use crate::error::ToolError;
use crate::server::{error_reply, tool_error_reply};

/// Default most mutating tool calls per second.
pub const DEFAULT_EDITS_PER_SEC: usize = 20;
/// Default most cheap read-only calls per second.
pub const DEFAULT_READS_PER_SEC: usize = 100;
/// Default longest wait for the window to answer a job.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// State the window and the endpoint threads share: the pause switch and the connection counters
/// behind the status bar indicator.
#[derive(Debug, Default)]
pub struct LiveShared {
    paused: AtomicBool,
    connections: AtomicUsize,
    calls: AtomicU64,
    last_call: Mutex<Option<Instant>>,
}

impl LiveShared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    /// Clients that passed the token check and are still connected.
    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::Relaxed)
    }

    /// Counts a client in. The endpoint calls it; a demo may too.
    pub fn connection_opened(&self) {
        self.connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn connection_closed(&self) {
        let _ = self
            .connections
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1));
    }

    /// Calls handed to the window so far.
    pub fn calls(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }

    fn note_call(&self) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        *self
            .last_call
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
    }

    /// True when a call came within `window`. The indicator pulses on it.
    pub fn recently_active(&self, window: Duration) -> bool {
        self.last_call
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some_and(|t| t.elapsed() < window)
    }
}

/// One message for the UI thread, and the way back.
pub struct Job {
    pub message: Value,
    reply: Sender<Option<Value>>,
    abandoned: Arc<AtomicBool>,
}

impl Job {
    /// False when the caller gave up waiting; the UI thread then must not run the job.
    pub fn wanted(&self) -> bool {
        !self.abandoned.load(Ordering::SeqCst)
    }

    /// Sends the answer to the waiting endpoint thread.
    pub fn finish(self, reply: Option<Value>) {
        let _ = self.reply.send(reply);
    }
}

/// The jobs waiting for the UI thread.
pub struct UiQueue {
    rx: Mutex<Receiver<Job>>,
}

impl UiQueue {
    pub fn try_next(&self) -> Option<Job> {
        self.rx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .try_recv()
            .ok()
    }
}

/// Settings of a [`UiDispatcher`].
pub struct DispatchConfig {
    /// Names of the tools that change documents or are heavy (calculate, render): the strict
    /// limit. Every other call counts against the read limit.
    pub mutating: HashSet<String>,
    pub edits_per_sec: usize,
    pub reads_per_sec: usize,
    pub timeout: Duration,
}

/// Hands messages to the UI thread. See the module doc.
pub struct UiDispatcher {
    tx: Mutex<Sender<Job>>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    shared: Arc<LiveShared>,
    config: DispatchConfig,
    recent: Mutex<VecDeque<Instant>>,
    recent_reads: Mutex<VecDeque<Instant>>,
}

/// Makes a dispatcher and the queue its jobs go to. `repaint` wakes the UI thread.
pub fn channel(
    shared: Arc<LiveShared>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    config: DispatchConfig,
) -> (UiDispatcher, UiQueue) {
    let (tx, rx) = mpsc::channel();
    (
        UiDispatcher {
            tx: Mutex::new(tx),
            repaint,
            shared,
            config,
            recent: Mutex::new(VecDeque::new()),
            recent_reads: Mutex::new(VecDeque::new()),
        },
        UiQueue { rx: Mutex::new(rx) },
    )
}

impl UiDispatcher {
    /// `Err` when this call would be over its limit; otherwise records it.
    fn admit(&self, heavy: bool) -> Result<(), ToolError> {
        let (log, limit, what) = if heavy {
            (
                &self.recent,
                self.config.edits_per_sec,
                "edits or heavy calls",
            )
        } else {
            (&self.recent_reads, self.config.reads_per_sec, "calls")
        };
        let mut recent = log.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        while recent
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(1))
        {
            recent.pop_front();
        }
        if recent.len() >= limit {
            return Err(ToolError::RateLimited(format!(
                "more than {limit} {what} per second; wait a moment and send fewer, larger calls (batch)"
            )));
        }
        recent.push_back(now);
        Ok(())
    }
}

/// How a refusal is shaped: a tool error for `tools/call`, a JSON-RPC error otherwise.
pub fn refuse(method: &str, id: &Value, e: &ToolError) -> Value {
    if method == "tools/call" {
        tool_error_reply(id, e)
    } else {
        error_reply(id, -32000, &e.to_string())
    }
}

impl Handler for UiDispatcher {
    fn handle(&self, message: Value) -> Option<Value> {
        let id = message.get("id").filter(|v| !v.is_null())?.clone();
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if matches!(method.as_str(), "tools/call" | "resources/read") {
            if self.shared.paused() {
                return Some(refuse(
                    &method,
                    &id,
                    &ToolError::Paused(
                        "the user paused the AI connection in Mathcraft; ask them to resume it, or work headless (target: \"headless\")".into(),
                    ),
                ));
            }
            let tool = message
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let heavy = method == "tools/call" && self.config.mutating.contains(tool);
            if let Err(e) = self.admit(heavy) {
                return Some(refuse(&method, &id, &e));
            }
        }
        self.shared.note_call();
        let (reply_tx, reply_rx) = mpsc::channel();
        let abandoned = Arc::new(AtomicBool::new(false));
        let job = Job {
            message,
            reply: reply_tx,
            abandoned: abandoned.clone(),
        };
        let sent = self
            .tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(job);
        if sent.is_err() {
            return Some(refuse(
                &method,
                &id,
                &ToolError::LiveUnavailable("the Mathcraft window is closing".into()),
            ));
        }
        (self.repaint)();
        let started = Instant::now();
        loop {
            let left = self.config.timeout.saturating_sub(started.elapsed());
            let step = left.min(Duration::from_millis(200));
            match reply_rx.recv_timeout(step) {
                Ok(reply) => return reply,
                Err(RecvTimeoutError::Timeout) => {
                    if super::endpoint::client_gone() {
                        // Nobody will read the answer: the UI thread skips the job.
                        abandoned.store(true, Ordering::SeqCst);
                        return None;
                    }
                    if left > step {
                        continue;
                    }
                    abandoned.store(true, Ordering::SeqCst);
                    return Some(refuse(
                        &method,
                        &id,
                        &ToolError::UserBusy(format!(
                            "the Mathcraft window did not answer within {} s (busy, minimised or in a dialog); the call was not applied",
                            self.config.timeout.as_secs()
                        )),
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Some(refuse(
                        &method,
                        &id,
                        &ToolError::LiveUnavailable(
                            "the Mathcraft window stopped answering".into(),
                        ),
                    ))
                }
            }
        }
    }
}

/// The pause state as JSON for `app_status`.
pub fn shared_json(shared: &LiveShared) -> Value {
    json!({
        "paused": shared.paused(),
        "clients": shared.connections(),
        "calls": shared.calls(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dispatcher(edits: usize) -> (UiDispatcher, UiQueue, Arc<LiveShared>) {
        let shared = LiveShared::new();
        let (d, q) = channel(
            shared.clone(),
            Arc::new(|| {}),
            DispatchConfig {
                mutating: ["insert_math".to_string()].into_iter().collect(),
                edits_per_sec: edits,
                reads_per_sec: 1000,
                timeout: Duration::from_millis(100),
            },
        );
        (d, q, shared)
    }

    fn call(tool: &str) -> Value {
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":{}}})
    }

    #[test]
    fn paused_and_rate_limit_refuse_before_the_queue() {
        let (d, q, shared) = dispatcher(2);
        shared.set_paused(true);
        let r = d.handle(call("read_outline")).unwrap();
        assert_eq!(r["result"]["structuredContent"]["error"]["code"], "paused");
        shared.set_paused(false);
        // Nobody serves the queue: the first two edits time out as user_busy, the third is limited.
        for _ in 0..2 {
            let r = d.handle(call("insert_math")).unwrap();
            assert_eq!(
                r["result"]["structuredContent"]["error"]["code"],
                "user_busy"
            );
        }
        let r = d.handle(call("insert_math")).unwrap();
        assert_eq!(
            r["result"]["structuredContent"]["error"]["code"],
            "rate_limited"
        );
        // The timed-out jobs were marked as not wanted.
        while let Some(job) = q.try_next() {
            assert!(!job.wanted());
        }
    }

    #[test]
    fn reads_have_their_own_higher_limit() {
        let shared = LiveShared::new();
        let (d, _q) = channel(
            shared,
            Arc::new(|| {}),
            DispatchConfig {
                mutating: HashSet::new(),
                edits_per_sec: 1,
                reads_per_sec: 2,
                timeout: Duration::from_millis(50),
            },
        );
        for _ in 0..2 {
            let r = d.handle(call("read_outline")).unwrap();
            assert_eq!(
                r["result"]["structuredContent"]["error"]["code"],
                "user_busy"
            );
        }
        let r = d.handle(call("read_outline")).unwrap();
        assert_eq!(
            r["result"]["structuredContent"]["error"]["code"],
            "rate_limited"
        );
    }

    #[test]
    fn notifications_get_no_reply() {
        let (d, _q, _) = dispatcher(2);
        assert!(d
            .handle(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .is_none());
    }
}
