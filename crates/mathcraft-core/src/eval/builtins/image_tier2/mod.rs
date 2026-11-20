//! The second image tier: convolution filters, intensity maps and histograms, binary morphology and
//! shape functions, transforms, geometric transforms and packed colour matrices.
//!
//! Every function here reads its image as a matrix (rows are image rows, a packed colour matrix
//! keeps its planes side by side, see `color`). Each loop charges its work to the statement's budget
//! before it runs, so a large or hostile image stops with `WorkLimit` instead of running on.

mod color;
mod filter;
mod geom;
mod level;
mod morph;
mod transform;
mod util;

use super::Registry;

pub(in crate::eval) use transform::{filter_image, filter_units, read_image};

/// Registers the second image tier (filters, morphology, histograms, transforms, colour planes).
pub fn register_image_tier2(r: &mut Registry) {
    filter::register(r);
    level::register(r);
    morph::register(r);
    transform::register(r);
    geom::register(r);
    color::register(r);
}

#[cfg(test)]
mod tests;
