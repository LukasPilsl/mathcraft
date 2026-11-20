//! Image regions: which package part each one uses, and the parts written for new ones.
//!
//! An image region names a worksheet relationship (`item-idref`), and the relationship names the
//! image part. [`plan`] keeps the source relationship and part of an image whose bytes are
//! unchanged. A new image gets a new part, `mathcad/media/ImageN.png` or `.jpeg`, and a
//! relationship whose ID is a hash of the region and the image, as for text.
//!
//! UNVERIFIED: no sample has an image part. The relationship type, the part folder and the part
//! naming are guesses; see `documentation/UNVERIFIED_FORMATS.md`.

use std::collections::{HashMap, HashSet};

use super::text::{fnv1a, fresh_id};
use super::WriteError;
use crate::package::Package;
use crate::regions::{RegionKind, Worksheet};

/// The relationship type of an image part. UNVERIFIED: the OPC standard type, not seen in a sample.
pub(super) const IMAGE_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

/// The image regions of a written worksheet, and the parts and relationships they need.
#[derive(Debug, Default)]
pub(super) struct ImagePlan {
    /// The `item-idref` of each image region, by region ID.
    pub items: HashMap<u32, String>,
    /// Relationship elements to add to the worksheet relationships, as raw XML.
    pub new_rels: Vec<String>,
    /// Name and bytes of each new image part.
    pub new_parts: Vec<(String, Vec<u8>)>,
    /// `Default` content types the new parts need: (extension, content type).
    pub defaults: Vec<(&'static str, &'static str)>,
}

/// Plans the image regions of `model`. `taken_names` and `taken_ids` are the part names and
/// relationship IDs already in use, including those the text plan adds.
pub(super) fn plan(
    model: &Worksheet,
    source: Option<(&Package, &Worksheet)>,
    taken_names: &HashSet<String>,
    taken_ids: &HashSet<String>,
) -> Result<ImagePlan, WriteError> {
    let mut source_by_id = HashMap::new();
    if let Some((_, src_model)) = source {
        for region in src_model.all_regions() {
            source_by_id.entry(region.id).or_insert(region);
        }
    }
    let mut names = taken_names.clone();
    let mut ids = taken_ids.clone();
    let mut next_number = 1usize;
    let mut plan = ImagePlan::default();
    for region in model.all_regions() {
        let RegionKind::Image(image) = &region.kind else {
            continue;
        };
        let media = image.media_type().ok_or_else(|| {
            WriteError::Unsupported(format!(
                "image region {} is neither a PNG nor a JPEG file",
                region.id
            ))
        })?;
        if let Some((pkg, _)) = source {
            let kept = source_by_id
                .get(&region.id)
                .and_then(|src| match &src.kind {
                    RegionKind::Image(src_image) if src_image.bytes == image.bytes => {
                        Some(src_image.item.clone())
                    }
                    _ => None,
                });
            if let Some(item) = kept.filter(|item| pkg.image(item).is_some()) {
                plan.items.insert(region.id, item);
                continue;
            }
        }
        let extension = if media == "image/png" { "png" } else { "jpeg" };
        let name = loop {
            let candidate = format!("mathcad/media/Image{next_number}.{extension}");
            next_number += 1;
            if !names.contains(&candidate) {
                break candidate;
            }
        };
        let id = fresh_id(
            region.id,
            &format!("image {:016x}", fnv1a(&image.bytes)),
            &mut ids,
        );
        plan.new_rels.push(format!(
            "<Relationship Type=\"{IMAGE_TYPE}\" Target=\"/{name}\" Id=\"{id}\" />"
        ));
        plan.new_parts.push((name.clone(), image.bytes.to_vec()));
        if !plan.defaults.iter().any(|(ext, _)| *ext == extension) {
            plan.defaults.push((extension, media));
        }
        names.insert(name);
        plan.items.insert(region.id, id);
    }
    Ok(plan)
}
