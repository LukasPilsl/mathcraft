//! PCM WAV sound files: `READWAV`, `WRITEWAV` and `GETWAVINFO` (help: "WAV Sound Files").
//!
//! Built:
//! - `GETWAVINFO("file")`: a column vector of the channel count, the sample rate, the bits per
//!   sample and the bytes per second, from the `fmt ` chunk.
//! - `READWAV("file")`: the samples as a matrix, one row per frame and one column per channel. The
//!   values are the stored integers: 0 to 255 for 8-bit data (unsigned), -32768 to 32767 for 16-bit
//!   data (signed), the same ranges `WRITEWAV` accepts.
//! - `WRITEWAV("file", s, b, M)`: writes `M` at sample rate `s` with bit resolution `b`. For b 1 to 8
//!   the samples are unsigned bytes (0 to 255); for b 9 to 16 they are signed 16-bit words. Values are
//!   rounded to the nearest integer and refused outside the range. Returns `M`.
//!
//! Accepted files: RIFF/WAVE with a PCM `fmt ` chunk (format tag 1, or 0xFFFE with the bits and
//! block alignment of 8- or 16-bit PCM; the sub-format GUID is not checked), 1 to 16 channels, 8 or
//! 16 bits. Other formats (24-bit, float, compressed) are refused. Chunk sizes are checked against the
//! file: a `data` chunk that claims more bytes than the file holds is read to the end of the file,
//! and a partial last frame is dropped. The file is read under the 64 MiB limit.
//!
//! The work budget is charged per frame, and a read or write above [`MAX_CELLS`] samples is refused
//! before any allocation.

use super::super::fileio::{FileError, FileResult, MAX_FILE_BYTES};
use super::super::matrix::expect_matrix;
use super::super::readwrite::{access_for, file_name, with_charge};
use super::super::{Arity, BuiltinCtx, Registry};
use crate::eval::env::integer_of;
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// Most samples (frames times channels) one read or write may hold.
const MAX_CELLS: usize = 1_000_000;
/// Bytes of the canonical 44-byte header this writer produces.
const HEADER_BYTES: usize = 44;

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("READWAV", Arity::Exact(1), read_wav);
    r.register_ctx("WRITEWAV", Arity::Exact(4), write_wav);
    r.register_ctx("GETWAVINFO", Arity::Exact(1), get_wav_info);
}

/// The format of a PCM WAV file and where its samples are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WavFormat {
    pub channels: u16,
    pub rate: u32,
    pub bits: u16,
    pub byte_rate: u32,
    pub block_align: u16,
    /// Offset of the first sample byte in the file.
    pub data_offset: usize,
    /// Bytes of sample data (clipped to the file).
    pub data_len: usize,
}

/// Reads the RIFF chunks of `bytes`: the `fmt ` chunk and the `data` chunk. Every size is checked
/// against the file before it is used, and the chunk walk advances by at least 8 bytes per chunk.
pub(super) fn parse_wav(bytes: &[u8]) -> FileResult<WavFormat> {
    let not_wav = || FileError::Format("not a PCM WAV file".to_string());
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(not_wav());
    }
    let mut fmt: Option<WavFormat> = None;
    let mut data: Option<(usize, usize)> = None;
    let mut pos = 12usize;
    while pos.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let id = &bytes[pos..pos + 4];
        let size = u32_at(bytes, pos + 4) as usize;
        let body = pos + 8;
        let body_end = body.saturating_add(size).min(bytes.len());
        if id == b"fmt " {
            if size < 16 || body + 16 > bytes.len() {
                return Err(FileError::Format("the fmt chunk is too short".to_string()));
            }
            let tag = u16_at(bytes, body);
            let channels = u16_at(bytes, body + 2);
            let rate = u32_at(bytes, body + 4);
            let byte_rate = u32_at(bytes, body + 8);
            let block_align = u16_at(bytes, body + 12);
            let bits = u16_at(bytes, body + 14);
            if tag != 1 && tag != 0xFFFE {
                return Err(FileError::Format(
                    "only PCM WAV files are read (no float or compressed data)".to_string(),
                ));
            }
            if !(1..=16).contains(&channels) {
                return Err(FileError::Format("1 to 16 channels are read".to_string()));
            }
            if bits != 8 && bits != 16 {
                return Err(FileError::Format(
                    "only 8 and 16 bit samples are read".to_string(),
                ));
            }
            if u32::from(block_align) != u32::from(channels) * u32::from(bits) / 8 || rate == 0 {
                return Err(FileError::Format(
                    "inconsistent WAV format fields".to_string(),
                ));
            }
            fmt = Some(WavFormat {
                channels,
                rate,
                bits,
                byte_rate,
                block_align,
                data_offset: 0,
                data_len: 0,
            });
        } else if id == b"data" && data.is_none() {
            data = Some((body, body_end - body));
        }
        // Chunks are word-aligned: an odd size has a pad byte.
        let next = body.saturating_add(size).saturating_add(size & 1);
        if next <= pos {
            break;
        }
        pos = next;
    }
    let (Some(mut format), Some((offset, len))) = (fmt, data) else {
        return Err(FileError::Format(
            "the file has no fmt or data chunk".to_string(),
        ));
    };
    format.data_offset = offset;
    format.data_len = len;
    Ok(format)
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// `GETWAVINFO("file")`: channels, sample rate, bits per sample and bytes per second, as a column.
fn get_wav_info(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let file = file_name(&args[0], "GETWAVINFO")?;
    let format = read_format(ctx, file)?;
    let values = [
        f64::from(format.channels),
        f64::from(format.rate),
        f64::from(format.bits),
        f64::from(format.byte_rate),
    ];
    let data = values
        .iter()
        .map(|&x| Scalar::real(x, Dimension::dimensionless()))
        .collect();
    Ok(Value::Matrix(Matrix::new(4, 1, data)?))
}

fn read_format(ctx: &BuiltinCtx<'_>, file: &str) -> Result<WavFormat, EvalError> {
    let (access, policy) = access_for(ctx, "GETWAVINFO", false)?;
    with_charge(ctx, |charge| {
        let bytes = access.read(file, policy, MAX_FILE_BYTES)?;
        charge(1 + bytes.len() as u64 / 1024)?;
        parse_wav(&bytes)
    })
}

/// `READWAV("file")`: the samples, one row per frame and one column per channel.
fn read_wav(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let file = file_name(&args[0], "READWAV")?;
    let (access, policy) = access_for(ctx, "READWAV", false)?;
    let (channels, samples) = with_charge(ctx, |charge| {
        let bytes = access.read(file, policy, MAX_FILE_BYTES)?;
        let format = parse_wav(&bytes)?;
        let channels = usize::from(format.channels);
        let frames = format.data_len / usize::from(format.block_align);
        let cells = frames
            .checked_mul(channels)
            .filter(|c| *c <= MAX_CELLS)
            .ok_or(FileError::TooLarge {
                size: (frames as u64).saturating_mul(channels as u64),
                limit: MAX_CELLS as u64,
            })?;
        let mut samples = Vec::with_capacity(cells);
        let width = usize::from(format.bits / 8);
        let mut at = format.data_offset;
        for _ in 0..frames {
            charge(channels as u64)?;
            for _ in 0..channels {
                let x = if width == 1 {
                    f64::from(bytes[at])
                } else {
                    f64::from(i16::from_le_bytes([bytes[at], bytes[at + 1]]))
                };
                samples.push(x);
                at += width;
            }
        }
        Ok((channels, samples))
    })?;
    let rows = samples.len().checked_div(channels).unwrap_or(0);
    let data = samples
        .iter()
        .map(|&x| Scalar::real(x, Dimension::dimensionless()))
        .collect();
    Ok(Value::Matrix(Matrix::new(rows, channels, data)?))
}

/// `WRITEWAV("file", s, b, M)`: writes `M` as PCM at sample rate `s` and bit resolution `b`.
fn write_wav(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let file = file_name(&args[0], "WRITEWAV")?;
    let rate = positive_integer(&args[1], "WRITEWAV rate")?;
    let bits = positive_integer(&args[2], "WRITEWAV bit resolution")?;
    if !(1..=16).contains(&bits) {
        return Err(EvalError::Unsupported(
            "WRITEWAV bit resolution is 1 to 16".to_string(),
        ));
    }
    let m = match &args[3] {
        Value::Scalar(s) => Matrix::new(1, 1, vec![*s])?,
        other => expect_matrix(other, "WRITEWAV")?.clone(),
    };
    let channels = m.cols();
    if !(1..=16).contains(&channels) || m.rows() == 0 {
        return Err(EvalError::Unsupported(
            "WRITEWAV needs 1 to 16 columns (channels) and at least one row".to_string(),
        ));
    }
    let (width, bits_field) = if bits <= 8 {
        (1usize, 8u16)
    } else {
        (2usize, 16u16)
    };
    let cells = m.rows().saturating_mul(channels);
    if cells > MAX_CELLS || cells * width + HEADER_BYTES > MAX_FILE_BYTES as usize {
        return Err(EvalError::TooLarge(cells));
    }
    let block_align = channels * width;
    let data_len = cells * width;
    let mut out = Vec::with_capacity(HEADER_BYTES + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(channels as u16).to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate.saturating_mul(block_align as u32)).to_le_bytes());
    out.extend_from_slice(&(block_align as u16).to_le_bytes());
    out.extend_from_slice(&bits_field.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    let (access, policy) = access_for(ctx, "WRITEWAV", true)?;
    with_charge(ctx, |charge| {
        for row in m.data().chunks(channels) {
            charge(channels as u64)?;
            for s in row {
                if s.value.im != 0.0 || !s.value.re.is_finite() {
                    return Err(FileError::Format(
                        "WRITEWAV writes real values only".to_string(),
                    ));
                }
                let x = s.value.re.round();
                if width == 1 {
                    if !(0.0..=255.0).contains(&x) {
                        return Err(FileError::Format("8-bit samples are 0 to 255".to_string()));
                    }
                    out.push(x as u8);
                } else {
                    if !(-32768.0..=32767.0).contains(&x) {
                        return Err(FileError::Format(
                            "16-bit samples are -32768 to 32767".to_string(),
                        ));
                    }
                    out.extend_from_slice(&(x as i16).to_le_bytes());
                }
            }
        }
        access.write(file, policy, &out)
    })?;
    Ok(args[3].clone())
}

/// An integer argument of at least 1 that fits in `u32` (a sample rate) or `u16` (a bit count).
fn positive_integer(v: &Value, what: &str) -> Result<u32, EvalError> {
    match v {
        Value::Scalar(s) => {
            ops::require_dimensionless(s.dim)?;
            match integer_of(s) {
                Some(k) if (1..=i64::from(u32::MAX)).contains(&k) => Ok(k as u32),
                _ => Err(EvalError::Unsupported(format!(
                    "{what} must be a positive integer"
                ))),
            }
        }
        _ => Err(EvalError::Unsupported(format!(
            "{what} must be a positive integer"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav_bytes(channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let block = channels * bits / 8;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((36 + data.len()) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
        out.extend_from_slice(&block.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn parses_the_header_of_a_stereo_16_bit_file() {
        let bytes = wav_bytes(2, 8000, 16, &[1, 0, 2, 0, 3, 0, 4, 0]);
        let f = parse_wav(&bytes).unwrap();
        assert_eq!(
            (f.channels, f.rate, f.bits, f.byte_rate),
            (2, 8000, 16, 32000)
        );
        assert_eq!((f.data_offset, f.data_len), (44, 8));
    }

    #[test]
    fn hostile_headers_are_refused_without_panicking() {
        let good = wav_bytes(1, 8000, 8, &[1, 2, 3]);
        assert!(parse_wav(&good[..11]).is_err());
        let mut not_riff = good.clone();
        not_riff[0] = b'X';
        assert!(parse_wav(&not_riff).is_err());
        let mut float = good.clone();
        float[20] = 3;
        assert!(parse_wav(&float).is_err());
        let mut zero_channels = good.clone();
        zero_channels[22] = 0;
        assert!(parse_wav(&zero_channels).is_err());
        let mut odd_bits = good.clone();
        odd_bits[34] = 24;
        assert!(parse_wav(&odd_bits).is_err());
        // A data chunk that claims 4 GiB is clipped to the file, not trusted.
        let mut huge = good.clone();
        huge[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        let f = parse_wav(&huge).unwrap();
        assert_eq!(f.data_len, 3);
        // A chunk size of u32::MAX in the walk ends the walk instead of overflowing.
        let mut junk = b"RIFF\0\0\0\0WAVEjunk".to_vec();
        junk.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_wav(&junk).is_err());
    }

    #[test]
    fn binary_garbage_is_refused() {
        let garbage: Vec<u8> = (0..4096u32)
            .map(|k| (k.wrapping_mul(2654435761) >> 13) as u8)
            .collect();
        let mut framed = b"RIFF\0\0\0\0WAVE".to_vec();
        framed.extend_from_slice(&garbage);
        assert!(parse_wav(&framed).is_err());
    }

    #[test]
    fn write_then_read_round_trips_through_memfs() {
        use super::super::super::fileio::{FileAccess, FilePolicy, MemFs};
        let fs = MemFs::new();
        let m = Matrix::new(
            3,
            2,
            vec![
                Scalar::real(0.0, Dimension::dimensionless()),
                Scalar::real(255.0, Dimension::dimensionless()),
                Scalar::real(128.0, Dimension::dimensionless()),
                Scalar::real(7.0, Dimension::dimensionless()),
                Scalar::real(1.4, Dimension::dimensionless()),
                Scalar::real(9.0, Dimension::dimensionless()),
            ],
        )
        .unwrap();
        let bytes = {
            let mut out = Vec::new();
            for s in m.data() {
                out.push(s.value.re.round() as u8);
            }
            wav_bytes(2, 8000, 8, &out)
        };
        fs.put("a.wav", FilePolicy::default(), &bytes).unwrap();
        let back = fs
            .read("a.wav", FilePolicy::default(), MAX_FILE_BYTES)
            .unwrap();
        let f = parse_wav(&back).unwrap();
        assert_eq!((f.channels, f.bits), (2, 8));
        let got: Vec<u8> = back[f.data_offset..f.data_offset + f.data_len].to_vec();
        assert_eq!(got, vec![0, 255, 128, 7, 1, 9]);
    }

    #[test]
    fn sixteen_bit_samples_are_signed() {
        let data: Vec<u8> = [-32768i16, 32767, -1, 0]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect();
        let bytes = wav_bytes(1, 44100, 16, &data);
        let f = parse_wav(&bytes).unwrap();
        let back: Vec<i16> = bytes[f.data_offset..f.data_offset + f.data_len]
            .chunks(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(back, vec![-32768, 32767, -1, 0]);
    }
}
