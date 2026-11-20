//! The window's side of live mode: starts the loopback endpoint, owns the job queue the UI thread
//! serves once per frame, the pause switch and the flashes of regions the AI just changed. The
//! tool table itself runs in `mathcraft-mcp` (`LiveCore`); `app/live_host.rs` lends the tabs to it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use mathcraft_mcp::live::ui::{
    DispatchConfig, DEFAULT_EDITS_PER_SEC, DEFAULT_READS_PER_SEC, DEFAULT_TIMEOUT,
};
use mathcraft_mcp::live::{self, Endpoint, EndpointConfig, LiveCore, LiveShared, UiQueue};
use mathcraft_mcp::paths::Roots;
use mathcraft_mcp::tools::Limits;

/// How long a changed region stays highlighted.
pub const FLASH: Duration = Duration::from_millis(1500);
/// Most documents the AI can have open in the window at once.
const MAX_DOCUMENTS: usize = 32;

/// What the status bar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiState {
    /// Endpoint on, no client connected.
    Ready,
    /// A client is connected.
    Connected,
    /// The user paused the connection: every live tool call answers `paused`.
    Paused,
}

/// A region the AI changed, highlighted until [`FLASH`] has passed.
#[derive(Debug, Clone)]
pub struct Flash {
    pub doc: u64,
    pub region: u32,
    pub at: Instant,
}

pub struct LiveRuntime {
    pub core: LiveCore,
    pub queue: UiQueue,
    pub shared: Arc<LiveShared>,
    /// Kept alive: dropping it stops the listener and removes the discovery file.
    _endpoint: Endpoint,
    pub flashes: Vec<Flash>,
}

impl LiveRuntime {
    /// Starts the endpoint. `dir` is where the discovery file goes (`None`: no file, for the
    /// screenshot harness).
    ///
    /// # Errors
    /// A message when the socket or the file cannot be created.
    pub fn start(ctx: &egui::Context, dir: Option<PathBuf>) -> Result<Self, String> {
        let roots = Roots::new(&Roots::default_dirs())?;
        let shared = LiveShared::new();
        let wake = ctx.clone();
        let (dispatcher, queue) = live::channel(
            shared.clone(),
            Arc::new(move || wake.request_repaint()),
            DispatchConfig {
                mutating: live::mutating_tool_names(),
                edits_per_sec: DEFAULT_EDITS_PER_SEC,
                reads_per_sec: DEFAULT_READS_PER_SEC,
                timeout: DEFAULT_TIMEOUT,
            },
        );
        let endpoint = Endpoint::start(
            Arc::new(dispatcher),
            shared.clone(),
            EndpointConfig { dir, token: None },
        )
        .map_err(|e| e.to_string())?;
        let core = LiveCore::new(roots, MAX_DOCUMENTS, Limits::default(), shared.clone());
        Ok(Self {
            core,
            queue,
            shared,
            _endpoint: endpoint,
            flashes: Vec::new(),
        })
    }

    pub fn state(&self) -> AiState {
        if self.shared.paused() {
            AiState::Paused
        } else if self.shared.connections() > 0 {
            AiState::Connected
        } else {
            AiState::Ready
        }
    }

    /// Intensities (1 down to 0) of the regions of document `doc` that are still flashing; also
    /// drops the finished flashes. True in the second field while any flash is left.
    pub fn flashing(&mut self, doc: u64) -> (Vec<(u32, f32)>, bool) {
        self.flashes.retain(|f| f.at.elapsed() < FLASH);
        let list = self
            .flashes
            .iter()
            .filter(|f| f.doc == doc)
            .map(|f| {
                (
                    f.region,
                    1.0 - f.at.elapsed().as_secs_f32() / FLASH.as_secs_f32(),
                )
            })
            .collect();
        (list, !self.flashes.is_empty())
    }
}
