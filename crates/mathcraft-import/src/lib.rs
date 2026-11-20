//! Reader and writer for Mathcad Prime `.mcdx` worksheet packages, and the worksheet evaluation
//! shared by the CLI and the GUI: [`evaluate`] runs a sheet, [`compare`] classifies each region.

pub mod compare;
pub mod describe;
pub mod dom;
pub mod evaluate;
pub mod evaluate_plots;
pub mod evaluate_plots3d;
pub mod expr;
pub mod package;
pub mod page;
pub mod regions;
pub mod results;
pub mod rich_text;
pub mod writer;
