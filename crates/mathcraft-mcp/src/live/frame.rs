//! Framing between the binary and the app: one JSON value per line. A line over the limit is an
//! error, so a hostile peer cannot make the other side buffer without end.

use std::io::{self, BufRead, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// Largest request line the app accepts (the client's tool arguments), in bytes.
pub const MAX_REQUEST_BYTES: u64 = 2 * 1024 * 1024;
/// Largest reply line the client accepts (a PNG as base64 is the biggest), in bytes.
pub const MAX_REPLY_BYTES: u64 = 48 * 1024 * 1024;

/// What reading one line gave.
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    /// The peer closed the stream (also in the middle of a line).
    Eof,
    /// The line was longer than the limit.
    TooLong,
    /// A line without its line end. May be empty or not UTF-8: the caller checks.
    Data(Vec<u8>),
}

/// Reads one line of at most `limit` bytes (the line end not counted).
///
/// # Errors
/// An I/O error of the stream, including a read timeout.
pub fn read_line<R: BufRead>(input: &mut R, limit: u64) -> io::Result<Line> {
    let mut buf = Vec::new();
    let n = input
        .by_ref()
        .take(limit.saturating_add(2))
        .read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(Line::Eof);
    }
    let ended = buf.last() == Some(&b'\n');
    if ended {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        if buf.len() as u64 > limit {
            return Ok(Line::TooLong);
        }
        return Ok(Line::Data(buf));
    }
    if buf.len() as u64 > limit {
        return Ok(Line::TooLong);
    }
    Ok(Line::Eof)
}

/// Like [`read_line`] on a socket, but with a TOTAL time limit, so a peer that sends one byte at
/// a time cannot hold the connection. With `from_start` the `budget` runs from the call; otherwise
/// the first byte may take up to `first_wait` and `budget` runs from that byte.
///
/// # Errors
/// An I/O error of the stream; `TimedOut` when the budget ran out.
pub fn read_line_deadline<R: BufRead>(
    input: &mut R,
    socket: &TcpStream,
    limit: u64,
    first_wait: Duration,
    budget: Duration,
    from_start: bool,
) -> io::Result<Line> {
    let mut deadline = from_start.then(|| Instant::now() + budget);
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let wait = match deadline {
            Some(d) => d
                .checked_duration_since(Instant::now())
                .filter(|w| !w.is_zero())
                .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))?,
            None => first_wait,
        };
        socket.set_read_timeout(Some(wait))?;
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            return Ok(Line::Eof);
        }
        deadline.get_or_insert_with(|| Instant::now() + budget);
        let (take, found) = match chunk.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        buf.extend_from_slice(&chunk[..take]);
        input.consume(take);
        if found {
            buf.pop();
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            return Ok(if buf.len() as u64 > limit {
                Line::TooLong
            } else {
                Line::Data(buf)
            });
        }
        if buf.len() as u64 > limit.saturating_add(1) {
            return Ok(Line::TooLong);
        }
    }
}

/// Writes `value` as one line and flushes.
///
/// # Errors
/// An I/O error of the stream.
pub fn write_line<W: Write>(out: &mut W, value: &serde_json::Value) -> io::Result<()> {
    let mut text = value.to_string();
    text.push('\n');
    out.write_all(text.as_bytes())?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lines_and_limits() {
        let mut r = io::Cursor::new(b"abc\r\nxyz\n".to_vec());
        assert_eq!(read_line(&mut r, 10).unwrap(), Line::Data(b"abc".to_vec()));
        assert_eq!(read_line(&mut r, 10).unwrap(), Line::Data(b"xyz".to_vec()));
        assert_eq!(read_line(&mut r, 10).unwrap(), Line::Eof);
        let mut r = io::Cursor::new(vec![b'a'; 100]);
        assert_eq!(read_line(&mut r, 10).unwrap(), Line::TooLong);
        let mut r = io::Cursor::new(b"abcdefghij\n".to_vec());
        assert_eq!(
            read_line(&mut r, 10).unwrap(),
            Line::Data(b"abcdefghij".to_vec())
        );
        let mut r = io::Cursor::new(b"abcdefghijk\n".to_vec());
        assert_eq!(read_line(&mut r, 10).unwrap(), Line::TooLong);
    }
}
