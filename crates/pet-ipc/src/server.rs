//! Bounded, ordered v1 protocol. The callback runs through the host event loop;
//! accepted means applied on that loop. Replay deduplication covers 128 recent commands.
use crate::{MAX_FRAME, frame};
use anyhow::{Context, Result, ensure};
use pet_protocol::DesktopCommand;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{BufRead, Write},
};

#[derive(Clone, Debug)]
pub enum Command {
    Hello,
    Ping,
    Desktop(DesktopCommand),
    Shutdown,
    Poll,
    Avatar(pet_protocol::AvatarCommand),
}

pub fn serve(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    mut apply: impl FnMut(Command) -> Result<()>,
) -> Result<()> {
    serve_with(reader, writer, |command| {
        apply(command)?;
        Ok(json!({}))
    })
}

pub fn serve_with(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    mut apply: impl FnMut(Command) -> Result<Value>,
) -> Result<()> {
    let mut session: Option<String> = None;
    let mut expected = 1_u64;
    // Retry window is bounded. Callers must not retry feedback beyond this
    // window; non-idempotent P2 actions require durable request deduplication.
    let mut recent: VecDeque<(String, Value, Value)> = VecDeque::new();
    while let Some(bytes) = frame(reader)? {
        let request: Value = serde_json::from_slice(&bytes).context("invalid_json")?;
        ensure!(
            request["protocol_version"].as_u64() == Some(1),
            "unsupported_protocol_version"
        );
        let id = request["session_id"]
            .as_str()
            .context("missing_session_id")?;
        let request_id = request["request_id"]
            .as_str()
            .context("missing_request_id")?;
        ensure!(!id.is_empty() && id.len() <= 128, "invalid_session_id");
        ensure!(
            !request_id.is_empty() && request_id.len() <= 128,
            "invalid_request_id"
        );
        ensure!(
            request["sequence"].as_u64() == Some(expected),
            "invalid_sequence"
        );
        ensure!(request["payload"].is_object(), "invalid_payload");
        let kind = request["type"].as_str().context("missing_type")?;
        let (reply_kind, payload) = if let Some(active) = &session {
            ensure!(active == id, "session_mismatch");
            match kind {
                "ping" => {
                    apply(Command::Ping)?;
                    ("pong", json!({}))
                }
                "shutdown" => {
                    apply(Command::Shutdown)?;
                    ("stopped", json!({}))
                }
                "poll" => ("events", apply(Command::Poll)?),
                "desktop" | "avatar" => {
                    let body = &json!({"kind":kind,"command":request["payload"]});
                    let result = if let Some((_, original, result)) =
                        recent.iter().find(|(r, _, _)| r == request_id)
                    {
                        ensure!(original == body, "request_id_reused_with_different_payload");
                        result.clone()
                    } else {
                        let decoded = if kind == "desktop" {
                            serde_json::from_value::<DesktopCommand>(request["payload"].clone())
                                .map(Command::Desktop)
                        } else {
                            serde_json::from_value::<pet_protocol::AvatarCommand>(
                                request["payload"].clone(),
                            )
                            .map(Command::Avatar)
                        };
                        let result = match decoded {
                            Ok(command) => match apply(command) {
                                Ok(_) => json!({"accepted":true}),
                                Err(error) => {
                                    json!({"accepted":false,"error":format!("{error:#}")})
                                }
                            },
                            Err(_) => json!({"accepted":false,"error":"unsupported_command"}),
                        };
                        if recent.len() == 128 {
                            recent.pop_front();
                        }
                        recent.push_back((request_id.to_owned(), body.clone(), result.clone()));
                        result
                    };
                    ("result", result)
                }
                _ => anyhow::bail!("unsupported_command"),
            }
        } else {
            ensure!(kind == "hello", "handshake_required");
            let metadata = apply(Command::Hello)?;
            session = Some(id.to_owned());
            (
                "ready",
                json!({"capabilities":["ping","shutdown","desktop","avatar","poll"],"max_frame_bytes":MAX_FRAME,"avatar":metadata["avatar"]}),
            )
        };
        serde_json::to_writer(
            &mut *writer,
            &json!({"protocol_version":1,"session_id":id,
            "sequence":expected,"request_id":request_id,"type":reply_kind,"payload":payload}),
        )?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        if kind == "shutdown" {
            return Ok(());
        }
        expected = expected.checked_add(1).context("sequence_exhausted")?;
    }
    Ok(()) // EOF tells the caller to stop the host.
}
