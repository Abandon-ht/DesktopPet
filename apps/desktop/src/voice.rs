use pet_voice_session::{
    NoSpeechTimeout, TurnToken, VoicePipeline, VoiceSettings, VoiceStatus, listen_for_keyword,
    local_pipeline, play_wav_with_volume,
};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::time::Duration;

#[derive(Clone)]
pub struct VoiceController {
    inner: Arc<Inner>,
}

struct Inner {
    settings: Mutex<VoiceSettings>,
    status: Mutex<VoiceStatus>,
    generation: Arc<AtomicU64>,
    turn_sequence: AtomicU64,
    requested: Mutex<Option<VoiceRequest>>,
    revision: AtomicU64,
    stopped: AtomicBool,
    active: AtomicBool,
    last_interaction: Mutex<std::time::Instant>,
    rest_claimed: AtomicBool,
    trigger: mpsc::SyncSender<()>,
}

struct VoiceRequest {
    session_id: u64,
    clip: Option<PathBuf>,
    conversation: bool,
}

impl VoiceController {
    pub fn new(settings: VoiceSettings) -> Self {
        let (trigger, receiver) = mpsc::sync_channel(1);
        let inner = Arc::new(Inner {
            settings: Mutex::new(settings),
            status: Mutex::new(VoiceStatus {
                phase: "idle".into(),
                ..Default::default()
            }),
            generation: Arc::new(AtomicU64::new(0)),
            turn_sequence: AtomicU64::new(0),
            requested: Mutex::new(None),
            revision: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
            active: AtomicBool::new(false),
            last_interaction: Mutex::new(std::time::Instant::now()),
            rest_claimed: AtomicBool::new(false),
            trigger,
        });
        let worker = inner.clone();
        std::thread::Builder::new()
            .name("voice-session".into())
            .spawn(move || run(worker, receiver))
            .expect("voice worker thread");
        let controller = Self { inner };
        let listener = controller.clone();
        std::thread::Builder::new()
            .name("voice-keyword-listener".into())
            .spawn(move || listen_for_wake_word(listener))
            .expect("KWS worker thread");
        controller
    }

    pub fn settings(&self) -> VoiceSettings {
        self.inner.settings.lock().unwrap().clone()
    }
    pub fn status(&self) -> VoiceStatus {
        self.inner.status.lock().unwrap().clone()
    }

    pub fn configure(&self, settings: VoiceSettings) {
        self.stop();
        *self.inner.settings.lock().unwrap() = settings;
        self.inner.revision.fetch_add(1, Ordering::AcqRel);
    }

    /// Starts a conversation without a prerecorded greeting.
    pub fn click(&self) {
        self.request(None, true);
    }

    /// Only a head hit uses the prerecorded wake greeting.
    pub fn head_click(&self) {
        let settings = self.settings();
        let clip = settings
            .wake_greeting_enabled
            .then_some(settings.greeting_dir.as_os_str())
            .filter(|dir| !dir.is_empty())
            .map(|_| settings.greeting_dir.join("心事.wav"));
        self.request(clip, true);
    }

    pub fn greet_if_idle(&self, name: &str) -> bool {
        let settings = self.settings();
        if !settings.enabled || settings.greeting_dir.as_os_str().is_empty() {
            return false;
        }
        let clip = settings.greeting_dir.join(format!("{name}.wav"));
        if !clip.is_file() {
            return false;
        }
        let phase = self.status().phase;
        if self.inner.active.load(Ordering::Acquire)
            || !matches!(phase.as_str(), "idle" | "faulted")
        {
            return false;
        }
        self.request(Some(clip), false);
        true
    }

    pub fn claim_rest_due(&self) -> bool {
        let settings = self.settings();
        if !settings.enabled
            || settings.rest_after_inactive_minutes == 0
            || self.inner.active.load(Ordering::Acquire)
            || self.status().phase != "idle"
            || self.inner.last_interaction.lock().unwrap().elapsed()
                < Duration::from_secs(settings.rest_after_inactive_minutes as u64 * 60)
        {
            return false;
        }
        !self.inner.rest_claimed.swap(true, Ordering::AcqRel)
    }

    fn request(&self, clip: Option<PathBuf>, conversation: bool) {
        if !self.settings().enabled || self.inner.stopped.load(Ordering::Acquire) {
            return;
        }
        let session_id = self.inner.generation.fetch_add(1, Ordering::AcqRel) + 1;
        *self.inner.last_interaction.lock().unwrap() = std::time::Instant::now();
        self.inner.rest_claimed.store(false, Ordering::Release);
        *self.inner.requested.lock().unwrap() = Some(VoiceRequest {
            session_id,
            clip,
            conversation,
        });
        *self.inner.status.lock().unwrap() = VoiceStatus {
            phase: "starting".into(),
            session_id,
            turn_id: session_id,
            detail: "正在准备语音模型…".into(),
            ..Default::default()
        };
        let _ = self.inner.trigger.try_send(());
    }

    pub fn stop(&self) {
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
        *self.inner.requested.lock().unwrap() = None;
        *self.inner.status.lock().unwrap() = VoiceStatus {
            phase: "idle".into(),
            detail: "已停止".into(),
            ..Default::default()
        };
    }

    pub fn shutdown(&self) {
        self.stop();
        self.inner.stopped.store(true, Ordering::Release);
        let _ = self.inner.trigger.try_send(());
    }
}

fn listen_for_wake_word(controller: VoiceController) {
    let inner = &controller.inner;
    while !inner.stopped.load(Ordering::Acquire) {
        let settings = controller.settings();
        if !settings.enabled
            || !settings.kws_enabled
            || inner.active.load(Ordering::Acquire)
            || !matches!(controller.status().phase.as_str(), "idle" | "faulted")
        {
            std::thread::sleep(Duration::from_millis(150));
            continue;
        }
        let revision = inner.revision.load(Ordering::Acquire);
        let result = listen_for_keyword(&settings, || {
            !inner.stopped.load(Ordering::Acquire)
                && inner.revision.load(Ordering::Acquire) == revision
                && !inner.active.load(Ordering::Acquire)
                && matches!(controller.status().phase.as_str(), "idle" | "faulted")
        });
        match result {
            Ok(Some(_keyword)) => controller.head_click(),
            Ok(None) => {}
            Err(error) => {
                if controller.status().phase == "idle"
                    && inner.revision.load(Ordering::Acquire) == revision
                {
                    inner.status.lock().unwrap().detail = format!("关键词监听失败：{error:#}");
                }
                std::thread::sleep(Duration::from_secs(3));
            }
        }
    }
}

fn run(inner: Arc<Inner>, receiver: mpsc::Receiver<()>) {
    struct ActiveGuard<'a>(&'a AtomicBool);
    impl Drop for ActiveGuard<'_> {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
    let mut loaded: Option<VoicePipeline> = None;
    let mut loaded_revision = u64::MAX;
    while receiver.recv().is_ok() {
        if inner.stopped.load(Ordering::Acquire) {
            break;
        }
        let Some(request) = inner.requested.lock().unwrap().take() else {
            continue;
        };
        let session_id = request.session_id;
        if inner.generation.load(Ordering::Acquire) != session_id {
            continue;
        }
        inner.active.store(true, Ordering::Release);
        let _active_guard = ActiveGuard(&inner.active);
        let settings = inner.settings.lock().unwrap().clone();
        if !settings.enabled {
            continue;
        }
        let revision = inner.revision.load(Ordering::Acquire);
        let session = TurnToken::new(inner.generation.clone(), session_id);
        if let Some(clip) = request.clip {
            {
                let mut status = inner.status.lock().unwrap();
                status.phase = "speaking".into();
                status.detail = format!(
                    "播放互动语音：{}",
                    clip.file_stem().unwrap_or_default().to_string_lossy()
                );
            }
            if let Err(error) =
                play_wav_with_volume(&clip, &session, settings.output_volume_percent)
            {
                if session.is_current() {
                    let mut status = inner.status.lock().unwrap();
                    status.phase = "faulted".into();
                    status.detail = format!("互动语音播放失败：{error:#}");
                }
                continue;
            }
        }
        if !request.conversation {
            if session.is_current() {
                *inner.status.lock().unwrap() = VoiceStatus {
                    phase: "idle".into(),
                    detail: "互动语音已播放".into(),
                    ..Default::default()
                };
            }
            continue;
        }
        if loaded_revision != revision || loaded.is_none() {
            loaded = None;
            match local_pipeline(&settings) {
                Ok(pipeline) => {
                    loaded = Some(pipeline);
                    loaded_revision = revision;
                }
                Err(error) => {
                    if session.is_current() {
                        let mut status = inner.status.lock().unwrap();
                        status.phase = "faulted".into();
                        status.detail = format!("语音初始化失败：{error:#}");
                    }
                    continue;
                }
            }
        }
        if !session.is_current() {
            continue;
        }
        loaded.as_mut().unwrap().reset_session();
        let mut completed = 0u8;
        loop {
            if !session.is_current() {
                break;
            }
            let turn_id = inner.turn_sequence.fetch_add(1, Ordering::AcqRel) + 1;
            let token = TurnToken::for_turn(inner.generation.clone(), session_id, turn_id);
            let result = loaded.as_mut().unwrap().run(&token, |status| {
                if token.is_current() {
                    *inner.status.lock().unwrap() = status;
                }
            });
            if !token.is_current() {
                break;
            }
            match result {
                Ok(()) => {
                    *inner.last_interaction.lock().unwrap() = std::time::Instant::now();
                    inner.rest_claimed.store(false, Ordering::Release);
                    completed += 1;
                    if completed >= 10 {
                        let mut status = inner.status.lock().unwrap();
                        status.phase = "idle".into();
                        status.detail = "已完成 10 轮对话；再次点击可继续".into();
                        break;
                    }
                    // Let the final speaker buffer drain before rearming VAD.
                    for _ in 0..18 {
                        if !session.is_current() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
                Err(error) if error.downcast_ref::<NoSpeechTimeout>().is_some() => {
                    let mut status = inner.status.lock().unwrap();
                    status.phase = "idle".into();
                    status.detail =
                        format!("{} 秒无语音，对话已结束", settings.no_speech_timeout_secs);
                    break;
                }
                Err(error) => {
                    let mut status = inner.status.lock().unwrap();
                    status.phase = "faulted".into();
                    status.detail = error.to_string();
                    break;
                }
            }
        }
    }
}
