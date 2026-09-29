use crate::{
    AsrBackend, AsrPort, InputPort, LlmBackend, LlmPort, NoSpeechTimeout, Pcm, PlaybackPort,
    TtsPort, TurnToken, VoicePipeline, VoiceSettings,
};
use anyhow::{Context, Result, anyhow, bail, ensure};
use cpal::{
    SampleFormat, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use crossbeam_queue::ArrayQueue;
use pet_inference_http::{ChatMessage, LmStudioBackend, LmStudioConfig};
use sherpa_onnx::{
    GenerationConfig, KeywordSpotter, KeywordSpotterConfig, LinearResampler, OfflineRecognizer,
    OfflineRecognizerConfig, OfflineTts, OfflineTtsConfig, OfflineTtsZipvoiceModelConfig,
    VadModelConfig, VoiceActivityDetector, Wave,
};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

fn file(path: &Path) -> Result<String> {
    ensure!(path.is_file(), "模型或音频文件不存在：{}", path.display());
    Ok(path.to_str().context("模型路径不是 UTF-8")?.to_owned())
}

pub fn local_pipeline(settings: &VoiceSettings) -> Result<VoicePipeline> {
    settings.validate()?;
    let dir = &settings.model_dir;
    let vad_path = if settings.vad_model_path.as_os_str().is_empty() {
        dir.join("silero_vad.onnx")
    } else {
        settings.vad_model_path.clone()
    };
    let vad_model = file(&vad_path)?;
    let asr: Box<dyn AsrPort> = match settings.asr_backend {
        AsrBackend::SherpaOnnx => {
            let asr_dir = dir.join("sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09");
            let mut config = OfflineRecognizerConfig::default();
            let model = if settings.asr_model_path.as_os_str().is_empty() {
                asr_dir.join("model.int8.onnx")
            } else {
                settings.asr_model_path.clone()
            };
            config.model_config.sense_voice.model = Some(file(&model)?);
            config.model_config.sense_voice.language = Some("auto".into());
            config.model_config.sense_voice.use_itn = true;
            config.model_config.tokens = Some(file(&asr_dir.join("tokens.txt"))?);
            config.model_config.num_threads = settings.asr_threads as i32;
            config.model_config.provider = Some("cpu".into());
            let recognizer = OfflineRecognizer::create(&config).context("SenseVoice 加载失败")?;
            Box::new(SenseVoice { recognizer })
        }
        AsrBackend::SherpaNcnn => {
            let model_dir = if settings.asr_ncnn_model_dir.as_os_str().is_empty() {
                dir.join("sherpa-ncnn-sense-voice-zh-en-ja-ko-yue-2025-09-09")
            } else {
                settings.asr_ncnn_model_dir.clone()
            };
            for name in ["model.ncnn.param", "model.ncnn.bin", "tokens.txt"] {
                file(&model_dir.join(name))?;
            }
            Box::new(NcnnSenseVoice {
                executable: settings.asr_ncnn_executable.clone(),
                model_dir,
                threads: settings.asr_ncnn_threads,
            })
        }
        AsrBackend::SherpaMlx => bail!("sherpa-mlx ASR 尚未接入"),
    };

    let zip = if settings.tts_model_dir.as_os_str().is_empty() {
        dir.join("sherpa-onnx-zipvoice-distill-int8-zh-en-emilia")
    } else {
        settings.tts_model_dir.clone()
    };
    let data_dir = zip.join("espeak-ng-data");
    ensure!(
        data_dir.is_dir(),
        "缺少 espeak-ng-data：{}",
        data_dir.display()
    );
    let tts_config = OfflineTtsConfig {
        model: sherpa_onnx::OfflineTtsModelConfig {
            zipvoice: OfflineTtsZipvoiceModelConfig {
                tokens: Some(file(&zip.join("tokens.txt"))?),
                encoder: Some(file(&zip.join("encoder.int8.onnx"))?),
                decoder: Some(file(&zip.join("decoder.int8.onnx"))?),
                vocoder: Some(file(&dir.join("vocos_24khz.onnx"))?),
                data_dir: Some(data_dir.to_str().context("模型路径不是 UTF-8")?.to_owned()),
                lexicon: Some(file(&zip.join("lexicon.txt"))?),
                feat_scale: 0.1,
                t_shift: 0.5,
                target_rms: 0.1,
                guidance_scale: 1.0,
            },
            num_threads: settings.tts_threads as i32,
            provider: Some("cpu".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    let tts = OfflineTts::create(&tts_config).context("ZipVoice 加载失败")?;
    let reference =
        Wave::read(&file(&settings.reference_audio)?).context("参考音频读取失败，需单声道 WAV")?;
    ensure!(!reference.samples().is_empty(), "参考音频为空");
    let reference_audio = reference.samples().to_vec();
    let reference_sample_rate = reference.sample_rate();

    let mut lm_config = LmStudioConfig::new(&settings.lm_studio_url, &settings.lm_studio_model);
    lm_config.api_key = Some(settings.llm_api_key.clone());
    let lm = LmStudioBackend::new(lm_config)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    if settings.llm_backend == LlmBackend::LmStudio {
        runtime
            .block_on(lm.probe())
            .context("LM Studio 模型不可用")?;
    }

    Ok(VoicePipeline {
        input: Box::new(MicrophoneVAD {
            model: vad_model,
            threshold: settings.vad_threshold,
            silence_ms: settings.vad_silence_ms,
            no_speech_timeout_secs: settings.no_speech_timeout_secs,
        }),
        asr,
        llm: Box::new(LmStudioDialogue {
            backend: lm,
            runtime,
            kind: settings.llm_backend,
            system_prompt: settings.system_prompt.clone(),
            remember_context: settings.remember_context,
            previous_response_id: None,
            history: Vec::new(),
        }),
        tts: Box::new(ZipVoice {
            tts,
            reference_audio,
            reference_sample_rate,
            reference_text: settings.reference_text.clone(),
        }),
        playback: Box::new(SpeakerOutput {
            gain: settings.output_volume_percent as f32 / 100.0,
        }),
    })
}

struct MicrophoneVAD {
    model: String,
    threshold: f32,
    silence_ms: u16,
    no_speech_timeout_secs: u16,
}

impl InputPort for MicrophoneVAD {
    fn capture(&mut self, token: &TurnToken) -> Result<Pcm> {
        let mut vad_config = VadModelConfig::default();
        vad_config.silero_vad.model = Some(self.model.clone());
        vad_config.silero_vad.threshold = self.threshold;
        vad_config.silero_vad.min_silence_duration = self.silence_ms as f32 / 1000.0;
        vad_config.silero_vad.min_speech_duration = 0.25;
        vad_config.silero_vad.max_speech_duration = 20.0;
        vad_config.silero_vad.window_size = 512;
        vad_config.sample_rate = 16000;
        vad_config.num_threads = 1;
        vad_config.provider = Some("cpu".into());
        let vad =
            VoiceActivityDetector::create(&vad_config, 22.0).context("Silero VAD 加载失败")?;
        let host = cpal::default_host();
        let device = host.default_input_device().context("没有麦克风输入设备")?;
        let supported = device
            .default_input_config()
            .context("无法读取麦克风格式或权限")?;
        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels() as usize;
        ensure!(channels > 0, "麦克风没有音频通道");
        let config: StreamConfig = supported.config();
        let queue = Arc::new(ArrayQueue::<f32>::new(
            (sample_rate as usize).saturating_mul(2),
        ));
        let overrun = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let stream = input_stream(
            &device,
            &config,
            supported.sample_format(),
            channels,
            queue.clone(),
            overrun.clone(),
            failed.clone(),
        )?;
        let resampler = if sample_rate == 16000 {
            None
        } else {
            Some(
                LinearResampler::create(sample_rate as i32, 16000)
                    .context("麦克风重采样器创建失败")?,
            )
        };
        stream
            .play()
            .context("麦克风启动失败；检查系统麦克风权限")?;
        let started = Instant::now();
        let mut speech_started: Option<Instant> = None;
        let mut pending = VecDeque::with_capacity(4096);
        loop {
            token.check()?;
            ensure!(!failed.load(Ordering::Acquire), "麦克风采集失败");
            ensure!(
                !overrun.load(Ordering::Acquire),
                "麦克风缓冲溢出，当前语句已取消"
            );
            let mut raw = Vec::with_capacity(2048);
            while raw.len() < 2048 {
                if let Some(sample) = queue.pop() {
                    raw.push(sample);
                } else {
                    break;
                }
            }
            if !raw.is_empty() {
                let converted = match &resampler {
                    Some(r) => r.resample(&raw, false),
                    None => raw,
                };
                pending.extend(converted);
                while pending.len() >= 512 {
                    let window: Vec<f32> = (0..512).filter_map(|_| pending.pop_front()).collect();
                    vad.accept_waveform(&window);
                    if vad.detected() && speech_started.is_none() {
                        speech_started = Some(Instant::now());
                    }
                    if let Some(segment) = vad.front() {
                        let samples = segment.samples().to_vec();
                        vad.pop();
                        ensure!(!samples.is_empty(), "VAD 输出空语句");
                        return Ok(Pcm {
                            samples,
                            sample_rate: 16000,
                        });
                    }
                }
            } else {
                std::thread::sleep(Duration::from_millis(8));
            }
            if speech_started.is_none()
                && started.elapsed() >= Duration::from_secs(self.no_speech_timeout_secs as u64)
            {
                return Err(NoSpeechTimeout.into());
            }
            if speech_started.is_some_and(|at| at.elapsed() >= Duration::from_secs(20)) {
                vad.flush();
                if let Some(segment) = vad.front() {
                    let samples = segment.samples().to_vec();
                    vad.pop();
                    ensure!(!samples.is_empty(), "VAD 输出空语句");
                    return Ok(Pcm {
                        samples,
                        sample_rate: 16000,
                    });
                }
                bail!("语音已超出 20 秒且未形成可识别片段");
            }
        }
    }
}

fn input_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    format: SampleFormat,
    channels: usize,
    queue: Arc<ArrayQueue<f32>>,
    overrun: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream> {
    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _| {
                for frame in data.chunks_exact(channels) {
                    let mono = frame.iter().copied().sum::<f32>() / channels as f32;
                    if queue.push(mono).is_err() {
                        overrun.store(true, Ordering::Release);
                    }
                }
            },
            move |_| {
                failed.store(true, Ordering::Release);
            },
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _| {
                for frame in data.chunks_exact(channels) {
                    let mono =
                        frame.iter().map(|&v| v as f32 / 32768.0).sum::<f32>() / channels as f32;
                    if queue.push(mono).is_err() {
                        overrun.store(true, Ordering::Release);
                    }
                }
            },
            move |_| {
                failed.store(true, Ordering::Release);
            },
            None,
        )?,
        SampleFormat::U16 => device.build_input_stream(
            config,
            move |data: &[u16], _| {
                for frame in data.chunks_exact(channels) {
                    let mono = frame
                        .iter()
                        .map(|&v| (v as f32 - 32768.0) / 32768.0)
                        .sum::<f32>()
                        / channels as f32;
                    if queue.push(mono).is_err() {
                        overrun.store(true, Ordering::Release);
                    }
                }
            },
            move |_| {
                failed.store(true, Ordering::Release);
            },
            None,
        )?,
        _ => bail!("麦克风采样格式暂不支持：{format:?}"),
    };
    Ok(stream)
}

struct SenseVoice {
    recognizer: OfflineRecognizer,
}
impl AsrPort for SenseVoice {
    fn transcribe(&mut self, pcm: &Pcm, token: &TurnToken) -> Result<String> {
        token.check()?;
        ensure!(
            pcm.sample_rate == 16000 && !pcm.samples.is_empty(),
            "ASR 需要 16 kHz 非空 PCM"
        );
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(16000, &pcm.samples);
        self.recognizer.decode(&stream);
        token.check()?;
        Ok(stream
            .get_result()
            .context("SenseVoice 未返回结果")?
            .text
            .trim()
            .to_owned())
    }
}

struct NcnnSenseVoice {
    executable: PathBuf,
    model_dir: PathBuf,
    threads: u8,
}

impl AsrPort for NcnnSenseVoice {
    fn transcribe(&mut self, pcm: &Pcm, token: &TurnToken) -> Result<String> {
        token.check()?;
        ensure!(
            pcm.sample_rate == 16000 && !pcm.samples.is_empty(),
            "ASR 需要 16 kHz 非空 PCM"
        );
        let temp_dir = tempfile::tempdir()?;
        let wav = temp_dir.path().join("input.wav");
        let wav_path = wav.to_str().context("临时 WAV 路径不是 UTF-8")?;
        ensure!(
            sherpa_onnx::write(wav_path, &pcm.samples, 16000),
            "无法写入 ASR 临时 WAV"
        );
        let mut child = Command::new(&self.executable)
            .arg(format!(
                "--tokens={}",
                self.model_dir.join("tokens.txt").display()
            ))
            .arg(format!(
                "--sense-voice-model-dir={}",
                self.model_dir.display()
            ))
            .arg("--sense-voice-use-itn=1")
            .arg(format!("--num-threads={}", self.threads))
            .arg(wav_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("无法启动 sherpa-ncnn：{}", self.executable.display()))?;
        loop {
            if !token.is_current() {
                let _ = child.kill();
                let _ = child.wait();
                token.check()?;
            }
            if child.try_wait()?.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let output = child.wait_with_output()?;
        ensure!(
            output.status.success(),
            "sherpa-ncnn 识别失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        token.check()?;
        // The official CLI currently writes its result to stderr, although
        // some builds and wrappers use stdout.
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let text = stdout
            .lines()
            .chain(stderr.lines())
            .find_map(|line| {
                let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
                value.get("text")?.as_str().map(str::to_owned)
            })
            .context("sherpa-ncnn 未输出识别结果 JSON")?;
        Ok(text.trim().to_owned())
    }
}

const DEFAULT_KWS_KEYWORDS: &str =
    "n ǐ h ǎo n à x ī d á @你好纳西妲\nHH AH0 L OW1 N AA0 HH IY1 D AH0 @Hello_Nahida\n";

/// Listen while the caller permits it; dropping the stream releases the microphone.
/// The configured keyword file must already contain phone+ppinyin tokens.
pub fn listen_for_keyword(
    settings: &VoiceSettings,
    keep_listening: impl Fn() -> bool,
) -> Result<Option<String>> {
    let spotter = create_keyword_spotter(settings)?;
    let kws_stream = spotter.create_stream();
    let host = cpal::default_host();
    let device = host.default_input_device().context("没有麦克风输入设备")?;
    let supported = device
        .default_input_config()
        .context("无法读取麦克风格式或权限")?;
    let sample_rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    ensure!(channels > 0, "麦克风没有音频通道");
    let queue = Arc::new(ArrayQueue::<f32>::new(
        (sample_rate as usize).saturating_mul(2),
    ));
    let overrun = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    let stream = input_stream(
        &device,
        &supported.config(),
        supported.sample_format(),
        channels,
        queue.clone(),
        overrun.clone(),
        failed.clone(),
    )?;
    let resampler = if sample_rate == 16000 {
        None
    } else {
        Some(LinearResampler::create(sample_rate as i32, 16000).context("KWS 重采样器创建失败")?)
    };
    stream.play().context("KWS 麦克风启动失败")?;
    // This point is reached only after the model and input device both work.
    while keep_listening() {
        ensure!(!failed.load(Ordering::Acquire), "KWS 麦克风采集失败");
        // A brief scheduler stall should not permanently disable wake-word
        // detection. Drop the overflow flag and continue with current audio.
        overrun.store(false, Ordering::Release);
        let mut raw = Vec::with_capacity(2048);
        while raw.len() < 2048 {
            if let Some(sample) = queue.pop() {
                raw.push(sample);
            } else {
                break;
            }
        }
        if raw.is_empty() {
            std::thread::sleep(Duration::from_millis(8));
            continue;
        }
        let samples = match &resampler {
            Some(r) => r.resample(&raw, false),
            None => raw,
        };
        if samples.is_empty() {
            continue;
        }
        kws_stream.accept_waveform(16000, &samples);
        while spotter.is_ready(&kws_stream) {
            spotter.decode(&kws_stream);
            if let Some(result) = spotter.get_result(&kws_stream) {
                if !result.keyword.is_empty() {
                    return Ok(Some(result.keyword));
                }
            }
        }
    }
    Ok(None)
}

fn create_keyword_spotter(settings: &VoiceSettings) -> Result<KeywordSpotter> {
    let dir = if settings.kws_model_dir.as_os_str().is_empty() {
        settings
            .model_dir
            .join("sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20")
    } else {
        settings.kws_model_dir.clone()
    };
    let mut config = KeywordSpotterConfig::default();
    let stem = "epoch-13-avg-2-chunk-16-left-64.onnx";
    config.model_config.transducer.encoder = Some(file(&dir.join(format!("encoder-{stem}")))?);
    config.model_config.transducer.decoder = Some(file(&dir.join(format!("decoder-{stem}")))?);
    config.model_config.transducer.joiner = Some(file(&dir.join(format!("joiner-{stem}")))?);
    config.model_config.tokens = Some(file(&dir.join("tokens.txt"))?);
    config.model_config.num_threads = settings.kws_threads as i32;
    config.model_config.provider = Some("cpu".into());
    config.keywords_threshold = settings.kws_threshold;
    if settings.kws_keywords_file.as_os_str().is_empty() {
        config.keywords_buf = Some(DEFAULT_KWS_KEYWORDS.into());
    } else {
        config.keywords_file = Some(file(&settings.kws_keywords_file)?);
    }
    KeywordSpotter::create(&config).context("KWS 模型加载失败")
}

#[cfg(test)]
mod backend_tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn ncnn_adapter_parses_official_json_line() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-sherpa-ncnn-offline");
        std::fs::write(&script, "#!/bin/sh\nprintf '%s\\n' 'Loading model' '{\"lang\":\"<|zh|>\",\"text\":\"你好纳西妲\"}'\n").unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();
        let mut asr = NcnnSenseVoice {
            executable: script,
            model_dir: dir.path().into(),
            threads: 2,
        };
        let token = TurnToken::new(Arc::new(std::sync::atomic::AtomicU64::new(1)), 1);
        let result = asr
            .transcribe(
                &Pcm {
                    samples: vec![0.0; 1600],
                    sample_rate: 16000,
                },
                &token,
            )
            .unwrap();
        assert_eq!(result, "你好纳西妲");
    }

    #[test]
    #[ignore = "requires the locally downloaded sherpa-onnx KWS model"]
    fn local_kws_model_accepts_default_bilingual_keywords() {
        let root =
            std::env::var_os("DESKTOPPET_KWS_MODEL_DIR").expect("set DESKTOPPET_KWS_MODEL_DIR");
        let settings = VoiceSettings {
            kws_model_dir: root.into(),
            ..VoiceSettings::default()
        };
        let spotter = create_keyword_spotter(&settings).unwrap();
        let wav_path =
            std::env::var_os("DESKTOPPET_KWS_TEST_WAV").expect("set DESKTOPPET_KWS_TEST_WAV");
        let wave = Wave::read(Path::new(&wav_path).to_str().unwrap()).unwrap();
        let stream = spotter.create_stream();
        let mut hit = false;
        let mut samples = wave.samples().to_vec();
        samples.extend(vec![0.0; 16000]);
        for chunk in samples.chunks(2048) {
            stream.accept_waveform(wave.sample_rate(), chunk);
            while spotter.is_ready(&stream) {
                spotter.decode(&stream);
                hit |= spotter
                    .get_result(&stream)
                    .is_some_and(|result| result.keyword == "你好纳西妲");
            }
        }
        assert!(hit, "expected Chinese wake word to be detected");
    }

    #[test]
    #[ignore = "requires a local sherpa-ncnn binary, model and sample WAV"]
    fn local_ncnn_sense_voice_decodes_test_wav() {
        let binary =
            std::env::var_os("DESKTOPPET_NCNN_BINARY").expect("set DESKTOPPET_NCNN_BINARY");
        let model_dir = PathBuf::from(
            std::env::var_os("DESKTOPPET_NCNN_MODEL_DIR").expect("set DESKTOPPET_NCNN_MODEL_DIR"),
        );
        let wav = Wave::read(model_dir.join("test_wavs/zh.wav").to_str().unwrap()).unwrap();
        let mut asr = NcnnSenseVoice {
            executable: binary.into(),
            model_dir,
            threads: 2,
        };
        let token = TurnToken::new(Arc::new(std::sync::atomic::AtomicU64::new(1)), 1);
        let text = asr
            .transcribe(
                &Pcm {
                    samples: wav.samples().to_vec(),
                    sample_rate: wav.sample_rate() as u32,
                },
                &token,
            )
            .unwrap();
        assert!(text.contains("早上"), "unexpected transcript: {text}");
    }
}

struct LmStudioDialogue {
    backend: LmStudioBackend,
    runtime: tokio::runtime::Runtime,
    kind: LlmBackend,
    system_prompt: String,
    remember_context: bool,
    previous_response_id: Option<String>,
    history: Vec<ChatMessage>,
}
impl LlmPort for LmStudioDialogue {
    fn reset_session(&mut self) {
        self.previous_response_id = None;
        self.history.clear();
    }
    fn reply(
        &mut self,
        text: &str,
        token: &TurnToken,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String> {
        let cancel = async {
            while token.is_current() {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        };
        let system = self.system_prompt.as_str();
        if self.kind == LlmBackend::LmStudio {
            let previous = self.previous_response_id.clone();
            let reply = self.runtime.block_on(async {
                tokio::select! {
                    result = self.backend.stream_native_chat_with_context(
                        text, system, previous.as_deref(), self.remember_context, on_delta) => result,
                    _ = cancel => Err(anyhow!("voice turn cancelled")),
                }
            })?;
            self.previous_response_id = reply.response_id;
            Ok(reply.text)
        } else {
            let mut messages = vec![ChatMessage::system(system)];
            if self.remember_context {
                messages.extend(self.history.iter().cloned());
            }
            messages.push(ChatMessage::user(text));
            let answer = self.runtime.block_on(async {
                tokio::select! {
                    result = self.backend.stream_chat(&messages, on_delta) => result,
                    _ = cancel => Err(anyhow!("voice turn cancelled")),
                }
            })?;
            if self.remember_context {
                self.history.push(ChatMessage::user(text));
                self.history.push(ChatMessage::assistant(&answer));
                if self.history.len() > 12 {
                    self.history.drain(..self.history.len() - 12);
                }
            }
            Ok(answer)
        }
    }
}

struct ZipVoice {
    tts: OfflineTts,
    reference_audio: Vec<f32>,
    reference_sample_rate: i32,
    reference_text: String,
}
impl TtsPort for ZipVoice {
    fn synthesize(
        &mut self,
        text: &str,
        token: &TurnToken,
        on_audio: &mut dyn FnMut(Pcm) -> Result<()>,
    ) -> Result<()> {
        token.check()?;
        let config = GenerationConfig {
            speed: 1.0,
            reference_audio: Some(self.reference_audio.clone()),
            reference_sample_rate: self.reference_sample_rate,
            reference_text: Some(self.reference_text.clone()),
            num_steps: 4,
            ..Default::default()
        };
        let active = token.clone();
        let result = self
            .tts
            .generate_with_config(text, &config, Some(move |_: &[f32], _| active.is_current()))
            .context("ZipVoice 合成失败或已取消")?;
        token.check()?;
        ensure!(
            !result.samples().is_empty() && result.sample_rate() > 0,
            "ZipVoice 输出为空"
        );
        on_audio(Pcm {
            samples: result.samples().to_vec(),
            sample_rate: result.sample_rate() as u32,
        })
    }
}

struct SpeakerOutput {
    gain: f32,
}
pub fn play_wav(path: &Path, token: &TurnToken) -> Result<()> {
    play_wav_with_volume(path, token, 100)
}

pub fn play_wav_with_volume(path: &Path, token: &TurnToken, volume_percent: u8) -> Result<()> {
    ensure!(volume_percent <= 100, "播放音量不在 0–100 范围内");
    let wav = Wave::read(&file(path)?).context("互动语音读取失败，需单声道 WAV")?;
    SpeakerOutput {
        gain: volume_percent as f32 / 100.0,
    }
    .play(
        &Pcm {
            samples: wav.samples().to_vec(),
            sample_rate: wav.sample_rate() as u32,
        },
        token,
    )
}
impl PlaybackPort for SpeakerOutput {
    fn play(&mut self, pcm: &Pcm, token: &TurnToken) -> Result<()> {
        token.check()?;
        let device = cpal::default_host()
            .default_output_device()
            .context("没有扬声器输出设备")?;
        let supported = device
            .default_output_config()
            .context("无法读取扬声器格式")?;
        let channels = supported.channels() as usize;
        ensure!(channels > 0 && pcm.sample_rate > 0, "无效输出格式");
        let out_rate = supported.sample_rate().0;
        let samples = if out_rate == pcm.sample_rate {
            pcm.samples.clone()
        } else {
            LinearResampler::create(pcm.sample_rate as i32, out_rate as i32)
                .context("扬声器重采样器创建失败")?
                .resample(&pcm.samples, true)
        };
        ensure!(!samples.is_empty(), "播放音频为空");
        let samples = Arc::new(samples);
        let position = Arc::new(AtomicUsize::new(0));
        let failed = Arc::new(AtomicBool::new(false));
        let stream = output_stream(
            &device,
            &supported.config(),
            supported.sample_format(),
            channels,
            samples.clone(),
            position.clone(),
            failed.clone(),
            token.clone(),
            self.gain,
        )?;
        stream.play().context("扬声器播放启动失败")?;
        while position.load(Ordering::Acquire) < samples.len() {
            token.check()?;
            ensure!(!failed.load(Ordering::Acquire), "扬声器播放失败或设备断开");
            std::thread::sleep(Duration::from_millis(8));
        }
        // The callback has filled its final buffer; allow CoreAudio's queued
        // frames to drain before dropping the stream.
        std::thread::sleep(Duration::from_millis(80));
        token.check()?;
        Ok(())
    }
}

fn output_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    format: SampleFormat,
    channels: usize,
    samples: Arc<Vec<f32>>,
    position: Arc<AtomicUsize>,
    failed: Arc<AtomicBool>,
    token: TurnToken,
    gain: f32,
) -> Result<cpal::Stream> {
    let stream = match format {
        SampleFormat::F32 => device.build_output_stream(
            config,
            move |data: &mut [f32], _| {
                for frame in data.chunks_mut(channels) {
                    let i = position.fetch_add(1, Ordering::AcqRel);
                    let value = if token.is_current() {
                        samples.get(i).copied().unwrap_or(0.0)
                    } else {
                        0.0
                    };
                    frame.fill((value * gain).clamp(-1.0, 1.0));
                }
            },
            move |_| {
                failed.store(true, Ordering::Release);
            },
            None,
        )?,
        SampleFormat::I16 => device.build_output_stream(
            config,
            move |data: &mut [i16], _| {
                for frame in data.chunks_mut(channels) {
                    let i = position.fetch_add(1, Ordering::AcqRel);
                    let value = if token.is_current() {
                        samples.get(i).copied().unwrap_or(0.0)
                    } else {
                        0.0
                    };
                    frame.fill(((value * gain).clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                }
            },
            move |_| {
                failed.store(true, Ordering::Release);
            },
            None,
        )?,
        SampleFormat::U16 => device.build_output_stream(
            config,
            move |data: &mut [u16], _| {
                for frame in data.chunks_mut(channels) {
                    let i = position.fetch_add(1, Ordering::AcqRel);
                    let value = if token.is_current() {
                        samples.get(i).copied().unwrap_or(0.0)
                    } else {
                        0.0
                    };
                    frame.fill((((value * gain).clamp(-1.0, 1.0) + 1.0) * 32767.5) as u16);
                }
            },
            move |_| {
                failed.store(true, Ordering::Release);
            },
            None,
        )?,
        _ => bail!("扬声器采样格式暂不支持：{format:?}"),
    };
    Ok(stream)
}
