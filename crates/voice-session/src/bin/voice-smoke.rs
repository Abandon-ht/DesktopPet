//! Exercise LM Studio -> ZipVoice -> system speaker without microphone access.
use anyhow::{Context, Result};
use pet_voice_session::{AsrPort, InputPort, Pcm, TurnToken, VoiceSettings, local_pipeline};
use std::sync::{Arc, atomic::AtomicU64};

struct FixedInput;
impl InputPort for FixedInput {
    fn capture(&mut self, _: &TurnToken) -> Result<Pcm> {
        Ok(Pcm {
            samples: vec![0.0; 16000],
            sample_rate: 16000,
        })
    }
}
struct FixedAsr(String);
impl AsrPort for FixedAsr {
    fn transcribe(&mut self, _: &Pcm, _: &TurnToken) -> Result<String> {
        Ok(self.0.clone())
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let first = args
        .next()
        .context("usage: voice-smoke --settings SETTINGS_JSON PROMPT, or MODEL_DIR REFERENCE_WAV REFERENCE_TEXT PROMPT")?;
    let (settings, prompt) = if first == "--settings" {
        let file = args.next().context("settings JSON path required")?;
        let settings: VoiceSettings = serde_json::from_str(&std::fs::read_to_string(file)?)?;
        let prompt = args.next().context("prompt required")?;
        (settings, prompt)
    } else {
        let reference_audio = args
            .next()
            .context("usage: voice-smoke MODEL_DIR REFERENCE_WAV REFERENCE_TEXT PROMPT")?;
        let reference_text = args
            .next()
            .context("usage: voice-smoke MODEL_DIR REFERENCE_WAV REFERENCE_TEXT PROMPT")?;
        let prompt = args
            .next()
            .context("usage: voice-smoke MODEL_DIR REFERENCE_WAV REFERENCE_TEXT PROMPT")?;
        let settings = VoiceSettings {
            enabled: true,
            model_dir: first.into(),
            reference_audio: reference_audio.into(),
            reference_text,
            ..Default::default()
        };
        (settings, prompt)
    };
    anyhow::ensure!(args.next().is_none(), "unexpected voice-smoke argument");
    let mut pipeline = local_pipeline(&settings)?;
    pipeline.input = Box::new(FixedInput);
    pipeline.asr = Box::new(FixedAsr(prompt));
    let token = TurnToken::new(Arc::new(AtomicU64::new(1)), 1);
    pipeline.run(&token, |status| {
        println!(
            "{}: {} {}",
            status.phase, status.transcript, status.response
        );
    })
}
