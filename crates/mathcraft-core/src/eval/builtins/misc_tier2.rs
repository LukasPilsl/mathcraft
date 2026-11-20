//! The second miscellaneous tier: expression-type and sign functions (`IsArray`, `IsScalar`,
//! `IsString`, `signum`, `csgn`), design-of-experiments helpers (`fractfact`, `fractruns`,
//! `fractresol`, `foldover`), and the file functions for fixed-width, delimited and PRN text plus
//! WAV and BMP files (see the submodules).
//!
//! The submodules are registered here and nowhere else; `eval/env.rs` calls
//! [`register_misc_tier2`] once.

mod bmp;
mod doe2;
mod files2;
mod rgb;
mod utility;
mod wav;

use super::Registry;

/// Registers every name of this tier.
pub fn register_misc_tier2(r: &mut Registry) {
    utility::register(r);
    doe2::register(r);
    files2::register(r);
    wav::register(r);
    bmp::register(r);
    rgb::register(r);
}
