//! Shared helpers for the integration tests.
//!
//! The sample worksheets live in `documentation/examples`. A public checkout does not contain them,
//! so the tests that need them start with `require_samples!();` and return early (with a message on
//! stderr) when the directory is absent. With the samples present the tests run strictly.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// `documentation/examples` at the repository root.
pub fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples")
}

/// True when the sample worksheets are on disk.
pub fn samples_available() -> bool {
    examples_dir().is_dir()
}

/// Returns early from the calling test when the sample worksheets are absent.
macro_rules! require_samples {
    () => {
        if !$crate::support::samples_available() {
            eprintln!("skipped: documentation/examples is not present (public checkout)");
            return;
        }
    };
}
