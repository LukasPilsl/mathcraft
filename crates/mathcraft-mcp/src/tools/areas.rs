//! Layout tools: insert_area, insert_page_break, insert_image. Each makes one region, one undo
//! step labelled `AI: <tool>`, and is also a `batch` op. The file forms of areas, images and page
//! breaks are guesses (see `documentation/UNVERIFIED_FORMATS.md`): saving a sheet that holds one
//! returns a warning from the writer.
//!
//! Image sizes and the decode budget follow the app's insert (`mathcraft-app`, `edit/session.rs`
//! and `images.rs`): this crate cannot depend on the app, so the numbers are repeated here, with
//! the same names. A PNG or JPEG is read from its header, checked against the pixel limits, then
//! decoded once to prove that it is an image.

use std::io::Cursor;
use std::path::Path;

use image::{ImageFormat, ImageReader, Limits};
use mathcraft_core::regular_file::{read_regular_file, RegularFileError};
use mathcraft_doc::{Doc, Placement};
use mathcraft_import::regions::{AreaModel, ImageModel, RegionKind, RegionModel};
use serde_json::Value;

use crate::error::ToolError;
use crate::schema::{self, Args};
use crate::tools::write::{self, dimension, invalid, placement, single};
use crate::tools::{Annotations, Tool, ToolCtx, ToolTable};

/// Largest image file, in bytes (the app's `MAX_IMAGE_BYTES`).
pub const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
/// Longest `data_base64` text. Kept under the 2 MiB request limit of the live link, so about 1 MB
/// of image; a bigger image goes in by `path`.
pub const MAX_BASE64_CHARS: usize = 1_400_000;
/// Most pixels of one image (the app's `MAX_PIXELS`).
pub const MAX_IMAGE_PIXELS: u64 = 16_000_000;
/// Most pixels of all images of one document (the app's `DOCUMENT_PIXEL_BUDGET`).
pub const DOCUMENT_IMAGE_PIXELS: u64 = 128_000_000;
/// Largest number of bytes the decoder may allocate for one image (the app's `MAX_ALLOC_BYTES`).
const MAX_ALLOC_BYTES: u64 = 128 * 1024 * 1024;
/// Frame of an area without width and height (the document's `insert_area`).
const AREA_SIZE: (f64, f64) = (360.0, 120.0);
/// Points per pixel for the first size of an image (96 dpi on a 72-point inch).
const PT_PER_PX: f64 = 0.75;
/// Widest an inserted image is at first, in points.
const IMAGE_MAX_WIDTH_PT: f64 = 400.0;
/// Largest coordinate or size, in page points (the write tools' limit).
const MAX_COORD: f64 = 1.0e6;

/// One layout region to insert.
pub(crate) struct Layout {
    kind: RegionKind,
    size: Option<(f64, f64)>,
    at: Placement,
    /// Pixels of an image (0 for the other kinds), counted against the document budget.
    pixels: u64,
}

pub fn register(t: &mut ToolTable) {
    let at = || {
        vec![
            (
                "x",
                write::number_schema("Left edge in page points (needs y)"),
            ),
            (
                "y",
                write::number_schema("Top edge in page points (needs x)"),
            ),
            (
                "after_region",
                schema::integer("Place directly below this region"),
            ),
        ]
    };
    let mut props = vec![schema::document_id()];
    props.extend(at());
    props.extend([
        (
            "width",
            write::number_schema("Width in page points, above 0"),
        ),
        (
            "height",
            write::number_schema("Height in page points, above 0"),
        ),
        (
            "locked",
            schema::boolean("Lock the area's display state (collapsed or expanded)"),
        ),
        (
            "collapsed",
            schema::boolean("Start collapsed (default false)"),
        ),
        (
            "hide_border",
            schema::boolean("Hide the area's border (not printed)"),
        ),
    ]);
    t.add(Tool {
        name: "insert_area",
        description: "Inserts an empty area: a collapsible group region. Default frame 360 x 120 points; `width`/`height` change it. `locked` fixes its display state, `collapsed` starts it collapsed, `hide_border` hides the border. Placement as insert_math. The area's file form is unverified: document_save returns a warning for it.".into(),
        input_schema: schema::object(&props, &["document_id"]),
        annotations: layout_annotations("Insert area"),
        handler: |ctx, args| single(ctx, args, "insert_area"),
    });

    let mut props = vec![schema::document_id()];
    props.extend(at());
    t.add(Tool {
        name: "insert_page_break",
        description: "Inserts a forced page break: the content below it starts a new page. It has no size. Placement as insert_math. The page break's file form is unverified: document_save returns a warning for it.".into(),
        input_schema: schema::object(&props, &["document_id"]),
        annotations: layout_annotations("Insert page break"),
        handler: |ctx, args| single(ctx, args, "insert_page_break"),
    });

    let mut props = vec![
        schema::document_id(),
        (
            "path",
            schema::string("A .png, .jpg or .jpeg file inside a --root folder"),
        ),
        (
            "data_base64",
            schema::string("Base64 of a PNG or JPEG file, at most about 1 MB of image"),
        ),
    ];
    props.extend(at());
    props.extend([
        (
            "width",
            write::number_schema(
                "Width in page points; default from the pixels (96 dpi), at most 400 pt wide",
            ),
        ),
        (
            "height",
            write::number_schema("Height in page points; default keeps the proportions"),
        ),
    ]);
    t.add(Tool {
        name: "insert_image",
        description: "Inserts a PNG or JPEG image, given as `path` (inside a root, same rules as documents) or `data_base64` (not both). Limits: 4 MB file, 16 megapixels per image, 128 megapixels per document. The image is decoded to check it. Placement as insert_math; a size from the pixels unless width or height is given. The image's file form is unverified: document_save returns a warning for it.".into(),
        input_schema: schema::object(&props, &["document_id"]),
        annotations: layout_annotations("Insert image"),
        handler: |ctx, args| single(ctx, args, "insert_image"),
    });
}

fn layout_annotations(title: &'static str) -> Annotations {
    Annotations {
        title,
        read_only: false,
        destructive: false,
        idempotent: false,
    }
}

/// Reads one layout op. Standalone ops carry `document_id`, batch ops carry `op`.
pub(crate) fn parse(
    ctx: &ToolCtx<'_>,
    name: &str,
    args: &Args<'_>,
    standalone: bool,
) -> Result<Layout, ToolError> {
    let head = if standalone { "document_id" } else { "op" };
    let allowed = |extra: &[&str]| -> Result<(), ToolError> {
        let mut all = vec![head];
        all.extend_from_slice(extra);
        args.only(&all)
    };
    match name {
        "insert_area" => {
            allowed(&[
                "x",
                "y",
                "after_region",
                "width",
                "height",
                "locked",
                "collapsed",
                "hide_border",
            ])?;
            let model = AreaModel {
                collapsed: args.opt_bool("collapsed")?,
                locked: args.opt_bool("locked")?,
                hide_border: args.opt_bool("hide_border")?,
            };
            let size = (
                optional_dimension(args, "width")?.unwrap_or(AREA_SIZE.0),
                optional_dimension(args, "height")?.unwrap_or(AREA_SIZE.1),
            );
            Ok(Layout {
                kind: RegionKind::Area(model),
                size: Some(size),
                at: placement(args)?,
                pixels: 0,
            })
        }
        "insert_page_break" => {
            allowed(&["x", "y", "after_region"])?;
            Ok(Layout {
                kind: RegionKind::PageBreak,
                size: Some((0.0, 0.0)),
                at: placement(args)?,
                pixels: 0,
            })
        }
        "insert_image" => {
            allowed(&[
                "path",
                "data_base64",
                "x",
                "y",
                "after_region",
                "width",
                "height",
            ])?;
            let bytes = image_bytes(ctx, args)?;
            let (px_w, px_h) = validate_image(&bytes)?;
            let size = image_size(
                (px_w, px_h),
                optional_dimension(args, "width")?,
                optional_dimension(args, "height")?,
            )?;
            Ok(Layout {
                kind: RegionKind::Image(ImageModel {
                    item: String::new(),
                    bytes: bytes.into(),
                }),
                size: Some(size),
                at: placement(args)?,
                pixels: u64::from(px_w) * u64::from(px_h),
            })
        }
        other => Err(invalid(format!("`{other}` is not a layout op"))),
    }
}

/// Inserts a parsed layout region. Images are checked against the document's pixel budget first.
pub(crate) fn apply(doc: &mut Doc, layout: Layout) -> Result<u32, ToolError> {
    if layout.pixels > 0 {
        let total = document_pixels(doc.regions()).saturating_add(layout.pixels);
        if total > DOCUMENT_IMAGE_PIXELS {
            return Err(ToolError::LimitReached(format!(
                "the document's images would hold {total} pixels, over the limit of {DOCUMENT_IMAGE_PIXELS} (128 megapixels); remove an image first"
            )));
        }
    }
    doc.insert_region(layout.kind, layout.size, layout.at)
        .map_err(write::doc_err)
}

/// A dimension that is optional: absent or null gives `None`, a number must be a valid size.
fn optional_dimension(args: &Args<'_>, key: &str) -> Result<Option<f64>, ToolError> {
    match args.0.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => dimension(args, key).map(Some),
    }
}

/// The image bytes of `path` or `data_base64`, one of the two.
fn image_bytes(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<Vec<u8>, ToolError> {
    match (args.opt_str("path")?, args.opt_str("data_base64")?) {
        (Some(_), Some(_)) => Err(invalid("give `path` or `data_base64`, not both")),
        (None, None) => Err(invalid("missing argument `path` (or `data_base64`)")),
        (Some(path), None) => {
            let file = ctx.backend.resolve_image(path)?;
            let bytes = read_image_file(&file)?;
            check_bytes(&bytes)?;
            Ok(bytes)
        }
        (None, Some(text)) => {
            if text.len() > MAX_BASE64_CHARS {
                return Err(ToolError::LimitReached(format!(
                    "`data_base64` is {} characters, over the limit of {MAX_BASE64_CHARS}; use `path` for a bigger image",
                    text.len()
                )));
            }
            let bytes = decode_base64(text)?;
            check_bytes(&bytes)?;
            Ok(bytes)
        }
    }
}

/// Reads a resolved image file once, through the regular-file checks: a FIFO, device or directory
/// is refused without blocking, and the file must still be the one the root check accepted.
fn read_image_file(file: &Path) -> Result<Vec<u8>, ToolError> {
    read_regular_file(file, file, MAX_IMAGE_BYTES as u64).map_err(|e| match e {
        RegularFileError::NotRegular => ToolError::PathDenied("not a regular file".into()),
        RegularFileError::TooLarge { limit } => ToolError::LimitReached(format!(
            "the image is over the limit of {limit} bytes (4 MB)"
        )),
        RegularFileError::Changed => {
            ToolError::PathDenied("the file changed while it was being read".into())
        }
        RegularFileError::Io(e) => ToolError::Doc(format!("cannot read the image: {e}")),
    })
}

fn check_bytes(bytes: &[u8]) -> Result<(), ToolError> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(ToolError::LimitReached(format!(
            "the image is {} bytes, over the limit of {MAX_IMAGE_BYTES} (4 MB)",
            bytes.len()
        )));
    }
    Ok(())
}

/// Strict standard base64: the alphabet, optional `=` padding, no whitespace. Padding must be the
/// exact count that completes the last group to four characters (`aGk=` is valid, `aGk==` is
/// not). The unused bits of the last character must be zero (`aA==` is valid, `aB==` is not).
pub(crate) fn decode_base64(text: &str) -> Result<Vec<u8>, ToolError> {
    let body = text.trim_end_matches('=');
    let pad = text.len() - body.len();
    let invalid_text = || invalid("`data_base64` is not valid base64");
    if body.len() % 4 == 1 {
        return Err(invalid_text());
    }
    if pad > 0 && pad != (4 - body.len() % 4) % 4 {
        return Err(invalid_text());
    }
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for b in body.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(invalid("`data_base64` is not valid base64")),
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // The bits left over after the last whole byte are the unused bits of the final character.
    if acc != 0 {
        return Err(invalid_text());
    }
    Ok(out)
}

/// A reader for PNG or JPEG bytes (by content, not by name), with the allocation limit.
fn reader(bytes: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>, ToolError> {
    let guessed = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| invalid("only PNG and JPEG images can be inserted"))?;
    let format = guessed
        .format()
        .filter(|f| matches!(f, ImageFormat::Png | ImageFormat::Jpeg))
        .ok_or_else(|| invalid("only PNG and JPEG images can be inserted"))?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_ALLOC_BYTES);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    Ok(reader)
}

/// The pixel size of a PNG or JPEG, after the pixel limit and a full decode have passed.
fn validate_image(bytes: &[u8]) -> Result<(u32, u32), ToolError> {
    let (w, h) = reader(bytes)?
        .into_dimensions()
        .map_err(|_| invalid("the image does not read as PNG or JPEG"))?;
    if w == 0 || h == 0 {
        return Err(invalid("the image has no pixels"));
    }
    let pixels = u64::from(w) * u64::from(h);
    if pixels > MAX_IMAGE_PIXELS {
        return Err(ToolError::LimitReached(format!(
            "the image has {pixels} pixels, over the limit of {MAX_IMAGE_PIXELS} (16 megapixels)"
        )));
    }
    reader(bytes)?
        .decode()
        .map_err(|_| invalid("the image does not decode"))?;
    Ok((w, h))
}

/// The first size of an image in points: from its pixels, at most 400 pt wide; a given width or
/// height keeps the proportions for the other side.
fn image_size(
    px: (u32, u32),
    width: Option<f64>,
    height: Option<f64>,
) -> Result<(f64, f64), ToolError> {
    let (pw, ph) = (f64::from(px.0), f64::from(px.1));
    let size = match (width, height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w * ph / pw),
        (None, Some(h)) => (h * pw / ph, h),
        (None, None) => {
            let (mut w, mut h) = (pw * PT_PER_PX, ph * PT_PER_PX);
            if w > IMAGE_MAX_WIDTH_PT {
                h *= IMAGE_MAX_WIDTH_PT / w;
                w = IMAGE_MAX_WIDTH_PT;
            }
            (w, h)
        }
    };
    if !(size.0 > 0.0 && size.1 > 0.0 && size.0 <= MAX_COORD && size.1 <= MAX_COORD) {
        return Err(invalid(format!(
            "the image size would be {:.0} x {:.0} points; give width and height both, within 1 to {MAX_COORD}",
            size.0, size.1
        )));
    }
    Ok(size)
}

/// Pixels of the image regions of a document, nested ones included. Bytes that do not read give 0
/// (they were checked when they were inserted).
fn document_pixels(regions: &[RegionModel]) -> u64 {
    regions
        .iter()
        .map(|r| {
            let own = match &r.kind {
                RegionKind::Image(image) => reader(&image.bytes)
                    .ok()
                    .and_then(|rd| rd.into_dimensions().ok())
                    .map_or(0, |(w, h)| u64::from(w) * u64::from(h)),
                _ => 0,
            };
            own.saturating_add(document_pixels(&r.children))
        })
        .fold(0, u64::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_and_refuses_bad_text() {
        assert_eq!(decode_base64("").unwrap(), Vec::<u8>::new());
        assert_eq!(decode_base64("aGk=").unwrap(), b"hi".to_vec());
        assert_eq!(decode_base64("aGk").unwrap(), b"hi".to_vec());
        assert_eq!(decode_base64("aGVsbG8h").unwrap(), b"hello!".to_vec());
        for bad in ["a", "a b=", "aGk===", "a*bc", "aGk\n", "é"] {
            assert!(decode_base64(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn base64_padding_must_be_canonical() {
        // The padding must complete the last group: `aGk` takes one `=`, not two.
        assert_eq!(decode_base64("aGk=").unwrap(), b"hi".to_vec());
        assert_eq!(decode_base64("aA==").unwrap(), b"h".to_vec());
        assert_eq!(decode_base64("aGVsbG8h").unwrap(), b"hello!".to_vec());
        for bad in ["aGk==", "aA=", "aGVsbG8==", "aGVsbG8h=", "aGVs=bG8h"] {
            assert!(decode_base64(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn base64_unused_bits_must_be_zero() {
        // `B` is 000001: the low four bits are unused in `aB` and must be zero. `l` is 100101:
        // the low two bits are unused in `aGl`.
        assert_eq!(decode_base64("aA").unwrap(), b"h".to_vec());
        assert_eq!(decode_base64("aA==").unwrap(), b"h".to_vec());
        assert_eq!(decode_base64("aGk").unwrap(), b"hi".to_vec());
        for bad in ["aB", "aB==", "aGl", "aGl="] {
            assert!(decode_base64(bad).is_err(), "{bad:?} should be refused");
        }
    }

    /// Runs `image_bytes` for a `path` argument on a worker thread, so a regression that blocks
    /// fails the test after five seconds instead of hanging the suite.
    #[cfg(unix)]
    fn image_path_result(root: std::path::PathBuf, path: &str) -> Result<Vec<u8>, ToolError> {
        let (tx, rx) = std::sync::mpsc::channel();
        let path = path.to_string();
        std::thread::spawn(move || {
            let backend = crate::headless::HeadlessBackend::new(
                crate::paths::Roots::new(&[root]).unwrap(),
                4,
            );
            let limits = crate::tools::Limits::default();
            let ctx = crate::tools::ToolCtx {
                backend: &backend,
                limits: &limits,
                catalog: &[],
            };
            let mut map = serde_json::Map::new();
            map.insert("path".into(), Value::String(path));
            let _ = tx.send(image_bytes(&ctx, &Args(&map)));
        });
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .expect("insert_image must not block on a FIFO")
    }

    #[cfg(unix)]
    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mathcraft-areas-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::canonicalize(&dir).unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_image_is_refused_without_hanging() {
        let dir = scratch_dir("fifo");
        let fifo = dir.join("fifo.png");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        let result = image_path_result(dir.clone(), "fifo.png");
        assert!(
            matches!(result, Err(ToolError::PathDenied(_))),
            "{result:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_regular_image_file_is_read_whole() {
        let dir = scratch_dir("regular");
        std::fs::write(dir.join("a.png"), b"not really a png").unwrap();
        let bytes = image_path_result(dir.clone(), "a.png").unwrap();
        assert_eq!(bytes, b"not really a png".to_vec());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn garbage_bytes_are_refused_not_decoded() {
        assert!(validate_image(b"").is_err());
        assert!(validate_image(b"GIF89a....").is_err());
        assert!(
            validate_image(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]).is_err()
        );
        assert!(validate_image(&[0xff, 0xd8, 0xff, 0xe0, 0, 0]).is_err());
    }

    #[test]
    fn sizes_follow_the_pixels_and_the_given_sides() {
        assert_eq!(image_size((100, 50), None, None).unwrap(), (75.0, 37.5));
        assert_eq!(image_size((1000, 500), None, None).unwrap(), (400.0, 200.0));
        assert_eq!(
            image_size((100, 50), Some(200.0), None).unwrap(),
            (200.0, 100.0)
        );
        assert_eq!(
            image_size((100, 50), None, Some(20.0)).unwrap(),
            (40.0, 20.0)
        );
        assert!(image_size((1, 16_000_000), Some(1.0), None).is_err());
        assert!(image_size((100, 50), Some(MAX_COORD), Some(MAX_COORD)).is_ok());
    }

    #[test]
    fn document_pixels_of_no_images_is_zero() {
        assert_eq!(document_pixels(&[]), 0);
    }
}
