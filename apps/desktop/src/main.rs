mod library;
mod voice;
use anyhow::{Context, Result};
use chrono::{Local, Timelike};
use pet_core::{
    Activity, Effect, Event, Intent, PetCore,
    care::{CareAction, Needs},
    touch::{PastTouch, TouchDecision, TouchHistory, select_touch_response},
};
use pet_ipc::supervisor::{Host, RestartBudget};
use pet_persistence::{CareOutcome, CareStore, StoreError, TouchOutcome};
use pet_protocol::{
    AvatarCommand, AvatarEvent, BaselineExpression, DesktopCommand, DesktopEvent, HitDetail,
    HitRegion,
};
use pet_voice_session::{TtsBackend, VoiceSettings, VoiceStatus};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{
    Manager,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

#[derive(Clone, Serialize)]
struct Status {
    phase: String,
    detail: String,
    host_pid: Option<u32>,
    visible: bool,
}
#[derive(Clone, Serialize)]
struct SelectionStatus {
    id: u64,
    phase: String,
    requested: PathBuf,
    error: Option<String>,
}
#[derive(Clone, Serialize)]
struct TouchView {
    event_id: u64,
    outcome: TouchOutcome,
    expressions: Vec<String>,
}
struct TouchBinding {
    manifest: avatar_pack::Manifest,
    host_session: String,
    variation_seed: u64,
}
impl TouchBinding {
    fn for_host(host: &Host, path: &std::path::Path) -> Result<Option<Self>> {
        if !host.avatar_capabilities.touch_reactions {
            return Ok(None);
        }
        let manifest = avatar_pack::read_json::<avatar_pack::Manifest>(path)?;
        anyhow::ensure!(
            !manifest.touch_reactions.is_empty(),
            "角色宿主声明了部位反馈，但角色包未提供绑定"
        );
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Ok(Some(Self {
            manifest,
            host_session: format!("{}-{}-{}", std::process::id(), nonce, host.id()),
            variation_seed: (nonce as u64) ^ ((nonce >> 64) as u64) ^ u64::from(host.id()),
        }))
    }
}
#[derive(Serialize)]
struct TouchPreview {
    decision: TouchDecision,
    expressions: Vec<String>,
}
struct Shared {
    care: Mutex<Option<CareStore>>,
    care_clock: CareClock,
    care_events: mpsc::SyncSender<CareSignal>,
    companion: Mutex<CompanionSettings>,
    companion_revision: AtomicU64,
    activity_stop_revision: AtomicU64,
    activity: Mutex<Activity>,
    library: Mutex<Option<PathBuf>>,
    requested: Mutex<Option<PathBuf>>,
    active: Mutex<Option<PathBuf>>,
    selection: AtomicU64,
    selection_status: Mutex<Option<SelectionStatus>>,
    preview_command: Mutex<Option<(PathBuf, AvatarCommand)>>,
    baseline: Mutex<BaselineExpression>,
    last_hit: Mutex<Option<HitDetail>>,
    last_touch: Mutex<Option<TouchView>>,
    voice: Mutex<Option<voice::VoiceController>>,
    ui_language: Mutex<voice::UiLanguage>,
    tray_items: Mutex<Option<[MenuItem<tauri::Wry>; 8]>>,
    feed_voice_sequence: AtomicU64,
    scale: AtomicU16,
    gaze_radius: AtomicU16,
    perch: AtomicU16,
    perch_overrides: Mutex<BTreeMap<String, u16>>,
    preferences_write: Mutex<()>,
    importing: AtomicBool,
    import_error: Mutex<Option<String>>,
    external_requested: AtomicBool,
    external_revision: AtomicU64,
    external_error: Mutex<Option<String>>,
    // Emergency controls never wait for PetCore or an ordinary command queue.
    visible: AtomicBool,
    revision: AtomicU64,
    retry: AtomicU64,
    stop: AtomicBool,
    done: AtomicBool,
    wake: mpsc::SyncSender<()>,
    status: Mutex<Status>,
}
#[derive(Clone, Copy)]
struct CareSignal {
    action: CareAction,
    needs: Needs,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompanionSettings {
    enabled: bool,
    do_not_disturb: bool,
    #[serde(default)]
    screen_play_enabled: bool,
    interval_minutes: u16,
    hourly_limit: u8,
}
impl Default for CompanionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            do_not_disturb: false,
            screen_play_enabled: false,
            interval_minutes: 15,
            hourly_limit: 2,
        }
    }
}
impl CompanionSettings {
    fn valid(self) -> bool {
        (1..=60).contains(&self.interval_minutes) && (1..=4).contains(&self.hourly_limit)
    }
}
struct CareClock {
    origin_utc_ms: i64,
    origin: Instant,
}
impl CareClock {
    fn new() -> Self {
        Self {
            origin_utc_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(i64::MAX as u128) as i64,
            origin: Instant::now(),
        }
    }
    fn now_ms(&self) -> i64 {
        self.origin_utc_ms
            .saturating_add(self.origin.elapsed().as_millis().min(i64::MAX as u128) as i64)
    }
    fn elapsed_ms(&self) -> u64 {
        self.origin.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
}
fn care_error(error: StoreError) -> String {
    match error {
        StoreError::Rules(pet_core::care::CareError::NoFood) => "食物已用完".into(),
        StoreError::Rules(pet_core::care::CareError::TooTired) => "精力不足，先休息一下".into(),
        StoreError::Rules(pet_core::care::CareError::Cooldown) => "刚玩过，请稍后再玩".into(),
        other => other.to_string(),
    }
}
#[tauri::command]
fn care_status(state: tauri::State<'_, Arc<Shared>>) -> Result<pet_core::care::CareState, String> {
    let mut guard = state
        .care
        .lock()
        .map_err(|_| "养成存档不可用".to_string())?;
    guard
        .as_mut()
        .ok_or("养成存档尚未就绪".to_string())?
        .load_at(state.care_clock.now_ms())
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn care_action(
    request_id: String,
    action: CareAction,
    state: tauri::State<'_, Arc<Shared>>,
) -> Result<CareOutcome, String> {
    let mut guard = state
        .care
        .lock()
        .map_err(|_| "养成存档不可用".to_string())?;
    let outcome = guard
        .as_mut()
        .ok_or("养成存档尚未就绪".to_string())?
        .apply(&request_id, action, state.care_clock.now_ms())
        .map_err(care_error)?;
    drop(guard);
    if !outcome.replayed {
        let _ = state.care_events.try_send(CareSignal {
            action,
            needs: outcome.state.needs,
        });
        let _ = state.wake.try_send(());
    }
    Ok(outcome)
}
#[tauri::command]
fn ui_language(state: tauri::State<'_, Arc<Shared>>) -> voice::UiLanguage {
    *state.ui_language.lock().unwrap()
}
#[tauri::command]
fn set_ui_language(
    language: voice::UiLanguage,
    state: tauri::State<'_, Arc<Shared>>,
) -> Result<(), String> {
    let serialized = serde_json::to_string(&language).map_err(|error| error.to_string())?;
    let mut care = state
        .care
        .lock()
        .map_err(|_| "设置存档不可用".to_string())?;
    care.as_mut()
        .ok_or("设置存档尚未就绪".to_string())?
        .set_setting("ui_language", &serialized)
        .map_err(|error| error.to_string())?;
    drop(care);
    *state.ui_language.lock().unwrap() = language;
    if let Some(controller) = state.voice.lock().unwrap().as_ref() {
        controller.set_language(language);
    }
    if let Some(items) = state.tray_items.lock().unwrap().as_ref() {
        for (item, label) in items.iter().zip(tray_labels(language)) {
            let _ = item.set_text(label);
        }
    }
    Ok(())
}
fn tray_labels(language: voice::UiLanguage) -> [&'static str; 8] {
    match language {
        voice::UiLanguage::Zh => [
            "显示角色",
            "隐藏角色",
            "角色与设置…",
            "重试连接",
            "停止当前互动",
            "开启免打扰",
            "关闭免打扰",
            "退出 DesktopPet",
        ],
        voice::UiLanguage::En => [
            "Show character",
            "Hide character",
            "Character and settings…",
            "Reconnect",
            "Stop interaction",
            "Enable Do Not Disturb",
            "Disable Do Not Disturb",
            "Quit DesktopPet",
        ],
        voice::UiLanguage::Ja => [
            "キャラクターを表示",
            "キャラクターを非表示",
            "キャラクターと設定…",
            "再接続",
            "交流を停止",
            "おやすみモードをオン",
            "おやすみモードをオフ",
            "DesktopPet を終了",
        ],
        voice::UiLanguage::Ko => [
            "캐릭터 표시",
            "캐릭터 숨기기",
            "캐릭터와 설정…",
            "다시 연결",
            "상호작용 중지",
            "방해 금지 켜기",
            "방해 금지 끄기",
            "DesktopPet 종료",
        ],
    }
}
#[tauri::command]
fn companion_settings(state: tauri::State<'_, Arc<Shared>>) -> CompanionSettings {
    *state.companion.lock().unwrap()
}
#[tauri::command]
fn set_companion_settings(
    settings: CompanionSettings,
    state: tauri::State<'_, Arc<Shared>>,
) -> Result<(), String> {
    state.save_companion(settings)
}
#[tauri::command]
fn stop_activity(state: tauri::State<'_, Arc<Shared>>) {
    state.activity_stop_revision.fetch_add(1, Ordering::AcqRel);
    let _ = state.wake.try_send(());
}
#[tauri::command]
fn companion_activity(state: tauri::State<'_, Arc<Shared>>) -> String {
    format!("{:?}", *state.activity.lock().unwrap())
}
impl Shared {
    fn save_companion(&self, settings: CompanionSettings) -> Result<(), String> {
        if !settings.valid() {
            return Err("主动陪伴配置超出范围".into());
        }
        let serialized = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
        let mut care = self.care.lock().map_err(|_| "养成存档不可用".to_string())?;
        care.as_mut()
            .ok_or("养成存档尚未就绪".to_string())?
            .set_setting("companion", &serialized)
            .map_err(|e| e.to_string())?;
        drop(care);
        *self.companion.lock().unwrap() = settings;
        self.companion_revision.fetch_add(1, Ordering::AcqRel);
        let _ = self.wake.try_send(());
        Ok(())
    }
    fn report(&self, phase: &str, detail: impl Into<String>, pid: Option<u32>) {
        let status = Status {
            phase: phase.into(),
            detail: detail.into(),
            host_pid: pid,
            visible: self.visible.load(Ordering::Acquire),
        };
        pet_ipc::event_log!(
            "{}",
            serde_json::json!({"event":"app_status","status":status})
        );
        *self.status.lock().unwrap() = status;
    }
    fn finish_selection(&self, id: u64, phase: &str, error: Option<String>) {
        let mut status = self.selection_status.lock().unwrap();
        if let Some(current) = status.as_mut()
            && current.id == id
        {
            current.phase = phase.into();
            current.error = error;
        }
    }
    fn request(&self, action: &str) -> Result<(), String> {
        match action {
            "show" | "hide" => {
                self.visible.store(action == "show", Ordering::Release);
                self.revision.fetch_add(1, Ordering::AcqRel);
            }
            "retry" => {
                self.retry.fetch_add(1, Ordering::AcqRel);
            }
            "quit" => {
                self.stop.store(true, Ordering::Release);
                if let Some(voice) = self.voice.lock().unwrap().as_ref() {
                    voice.shutdown();
                }
            }
            "stop_activity" => {
                self.activity_stop_revision.fetch_add(1, Ordering::AcqRel);
            }
            _ => return Err("未知操作".into()),
        }
        let _ = self.wake.try_send(()); // Latest values survive a full wake queue.
        Ok(())
    }
}
#[tauri::command]
fn status(state: tauri::State<'_, Arc<Shared>>) -> Status {
    let mut status = state.status.lock().unwrap().clone();
    status.visible = state.visible.load(Ordering::Acquire);
    status
}
#[tauri::command]
fn voice_settings(state: tauri::State<'_, Arc<Shared>>) -> Result<VoiceSettings, String> {
    state
        .voice
        .lock()
        .unwrap()
        .as_ref()
        .map(|voice| voice.settings())
        .ok_or("语音尚未就绪".into())
}
#[tauri::command]
fn set_voice_settings(
    settings: VoiceSettings,
    state: tauri::State<'_, Arc<Shared>>,
) -> Result<(), String> {
    settings.validate().map_err(|error| error.to_string())?;
    if settings.enabled
        && settings.tts_backend == TtsBackend::SherpaOnnx
        && !settings.reference_audio.is_file()
    {
        return Err(format!(
            "克隆参考 WAV 不存在：{}",
            settings.reference_audio.display()
        ));
    }
    let serialized = serde_json::to_string(&settings).map_err(|error| error.to_string())?;
    let mut care = state
        .care
        .lock()
        .map_err(|_| "设置存档不可用".to_string())?;
    care.as_mut()
        .ok_or("设置存档尚未就绪".to_string())?
        .set_setting("voice", &serialized)
        .map_err(|error| error.to_string())?;
    drop(care);
    state
        .voice
        .lock()
        .unwrap()
        .as_ref()
        .ok_or("语音尚未就绪".to_string())?
        .configure(settings);
    Ok(())
}
#[tauri::command]
fn voice_status(state: tauri::State<'_, Arc<Shared>>) -> VoiceStatus {
    state
        .voice
        .lock()
        .unwrap()
        .as_ref()
        .map(|voice| voice.status())
        .unwrap_or_default()
}
#[tauri::command]
fn voice_start(state: tauri::State<'_, Arc<Shared>>) {
    if let Some(voice) = state.voice.lock().unwrap().as_ref() {
        voice.click();
    }
}
#[tauri::command]
fn voice_stop(state: tauri::State<'_, Arc<Shared>>) {
    if let Some(voice) = state.voice.lock().unwrap().as_ref() {
        voice.stop();
    }
}

fn maybe_voice_greetings(shared: &Shared) {
    if !shared.visible.load(Ordering::Acquire) {
        return;
    }
    let voice_guard = shared.voice.lock().unwrap();
    let Some(voice) = voice_guard.as_ref() else {
        return;
    };
    let settings = voice.settings();
    if !settings.enabled {
        return;
    }
    let mut care_guard = shared.care.lock().unwrap();
    let Some(care) = care_guard.as_mut() else {
        return;
    };
    let now = Local::now();
    let today = now.format("%Y-%m-%d").to_string();
    if settings.birthday == now.format("%m-%d").to_string()
        && care
            .setting("voice_last_birthday")
            .ok()
            .flatten()
            .as_deref()
            != Some(&today)
        && voice.cue_if_idle(voice::Cue::Birthday)
    {
        let _ = care.set_setting("voice_last_birthday", &today);
        return;
    }
    if settings.first_greeting_enabled
        && care
            .setting("voice_first_greeting_done")
            .ok()
            .flatten()
            .as_deref()
            != Some("1")
    {
        if voice.cue_if_idle(voice::Cue::FirstMeeting) {
            let _ = care.set_setting("voice_first_greeting_done", "1");
            return;
        }
    }
    let level = care
        .load_at(shared.care_clock.now_ms())
        .ok()
        .map(|state| state.needs.intimacy)
        .unwrap_or(0);
    let milestone = [
        (100, voice::Cue::IntimacyBlessing),
        (75, voice::Cue::IntimacyFeeling),
        (50, voice::Cue::IntimacyOpen),
        (25, voice::Cue::IntimacySmart),
    ]
    .into_iter()
    .find(|(threshold, _)| level >= *threshold);
    let announced = care
        .setting("voice_intimacy_announced")
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(0);
    if let Some((threshold, cue)) = milestone
        && threshold > announced
        && voice.cue_if_idle(cue)
    {
        let _ = care.set_setting("voice_intimacy_announced", &threshold.to_string());
        return;
    }
    if !settings.timed_greetings_enabled {
        return;
    }
    let cue = match now.hour() {
        8 => voice::Cue::Morning,
        12 => voice::Cue::Noon,
        19 => voice::Cue::Evening,
        22 => voice::Cue::Night,
        _ => return,
    };
    let slot = now.format("%Y-%m-%d-%H").to_string();
    if care
        .setting("voice_last_timed_greeting")
        .ok()
        .flatten()
        .as_deref()
        != Some(&slot)
        && voice.cue_if_idle(cue)
    {
        let _ = care.set_setting("voice_last_timed_greeting", &slot);
    }
}

fn maybe_voice_rest(shared: &Shared) {
    let guard = shared.voice.lock().unwrap();
    let Some(voice) = guard.as_ref() else { return };
    if !voice.claim_rest_due() {
        return;
    }
    let mut care_guard = shared.care.lock().unwrap();
    let Some(care) = care_guard.as_mut() else {
        return;
    };
    let request_id = format!("voice-idle-rest-{}", shared.care_clock.now_ms());
    if let Ok(outcome) = care.apply(&request_id, CareAction::Rest, shared.care_clock.now_ms()) {
        let _ = shared.care_events.try_send(CareSignal {
            action: CareAction::Rest,
            needs: outcome.state.needs,
        });
        let _ = shared.wake.try_send(());
    }
}
#[derive(Serialize)]
struct ExpressionItem {
    id: String,
    label: String,
}
#[tauri::command]
fn expression_catalog(state: tauri::State<'_, Arc<Shared>>) -> Result<Vec<ExpressionItem>, String> {
    let Some(path) = state.active.lock().unwrap().clone() else {
        return Ok(Vec::new());
    };
    if path.file_name().is_none_or(|name| name != "manifest.json") {
        return Ok(Vec::new());
    }
    let manifest =
        avatar_pack::read_json::<avatar_pack::Manifest>(&path).map_err(|e| e.to_string())?;
    Ok(manifest
        .expression_profile
        .map(|p| {
            p.catalog
                .into_iter()
                .map(|(id, asset)| ExpressionItem {
                    id,
                    label: asset.label,
                })
                .collect()
        })
        .unwrap_or_default())
}
#[tauri::command]
fn expression_baseline(state: tauri::State<'_, Arc<Shared>>) -> BaselineExpression {
    *state.baseline.lock().unwrap()
}
#[tauri::command]
fn last_hit(state: tauri::State<'_, Arc<Shared>>) -> Option<HitDetail> {
    *state.last_hit.lock().unwrap()
}
#[tauri::command]
fn last_touch(state: tauri::State<'_, Arc<Shared>>) -> Option<TouchView> {
    state.last_touch.lock().unwrap().clone()
}
#[tauri::command]
fn preview_touch(
    region: HitRegion,
    intimacy: u8,
    prior_count: u8,
    state: tauri::State<'_, Arc<Shared>>,
) -> Result<TouchPreview, String> {
    if ![0, 30, 70].contains(&intimacy) || prior_count > 2 {
        return Err("预览参数超出范围".into());
    }
    let path = state.active.lock().unwrap().clone().ok_or("请先加载角色")?;
    let manifest =
        avatar_pack::read_json::<avatar_pack::Manifest>(&path).map_err(|e| e.to_string())?;
    if manifest.touch_reactions.is_empty() {
        return Err("当前角色包没有部位互动预览".into());
    }
    let now = state.care_clock.now_ms();
    let mut history = TouchHistory::default();
    if matches!(region, HitRegion::UpperBody | HitRegion::LowerBody) {
        for index in 0..prior_count {
            history.recent.push(PastTouch {
                region,
                occurred_ms: now - 20_000 + i64::from(index) * 10_000,
                rule_id: if index == 0 {
                    "boundary_first"
                } else {
                    "boundary_second"
                }
                .into(),
                intimacy_delta: if index == 0 { -5 } else { -15 },
            });
        }
    }
    let mut needs = Needs {
        intimacy,
        ..Needs::default()
    };
    let mut decision = select_touch_response(region, needs, &history, now);
    decision.apply_intimacy(&mut needs);
    let chosen_cue = decision.expression_cue.map(|cue| {
        let entropy = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        manifest.choose_touch_cue(region, cue, entropy)
    });
    let expressions = chosen_cue
        .and_then(|cue| manifest.touch_reactions.get(&cue))
        .map(|stages| {
            stages
                .iter()
                .map(|stage| stage.expression.clone())
                .collect()
        })
        .unwrap_or_default();
    if let Some(cue) = chosen_cue {
        *state.preview_command.lock().unwrap() = Some((path, AvatarCommand::PlayTouchCue(cue)));
        let _ = state.wake.try_send(());
    }
    Ok(TouchPreview {
        decision,
        expressions,
    })
}
#[tauri::command]
fn available_hit_regions(state: tauri::State<'_, Arc<Shared>>) -> Vec<HitRegion> {
    let Some(path) = state.active.lock().unwrap().clone() else {
        return Vec::new();
    };
    let Ok(manifest) = avatar_pack::read_json::<avatar_pack::Manifest>(&path) else {
        return Vec::new();
    };
    let mut regions = manifest
        .interaction
        .regions
        .keys()
        .copied()
        .collect::<Vec<_>>();
    regions.extend([HitRegion::Head, HitRegion::Body]);
    regions
}
#[tauri::command]
fn preview_expression(id: String, state: tauri::State<'_, Arc<Shared>>) -> Result<(), String> {
    let path = state.active.lock().unwrap().clone().ok_or("请先加载角色")?;
    let manifest =
        avatar_pack::read_json::<avatar_pack::Manifest>(&path).map_err(|e| e.to_string())?;
    if !manifest
        .expression_profile
        .as_ref()
        .is_some_and(|p| p.catalog.contains_key(&id))
    {
        return Err("当前角色不包含该表情".into());
    }
    if !state.visible.load(Ordering::Acquire) || state.status.lock().unwrap().phase != "ready" {
        return Err("角色尚未显示或连接".into());
    }
    *state.preview_command.lock().unwrap() = Some((path, AvatarCommand::PreviewExpression(id)));
    let _ = state.wake.try_send(());
    Ok(())
}
#[tauri::command]
fn end_expression_preview(state: tauri::State<'_, Arc<Shared>>) {
    if let Some(path) = state.active.lock().unwrap().clone() {
        *state.preview_command.lock().unwrap() = Some((path, AvatarCommand::EndPreview));
    }
    let _ = state.wake.try_send(());
}
#[tauri::command]
fn control(action: String, state: tauri::State<'_, Arc<Shared>>) -> Result<(), String> {
    state.request(&action)
}
#[tauri::command]
fn set_external_snap(enabled: bool, state: tauri::State<'_, Arc<Shared>>) {
    state.external_requested.store(enabled, Ordering::Release);
    state.external_revision.fetch_add(1, Ordering::AcqRel);
    *state.external_error.lock().unwrap() = None;
    let _ = state.wake.try_send(());
}
fn apply(host: &mut Host, effects: Vec<Effect>) -> Result<()> {
    for effect in effects {
        match effect {
            Effect::Desktop(command) => host.desktop(command)?,
            Effect::Avatar(command) => host.avatar(command)?,
        }
    }
    Ok(())
}
fn monitor(
    shared: &Shared,
    wake: mpsc::Receiver<()>,
    care_events: mpsc::Receiver<CareSignal>,
    executable: PathBuf,
    model: Option<PathBuf>,
) {
    let mut model = shared.requested.lock().unwrap().clone().or(model);
    let mut selection_seen = shared.selection.load(Ordering::Acquire);
    let mut core = PetCore::default();
    let mut current_needs = shared
        .care
        .lock()
        .unwrap()
        .as_mut()
        .and_then(|care| care.load_at(shared.care_clock.now_ms()).ok())
        .map(|state| state.needs)
        .unwrap_or_default();
    let mut next_needs_refresh = Instant::now() + Duration::from_secs(60);
    let mut companion_seen = shared.companion_revision.load(Ordering::Acquire);
    let mut stop_activity_seen = shared.activity_stop_revision.load(Ordering::Acquire);
    let settings = *shared.companion.lock().unwrap();
    core.set_companion_limits(settings.interval_minutes, settings.hourly_limit);
    core.update(Event::Intent(Intent::SetCompanionEnabled(settings.enabled)));
    core.update(Event::Intent(Intent::SetDoNotDisturb(
        settings.do_not_disturb,
    )));
    core.update(Event::Intent(Intent::SetScreenPlayEnabled(
        settings.screen_play_enabled,
    )));
    let mut budget = RestartBudget::new(Duration::from_millis(250));
    let mut retry_seen = shared.retry.load(Ordering::Acquire);
    let mut blocked = false;
    loop {
        if shared.stop.load(Ordering::Acquire) {
            return;
        }
        if shared.selection.load(Ordering::Acquire) != selection_seen {
            selection_seen = shared.selection.load(Ordering::Acquire);
            model = shared.requested.lock().unwrap().clone();
            budget = RestartBudget::new(Duration::from_millis(250));
            blocked = false;
        }
        if blocked {
            let retry = shared.retry.load(Ordering::Acquire);
            if retry == retry_seen {
                let _ = wake.recv_timeout(Duration::from_secs(1));
                continue;
            }
            retry_seen = retry;
            budget = RestartBudget::new(Duration::from_millis(250));
            blocked = false;
        }
        // Preserve requested visibility before every new host handshake. The
        // host starts hidden, preventing a hidden pet flashing during recovery.
        core.update(Event::Intent(Intent::SetVisible(
            shared.visible.load(Ordering::Acquire),
        )));
        shared.report("connecting", "正在连接角色…", None);
        let result = (|| -> Result<()> {
            let selected_model = model.as_ref().context(
                "未配置角色。请设置 DESKTOPPET_MODEL 为本地 model3.json 路径后重新启动应用。",
            )?;
            anyhow::ensure!(
                selected_model.is_file(),
                "角色文件不存在：{}",
                selected_model.display()
            );
            let mut command = Command::new(&executable);
            command.arg(selected_model);
            if let Some(directory) = shared.library.lock().unwrap().as_ref() {
                command.env("DESKTOPPET_LAYOUT", directory.join("placement.json"));
            }
            let mut host = Host::start(&mut command, Duration::from_secs(5))?;
            let mut touch_binding = TouchBinding::for_host(&host, selected_model)?;
            // Ordinary requests use a shorter bound after renderer startup.
            host.set_timeout(Duration::from_secs(2))?;
            shared.perch.store(
                library::perch_for(shared, selected_model),
                Ordering::Release,
            );
            host.desktop(DesktopCommand::SetScale(
                shared.scale.load(Ordering::Acquire),
            ))?;
            host.desktop(DesktopCommand::SetWindowPerch(
                shared.perch.load(Ordering::Acquire),
            ))?;
            host.desktop(DesktopCommand::SetGazeRadius(
                shared.gaze_radius.load(Ordering::Acquire),
            ))?;
            let capabilities = host.avatar_capabilities;
            core.update(Event::Desktop(DesktopEvent::Stopped));
            core.update(Event::Intent(Intent::SetVisible(
                shared.visible.load(Ordering::Acquire),
            )));
            apply(
                &mut host,
                core.update(Event::Avatar(AvatarEvent::Ready(capabilities))),
            )?;
            shared.report(
                "ready",
                "角色已连接。可从菜单栏显示、隐藏或退出。",
                Some(host.id()),
            );
            library::save_selection(shared, selected_model);
            *shared.active.lock().unwrap() = Some(selected_model.clone());
            *shared.last_hit.lock().unwrap() = None;
            *shared.last_touch.lock().unwrap() = None;
            shared.finish_selection(selection_seen, "succeeded", None);
            let mut applied_scale = shared.scale.load(Ordering::Acquire);
            let mut applied_perch = shared.perch.load(Ordering::Acquire);
            let mut applied_gaze_radius = shared.gaze_radius.load(Ordering::Acquire);
            let mut external_seen = 0;
            let mut revision = shared.revision.load(Ordering::Acquire);
            let mut applied_visible = core.state().visible;
            let mut next_ping = Instant::now() + Duration::from_secs(1);
            let mut next_voice_greeting_check = Instant::now();
            loop {
                if shared.stop.load(Ordering::Acquire) {
                    // Direct lifecycle route, independent of core effects.
                    return host.shutdown();
                }
                let companion_revision = shared.companion_revision.load(Ordering::Acquire);
                if companion_revision != companion_seen {
                    let settings = *shared.companion.lock().unwrap();
                    core.set_companion_limits(settings.interval_minutes, settings.hourly_limit);
                    apply(
                        &mut host,
                        core.update(Event::Intent(Intent::SetCompanionEnabled(settings.enabled))),
                    )?;
                    apply(
                        &mut host,
                        core.update(Event::Intent(Intent::SetDoNotDisturb(
                            settings.do_not_disturb,
                        ))),
                    )?;
                    apply(
                        &mut host,
                        core.update(Event::Intent(Intent::SetScreenPlayEnabled(
                            settings.screen_play_enabled,
                        ))),
                    )?;
                    companion_seen = companion_revision;
                }
                let selection = shared.selection.load(Ordering::Acquire);
                if selection != selection_seen {
                    selection_seen = selection;
                    let requested = shared
                        .selection_status
                        .lock()
                        .unwrap()
                        .as_ref()
                        .filter(|status| status.id == selection)
                        .map(|status| status.requested.clone());
                    if let Some(path) = requested {
                        let candidate = (|| -> Result<(Host, Option<TouchBinding>)> {
                            let mut command = Command::new(&executable);
                            command.arg(&path);
                            if let Some(directory) = shared.library.lock().unwrap().as_ref() {
                                command.env("DESKTOPPET_LAYOUT", directory.join("placement.json"));
                            }
                            let mut candidate = Host::start(&mut command, Duration::from_secs(5))?;
                            candidate.set_timeout(Duration::from_secs(2))?;
                            candidate.desktop(DesktopCommand::SetScale(
                                shared.scale.load(Ordering::Acquire),
                            ))?;
                            candidate.desktop(DesktopCommand::SetWindowPerch(
                                library::perch_for(shared, &path),
                            ))?;
                            candidate.desktop(DesktopCommand::SetGazeRadius(
                                shared.gaze_radius.load(Ordering::Acquire),
                            ))?;
                            let touch_binding = TouchBinding::for_host(&candidate, &path)?;
                            Ok((candidate, touch_binding))
                        })();
                        match candidate {
                            Ok((candidate, candidate_touch_binding)) => {
                                if shared.stop.load(Ordering::Acquire) {
                                    drop(candidate);
                                    return host.shutdown();
                                }
                                let old = std::mem::replace(&mut host, candidate);
                                let _ = old.shutdown();
                                touch_binding = candidate_touch_binding;
                                model = Some(path.clone());
                                *shared.active.lock().unwrap() = Some(path.clone());
                                *shared.last_hit.lock().unwrap() = None;
                                *shared.last_touch.lock().unwrap() = None;
                                shared
                                    .perch
                                    .store(library::perch_for(shared, &path), Ordering::Release);
                                library::save_selection(shared, &path);
                                core.update(Event::Desktop(DesktopEvent::Stopped));
                                core.update(Event::Intent(Intent::SetVisible(
                                    shared.visible.load(Ordering::Acquire),
                                )));
                                let capabilities = host.avatar_capabilities;
                                apply(
                                    &mut host,
                                    core.update(Event::Avatar(AvatarEvent::Ready(capabilities))),
                                )?;
                                applied_visible = core.state().visible;
                                applied_scale = shared.scale.load(Ordering::Acquire);
                                applied_perch = shared.perch.load(Ordering::Acquire);
                                applied_gaze_radius = shared.gaze_radius.load(Ordering::Acquire);
                                external_seen = 0;
                                *shared.import_error.lock().unwrap() = None;
                                shared.finish_selection(selection, "succeeded", None);
                                shared.report("ready", "角色切换完成", Some(host.id()));
                            }
                            Err(error) => {
                                let message = format!("候选角色加载失败，保留当前角色：{error:#}");
                                *shared.import_error.lock().unwrap() = Some(message.clone());
                                shared.finish_selection(selection, "failed", Some(message));
                            }
                        }
                    }
                }
                let scale = shared.scale.load(Ordering::Acquire);
                if scale != applied_scale {
                    host.desktop(DesktopCommand::SetScale(scale))?;
                    applied_scale = scale;
                    if let Some(path) = &model {
                        library::save_selection(shared, path);
                    }
                }
                let perch = shared.perch.load(Ordering::Acquire);
                if perch != applied_perch {
                    host.desktop(DesktopCommand::SetWindowPerch(perch))?;
                    applied_perch = perch;
                }
                let gaze_radius = shared.gaze_radius.load(Ordering::Acquire);
                if gaze_radius != applied_gaze_radius {
                    host.desktop(DesktopCommand::SetGazeRadius(gaze_radius))?;
                    applied_gaze_radius = gaze_radius;
                }
                let new_revision = shared.revision.load(Ordering::Acquire);
                let visible = shared.visible.load(Ordering::Acquire);
                if new_revision != revision || visible != applied_visible {
                    host.desktop(DesktopCommand::SetVisible(visible))?;
                    core.update(Event::Intent(Intent::SetVisible(visible)));
                    revision = new_revision;
                    applied_visible = visible;
                    shared.report(
                        "ready",
                        if visible {
                            "角色已显示"
                        } else {
                            "角色已隐藏"
                        },
                        Some(host.id()),
                    );
                }
                let external_revision = shared.external_revision.load(Ordering::Acquire);
                if external_revision != external_seen {
                    let enabled = shared.external_requested.load(Ordering::Acquire);
                    match host.desktop(DesktopCommand::SetExternalSnapEnabled(enabled)) {
                        Ok(()) => *shared.external_error.lock().unwrap() = None,
                        Err(error) => {
                            shared.external_requested.store(false, Ordering::Release);
                            *shared.external_error.lock().unwrap() = Some(format!(
                                "他应用吸附未启用：{error:#}。检查辅助功能权限后可再次打开。"
                            ));
                        }
                    }
                    external_seen = external_revision;
                }
                while let Ok(signal) = care_events.try_recv() {
                    current_needs = signal.needs;
                    if shared.visible.load(Ordering::Acquire) {
                        apply(
                            &mut host,
                            core.update(Event::CareCommitted {
                                action: signal.action,
                                now_ms: shared.care_clock.elapsed_ms(),
                                needs: signal.needs,
                            }),
                        )?;
                    }
                    if signal.action == CareAction::Feed {
                        let cue =
                            if shared.feed_voice_sequence.fetch_add(1, Ordering::AcqRel) % 2 == 0 {
                                voice::Cue::FeedTaste
                            } else {
                                voice::Cue::FeedThought
                            };
                        if let Some(controller) = shared.voice.lock().unwrap().as_ref() {
                            controller.cue_if_idle(cue);
                        }
                    }
                }
                if let Some((path, command)) = shared.preview_command.lock().unwrap().take()
                    && model.as_ref() == Some(&path)
                {
                    host.avatar(command)?;
                }
                let stop_revision = shared.activity_stop_revision.load(Ordering::Acquire);
                if stop_revision != stop_activity_seen {
                    apply(&mut host, core.update(Event::Intent(Intent::StopActivity)))?;
                    stop_activity_seen = stop_revision;
                }
                if Instant::now() >= next_needs_refresh {
                    if let Some(care) = shared.care.lock().unwrap().as_mut()
                        && let Ok(state) = care.load_at(shared.care_clock.now_ms())
                    {
                        current_needs = state.needs;
                    }
                    next_needs_refresh = Instant::now() + Duration::from_secs(60);
                }
                if shared.visible.load(Ordering::Acquire) {
                    apply(
                        &mut host,
                        core.update(Event::Tick {
                            now_ms: shared.care_clock.elapsed_ms(),
                            needs: current_needs,
                        }),
                    )?;
                }
                if Instant::now() >= next_voice_greeting_check {
                    maybe_voice_greetings(shared);
                    maybe_voice_rest(shared);
                    next_voice_greeting_check = Instant::now() + Duration::from_secs(15);
                }
                {
                    let mut activity = shared.activity.lock().unwrap();
                    if *activity != Activity::Greeting
                        && core.state().activity == Activity::Greeting
                        && let Some(controller) = shared.voice.lock().unwrap().as_ref()
                    {
                        controller.cue_if_idle(voice::Cue::Greeting);
                    }
                    *activity = core.state().activity;
                }
                *shared.baseline.lock().unwrap() = core.state().baseline;
                // A wake-up rechecks emergency atomics immediately; timeout is
                // the heartbeat cadence, not a per-frame busy poll.
                if Instant::now() >= next_ping {
                    for event in host.poll()? {
                        if let AvatarEvent::Hit(hit) = event {
                            *shared.last_hit.lock().unwrap() = Some(hit);
                            if hit.region == HitRegion::Head
                                && let Some(voice) = shared.voice.lock().unwrap().as_ref()
                            {
                                voice.head_click();
                            }
                            if let Some(binding) = &touch_binding {
                                if let Some(care) = shared.care.lock().unwrap().as_mut() {
                                    match care.apply_touch(
                                        &binding.host_session,
                                        hit.event_id,
                                        &binding.manifest.id,
                                        hit.region,
                                        shared.care_clock.now_ms(),
                                    ) {
                                        Ok(outcome) => {
                                            current_needs = outcome.state.needs;
                                            let chosen_cue =
                                                outcome.decision.expression_cue.map(|cue| {
                                                    binding.manifest.choose_touch_cue(
                                                        hit.region,
                                                        cue,
                                                        binding.variation_seed ^ hit.event_id,
                                                    )
                                                });
                                            let expressions = chosen_cue
                                                .and_then(|cue| {
                                                    binding.manifest.touch_reactions.get(&cue)
                                                })
                                                .map(|stages| {
                                                    stages
                                                        .iter()
                                                        .map(|stage| stage.expression.clone())
                                                        .collect()
                                                })
                                                .unwrap_or_default();
                                            *shared.last_touch.lock().unwrap() = Some(TouchView {
                                                event_id: hit.event_id,
                                                outcome: outcome.clone(),
                                                expressions,
                                            });
                                            if !outcome.replayed {
                                                apply(
                                                    &mut host,
                                                    core.update(Event::Intent(
                                                        Intent::StopActivity,
                                                    )),
                                                )?;
                                                apply(
                                                    &mut host,
                                                    core.update(Event::Tick {
                                                        now_ms: shared.care_clock.elapsed_ms(),
                                                        needs: current_needs,
                                                    }),
                                                )?;
                                                if let Some(cue) = chosen_cue {
                                                    host.avatar(AvatarCommand::PlayTouchCue(cue))?;
                                                }
                                            }
                                        }
                                        Err(error) => pet_ipc::event_log!(
                                            "{}",
                                            serde_json::json!({"event":"touch_store_error","error":error.to_string()})
                                        ),
                                    }
                                }
                                continue;
                            }
                        }
                        apply(&mut host, core.update(Event::Avatar(event)))?;
                    }
                    next_ping = Instant::now()
                        + if applied_visible {
                            Duration::from_millis(100)
                        } else {
                            Duration::from_secs(1)
                        };
                }
                match wake.recv_timeout(next_ping.saturating_duration_since(Instant::now())) {
                    Ok(()) => {}
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return host.shutdown(),
                }
            }
        })();
        core.update(Event::Desktop(DesktopEvent::Stopped));
        if shared.stop.load(Ordering::Acquire) {
            return;
        }
        if let Err(error) = result {
            core.update(Event::Avatar(AvatarEvent::Fault(format!("{error:#}"))));
            *shared.last_hit.lock().unwrap() = None;
            *shared.last_touch.lock().unwrap() = None;
            if let Some(delay) = budget.failure(Instant::now()) {
                shared.report("recovering", format!("连接中断，正在重试：{error:#}"), None);
                // Visibility wakes must not skip the restart backoff.
                let deadline = Instant::now() + delay;
                while Instant::now() < deadline && !shared.stop.load(Ordering::Acquire) {
                    let _ = wake.recv_timeout(deadline.saturating_duration_since(Instant::now()));
                }
            } else {
                shared.finish_selection(
                    selection_seen,
                    "failed",
                    Some(format!("角色加载失败：{error:#}")),
                );
                shared.report(
                    "fault",
                    format!("已暂停自动重试：{error:#}。修复后点击“重试连接”。"),
                    None,
                );
                blocked = true;
                retry_seen = shared.retry.load(Ordering::Acquire);
            }
        }
    }
}
fn run() -> Result<()> {
    let macos = std::env::current_exe()?
        .parent()
        .context("missing executable directory")?
        .to_path_buf();
    let contents = macos.parent().context("missing app contents directory")?;
    let bundled_host =
        contents.join("Helpers/DesktopPet Avatar Host.app/Contents/MacOS/avatar-host-2d");
    let executable = if bundled_host.is_file() {
        bundled_host
    } else {
        macos.join("avatar-host-2d")
    };
    let model = match std::env::var_os("DESKTOPPET_MODEL") {
        Some(path) => Some(PathBuf::from(path)),
        None => {
            let config = contents.join("Resources/model-path.txt");
            if config.is_file() {
                Some(PathBuf::from(std::fs::read_to_string(config)?.trim()))
            } else {
                None
            }
        }
    };
    let (wake_tx, wake_rx) = mpsc::sync_channel(1);
    let (care_tx, care_rx) = mpsc::sync_channel(32);
    let shared = Arc::new(Shared {
        care: Mutex::new(None),
        care_clock: CareClock::new(),
        care_events: care_tx,
        companion: Mutex::new(CompanionSettings::default()),
        companion_revision: AtomicU64::new(0),
        activity_stop_revision: AtomicU64::new(0),
        activity: Mutex::new(Activity::Idle),
        library: Mutex::new(None),
        requested: Mutex::new(None),
        active: Mutex::new(None),
        selection: AtomicU64::new(0),
        selection_status: Mutex::new(None),
        preview_command: Mutex::new(None),
        baseline: Mutex::new(BaselineExpression::Neutral),
        last_hit: Mutex::new(None),
        last_touch: Mutex::new(None),
        voice: Mutex::new(None),
        ui_language: Mutex::new(voice::UiLanguage::default()),
        tray_items: Mutex::new(None),
        feed_voice_sequence: AtomicU64::new(0),
        scale: AtomicU16::new(100),
        gaze_radius: AtomicU16::new(400),
        perch: AtomicU16::new(50),
        perch_overrides: Mutex::new(BTreeMap::new()),
        preferences_write: Mutex::new(()),
        importing: AtomicBool::new(false),
        import_error: Mutex::new(None),
        external_requested: AtomicBool::new(false),
        external_revision: AtomicU64::new(0),
        external_error: Mutex::new(None),
        visible: AtomicBool::new(true),
        revision: AtomicU64::new(0),
        retry: AtomicU64::new(0),
        stop: AtomicBool::new(false),
        done: AtomicBool::new(false),
        wake: wake_tx,
        status: Mutex::new(Status {
            phase: "starting".into(),
            detail: "正在启动…".into(),
            host_pid: None,
            visible: true,
        }),
    });
    let setup_shared = shared.clone();
    let app = tauri::Builder::default()
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![
            status,
            control,
            library::packs,
            library::import_pack,
            library::select_pack,
            library::set_scale,
            library::set_gaze_radius,
            library::set_window_perch,
            set_external_snap,
            care_status,
            care_action,
            companion_settings,
            set_companion_settings,
            stop_activity,
            companion_activity,
            voice_settings,
            set_voice_settings,
            voice_status,
            ui_language,
            set_ui_language,
            voice_start,
            voice_stop,
            library::preferences,
            expression_catalog,
            expression_baseline,
            last_hit,
            last_touch,
            preview_touch,
            available_hit_regions,
            preview_expression,
            end_expression_preview
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| {
            let resource_dir = app.path().resource_dir()?;
            let dev_data_dir = resource_dir.join("dev-data-dir.txt");
            let directory = if dev_data_dir.is_file() {
                PathBuf::from(std::fs::read_to_string(dev_data_dir)?.trim())
            } else {
                app.path().app_data_dir()?
            };
            std::fs::create_dir_all(&directory)?;
            let care = CareStore::open(
                directory.join("care.sqlite3"),
                setup_shared.care_clock.now_ms(),
            )?;
            if let Some(saved) = care.setting("companion")?
                && let Ok(settings) = serde_json::from_str::<CompanionSettings>(&saved)
                && settings.valid()
            {
                *setup_shared.companion.lock().unwrap() = settings;
            }
            if let Some(saved) = care.setting("ui_language")?
                && let Ok(language) = serde_json::from_str::<voice::UiLanguage>(&saved)
            {
                *setup_shared.ui_language.lock().unwrap() = language;
            }
            let mut voice_settings = VoiceSettings::default();
            if let Ok(current) = std::env::current_dir() {
                let candidate = current.join("models/local");
                if candidate.join("silero_vad.onnx").is_file() {
                    voice_settings.model_dir = candidate;
                }
            }
            let dev_voice_config = resource_dir.join("voice-settings.json");
            if dev_voice_config.is_file()
                && let Ok(contents) = std::fs::read_to_string(dev_voice_config)
                && let Ok(settings) = serde_json::from_str::<VoiceSettings>(&contents)
                && settings.validate().is_ok()
            {
                voice_settings = settings;
            }
            if let Some(saved) = care.setting("voice")?
                && let Ok(settings) = serde_json::from_str::<VoiceSettings>(&saved)
                && settings.validate().is_ok()
            {
                voice_settings = settings;
            }
            if voice_settings.greeting_dir.ends_with("artifacts/wav")
                && let Some(project_root) = voice_settings
                    .greeting_dir
                    .parent()
                    .and_then(|path| path.parent())
                && project_root
                    .join("artifacts/voice/zh-CN/greetings/wake.wav")
                    .is_file()
            {
                voice_settings.greeting_dir = project_root.join("artifacts/voice");
            }
            if voice_settings.greeting_dir.as_os_str().is_empty()
                && let Ok(current) = std::env::current_dir()
            {
                if current
                    .join("artifacts/voice/zh-CN/greetings/wake.wav")
                    .is_file()
                {
                    voice_settings.greeting_dir = current.join("artifacts/voice");
                } else if current.join("artifacts/wav/心事.wav").is_file() {
                    voice_settings.greeting_dir = current.join("artifacts/wav");
                }
            }
            if voice_settings.reference_audio.as_os_str().is_empty()
                && !voice_settings.greeting_dir.as_os_str().is_empty()
            {
                let reference = voice_settings.greeting_dir.join("zh-CN/greetings/noon");
                let reference = if reference.with_extension("wav").is_file() {
                    reference
                } else {
                    voice_settings.greeting_dir.join("午休时间到")
                };
                voice_settings.reference_audio = reference.with_extension("wav");
                voice_settings.reference_text =
                    std::fs::read_to_string(reference.with_extension("txt")).unwrap_or_default();
            }
            let controller = voice::VoiceController::new(voice_settings);
            controller.set_language(*setup_shared.ui_language.lock().unwrap());
            *setup_shared.voice.lock().unwrap() = Some(controller);
            *setup_shared.care.lock().unwrap() = Some(care);
            *setup_shared.library.lock().unwrap() = Some(directory.clone());
            library::restore(&setup_shared, &directory, model.as_deref());
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let labels = tray_labels(*setup_shared.ui_language.lock().unwrap());
            let show = MenuItem::with_id(app, "show", labels[0], true, None::<&str>)?;
            let hide = MenuItem::with_id(app, "hide", labels[1], true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", labels[2], true, None::<&str>)?;
            let retry = MenuItem::with_id(app, "retry", labels[3], true, None::<&str>)?;
            let stop_activity =
                MenuItem::with_id(app, "stop_activity", labels[4], true, None::<&str>)?;
            let dnd_on = MenuItem::with_id(app, "dnd_on", labels[5], true, None::<&str>)?;
            let dnd_off = MenuItem::with_id(app, "dnd_off", labels[6], true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", labels[7], true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &show,
                    &hide,
                    &settings,
                    &retry,
                    &stop_activity,
                    &dnd_on,
                    &dnd_off,
                    &quit,
                ],
            )?;
            *setup_shared.tray_items.lock().unwrap() = Some([
                show,
                hide,
                settings,
                retry,
                stop_activity,
                dnd_on,
                dnd_off,
                quit,
            ]);
            let tray_icon = if cfg!(target_os = "macos") {
                tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?
            } else {
                tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))?
            };
            TrayIconBuilder::with_id("desktop-pet")
                .icon(tray_icon)
                .icon_as_template(cfg!(target_os = "macos"))
                .tooltip("DesktopPet")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "settings" {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    } else if event.id.as_ref() == "dnd_on" || event.id.as_ref() == "dnd_off" {
                        let shared = app.state::<Arc<Shared>>();
                        let mut settings = *shared.companion.lock().unwrap();
                        settings.do_not_disturb = event.id.as_ref() == "dnd_on";
                        let _ = shared.save_companion(settings);
                    } else {
                        let _ = app.state::<Arc<Shared>>().request(event.id.as_ref());
                    }
                })
                .build(app)?;
            if app
                .path()
                .resource_dir()?
                .join("show-settings-on-launch")
                .is_file()
                && let Some(window) = app.get_webview_window("main")
            {
                window.show()?;
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                // A panic must also release the host via Drop and end the app.
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    monitor(&setup_shared, wake_rx, care_rx, executable, model)
                }));
                setup_shared.done.store(true, Ordering::Release);
                handle.exit(if outcome.is_ok() { 0 } else { 1 });
            });
            Ok(())
        })
        .build(tauri::generate_context!())?;
    app.run_return(move |_, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event
            && !shared.done.load(Ordering::Acquire)
        {
            api.prevent_exit();
            let _ = shared.request("quit");
        }
    });
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("DesktopPet: {error:#}");
        std::process::exit(1);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn controls() -> (Arc<Shared>, mpsc::Receiver<()>) {
        let (tx, rx) = mpsc::sync_channel(1);
        let (care_tx, _care_rx) = mpsc::sync_channel(32);
        (
            Arc::new(Shared {
                care: Mutex::new(None),
                care_clock: CareClock::new(),
                care_events: care_tx,
                companion: Mutex::new(CompanionSettings::default()),
                companion_revision: AtomicU64::new(0),
                activity_stop_revision: AtomicU64::new(0),
                activity: Mutex::new(Activity::Idle),
                library: Mutex::new(None),
                requested: Mutex::new(None),
                active: Mutex::new(None),
                selection: AtomicU64::new(0),
                selection_status: Mutex::new(None),
                preview_command: Mutex::new(None),
                baseline: Mutex::new(BaselineExpression::Neutral),
                last_hit: Mutex::new(None),
                last_touch: Mutex::new(None),
                voice: Mutex::new(None),
                ui_language: Mutex::new(voice::UiLanguage::default()),
                tray_items: Mutex::new(None),
                feed_voice_sequence: AtomicU64::new(0),
                scale: AtomicU16::new(100),
                gaze_radius: AtomicU16::new(400),
                perch: AtomicU16::new(50),
                perch_overrides: Mutex::new(BTreeMap::new()),
                preferences_write: Mutex::new(()),
                importing: AtomicBool::new(false),
                import_error: Mutex::new(None),
                external_requested: AtomicBool::new(false),
                external_revision: AtomicU64::new(0),
                external_error: Mutex::new(None),
                visible: AtomicBool::new(true),
                revision: AtomicU64::new(0),
                retry: AtomicU64::new(0),
                stop: AtomicBool::new(false),
                done: AtomicBool::new(false),
                wake: tx,
                status: Mutex::new(Status {
                    phase: "starting".into(),
                    detail: String::new(),
                    host_pid: None,
                    visible: true,
                }),
            }),
            rx,
        )
    }
    fn wait_for(shared: &Shared, predicate: impl Fn(&Status) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            if predicate(&shared.status.lock().unwrap()) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "monitor did not reach expected status"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    struct Fixture(PathBuf);
    impl Fixture {
        fn new(mode: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "desktop-pet-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("host"), include_str!("../tests/fake_host.py")).unwrap();
            std::fs::set_permissions(path.join("host"), std::fs::Permissions::from_mode(0o700))
                .unwrap();
            std::fs::write(path.join("model"), mode).unwrap();
            Self(path)
        }
        fn start(
            &self,
            shared: Arc<Shared>,
            rx: mpsc::Receiver<()>,
        ) -> std::thread::JoinHandle<()> {
            let host = self.0.join("host");
            let model = self.0.join("model");
            std::thread::spawn(move || {
                monitor(&shared, rx, mpsc::sync_channel(1).1, host, Some(model))
            })
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn saturated_controls_keep_latest_hide_and_exit_cleans_up_child() {
        let fixture = Fixture::new("normal");
        let (shared, rx) = controls();
        let worker = fixture.start(shared.clone(), rx);
        wait_for(&shared, |s| s.phase == "ready");
        let pid = shared.status.lock().unwrap().host_pid.unwrap();
        for _ in 0..10_000 {
            shared.request("show").unwrap();
            shared.request("hide").unwrap();
        }
        wait_for(&shared, |s| s.detail == "角色已隐藏");
        shared.request("quit").unwrap();
        worker.join().unwrap();
        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "pid="])
            .output()
            .unwrap();
        assert!(output.stdout.is_empty());
    }
    #[test]
    fn crashes_exhaust_budget_but_manual_retry_recovers_and_preserves_hide() {
        let fixture = Fixture::new("crash");
        let (shared, rx) = controls();
        shared.request("hide").unwrap();
        let worker = fixture.start(shared.clone(), rx);
        wait_for(&shared, |s| s.phase == "fault");
        std::fs::write(fixture.0.join("model"), "normal").unwrap();
        shared.request("retry").unwrap();
        wait_for(&shared, |s| s.phase == "ready");
        assert!(!shared.status.lock().unwrap().visible);
        shared.request("quit").unwrap();
        worker.join().unwrap();
    }
    #[test]
    fn missing_model_leaves_controls_available() {
        let (shared, rx) = controls();
        let worker_shared = shared.clone();
        let worker = std::thread::spawn(move || {
            monitor(
                &worker_shared,
                rx,
                mpsc::sync_channel(1).1,
                PathBuf::new(),
                None,
            )
        });
        wait_for(&shared, |s| s.phase == "fault");
        shared.request("hide").unwrap();
        shared.request("quit").unwrap();
        worker.join().unwrap();
    }
    #[test]
    fn concurrent_selections_keep_id_and_path_together() {
        let (shared, _rx) = controls();
        let requests: Vec<_> = (0..16)
            .map(|index| {
                let shared = shared.clone();
                std::thread::spawn(move || {
                    library::choose(&shared, PathBuf::from(format!("candidate-{index}")))
                })
            })
            .map(|thread| thread.join().unwrap())
            .collect();
        let latest = requests.iter().max_by_key(|request| request.id).unwrap();
        let status = shared.selection_status.lock().unwrap();
        let status = status.as_ref().unwrap();
        assert_eq!(status.id, latest.id);
        assert_eq!(status.requested.to_string_lossy(), latest.path);
        assert_eq!(shared.selection.load(Ordering::Acquire), latest.id);
        assert_eq!(
            shared.requested.lock().unwrap().as_ref(),
            Some(&status.requested)
        );
    }
    #[test]
    fn failed_candidate_keeps_previous_host_and_success_switches() {
        let fixture = Fixture::new("normal");
        let (shared, rx) = controls();
        let worker = fixture.start(shared.clone(), rx);
        wait_for(&shared, |s| s.phase == "ready");
        let old_pid = shared.status.lock().unwrap().host_pid.unwrap();
        let candidate = fixture.0.join("candidate");
        std::fs::write(&candidate, "reject").unwrap();
        library::choose(&shared, candidate.clone());
        let deadline = Instant::now() + Duration::from_secs(5);
        while shared
            .selection_status
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(|s| s.phase != "failed")
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(shared.import_error.lock().unwrap().is_some());
        assert_eq!(shared.status.lock().unwrap().host_pid, Some(old_pid));
        assert_eq!(shared.status.lock().unwrap().phase, "ready");
        std::fs::write(&candidate, "normal").unwrap();
        library::choose(&shared, candidate.clone());
        wait_for(&shared, |s| s.detail == "角色切换完成");
        assert_eq!(
            shared
                .selection_status
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .phase,
            "succeeded"
        );
        assert_ne!(shared.status.lock().unwrap().host_pid, Some(old_pid));
        shared.request("quit").unwrap();
        worker.join().unwrap();
    }
    #[test]
    fn switching_to_touch_pack_enables_real_hit_feedback() {
        let fixture = Fixture::new("normal");
        let (shared, rx) = controls();
        let voice_settings = VoiceSettings {
            enabled: true,
            model_dir: fixture.0.join("missing-models"),
            reference_audio: fixture.0.join("missing-reference.wav"),
            reference_text: "test fixture".into(),
            ..Default::default()
        };
        *shared.voice.lock().unwrap() = Some(voice::VoiceController::new(voice_settings));
        *shared.library.lock().unwrap() = Some(fixture.0.clone());
        *shared.care.lock().unwrap() = Some(
            CareStore::open(fixture.0.join("care.sqlite3"), shared.care_clock.now_ms()).unwrap(),
        );
        let worker = fixture.start(shared.clone(), rx);
        wait_for(&shared, |status| status.phase == "ready");

        let mut manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../assets/demo/pack-template/manifest.v4.example.json"
        ))
        .unwrap();
        manifest["id"] = serde_json::json!("touch-switch-test");
        manifest["touch_reactions"] = serde_json::json!({
            "arm": [{"expression":"neutral", "duration_ms":1000}]
        });
        let candidate = fixture.0.join("touch-manifest.json");
        std::fs::write(&candidate, serde_json::to_vec(&manifest).unwrap()).unwrap();
        library::choose(&shared, candidate);
        wait_for(&shared, |status| status.detail == "角色切换完成");

        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(view) = shared.last_touch.lock().unwrap().as_ref() {
                assert_eq!(view.outcome.decision.rule_id, "arm");
                assert_eq!(view.expressions, ["neutral"]);
                assert!(
                    shared
                        .voice
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .status()
                        .turn_id
                        == 0
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "switched host hit had no touch feedback"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        shared.request("quit").unwrap();
        worker.join().unwrap();
    }
    #[test]
    fn selection_size_and_gaze_range_restore_after_restart() {
        let fixture = Fixture::new("normal");
        let (shared, _) = controls();
        *shared.library.lock().unwrap() = Some(fixture.0.clone());
        shared.scale.store(75, Ordering::Release);
        shared.gaze_radius.store(650, Ordering::Release);
        library::save_selection(&shared, &fixture.0.join("model"));
        let (restored, _) = controls();
        library::restore(&restored, &fixture.0, None);
        assert_eq!(restored.scale.load(Ordering::Acquire), 75);
        assert_eq!(restored.gaze_radius.load(Ordering::Acquire), 650);
        assert_eq!(
            *restored.requested.lock().unwrap(),
            Some(fixture.0.join("model"))
        );
    }
    #[test]
    fn older_preferences_use_default_gaze_range() {
        let fixture = Fixture::new("normal");
        let selected = fixture.0.join("model");
        std::fs::write(
            fixture.0.join("preferences.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": 1,
                "selected": selected,
                "scale": 100
            }))
            .unwrap(),
        )
        .unwrap();
        let (restored, _) = controls();
        library::restore(&restored, &fixture.0, None);
        assert_eq!(restored.gaze_radius.load(Ordering::Acquire), 400);
    }
    #[test]
    fn window_perch_overrides_remain_independent_per_role_after_restart() {
        let fixture = Fixture::new("normal");
        let first = fixture.0.join("first.json");
        let second = fixture.0.join("second.json");
        let mut manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../assets/demo/pack-template/manifest.example.json"
        ))
        .unwrap();
        std::fs::write(&first, serde_json::to_vec(&manifest).unwrap()).unwrap();
        manifest["id"] = serde_json::json!("second-role");
        std::fs::write(&second, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let (shared, _) = controls();
        *shared.library.lock().unwrap() = Some(fixture.0.clone());
        shared
            .perch_overrides
            .lock()
            .unwrap()
            .insert("demo-template".into(), 36);
        shared
            .perch_overrides
            .lock()
            .unwrap()
            .insert("second-role".into(), 62);
        shared.perch.store(36, Ordering::Release);
        library::save_selection(&shared, &first);
        let (restored, _) = controls();
        library::restore(&restored, &fixture.0, None);
        assert_eq!(restored.perch.load(Ordering::Acquire), 36);
        assert_eq!(library::perch_for(&restored, &second), 62);
    }
    #[test]
    fn legacy_raw_selection_upgrades_only_with_a_bundled_pack() {
        use std::path::Path;
        let raw = Path::new("/tmp/Nahida.model3.json");
        let bundled = Path::new("/tmp/manifest.json");
        let imported = PathBuf::from("/private/packs/manifest.json");
        assert_eq!(
            library::legacy_model_upgrade(raw, Some(bundled), |_| Ok(imported.clone())).unwrap(),
            Some(imported.clone())
        );
        assert!(
            library::legacy_model_upgrade(&imported, Some(bundled), |_| {
                panic!("imported selection must be preserved")
            })
            .unwrap()
            .is_none()
        );
        assert!(
            library::legacy_model_upgrade(raw, None, |_| panic!("no bundled pack"))
                .unwrap()
                .is_none()
        );
        assert!(
            library::legacy_model_upgrade(raw, Some(bundled), |_| anyhow::bail!("broken pack"))
                .is_err()
        );
    }
    #[test]
    fn rejected_external_permission_keeps_screen_pet_running() {
        let fixture = Fixture::new("deny_external");
        let (shared, rx) = controls();
        let worker = fixture.start(shared.clone(), rx);
        wait_for(&shared, |status| status.phase == "ready");
        let pid = shared.status.lock().unwrap().host_pid;
        shared.external_requested.store(true, Ordering::Release);
        shared.external_revision.fetch_add(1, Ordering::AcqRel);
        let _ = shared.wake.try_send(());
        let deadline = Instant::now() + Duration::from_secs(3);
        while shared.external_error.lock().unwrap().is_none() {
            assert!(
                Instant::now() < deadline,
                "permission denial was not reported"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!shared.external_requested.load(Ordering::Acquire));
        assert_eq!(shared.status.lock().unwrap().host_pid, pid);
        assert_eq!(shared.status.lock().unwrap().phase, "ready");
        shared.request("quit").unwrap();
        worker.join().unwrap();
    }
}
