//! The one error type of every tool. It becomes a tool result with `isError: true` and a stable
//! machine-readable `code`, so a client can react without parsing the message.

use std::fmt;

use mathcraft_doc::DocError;

#[derive(Debug, Clone, PartialEq)]
pub enum ToolError {
    /// A missing or malformed argument.
    InvalidParams(String),
    /// No open document has this id.
    UnknownDocument(String),
    /// The document has no such region (or another named item is missing).
    NotFound(String),
    /// The path is outside the configured roots, escapes through `..` or a symlink, or is not a
    /// `.mcdx` file.
    PathDenied(String),
    /// The target file exists and `overwrite` was not set.
    Exists(String),
    /// The document has unsaved changes and `discard` was not set.
    Unsaved(String),
    /// A server limit (open documents, response size) was reached.
    LimitReached(String),
    /// The document layer refused: bad file, bad text, locked region.
    Doc(String),
    /// Something the server did not expect.
    Internal(String),
    /// No GPU renderer on this machine, or the server was built without the `render` feature.
    RenderUnavailable(String),
    /// A render failed, or ran past its 30 s deadline.
    RenderFailed(String),
    /// Live mode: the user is typing in this document (an open draft); try again shortly.
    UserBusy(String),
    /// Live mode: the user paused the AI connection in the window.
    Paused(String),
    /// A live-only tool was called without a running app (or with the endpoint switched off).
    LiveUnavailable(String),
    /// Live mode: too many edits per second.
    RateLimited(String),
}

impl ToolError {
    /// The stable code a client can match on.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidParams(_) => "invalid_params",
            Self::UnknownDocument(_) => "unknown_document",
            Self::NotFound(_) => "not_found",
            Self::PathDenied(_) => "path_denied",
            Self::Exists(_) => "exists",
            Self::Unsaved(_) => "unsaved_changes",
            Self::LimitReached(_) => "limit_reached",
            Self::Doc(_) => "document_error",
            Self::Internal(_) => "internal",
            Self::RenderUnavailable(_) => "render_unavailable",
            Self::RenderFailed(_) => "render_failed",
            Self::UserBusy(_) => "user_busy",
            Self::Paused(_) => "paused",
            Self::LiveUnavailable(_) => "live_unavailable",
            Self::RateLimited(_) => "rate_limited",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::InvalidParams(m)
            | Self::UnknownDocument(m)
            | Self::NotFound(m)
            | Self::PathDenied(m)
            | Self::Exists(m)
            | Self::Unsaved(m)
            | Self::LimitReached(m)
            | Self::Doc(m)
            | Self::Internal(m)
            | Self::RenderUnavailable(m)
            | Self::RenderFailed(m)
            | Self::UserBusy(m)
            | Self::Paused(m)
            | Self::LiveUnavailable(m)
            | Self::RateLimited(m) => m,
        }
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for ToolError {}

/// The error for a region id the document does not have. It says where to find the ids.
pub fn no_region(id: u32) -> ToolError {
    ToolError::NotFound(format!(
        "no region with id {id}; use read_outline to list the region ids"
    ))
}

impl From<DocError> for ToolError {
    fn from(e: DocError) -> Self {
        match e {
            DocError::NotFound(id) => no_region(id),
            other => Self::Doc(other.to_string()),
        }
    }
}
