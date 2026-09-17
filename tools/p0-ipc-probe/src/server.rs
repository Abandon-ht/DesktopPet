use crate::{MAX_FRAME, frame};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

pub fn serve(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    mut on_command: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    let mut session: Option<String> = None;
    let mut expected = 1_u64;
    while let Some(bytes) = frame(reader)? {
        let request: Value = serde_json::from_slice(&bytes).context("invalid_json")?;
        ensure!(
            request["protocol_version"].as_u64() == Some(1),
            "unsupported_protocol_version"
        );
        let id = request["session_id"]
            .as_str()
            .context("missing_session_id")?;
        ensure!(!id.is_empty() && id.len() <= 128, "invalid_session_id");
        let request_id = request["request_id"]
            .as_str()
            .context("missing_request_id")?;
        ensure!(
            !request_id.is_empty() && request_id.len() <= 128,
            "invalid_request_id"
        );
        ensure!(
            request["sequence"].as_u64() == Some(expected),
            "invalid_sequence"
        );
        ensure!(request["payload"].is_object(), "invalid_payload");
        let command = request["type"].as_str().context("missing_type")?;
        let (kind, payload) = if let Some(active) = &session {
            ensure!(active == id, "session_mismatch");
            match command {
                "ping" => ("pong", json!({})),
                "shutdown" => ("stopped", json!({})),
                _ => bail!("unsupported_command"),
            }
        } else {
            ensure!(command == "hello", "handshake_required");
            session = Some(id.to_owned());
            (
                "ready",
                json!({"capabilities":["ping", "shutdown"], "max_frame_bytes":MAX_FRAME}),
            )
        };
        on_command(command)?;
        let reply = json!({"protocol_version":1,"session_id":id,"sequence":expected,"request_id":request_id,"type":kind,"payload":payload});
        serde_json::to_writer(&mut *writer, &reply)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        if command == "shutdown" {
            return Ok(());
        }
        expected = expected.checked_add(1).context("sequence_exhausted")?;
    }
    // Parent closed its pipe (including parent death): no orphan fixture.
    Ok(())
}
