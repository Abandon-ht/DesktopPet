//! Play one local PCM WAV through the same output path as interaction clips.
use anyhow::{Context, Result};
use pet_voice_session::{TurnToken, play_wav_with_volume};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64},
};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let path = args
        .next()
        .context("usage: voice-clip-smoke PATH.wav [VOLUME]")?;
    let volume = args
        .next()
        .map(|value| value.to_string_lossy().parse::<u8>())
        .transpose()?
        .unwrap_or(100);
    let token = TurnToken::new(Arc::new(AtomicU64::new(1)), 1);
    play_wav_with_volume(&PathBuf::from(path), &token, volume)
}
