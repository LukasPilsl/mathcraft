//! The worksheet document API, with no GUI. The GUI (`mathcraft-app`) and a future MCP server call
//! the same functions: open, create and save a worksheet, list its regions under ids that stay
//! the same across edits, read and edit math, text and plots, undo and redo, calculate, and read
//! the variables, the errors and a plain-text transcript.
//!
//! - [`doc::Doc`] is the handle. It owns the [`document::Document`] and its undo [`command::History`].
//! - [`calc`] evaluates a document under the engine's limits (work budget, 60 s deadline, cancel).
//! - [`read`] holds the read-only views: outline, region content, variables, errors, transcript.

pub mod calc;
pub mod command;
pub mod context;
pub mod doc;
pub mod document;
pub mod extent;
pub mod functions_table;
pub mod locale;
pub mod plot;
pub mod program_text;
pub mod read;
pub mod search;

pub use calc::{CalcOptions, Calculation, RegionResult, ResultStatus};
pub use command::EditError;
pub use doc::{
    normalise_equation, Deleted, Doc, DocError, Placement, SolveBlockIds, SolveBlockSpec,
    SOLVER_NAMES,
};
pub use document::Document;
pub use read::{Problem, RegionContent, RegionDetail, RegionSummary, Variable, VariableKind};
pub use search::{Match as SearchMatch, MatchKind, ReplaceReport, SearchOptions};

#[cfg(test)]
mod doc_tests;
#[cfg(test)]
mod qa_engine_tests;
#[cfg(test)]
mod search_tests;
