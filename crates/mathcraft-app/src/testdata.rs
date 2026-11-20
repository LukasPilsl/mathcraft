//! Paths of the sample worksheets used by the tests.

use std::path::{Path, PathBuf};

/// The sample worksheets, `documentation/examples` at the repository root.
pub fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples")
}

/// The path of one sample worksheet.
pub fn sample(name: &str) -> PathBuf {
    examples_dir().join(name)
}

/// True when the sample worksheets are on disk. A public checkout has no `documentation/examples`.
pub fn samples_available() -> bool {
    examples_dir().is_dir()
}

/// Returns early from the calling test when the sample worksheets are absent. Declared first in
/// `lib.rs` (`#[macro_use]`) so that every test module can use it; `#[macro_export]` lets the
/// examples use it too (`use mathcraft_app::require_samples;`). With the samples present the test
/// runs strictly.
#[macro_export]
macro_rules! require_samples {
    () => {
        if !$crate::testdata::samples_available() {
            eprintln!("skipped: documentation/examples is not present (public checkout)");
            return;
        }
    };
}
