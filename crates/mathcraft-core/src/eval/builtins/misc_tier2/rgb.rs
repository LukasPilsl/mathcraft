//! Colour BMP files as packed RGB matrices: `READRGB` and `WRITERGB` (help: "Image Files", "Example:
//! Using READRGB and READBMP", "Example: Separating and Combining RGB Components").
//!
//! A packed RGB matrix has one row per image row and three blocks of columns, the red values of the
//! `width` columns, then the green, then the blue (the layout of `augment(red, green, blue)` in the
//! help, and of `submatrix(M1, 1, 122, 1, width)` for the red block).
//!
//! Built:
//! - `READRGB("file.bmp")`: an uncompressed 24-bit BMP file. Values are the 8-bit levels 0 to 255.
//! - `WRITERGB("file.bmp", M)`: a 24-bit uncompressed BMP file from a packed matrix whose column
//!   count is a multiple of 3. Values are rounded to the nearest integer and must lie in 0 to 255.
//!   The help says that values above 255 are written 16-bit, but the 16-bit BMP layout is not given,
//!   so they are refused. Only the `.bmp` extension is written.
//!
//! Refused: other image formats (GIF, JPG, TGA, PBM, PGM, PPM, TIF), 8-bit palette BMP (the help
//! gives no rule for turning a palette into colour), compressed files and any other bit depth.
//! Headers are checked before the pixels are read, and the image is read under the same size limits
//! and work charges as `READBMP`.

use super::super::fileio::{FileError, FileResult, MAX_FILE_BYTES};
use super::super::matrix::expect_matrix;
use super::super::readwrite::{access_for, file_name, with_charge};
use super::super::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// Most pixels one colour image may hold (its packed matrix has three times as many cells, which
/// stays within the matrix limit of one million cells used by `READBMP`).
const MAX_PIXELS: usize = 333_333;
/// File header (14 bytes) plus the 40-byte BITMAPINFOHEADER this writer produces; no palette.
const PIXEL_OFFSET: usize = 14 + 40;

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("READRGB", Arity::Exact(1), read_rgb);
    r.register_ctx("WRITERGB", Arity::Exact(2), write_rgb);
}

/// `READRGB("file")`: the packed RGB matrix of a 24-bit BMP file.
fn read_rgb(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let file = file_name(&args[0], "READRGB")?;
    let (access, policy) = access_for(ctx, "READRGB", false)?;
    let (rows, cols, levels) = with_charge(ctx, |charge| {
        let bytes = access.read(file, policy, MAX_FILE_BYTES)?;
        let image = parse_bmp24(&bytes)?;
        let (w, h) = (image.width, image.height);
        let mut levels = Vec::with_capacity(h * 3 * w);
        for y in 0..h {
            charge(1 + 3 * w as u64 / 64)?;
            // A bottom-up image stores its first row last.
            let file_row = if image.top_down { y } else { h - 1 - y };
            let start = image.pixels + file_row * image.stride;
            // Blocks of red, green and blue values: the byte order in a pixel is blue, green, red.
            for channel_offset in [2usize, 1, 0] {
                for x in 0..w {
                    levels.push(f64::from(bytes[start + 3 * x + channel_offset]));
                }
            }
        }
        Ok((h, 3 * w, levels))
    })?;
    let data = levels
        .iter()
        .map(|&x| Scalar::real(x, Dimension::dimensionless()))
        .collect();
    Ok(Value::Matrix(Matrix::new(rows, cols, data)?))
}

/// `WRITERGB("file.bmp", M)`: writes the packed RGB matrix `M` as a 24-bit BMP and returns it.
fn write_rgb(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let file = file_name(&args[0], "WRITERGB")?;
    if !file.to_ascii_lowercase().ends_with(".bmp") {
        return Err(EvalError::Unsupported(
            "WRITERGB writes .bmp files (.ppm and .tif are not built)".to_string(),
        ));
    }
    let m = expect_matrix(&args[1], "WRITERGB")?.clone();
    let (rows, cols) = (m.rows(), m.cols());
    if rows == 0 || cols == 0 || cols % 3 != 0 {
        return Err(EvalError::Unsupported(
            "WRITERGB needs a packed RGB matrix: its columns are the red, green and blue blocks"
                .to_string(),
        ));
    }
    let width = cols / 3;
    let pixels = rows.saturating_mul(width);
    if pixels > MAX_PIXELS {
        return Err(EvalError::TooLarge(pixels));
    }
    let stride = (width * 3).div_ceil(4) * 4;
    let body = stride.saturating_mul(rows);
    if body.saturating_add(PIXEL_OFFSET) > MAX_FILE_BYTES as usize {
        return Err(EvalError::TooLarge(pixels));
    }
    let (access, policy) = access_for(ctx, "WRITERGB", true)?;
    let mut out = Vec::with_capacity(PIXEL_OFFSET + body);
    // BITMAPFILEHEADER.
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((PIXEL_OFFSET + body) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());
    // BITMAPINFOHEADER: bottom-up (positive height), 24 bits, no compression, no palette.
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(rows as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(body as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    with_charge(ctx, |charge| {
        for file_row in 0..rows {
            charge(1 + 3 * width as u64 / 64)?;
            let y = rows - 1 - file_row;
            let mut line = Vec::with_capacity(stride);
            for x in 0..width {
                // Blue, green, red in that order within a pixel.
                for block in [2usize, 1, 0] {
                    let level = rgb_level(&m, y, block * width + x)?;
                    line.push(level);
                }
            }
            line.resize(stride, 0);
            out.extend_from_slice(&line);
        }
        access.write(file, policy, &out)
    })?;
    Ok(args[1].clone())
}

/// One colour level of the packed matrix as a byte: real, rounded, and in 0 to 255.
fn rgb_level(m: &Matrix, row: usize, col: usize) -> FileResult<u8> {
    let s = m
        .get(row, col)
        .map_err(|_| FileError::Format("bad image".to_string()))?;
    if s.value.im != 0.0 {
        return Err(FileError::Format(
            "WRITERGB writes real values only".to_string(),
        ));
    }
    let level = s.value.re.round();
    if !(0.0..=255.0).contains(&level) {
        return Err(FileError::Format(
            "WRITERGB: colour levels are 0 to 255 in a 24-bit BMP file (16-bit output is not built)"
                .to_string(),
        ));
    }
    Ok(level as u8)
}

/// The parts of a 24-bit BMP that the colour reader needs.
#[derive(Debug, PartialEq, Eq)]
struct Bmp24 {
    width: usize,
    height: usize,
    top_down: bool,
    /// Offset of the first pixel byte.
    pixels: usize,
    /// Bytes per row, padded to 4.
    stride: usize,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Checks the headers of an uncompressed 24-bit BMP and returns its layout.
fn parse_bmp24(bytes: &[u8]) -> FileResult<Bmp24> {
    let bad = |m: &str| FileError::Format(format!("BMP: {m}"));
    let truncated = || bad("truncated header");
    if bytes.get(0..2) != Some(b"BM") {
        return Err(bad("not a BMP file"));
    }
    let pixel_offset = u32_at(bytes, 10).ok_or_else(truncated)? as usize;
    let info_size = u32_at(bytes, 14).ok_or_else(truncated)? as usize;
    if !(40..=1024).contains(&info_size) {
        return Err(bad("unsupported header (BITMAPINFOHEADER is needed)"));
    }
    let width = u32_at(bytes, 18).ok_or_else(truncated)? as i32;
    let height = u32_at(bytes, 22).ok_or_else(truncated)? as i32;
    let planes = u16_at(bytes, 26).ok_or_else(truncated)?;
    let bpp = u16_at(bytes, 28).ok_or_else(truncated)?;
    let compression = u32_at(bytes, 30).ok_or_else(truncated)?;
    if planes != 1 || compression != 0 {
        return Err(bad("only uncompressed single-plane images are read"));
    }
    if bpp != 24 {
        return Err(bad(
            "READRGB reads 24-bit colour images; other bit depths and palettes are not built",
        ));
    }
    if width <= 0 || height == 0 || height.unsigned_abs() as usize > (1 << 24) {
        return Err(bad("bad image size"));
    }
    // A negative height marks a top-down image.
    let top_down = height < 0;
    let (width, height) = (width as usize, height.unsigned_abs() as usize);
    let pixels_count = width.saturating_mul(height);
    if pixels_count > MAX_PIXELS {
        return Err(FileError::TooLarge {
            size: pixels_count as u64,
            limit: MAX_PIXELS as u64,
        });
    }
    let stride = (width * 3).div_ceil(4) * 4;
    let needed = stride
        .checked_mul(height)
        .and_then(|n| n.checked_add(pixel_offset))
        .ok_or_else(|| bad("bad pixel size"))?;
    if pixel_offset < 14 + info_size || needed > bytes.len() {
        return Err(bad("pixel data is out of the file"));
    }
    Ok(Bmp24 {
        width,
        height,
        top_down,
        pixels: pixel_offset,
        stride,
    })
}

#[cfg(test)]
mod tests {
    use super::super::super::fileio::{FileOptions, FilePolicy, FileSwitches, MemFs};
    use super::*;
    use crate::eval::builtins::BuiltinCtx;
    use std::sync::Arc;

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

    fn matrix_of(rows: usize, cols: usize, values: &[f64]) -> Value {
        let data = values
            .iter()
            .map(|&x| Scalar::real(x, Dimension::dimensionless()))
            .collect();
        Value::Matrix(Matrix::new(rows, cols, data).unwrap())
    }

    /// A 24-bit BMP with the pixel bytes given as (blue, green, red) per pixel, bottom row first.
    fn bmp24(width: usize, height: usize, pixels_bgr: &[[u8; 3]]) -> Vec<u8> {
        let stride = (width * 3).div_ceil(4) * 4;
        let mut out = Vec::new();
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&((PIXEL_OFFSET + stride * height) as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&(width as i32).to_le_bytes());
        out.extend_from_slice(&(height as i32).to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&24u16.to_le_bytes());
        out.extend_from_slice(&[0u8; 24]);
        for row in 0..height {
            let mut line = Vec::new();
            for px in &pixels_bgr[row * width..(row + 1) * width] {
                line.extend_from_slice(px);
            }
            line.resize(stride, 0);
            out.extend_from_slice(&line);
        }
        out
    }

    #[test]
    fn reads_the_packed_layout_of_a_colour_bmp() {
        let fs = Arc::new(MemFs::new());
        // 2 wide, 2 high, bottom row first in the file: file row 0 is the image's bottom row.
        // Image top row: (R 1, G 2, B 3), (R 4, G 5, B 6); bottom row: (7, 8, 9), (10, 11, 12).
        let file = bmp24(2, 2, &[[9, 8, 7], [12, 11, 10], [3, 2, 1], [6, 5, 4]]);
        fs.put("c.bmp", FilePolicy::default(), &file).unwrap();
        let got = read_rgb(&file_ctx(fs), &[Value::Str("c.bmp".to_string())]).unwrap();
        assert_eq!(
            got,
            matrix_of(
                2,
                6,
                &[1.0, 4.0, 2.0, 5.0, 3.0, 6.0, 7.0, 10.0, 8.0, 11.0, 9.0, 12.0]
            )
        );
    }

    #[test]
    fn write_then_read_round_trips_a_packed_matrix() {
        let fs = Arc::new(MemFs::new());
        let m = matrix_of(
            2,
            6,
            &[
                10.0, 20.0, 30.0, 40.0, 50.0, 255.0, 0.0, 1.4, 2.0, 3.0, 4.0, 5.0,
            ],
        );
        let ctx = file_ctx(fs.clone());
        write_rgb(&ctx, &[Value::Str("r.bmp".to_string()), m.clone()]).unwrap();
        assert_eq!(
            read_rgb(&ctx, &[Value::Str("r.bmp".to_string())]).unwrap(),
            {
                matrix_of(
                    2,
                    6,
                    &[
                        10.0, 20.0, 30.0, 40.0, 50.0, 255.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0,
                    ],
                )
            }
        );
    }

    #[test]
    fn refuses_bad_input() {
        let fs = Arc::new(MemFs::new());
        let good = bmp24(1, 1, &[[1, 2, 3]]);
        assert!(parse_bmp24(&good).is_ok());
        let mut eight_bit = good.clone();
        eight_bit[28] = 8;
        assert!(parse_bmp24(&eight_bit).is_err());
        let mut compressed = good.clone();
        compressed[30] = 1;
        assert!(parse_bmp24(&compressed).is_err());
        assert!(parse_bmp24(&good[..30]).is_err());
        let ctx = file_ctx(fs.clone());
        // Not a multiple of three columns, out of range, and a non-BMP name.
        let ragged = matrix_of(1, 4, &[1.0, 2.0, 3.0, 4.0]);
        assert!(write_rgb(&ctx, &[Value::Str("a.bmp".to_string()), ragged]).is_err());
        let big = matrix_of(1, 3, &[256.0, 0.0, 0.0]);
        assert!(write_rgb(&ctx, &[Value::Str("a.bmp".to_string()), big]).is_err());
        let ok = matrix_of(1, 3, &[1.0, 0.0, 0.0]);
        assert!(write_rgb(&ctx, &[Value::Str("a.ppm".to_string()), ok]).is_err());
    }
}
