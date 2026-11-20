//! Inspection and golden harness for Mathcad Prime worksheets (`.mcdx`).
//!
//! - [`inspect_file`] prints one worksheet in evaluation order, with Prime's stored results.
//! - [`golden`] loads worksheets, evaluates them with `mathcraft_import::evaluate` and classifies
//!   each region with `mathcraft_import::compare::classify`. Nothing is hidden: mismatches and
//!   engine errors stay in the report.
//!
//! The evaluation and the comparison rules live in `mathcraft-import`. This crate keeps the
//! arguments, the file loops, the tables and the exit codes.

mod args;
mod common;
mod golden;
mod inspect;
mod render;
mod render_png;

pub use args::{
    default_sample_files, parse_args, run_command, Command, ParseError, RenderArgs, USAGE,
};
pub use golden::{golden, FileReport, GoldenRun, RegionRow, Tally};
pub use inspect::inspect_file;
pub use mathcraft_import::compare::Category;
pub use mathcraft_import::describe::{describe_stored, describe_value};
pub use mathcraft_import::evaluate::STACK_SIZE;
pub use render::{describe_outcome, render_run};
