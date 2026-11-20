//! The stdio transport: one JSON-RPC message per line on stdin, one reply per line on stdout.
//! Nothing but protocol goes to stdout; diagnostics go to stderr.

use std::io::{self, BufRead, Read, Write};

use crate::server::Server;

/// Longest accepted message, in bytes. A longer line ends the session.
pub const MAX_MESSAGE_BYTES: u64 = 16 * 1024 * 1024;

/// Serves until the input ends.
///
/// # Errors
/// An I/O error on the streams, or a message over [`MAX_MESSAGE_BYTES`].
pub fn serve<R: BufRead, W: Write>(server: &Server, input: R, output: W) -> io::Result<()> {
    serve_with(&|m| server.handle(m), input, output)
}

/// [`serve`] for any message handler, such as the live [`crate::live::Router`].
///
/// # Errors
/// An I/O error on the streams, or a message over [`MAX_MESSAGE_BYTES`].
pub fn serve_with<R: BufRead, W: Write>(
    handle: &dyn Fn(serde_json::Value) -> Option<serde_json::Value>,
    mut input: R,
    mut output: W,
) -> io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = input
            .by_ref()
            .take(MAX_MESSAGE_BYTES + 1)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            return Ok(());
        }
        if line.len() as u64 > MAX_MESSAGE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "message too large",
            ));
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let reply = match serde_json::from_str(text) {
            Ok(message) => handle(message),
            Err(_) => Some(Server::parse_error()),
        };
        if let Some(reply) = reply {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
}
