//! Live mode: the running Mathcraft window hosts the MCP tools, and the standalone `mathcraft-mcp`
//! binary forwards to it.
//!
//! ```text
//! MCP client --stdio--> mathcraft-mcp (Router) --127.0.0.1 + token--> app endpoint thread
//!                                                                        | job queue + repaint
//!                                                                        v
//!                                       UI thread: LiveCore runs the same tool table on LiveBackend
//! ```
//!
//! * [`discovery`]: the 0600 file with port and token the app writes, the client reads.
//! * [`frame`]: newline-delimited JSON with a size cap.
//! * [`endpoint`]: the app side: a loopback TCP listener, token check, one thread per connection.
//! * [`ui`]: the hand-over to the UI thread: job queue, pause switch, rate limit.
//! * [`backend`] and [`core`]: the live [`crate::backend::DocBackend`] and the loop body the UI
//!   thread runs for each job. The window (or a fake one in tests) implements [`LiveHost`].
//! * [`router`]: the client side: decides per call between the live app and the headless server.
//!
//! The tool handlers receive a closure over `&mut Doc` and cannot cross a process boundary, so the
//! tool table runs inside the app; the standalone binary forwards whole JSON-RPC calls instead.

pub mod backend;
pub mod core;
pub mod discovery;
pub mod endpoint;
pub mod frame;
pub mod router;
pub mod ui;

use std::sync::atomic::{AtomicU64, Ordering};

pub use self::backend::{Effects, LentDoc, LiveBackend, NewDoc};
pub use self::core::{Lend, LiveCore, LiveHost};
pub use self::endpoint::{Endpoint, EndpointConfig, Handler};
pub use self::router::{LiveClient, Router};
pub use self::ui::{channel, DispatchConfig, LiveShared, UiDispatcher, UiQueue};

/// Version of the wire protocol between the binary and the app.
pub const PROTOCOL: u32 = 1;

/// Prefix of the ids of live documents (`live1`, `live2`, ...). Headless ids are `doc<N>`, so an
/// id says where its document lives.
pub const LIVE_PREFIX: &str = "live";

static NEXT_DOC: AtomicU64 = AtomicU64::new(1);

/// A fresh number for a live document id. The window gives one to every tab it creates.
pub fn next_doc_number() -> u64 {
    NEXT_DOC.fetch_add(1, Ordering::Relaxed)
}

/// The id of live document number `n`.
pub fn doc_id(n: u64) -> String {
    format!("{LIVE_PREFIX}{n}")
}

/// True for an id that belongs to a live document.
pub fn is_live_id(id: &str) -> bool {
    id.strip_prefix(LIVE_PREFIX)
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

/// The names of the tools that change documents (the ones the edit rate limit counts).
pub fn mutating_tool_names() -> std::collections::HashSet<String> {
    crate::tools::default_table()
        .catalog()
        .into_iter()
        .filter(|m| {
            !m.read_only
                || matches!(
                    m.name,
                    "calculate" | "evaluate_expression" | "app_screenshot"
                )
                || m.name.starts_with("render")
        })
        .map(|m| m.name.to_string())
        .collect()
}
