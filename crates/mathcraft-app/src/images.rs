//! Decoding of image regions: the PNG and JPEG bytes of a package part, to pixels for the canvas.
//!
//! Only the `image` crate's PNG and JPEG decoders are compiled in (MIT or Apache-2.0). Decoding
//! happens on the first draw of an image and its result is cached (see `canvas.rs`). Nothing here
//! touches the file system.
//!
//! Memory is bounded three ways: an image over [`MAX_PIXELS`] is refused from its header before
//! any pixel is allocated, the decoder may not allocate more than [`MAX_ALLOC_BYTES`], and the
//! decoded pixels of one document are capped by a [`PixelBudget`] of [`DOCUMENT_PIXEL_BUDGET`].

use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use eframe::egui::ColorImage;
use image::{DynamicImage, ImageFormat, ImageReader, Limits};

/// Largest number of pixels (width times height) of an image that is decoded.
pub const MAX_PIXELS: u64 = 16_000_000;

/// Most decoded pixels of all images of one document. Once the budget is spent, further images
/// are not decoded and are shown as unavailable.
pub const DOCUMENT_PIXEL_BUDGET: u64 = 128_000_000;

/// Largest number of bytes the decoder may allocate at one time for one image.
const MAX_ALLOC_BYTES: u64 = 128 * 1024 * 1024;

/// Decoded pixels counted against a document's [`DOCUMENT_PIXEL_BUDGET`]. Clones share the count.
#[derive(Clone, Debug, Default)]
pub struct PixelBudget(Arc<AtomicU64>);

impl PartialEq for PixelBudget {
    /// Two budgets are equal when they are the same budget.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl PixelBudget {
    /// A budget with nothing spent.
    pub fn new() -> Self {
        Self::default()
    }

    /// Spends `pixels` from the budget. False, and nothing spent, when that would exceed
    /// [`DOCUMENT_PIXEL_BUDGET`].
    pub fn reserve(&self, pixels: u64) -> bool {
        self.0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(pixels)
                    .filter(|total| *total <= DOCUMENT_PIXEL_BUDGET)
            })
            .is_ok()
    }
}

/// The image format of PNG or JPEG bytes, from their content. `None` for other formats.
fn format_of(bytes: &[u8]) -> Option<ImageFormat> {
    let format = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .format()?;
    matches!(format, ImageFormat::Png | ImageFormat::Jpeg).then_some(format)
}

/// A reader over `bytes` for a known format, with the allocation limit applied.
fn reader(bytes: &[u8], format: ImageFormat) -> ImageReader<Cursor<&[u8]>> {
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_ALLOC_BYTES);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    reader
}

/// The width and height in pixels of PNG or JPEG bytes, read from their header only. `None` for
/// other formats and for bytes that do not parse.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let format = format_of(bytes)?;
    reader(bytes, format).into_dimensions().ok()
}

/// Decodes PNG or JPEG bytes to RGBA pixels. `None` when the bytes are not an image of those
/// formats, when the image has more than [`MAX_PIXELS`] pixels, when `budget` cannot cover its
/// pixels, or when the bytes do not decode.
///
/// An image with a side longer than `max_side` (the graphics driver's texture limit) is scaled
/// down, keeping its proportions, so that it fits.
pub fn decode(bytes: &[u8], max_side: u32, budget: &PixelBudget) -> Option<ColorImage> {
    let format = format_of(bytes)?;
    let (width, height) = reader(bytes, format).into_dimensions().ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_PIXELS || !budget.reserve(pixels) {
        return None;
    }
    let decoded = reader(bytes, format).decode().ok()?;
    let max_side = max_side.max(1);
    let decoded = if width > max_side || height > max_side {
        let scale = f64::from(max_side) / f64::from(width.max(height));
        let new_w = ((f64::from(width) * scale).floor() as u32).max(1);
        let new_h = ((f64::from(height) * scale).floor() as u32).max(1);
        decoded.thumbnail_exact(new_w, new_h)
    } else {
        decoded
    };
    let rgba = match decoded {
        DynamicImage::ImageRgba8(rgba) => rgba,
        other => other.to_rgba8(),
    };
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_png() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../documentation/examples/operations/Bild.png"
        ))
        .expect("sample image")
    }

    /// A PNG of the given size, filled with one colour. Encoding is cheap for flat pixels.
    fn flat_png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .expect("encodes");
        out
    }

    #[test]
    fn decodes_the_sample_png() {
        require_samples!();
        let pixels = decode(&sample_png(), u32::MAX, &PixelBudget::new()).expect("decodes");
        assert_eq!(pixels.size, [661, 748]);
    }

    #[test]
    fn rejects_bytes_that_are_not_an_image() {
        let budget = PixelBudget::new();
        assert!(decode(b"not an image at all", u32::MAX, &budget).is_none());
        assert!(decode(b"GIF89a\x01\x00\x01\x00", u32::MAX, &budget).is_none());
    }

    #[test]
    fn refuses_an_image_over_the_pixel_cap_before_decoding() {
        // 4001 x 4001 is just over MAX_PIXELS, and refused from the header.
        let bytes = flat_png(4001, 4001);
        assert_eq!(dimensions(&bytes), Some((4001, 4001)));
        let budget = PixelBudget::new();
        assert!(decode(&bytes, u32::MAX, &budget).is_none());
        // Nothing was spent on it.
        assert!(budget.reserve(DOCUMENT_PIXEL_BUDGET));
    }

    #[test]
    fn the_budget_stops_decoding_once_spent() {
        let bytes = flat_png(64, 64);
        let budget = PixelBudget::new();
        assert!(budget.reserve(DOCUMENT_PIXEL_BUDGET - 4096 + 1));
        assert!(decode(&bytes, u32::MAX, &budget).is_none());
        let other = PixelBudget::new();
        assert!(decode(&bytes, u32::MAX, &other).is_some());
        // Clones share the count; a fresh budget does not.
        assert!(budget.clone().reserve(4095));
        assert!(!budget.reserve(2));
    }

    #[test]
    fn scales_an_image_down_to_the_texture_side() {
        let bytes = flat_png(300, 100);
        let pixels = decode(&bytes, 120, &PixelBudget::new()).expect("decodes");
        assert_eq!(pixels.size, [120, 40]);
    }

    #[test]
    fn keeps_an_image_within_the_texture_side() {
        let bytes = flat_png(40, 30);
        let pixels = decode(&bytes, 120, &PixelBudget::new()).expect("decodes");
        assert_eq!(pixels.size, [40, 30]);
    }
}
