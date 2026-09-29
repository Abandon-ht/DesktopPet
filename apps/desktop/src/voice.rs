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

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum UiLanguage {
    #[default]
    #[serde(rename = "zh-CN")]
    Zh,
    #[serde(rename = "en-US")]
    En,
    #[serde(rename = "ja-JP")]
    Ja,
    #[serde(rename = "ko-KR")]
    Ko,
}
impl UiLanguage {
    fn directory(self) -> &'static str {
        match self {
            Self::Zh => "zh-CN",
            Self::En => "en-US",
            Self::Ja => "ja-JP",
            Self::Ko => "ko-KR",
        }
    }
}

#[derive(Clone, Copy)]
pub enum Cue {
    FirstMeeting,
    Wake,
    Morning,
    Noon,
    Evening,
    Night,
    Birthday,
    FeedTaste,
    FeedThought,
    Greeting,
    IntimacySmart,
    IntimacyOpen,
    IntimacyFeeling,
    IntimacyBlessing,
}
impl Cue {
    fn category(self) -> &'static str {
        match self {
            Self::FeedTaste | Self::FeedThought => "care",
            Self::IntimacySmart
            | Self::IntimacyOpen
            | Self::IntimacyFeeling
            | Self::IntimacyBlessing => "intimacy",
            _ => "greetings",
        }
    }
    fn file(self) -> &'static str {
        match self {
            Self::FirstMeeting => "first_meeting",
            Self::Wake => "wake",
            Self::Morning => "morning",
            Self::Noon => "noon",
            Self::Evening => "evening",
            Self::Night => "night",
            Self::Birthday => "birthday",
            Self::FeedTaste => "feed_taste",
            Self::FeedThought => "feed_thought",
            Self::Greeting => "greeting",
            Self::IntimacySmart => "intimacy_smart",
            Self::IntimacyOpen => "intimacy_open",
            Self::IntimacyFeeling => "intimacy_feeling",
            Self::IntimacyBlessing => "intimacy_blessing",
        }
    }
    fn legacy(self) -> &'static str {
        match self {
            Self::FirstMeeting => "初次见面",
            Self::Wake => "心事",
            Self::Morning => "早上好",
            Self::Noon => "午休时间到",
            Self::Evening => "太阳落山",
            Self::Night => "快去睡吧",
            Self::Birthday => "生日",
            Self::FeedTaste => "好味道",
            Self::FeedThought => "心意",
            Self::Greeting => "去转转",
            Self::IntimacySmart => "变聪明啦",
            Self::IntimacyOpen => "思路变开阔了",
            Self::IntimacyFeeling => "这种感觉",
            Self::IntimacyBlessing => "赐福",
        }
    }
}

fn cue_path(settings: &VoiceSettings, language: UiLanguage, cue: Cue) -> Option<PathBuf> {
    let root = &settings.greeting_dir;
    if root.as_os_str().is_empty() {
        return None;
    }
    for locale in [language.directory(), "zh-CN"] {
        let path = root
            .join(locale)
            .join(cue.category())
            .join(format!("{}.wav", cue.file()));
        if path.is_file() {
            return Some(path);
        }
    }
    let legacy = root.join(format!("{}.wav", cue.legacy()));
    legacy.is_file().then_some(legacy)
}

#[derive(Clone)]
pub struct VoiceController {
    inner: Arc<Inner>,
}

struct Inner {
    settings: Mutex<VoiceSettings>,
    language: Mutex<UiLanguage>,
    status: Mutex<VoiceStatus>,
    kws_status: Mutex<String>,
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
    clips: Vec<PathBuf>,
    conversation: bool,
}

impl VoiceController {
    pub fn new(settings: VoiceSettings) -> Self {
        let (trigger, receiver) = mpsc::sync_channel(1);
        let inner = Arc::new(Inner {
            settings: Mutex::new(settings),
            language: Mutex::new(UiLanguage::default()),
            status: Mutex::new(VoiceStatus {
                phase: "idle".into(),
                ..Default::default()
            }),
            kws_status: Mutex::new("off".into()),
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
        let mut status = self.inner.status.lock().unwrap().clone();
        status.kws_status = self.inner.kws_status.lock().unwrap().clone();
        status
    }

    pub fn configure(&self, settings: VoiceSettings) {
        self.stop();
        *self.inner.settings.lock().unwrap() = settings;
        self.inner.revision.fetch_add(1, Ordering::AcqRel);
    }

    pub fn set_language(&self, language: UiLanguage) {
        *self.inner.language.lock().unwrap() = language;
    }

    /// Starts a conversation without a prerecorded greeting.
    pub fn click(&self) {
        self.request(Vec::new(), true);
    }

    /// Only a head hit uses the prerecorded wake greeting.
    pub fn head_click(&self) {
        let settings = self.settings();
        let clips = settings
            .wake_greeting_enabled
            .then(|| cue_path(&settings, *self.inner.language.lock().unwrap(), Cue::Wake))
            .flatten()
            .into_iter()
            .collect();
        self.request(clips, true);
    }

    pub fn cue_if_idle(&self, cue: Cue) -> bool {
        self.cues_if_idle(&[cue])
    }

    pub fn cues_if_idle(&self, cues: &[Cue]) -> bool {
        let settings = self.settings();
        if !settings.enabled || settings.greeting_dir.as_os_str().is_empty() {
            return false;
        }
        let language = *self.inner.language.lock().unwrap();
        let clips: Vec<_> = cues
            .iter()
            .filter_map(|cue| cue_path(&settings, language, *cue))
            .collect();
        if clips.len() != cues.len() || clips.is_empty() {
            return false;
        }
        let phase = self.status().phase;
        if self.inner.active.load(Ordering::Acquire)
            || !matches!(phase.as_str(), "idle" | "faulted")
        {
            return false;
        }
        self.request(clips, false);
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

    fn request(&self, clips: Vec<PathBuf>, conversation: bool) {
        if !self.settings().enabled || self.inner.stopped.load(Ordering::Acquire) {
            return;
        }
        let session_id = self.inner.generation.fetch_add(1, Ordering::AcqRel) + 1;
        *self.inner.last_interaction.lock().unwrap() = std::time::Instant::now();
        self.inner.rest_claimed.store(false, Ordering::Release);
        *self.inner.requested.lock().unwrap() = Some(VoiceRequest {
            session_id,
            clips,
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
            if settings.kws_enabled && !settings.enabled {
                *inner.kws_status.lock().unwrap() = "voice_off".into();
            } else if !settings.kws_enabled {
                *inner.kws_status.lock().unwrap() = "off".into();
            } else if inner.active.load(Ordering::Acquire) {
                *inner.kws_status.lock().unwrap() = "paused".into();
            }
            std::thread::sleep(Duration::from_millis(150));
            continue;
        }
        let revision = inner.revision.load(Ordering::Acquire);
        *inner.kws_status.lock().unwrap() = "loading".into();
        let announced = AtomicBool::new(false);
        let result = listen_for_keyword(&settings, || {
            if !announced.swap(true, Ordering::AcqRel) {
                *inner.kws_status.lock().unwrap() = "listening".into();
            }
            !inner.stopped.load(Ordering::Acquire)
                && inner.revision.load(Ordering::Acquire) == revision
                && !inner.active.load(Ordering::Acquire)
                && matches!(controller.status().phase.as_str(), "idle" | "faulted")
        });
        match result {
            Ok(Some(keyword)) => {
                *inner.kws_status.lock().unwrap() = format!("matched:{keyword}");
                controller.head_click();
            }
            Ok(None) => {}
            Err(error) => {
                *inner.kws_status.lock().unwrap() = format!("error:{error:#}");
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
        let mut clip_failed = false;
        for clip in request.clips {
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
                clip_failed = true;
                break;
            }
        }
        if clip_failed || !session.is_current() {
            continue;
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
