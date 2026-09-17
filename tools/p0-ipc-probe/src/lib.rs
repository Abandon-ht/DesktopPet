pub mod supervisor;

use anyhow::{Result, ensure};
use std::io::BufRead;

pub const MAX_FRAME: usize = 256 * 1024;

// Bound memory before parsing. A nonempty unterminated frame is a protocol error.
pub fn frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>> {
    let mut out = Vec::new();
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            ensure!(out.is_empty(), "truncated_frame");
            return Ok(None);
        }
        let end = chunk.iter().position(|&b| b == b'\n');
        let n = end.map_or(chunk.len(), |i| i + 1);
        ensure!(out.len() + n <= MAX_FRAME, "frame_too_large");
        out.extend_from_slice(&chunk[..n]);
        reader.consume(n);
        if end.is_some() {
            return Ok(Some(out));
        }
    }
}
