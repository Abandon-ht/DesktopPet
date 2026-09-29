//! Replaceable ports and single-turn orchestration for click-initiated voice.
//! Providers may be local sherpa-onnx, another native runtime, or cloud APIs.

mod local;
pub use local::{
    listen_for_keyword, local_pipeline, local_pipeline_with_memory, play_wav, play_wav_with_volume,
};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    fmt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsrBackend {
    #[default]
    SherpaOnnx,
    SherpaNcnn,
    SherpaMlx,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmBackend {
    #[default]
    LmStudio,
    Ollama,
    LlamaCpp,
    OpenAi,
    Anthropic,
    Gemini,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TtsBackend {
    #[default]
    SherpaOnnx,
    QwenTtsCpp,
    Kokoro,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VoiceSettings {
    pub enabled: bool,
    pub asr_backend: AsrBackend,
    pub llm_backend: LlmBackend,
    pub tts_backend: TtsBackend,
    pub model_dir: PathBuf,
    pub asr_model_path: PathBuf,
    pub asr_threads: u8,
    pub asr_gpu: bool,
    pub asr_ncnn_model_dir: PathBuf,
    pub asr_ncnn_executable: PathBuf,
    pub asr_ncnn_threads: u8,
    pub asr_ncnn_gpu: bool,
    pub asr_mlx_model_dir: PathBuf,
    pub asr_mlx_device: String,
    pub tts_model_dir: PathBuf,
    pub tts_gpu: bool,
    pub tts_speaker: String,
    pub tts_service_url: String,
    pub kokoro_model_dir: PathBuf,
    pub kokoro_threads: u8,
    pub kokoro_gpu: bool,
    pub vad_model_path: PathBuf,
    pub vad_threshold: f32,
    pub vad_silence_ms: u16,
    pub no_speech_timeout_secs: u16,
    pub rest_after_inactive_minutes: u16,
    pub aec_enabled: bool,
    pub kws_enabled: bool,
    pub kws_model_dir: PathBuf,
    pub kws_keyword: String,
    pub kws_keyword_en: String,
    pub kws_keywords_file: PathBuf,
    pub kws_threshold: f32,
    pub kws_threads: u8,
    pub wake_greeting_enabled: bool,
    pub timed_greetings_enabled: bool,
    pub first_greeting_enabled: bool,
    pub greeting_dir: PathBuf,
    /// Recurring local-calendar birthday in MM-DD form; empty disables the cue.
    pub birthday: String,
    pub reference_audio: PathBuf,
    pub reference_text: String,
    pub lm_studio_url: String,
    pub lm_studio_model: String,
    pub llm_api_key: String,
    pub system_prompt: String,
    pub tts_threads: u8,
    pub remember_context: bool,
    pub output_volume_percent: u8,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            asr_backend: AsrBackend::default(),
            llm_backend: LlmBackend::default(),
            tts_backend: TtsBackend::default(),
            model_dir: PathBuf::new(),
            asr_model_path: PathBuf::new(),
            asr_threads: 1,
            asr_gpu: false,
            asr_ncnn_model_dir: PathBuf::new(),
            asr_ncnn_executable: PathBuf::from("sherpa-ncnn-offline"),
            asr_ncnn_threads: 1,
            asr_ncnn_gpu: false,
            asr_mlx_model_dir: PathBuf::new(),
            asr_mlx_device: "auto".into(),
            tts_model_dir: PathBuf::new(),
            tts_gpu: false,
            tts_speaker: String::new(),
            tts_service_url: "http://127.0.0.1:8080".into(),
            kokoro_model_dir: PathBuf::new(),
            kokoro_threads: 4,
            kokoro_gpu: false,
            vad_model_path: PathBuf::new(),
            vad_threshold: 0.5,
            vad_silence_ms: 600,
            no_speech_timeout_secs: 8,
            rest_after_inactive_minutes: 0,
            aec_enabled: false,
            kws_enabled: false,
            kws_model_dir: PathBuf::new(),
            kws_keyword: "你好纳西妲".into(),
            kws_keyword_en: "Hello Nahida".into(),
            kws_keywords_file: PathBuf::new(),
            kws_threshold: 0.25,
            kws_threads: 1,
            wake_greeting_enabled: true,
            timed_greetings_enabled: true,
            first_greeting_enabled: true,
            greeting_dir: PathBuf::new(),
            birthday: String::new(),
            reference_audio: PathBuf::new(),
            reference_text: String::new(),
            lm_studio_url: "http://127.0.0.1:1234".into(),
            lm_studio_model: "qwen/qwen3.6-35b-a3b".into(),
            llm_api_key: String::new(),
            system_prompt: "你是桌面宠物。请用自然、简短的中文回答，只输出适合朗读的正文。".into(),
            tts_threads: 8,
            remember_context: true,
            output_volume_percent: 100,
        }
    }
}

impl VoiceSettings {
    pub fn validate(&self) -> Result<()> {
        if !self.birthday.is_empty() {
            let bytes = self.birthday.as_bytes();
            let valid_shape = bytes.len() == 5
                && bytes[2] == b'-'
                && bytes
                    .iter()
                    .enumerate()
                    .all(|(index, byte)| index == 2 || byte.is_ascii_digit());
            if !valid_shape {
                bail!("生日须填写 MM-DD，例如 08-16");
            }
            let month: u8 = self.birthday[0..2].parse()?;
            let day: u8 = self.birthday[3..5].parse()?;
            let max_day = match month {
                1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
                4 | 6 | 9 | 11 => 30,
                2 => 29,
                _ => 0,
            };
            if day == 0 || day > max_day {
                bail!("生日日期无效");
            }
        }
        if self.enabled && self.asr_backend == AsrBackend::SherpaMlx {
            bail!("sherpa-mlx ASR 尚未接入");
        }
        if self.enabled
            && !matches!(
                self.llm_backend,
                LlmBackend::LmStudio
                    | LlmBackend::Ollama
                    | LlmBackend::LlamaCpp
                    | LlmBackend::OpenAi
            )
        {
            bail!("所选 LLM 后端尚未接入，请选择 LM Studio 或兼容 Chat Completions 的服务");
        }
        if self.enabled && self.tts_backend != TtsBackend::SherpaOnnx {
            bail!("所选 TTS 后端尚未接入，请选择 sherpa-onnx");
        }
        if self.enabled && (self.asr_gpu || self.asr_ncnn_gpu || self.tts_gpu || self.aec_enabled) {
            bail!("当前构建尚未接入 GPU 或 AEC；请关闭相应开关");
        }
        if self.enabled
            && self.asr_backend == AsrBackend::SherpaNcnn
            && self.asr_ncnn_executable.as_os_str().is_empty()
        {
            bail!("sherpa-ncnn 可执行程序路径不能为空");
        }
        if self.enabled && self.kws_enabled {
            if self.kws_keyword.trim().is_empty() && self.kws_keyword_en.trim().is_empty() {
                bail!("至少填写一个 KWS 唤醒词");
            }
            if self.kws_keywords_file.as_os_str().is_empty()
                && (self.kws_keyword.trim() != "你好纳西妲"
                    || self.kws_keyword_en.trim() != "Hello Nahida")
            {
                bail!("自定义唤醒词需指定经 sherpa-onnx text2token 转换的关键词文件");
            }
        }
        if self.enabled
            && (self.model_dir.as_os_str().is_empty()
                || self.reference_audio.as_os_str().is_empty()
                || self.reference_text.trim().is_empty())
        {
            bail!("启用语音前需指定模型目录、参考音频和对应文字");
        }
        if !(1..=16).contains(&self.tts_threads) {
            bail!("TTS 线程数不在 1–16 范围内");
        }
        if !(1..=16).contains(&self.asr_threads) {
            bail!("ASR 线程数不在 1–16 范围内");
        }
        if !(1..=16).contains(&self.asr_ncnn_threads)
            || !(1..=16).contains(&self.kokoro_threads)
            || !(1..=16).contains(&self.kws_threads)
            || !matches!(self.asr_mlx_device.as_str(), "auto" | "cpu" | "gpu")
        {
            bail!("预留后端的线程数或设备选择无效");
        }
        if self.enabled && self.system_prompt.trim().is_empty() {
            bail!("System Prompt 不能为空");
        }
        if self.system_prompt.chars().count() > 4000 {
            bail!("System Prompt 最多 4000 字");
        }
        if !self.kws_threshold.is_finite() || !(0.01..=1.0).contains(&self.kws_threshold) {
            bail!("KWS 阈值不在 0.01–1.0 范围内");
        }
        if self.output_volume_percent > 100 {
            bail!("播放音量不在 0–100 范围内");
        }
        if !self.vad_threshold.is_finite()
            || !(0.1..=0.99).contains(&self.vad_threshold)
            || !(200..=2000).contains(&self.vad_silence_ms)
            || !(2..=60).contains(&self.no_speech_timeout_secs)
            || self.rest_after_inactive_minutes > 240
        {
            bail!("VAD 阈值、静音时长或无语音超时超出允许范围");
        }
        if self.enabled {
            pet_inference_http::LmStudioBackend::new(pet_inference_http::LmStudioConfig::new(
                &self.lm_studio_url,
                &self.lm_studio_model,
            ))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct VoiceStatus {
    pub phase: String,
    pub kws_status: String,
    pub session_id: u64,
    pub turn_id: u64,
    pub transcript: String,
    pub response: String,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct Pcm {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

#[derive(Clone)]
pub struct TurnToken {
    generation: Arc<AtomicU64>,
    generation_id: u64,
    turn_id: u64,
}

impl TurnToken {
    pub fn new(generation: Arc<AtomicU64>, turn_id: u64) -> Self {
        Self {
            generation,
            generation_id: turn_id,
            turn_id,
        }
    }
    pub fn for_turn(generation: Arc<AtomicU64>, generation_id: u64, turn_id: u64) -> Self {
        Self {
            generation,
            generation_id,
            turn_id,
        }
    }
    pub fn session_id(&self) -> u64 {
        self.generation_id
    }
    pub fn id(&self) -> u64 {
        self.turn_id
    }
    pub fn is_current(&self) -> bool {
        self.generation.load(Ordering::Acquire) == self.generation_id
    }
    pub fn check(&self) -> Result<()> {
        if self.is_current() {
            Ok(())
        } else {
            bail!("voice turn cancelled")
        }
    }
}

pub trait InputPort: Send {
    fn capture(&mut self, token: &TurnToken) -> Result<Pcm>;
}
pub trait AsrPort: Send {
    fn transcribe(&mut self, pcm: &Pcm, token: &TurnToken) -> Result<String>;
}
pub trait LlmPort: Send {
    fn reset_session(&mut self) {}
    fn reply(
        &mut self,
        text: &str,
        token: &TurnToken,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String>;
}
pub trait TtsPort: Send {
    /// Emit one or more PCM chunks. Local ZipVoice emits one complete chunk;
    /// a cloud or qwentts.cpp adapter may emit chunks as they arrive.
    fn synthesize(
        &mut self,
        text: &str,
        token: &TurnToken,
        on_audio: &mut dyn FnMut(Pcm) -> Result<()>,
    ) -> Result<()>;
}
pub trait PlaybackPort: Send {
    fn play(&mut self, pcm: &Pcm, token: &TurnToken) -> Result<()>;
}

pub struct VoicePipeline {
    pub input: Box<dyn InputPort>,
    pub asr: Box<dyn AsrPort>,
    pub llm: Box<dyn LlmPort>,
    pub tts: Box<dyn TtsPort>,
    pub playback: Box<dyn PlaybackPort>,
}

#[derive(Debug)]
pub struct NoSpeechTimeout;
impl fmt::Display for NoSpeechTimeout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "等待语音超时")
    }
}
impl Error for NoSpeechTimeout {}

impl VoicePipeline {
    pub fn reset_session(&mut self) {
        self.llm.reset_session();
    }
    pub fn run(&mut self, token: &TurnToken, mut report: impl FnMut(VoiceStatus)) -> Result<()> {
        let mut status = VoiceStatus {
            phase: "listening".into(),
            session_id: token.session_id(),
            turn_id: token.id(),
            ..Default::default()
        };
        report(status.clone());
        let captured = self.input.capture(token)?;
        token.check()?;
        status.phase = "recognizing".into();
        report(status.clone());
        let transcript = self.asr.transcribe(&captured, token)?;
        token.check()?;
        if transcript.trim().is_empty() {
            bail!("未识别到可用语音");
        }
        status.transcript = transcript.clone();
        status.phase = "thinking".into();
        report(status.clone());
        let response = self.llm.reply(&transcript, token, &mut |delta| {
            if token.is_current() {
                status.response.push_str(delta);
                report(status.clone());
            }
        })?;
        token.check()?;
        if response.trim().is_empty() {
            bail!("LLM 未返回可朗读正文");
        }
        status.response = response.clone();
        for sentence in spoken_segments(&response, 80) {
            token.check()?;
            status.phase = "synthesizing".into();
            report(status.clone());
            let playback = &mut self.playback;
            let mut chunks = 0usize;
            self.tts.synthesize(&sentence, token, &mut |speech| {
                token.check()?;
                if speech.samples.is_empty() || speech.sample_rate == 0 {
                    bail!("TTS 输出了无效音频块");
                }
                chunks += 1;
                status.phase = "speaking".into();
                report(status.clone());
                playback.play(&speech, token)
            })?;
            if chunks == 0 {
                bail!("TTS 未输出音频");
            }
        }
        token.check()?;
        status.phase = "idle".into();
        report(status);
        Ok(())
    }
}

fn spoken_segments(text: &str, limit: usize) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if "。！？；.!?\n".contains(ch) || current.chars().count() >= limit {
            let segment = current.trim();
            if !segment.is_empty() {
                result.push(segment.to_owned());
            }
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        result.push(current.trim().to_owned());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    #[test]
    fn older_voice_settings_gain_safe_provider_defaults() {
        let old = r#"{"enabled":true,"model_dir":"/models","reference_audio":"/voice.wav","reference_text":"hello","lm_studio_url":"http://127.0.0.1:1234","lm_studio_model":"demo","tts_threads":8,"remember_context":true}"#;
        let settings: VoiceSettings = serde_json::from_str(old).unwrap();
        assert_eq!(settings.asr_backend, AsrBackend::SherpaOnnx);
        assert_eq!(settings.llm_backend, LlmBackend::LmStudio);
        assert_eq!(settings.tts_backend, TtsBackend::SherpaOnnx);
        assert_eq!(settings.no_speech_timeout_secs, 8);
        assert_eq!(settings.output_volume_percent, 100);
        assert!(!settings.system_prompt.is_empty());
        assert!(settings.validate().is_ok());
        let unsupported = VoiceSettings {
            tts_backend: TtsBackend::QwenTtsCpp,
            ..settings.clone()
        };
        assert!(unsupported.validate().is_err());
        let no_prompt = VoiceSettings {
            system_prompt: String::new(),
            ..settings
        };
        assert!(no_prompt.validate().is_err());
        let planned = VoiceSettings {
            llm_backend: LlmBackend::Anthropic,
            lm_studio_url: "https://api.anthropic.com/v1/messages".into(),
            ..VoiceSettings::default()
        };
        assert!(planned.validate().is_ok());
    }

    #[test]
    fn ncnn_and_default_bilingual_kws_can_be_selected() {
        let settings = VoiceSettings {
            enabled: true,
            asr_backend: AsrBackend::SherpaNcnn,
            kws_enabled: true,
            model_dir: "/models".into(),
            reference_audio: "/voice.wav".into(),
            reference_text: "reference".into(),
            ..VoiceSettings::default()
        };
        assert!(settings.validate().is_ok());
        let custom = VoiceSettings {
            kws_keyword_en: "Hi Nahida".into(),
            ..settings.clone()
        };
        assert!(custom.validate().is_err());
        let with_file = VoiceSettings {
            kws_keywords_file: "/keywords.txt".into(),
            ..custom
        };
        assert!(with_file.validate().is_ok());
        let gpu = VoiceSettings {
            asr_ncnn_gpu: true,
            ..settings
        };
        assert!(gpu.validate().is_err());
    }

    #[test]
    fn recurring_birthday_accepts_leap_day_but_rejects_invalid_dates() {
        let mut settings = VoiceSettings::default();
        settings.birthday = "02-29".into();
        assert!(settings.validate().is_ok());
        for invalid in ["2-29", "02-30", "00-10", "13-01", "06-31"] {
            settings.birthday = invalid.into();
            assert!(settings.validate().is_err(), "{invalid}");
        }
    }
    #[test]
    fn chinese_sentences_keep_order_and_limit() {
        assert_eq!(
            spoken_segments("你好。今天好吗？", 80),
            ["你好。", "今天好吗？"]
        );
        assert_eq!(spoken_segments("一二三四五", 3), ["一二三", "四五"]);
    }
    #[test]
    fn stale_turn_is_cancelled() {
        let generation = Arc::new(AtomicU64::new(1));
        let token = TurnToken::new(generation.clone(), 1);
        assert!(token.check().is_ok());
        let follow_up = TurnToken::for_turn(generation.clone(), 1, 42);
        assert!(follow_up.check().is_ok());
        assert_eq!(follow_up.id(), 42);
        generation.store(2, Ordering::Release);
        assert!(token.check().is_err());
        assert!(follow_up.check().is_err());
    }

    struct FixedInput;
    impl InputPort for FixedInput {
        fn capture(&mut self, _: &TurnToken) -> Result<Pcm> {
            Ok(Pcm {
                samples: vec![0.0; 160],
                sample_rate: 16000,
            })
        }
    }
    struct FixedAsr;
    impl AsrPort for FixedAsr {
        fn transcribe(&mut self, _: &Pcm, _: &TurnToken) -> Result<String> {
            Ok("你好".into())
        }
    }
    struct FixedLlm;
    impl LlmPort for FixedLlm {
        fn reply(
            &mut self,
            _: &str,
            _: &TurnToken,
            on_delta: &mut dyn FnMut(&str),
        ) -> Result<String> {
            on_delta("答");
            on_delta("复。");
            Ok("答复。".into())
        }
    }
    struct ChunkedTts;
    impl TtsPort for ChunkedTts {
        fn synthesize(
            &mut self,
            _: &str,
            _: &TurnToken,
            on_audio: &mut dyn FnMut(Pcm) -> Result<()>,
        ) -> Result<()> {
            for _ in 0..2 {
                on_audio(Pcm {
                    samples: vec![0.0; 160],
                    sample_rate: 16000,
                })?;
            }
            Ok(())
        }
    }
    struct CountPlayback(Arc<AtomicUsize>);
    impl PlaybackPort for CountPlayback {
        fn play(&mut self, _: &Pcm, _: &TurnToken) -> Result<()> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }
    #[test]
    fn chunked_tts_uses_same_playback_port_and_finishes_turn() {
        let played = Arc::new(AtomicUsize::new(0));
        let mut pipeline = VoicePipeline {
            input: Box::new(FixedInput),
            asr: Box::new(FixedAsr),
            llm: Box::new(FixedLlm),
            tts: Box::new(ChunkedTts),
            playback: Box::new(CountPlayback(played.clone())),
        };
        let token = TurnToken::new(Arc::new(AtomicU64::new(1)), 1);
        let mut last = VoiceStatus::default();
        pipeline.run(&token, |status| last = status).unwrap();
        assert_eq!(played.load(Ordering::Relaxed), 2);
        assert_eq!(last.phase, "idle");
        assert_eq!(last.transcript, "你好");
        assert_eq!(last.response, "答复。");
    }
}
