//! Shared test helpers.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub mod baseline;

/// `documentation/examples` at the repository root.
pub fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples")
}

/// True when the sample worksheets are on disk.
pub fn samples_available() -> bool {
    examples_dir().is_dir()
}

/// Returns early from the calling test when the sample worksheets are absent (a public checkout
/// has none). With the samples present the test runs strictly.
macro_rules! require_samples {
    () => {
        if !$crate::common::samples_available() {
            eprintln!("skipped: documentation/examples is not present (public checkout)");
            return;
        }
    };
}
