//! Grayscale BMP image files: `READBMP` and `WRITEBMP` (help: "Image Files").
//!
//! Built:
//! - `WRITEBMP("file.bmp", M)`: writes the image matrix `M` (rows are image rows, the first row at
//!   the top) as an 8-bit BMP with a grey palette. Values are rounded to the nearest integer and
//!   must lie in 0 to 255. Only the `.bmp` extension is written; `.ppm` and `.tif` are not built.
//! - `READBMP("file")`: the image as a matrix of grey levels. Only 8-bit uncompressed (BI_RGB) BMP
//!   files whose palette is grey (red = green = blue in every entry) are read; the value of a pixel
//!   is its palette level. Colour BMP files would need a grey conversion rule that the help does not
//!   give, so they are refused, as are 1, 4, 16, 24 and 32 bit files and compressed files.
//!
//! Hostile headers are checked before any allocation: the dimensions, the palette and pixel offsets,
//! the row stride, and the total pixel count (at most [`MAX_CELLS`]). The file is read under the
//! 64 MiB limit. Rows are charged to the work budget as they are read or written.

use super::super::fileio::{FileError, FileResult, MAX_FILE_BYTES};
use super::super::matrix::expect_matrix;
use super::super::readwrite::{access_for, file_name, with_charge};
use super::super::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// Most pixels one image may hold.
const MAX_CELLS: usize = 1_000_000;
/// File header plus the BITMAPINFOHEADER this writer produces.
const PIXEL_OFFSET: usize = 14 + 40 + 256 * 4;

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("READBMP", Arity::Exact(1), read_bmp);
    r.register_ctx("WRITEBMP", Arity::Exact(2), write_bmp);
}

/// `READBMP("file")`: the grey levels of an 8-bit grey BMP, one row per image row.
fn read_bmp(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let file = file_name(&args[0], "READBMP")?;
    let (access, policy) = access_for(ctx, "READBMP", false)?;
    let (rows, cols, levels) = with_charge(ctx, |charge| {
        let bytes = access.read(file, policy, MAX_FILE_BYTES)?;
        let image = parse_bmp(&bytes)?;
        let mut levels = Vec::with_capacity(image.width * image.height);
        for file_row in 0..image.height {
            charge(1 + image.width as u64 / 64)?;
            // A bottom-up image stores its first row last.
            let y = if image.top_down {
                file_row
            } else {
                image.height - 1 - file_row
            };
            let start = image.pixels + y * image.stride;
            for &index in &bytes[start..start + image.width] {
                levels.push(f64::from(image.palette[usize::from(index)]));
            }
        }
        Ok((image.height, image.width, levels))
    })?;
    let data = levels
        .iter()
        .map(|&x| Scalar::real(x, Dimension::dimensionless()))
        .collect();
    Ok(Value::Matrix(Matrix::new(rows, cols, data)?))
}

/// `WRITEBMP("file.bmp", M)`: writes the grey image `M` and returns it.
fn write_bmp(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let file = file_name(&args[0], "WRITEBMP")?;
    if !file.to_ascii_lowercase().ends_with(".bmp") {
        return Err(EvalError::Unsupported(
            "WRITEBMP writes .bmp files (.ppm and .tif are not built)".to_string(),
        ));
    }
    let m = match &args[1] {
        Value::Scalar(s) => Matrix::new(1, 1, vec![*s])?,
        other => expect_matrix(other, "WRITEBMP")?.clone(),
    };
    let (rows, cols) = (m.rows(), m.cols());
    if rows == 0 || cols == 0 {
        return Err(EvalError::Unsupported(
            "WRITEBMP needs an image".to_string(),
        ));
    }
    let cells = rows.saturating_mul(cols);
    let stride = cols.div_ceil(4) * 4;
    let body = stride.saturating_mul(rows);
    if cells > MAX_CELLS || body.saturating_add(PIXEL_OFFSET) > MAX_FILE_BYTES as usize {
        return Err(EvalError::TooLarge(cells));
    }
    let (access, policy) = access_for(ctx, "WRITEBMP", true)?;
    let mut out = Vec::with_capacity(PIXEL_OFFSET + body);
    // BITMAPFILEHEADER.
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((PIXEL_OFFSET + body) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());
    // BITMAPINFOHEADER: bottom-up (positive height), 8 bits, no compression.
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(cols as i32).to_le_bytes());
    out.extend_from_slice(&(rows as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(body as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    out.extend_from_slice(&256u32.to_le_bytes());
    out.extend_from_slice(&256u32.to_le_bytes());
    // The grey palette.
    for level in 0..=255u8 {
        out.extend_from_slice(&[level, level, level, 0]);
    }
    with_charge(ctx, |charge| {
        for file_row in 0..rows {
            charge(1 + cols as u64 / 64)?;
            let y = rows - 1 - file_row;
            let mut line = Vec::with_capacity(stride);
            for x in 0..cols {
                let s = m
                    .get(y, x)
                    .map_err(|_| FileError::Format("bad image".to_string()))?;
                if s.value.im != 0.0 {
                    return Err(FileError::Format(
                        "WRITEBMP writes real values only".to_string(),
                    ));
                }
                let level = s.value.re.round();
                if !(0.0..=255.0).contains(&level) {
                    return Err(FileError::Format(
                        "grey levels are 0 to 255 in a BMP file".to_string(),
                    ));
                }
                line.push(level as u8);
            }
            line.resize(stride, 0);
            out.extend_from_slice(&line);
        }
        access.write(file, policy, &out)
    })?;
    Ok(args[1].clone())
}

/// The parts of a grey BMP that the reader needs.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct BmpImage {
    pub width: usize,
    pub height: usize,
    pub top_down: bool,
    /// Offset of the first pixel byte.
    pub pixels: usize,
    /// Bytes per row, padded to 4.
    pub stride: usize,
    /// The grey level of each palette index.
    pub palette: Vec<u8>,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Checks the headers of an 8-bit grey BMP and returns its layout. Nothing is allocated for the
/// pixels here; the caller reads them row by row.
pub(super) fn parse_bmp(bytes: &[u8]) -> FileResult<BmpImage> {
    let bad = |m: &str| FileError::Format(format!("BMP: {m}"));
    if bytes.get(0..2) != Some(b"BM") {
        return Err(bad("not a BMP file"));
    }
    let pixel_offset = u32_at(bytes, 10).ok_or_else(|| bad("truncated header"))? as usize;
    let info_size = u32_at(bytes, 14).ok_or_else(|| bad("truncated header"))? as usize;
    if !(40..=1024).contains(&info_size) {
        return Err(bad("unsupported header (BITMAPINFOHEADER is needed)"));
    }
    let width = u32_at(bytes, 18).ok_or_else(|| bad("truncated header"))? as i32;
    let height = u32_at(bytes, 22).ok_or_else(|| bad("truncated header"))? as i32;
    let planes = u16_at(bytes, 26).ok_or_else(|| bad("truncated header"))?;
    let bpp = u16_at(bytes, 28).ok_or_else(|| bad("truncated header"))?;
    let compression = u32_at(bytes, 30).ok_or_else(|| bad("truncated header"))?;
    let used = u32_at(bytes, 46).ok_or_else(|| bad("truncated header"))? as usize;
    if planes != 1 || compression != 0 {
        return Err(bad("only uncompressed single-plane images are read"));
    }
    if bpp != 8 {
        return Err(bad(
            "only 8-bit grey images are read (colour needs a grey rule the help does not give)",
        ));
    }
    if width <= 0 || height == 0 || height.unsigned_abs() as usize > (1 << 24) {
        return Err(bad("bad image size"));
    }
    let (width, height) = (width as usize, height.unsigned_abs() as usize);
    let cells = width.saturating_mul(height);
    if cells > MAX_CELLS {
        return Err(FileError::TooLarge {
            size: cells as u64,
            limit: MAX_CELLS as u64,
        });
    }
    let entries = if used == 0 { 256 } else { used.min(256) };
    let palette_at = 14usize
        .checked_add(info_size)
        .ok_or_else(|| bad("bad header size"))?;
    let palette_end = palette_at
        .checked_add(entries * 4)
        .ok_or_else(|| bad("bad palette"))?;
    if palette_end > bytes.len() {
        return Err(bad("palette runs past the end of the file"));
    }
    let mut palette = Vec::with_capacity(256);
    for k in 0..entries {
        let at = palette_at + 4 * k;
        let (b, g, r) = (bytes[at], bytes[at + 1], bytes[at + 2]);
        if b != r || g != r {
            return Err(bad("colour palette: only grey images are read"));
        }
        palette.push(r);
    }
    // Indices beyond the palette read as black, as most readers do.
    palette.resize(256, 0);
    let stride = (width * 8).div_ceil(32) * 4;
    let pixels = pixel_offset.max(palette_end);
    let needed = stride
        .checked_mul(height)
        .and_then(|n| n.checked_add(pixels))
        .ok_or_else(|| bad("bad pixel size"))?;
    if needed > bytes.len() {
        return Err(bad("pixel data runs past the end of the file"));
    }
    Ok(BmpImage {
        width,
        height,
        top_down: height_is_negative(bytes),
        pixels,
        stride,
        palette,
    })
}

fn height_is_negative(bytes: &[u8]) -> bool {
    u32_at(bytes, 22).is_some_and(|h| (h as i32) < 0)
}

#[cfg(test)]
mod tests {
    use super::super::super::fileio::{FileAccess, FileOptions, FilePolicy, FileSwitches, MemFs};
    use super::*;
    use crate::eval::builtins::BuiltinCtx;
    use std::sync::Arc;

    fn ctx() -> BuiltinCtx<'static> {
        BuiltinCtx::new(Ok(0), Ok(1e-3), Ok(1e-3))
    }

    /// A context whose file access is `fs` (the same policy as the worksheet default).
    fn file_ctx(fs: Arc<MemFs>) -> BuiltinCtx<'static> {
        let budget: &'static crate::eval::work::Budget =
            Box::leak(Box::new(crate::eval::work::Budget::new()));
        BuiltinCtx::from_scope(
            Ok(0),
            Ok(1e-3),
            Ok(1e-3),
            FileOptions::with_access(fs, FilePolicy::default()).with_switches(FileSwitches {
                read: true,
                write: true,
            }),
            budget,
        )
    }

    fn grey_image(width: usize, height: usize, pixels: &[u8]) -> Vec<u8> {
        let stride = (width * 8).div_ceil(32) * 4;
        let mut out = Vec::new();
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&((PIXEL_OFFSET + stride * height) as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&(width as i32).to_le_bytes());
        out.extend_from_slice(&(height as i32).to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&[0u8; 24]);
        for level in 0..=255u8 {
            out.extend_from_slice(&[level, level, level, 0]);
        }
        for row in 0..height {
            let mut line = pixels[row * width..(row + 1) * width].to_vec();
            line.resize(stride, 0);
            out.extend_from_slice(&line);
        }
        out
    }

    fn matrix_of(rows: usize, cols: usize, values: &[f64]) -> Value {
        let data = values
            .iter()
            .map(|&x| Scalar::real(x, Dimension::dimensionless()))
            .collect();
        Value::Matrix(Matrix::new(rows, cols, data).unwrap())
    }

    #[test]
    fn writes_and_reads_back_a_grey_image() {
        let fs = Arc::new(MemFs::new());
        // A 3 by 2 image: top row 10 20 30, bottom row 40 50 255.
        let m = matrix_of(2, 3, &[10.0, 20.0, 30.0, 40.0, 50.0, 255.0]);
        let ctx = file_ctx(fs.clone());
        write_bmp(&ctx, &[Value::Str("a.bmp".to_string()), m.clone()]).unwrap();
        assert_eq!(
            read_bmp(&ctx, &[Value::Str("a.bmp".to_string())]).unwrap(),
            m
        );
        let bytes = fs
            .read("a.bmp", FilePolicy::default(), MAX_FILE_BYTES)
            .unwrap();
        let image = parse_bmp(&bytes).unwrap();
        assert_eq!((image.width, image.height, image.top_down), (3, 2, false));
    }

    #[test]
    fn read_bmp_reads_a_top_down_file_in_order() {
        let fs = Arc::new(MemFs::new());
        // Height -2 marks a top-down image: the first stored row is the top row.
        let mut bytes = grey_image(2, 2, &[1, 2, 3, 4]);
        bytes[22..26].copy_from_slice(&(-2i32).to_le_bytes());
        fs.put("t.bmp", FilePolicy::default(), &bytes).unwrap();
        let got = read_bmp(&file_ctx(fs), &[Value::Str("t.bmp".to_string())]).unwrap();
        assert_eq!(got, matrix_of(2, 2, &[1.0, 2.0, 3.0, 4.0]));
    }

    #[test]
    fn hostile_bmp_headers_are_refused() {
        let good = grey_image(2, 2, &[1, 2, 3, 4]);
        assert!(parse_bmp(&good).is_ok());
        assert!(parse_bmp(&good[..30]).is_err());
        let mut colour = good.clone();
        colour[28] = 24;
        assert!(parse_bmp(&colour).is_err());
        let mut compressed = good.clone();
        compressed[30] = 1;
        assert!(parse_bmp(&compressed).is_err());
        let mut huge = good.clone();
        huge[18..22].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(parse_bmp(&huge).is_err());
        let mut zero = good.clone();
        zero[18..22].copy_from_slice(&0i32.to_le_bytes());
        assert!(parse_bmp(&zero).is_err());
        let mut far_palette = good.clone();
        far_palette[14..18].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_bmp(&far_palette).is_err());
        let mut far_pixels = good.clone();
        far_pixels[10..14].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_bmp(&far_pixels).is_err());
    }

    #[test]
    fn writer_refuses_other_extensions_and_out_of_range_levels() {
        let m = matrix_of(1, 1, &[300.0]);
        assert!(write_bmp(&ctx(), &[Value::Str("a.ppm".to_string()), m.clone()]).is_err());
        assert!(write_bmp(&ctx(), &[Value::Str("a.bmp".to_string()), m]).is_err());
    }
}
