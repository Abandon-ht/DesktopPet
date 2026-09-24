mod library;
use anyhow::{Context, Result};
use pet_core::{Effect, Event, Intent, PetCore};
use pet_ipc::supervisor::{Host, RestartBudget};
use pet_protocol::{AvatarEvent, DesktopCommand, DesktopEvent};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
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
struct Shared {
    library: Mutex<Option<PathBuf>>,
    requested: Mutex<Option<PathBuf>>,
    active: Mutex<Option<PathBuf>>,
    selection: AtomicU64,
    scale: AtomicU16,
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
impl Shared {
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
fn monitor(shared: &Shared, wake: mpsc::Receiver<()>, executable: PathBuf, model: Option<PathBuf>) {
    let mut model = shared.requested.lock().unwrap().clone().or(model);
    let mut selection_seen = shared.selection.load(Ordering::Acquire);
    let mut core = PetCore::default();
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
            let mut applied_scale = shared.scale.load(Ordering::Acquire);
            let mut applied_perch = shared.perch.load(Ordering::Acquire);
            let mut external_seen = 0;
            let mut revision = shared.revision.load(Ordering::Acquire);
            let mut applied_visible = core.state().visible;
            let mut next_ping = Instant::now() + Duration::from_secs(1);
            loop {
                if shared.stop.load(Ordering::Acquire) {
                    // Direct lifecycle route, independent of core effects.
                    return host.shutdown();
                }
                let selection = shared.selection.load(Ordering::Acquire);
                if selection != selection_seen {
                    selection_seen = selection;
                    let requested = shared.requested.lock().unwrap().clone();
                    if let Some(path) = requested {
                        let candidate = (|| -> Result<Host> {
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
                            Ok(candidate)
                        })();
                        match candidate {
                            Ok(candidate) => {
                                if shared.stop.load(Ordering::Acquire) {
                                    drop(candidate);
                                    return host.shutdown();
                                }
                                let old = std::mem::replace(&mut host, candidate);
                                let _ = old.shutdown();
                                model = Some(path.clone());
                                *shared.active.lock().unwrap() = Some(path.clone());
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
                                external_seen = 0;
                                *shared.import_error.lock().unwrap() = None;
                                shared.report("ready", "角色切换完成", Some(host.id()));
                            }
                            Err(error) => {
                                *shared.import_error.lock().unwrap() =
                                    Some(format!("候选角色加载失败，保留当前角色：{error:#}"));
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
                // A wake-up rechecks emergency atomics immediately; timeout is
                // the heartbeat cadence, not a per-frame busy poll.
                if Instant::now() >= next_ping {
                    for event in host.poll()? {
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
            if let Some(delay) = budget.failure(Instant::now()) {
                shared.report("recovering", format!("连接中断，正在重试：{error:#}"), None);
                // Visibility wakes must not skip the restart backoff.
                let deadline = Instant::now() + delay;
                while Instant::now() < deadline && !shared.stop.load(Ordering::Acquire) {
                    let _ = wake.recv_timeout(deadline.saturating_duration_since(Instant::now()));
                }
            } else {
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
    let shared = Arc::new(Shared {
        library: Mutex::new(None),
        requested: Mutex::new(None),
        active: Mutex::new(None),
        selection: AtomicU64::new(0),
        scale: AtomicU16::new(100),
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
            library::set_window_perch,
            set_external_snap,
            library::preferences
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| {
            let directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            *setup_shared.library.lock().unwrap() = Some(directory.clone());
            library::restore(&setup_shared, &directory, model.as_deref());
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let show = MenuItem::with_id(app, "show", "显示角色", true, None::<&str>)?;
            let hide = MenuItem::with_id(app, "hide", "隐藏角色", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "角色与设置…", true, None::<&str>)?;
            let retry = MenuItem::with_id(app, "retry", "重试连接", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出 DesktopPet", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &hide, &settings, &retry, &quit])?;
            TrayIconBuilder::with_id("desktop-pet")
                .title("Pet")
                .tooltip("DesktopPet")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "settings" {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
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
                    monitor(&setup_shared, wake_rx, executable, model)
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
        (
            Arc::new(Shared {
                library: Mutex::new(None),
                requested: Mutex::new(None),
                active: Mutex::new(None),
                selection: AtomicU64::new(0),
                scale: AtomicU16::new(100),
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
            std::thread::spawn(move || monitor(&shared, rx, host, Some(model)))
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
        let worker = std::thread::spawn(move || monitor(&worker_shared, rx, PathBuf::new(), None));
        wait_for(&shared, |s| s.phase == "fault");
        shared.request("hide").unwrap();
        shared.request("quit").unwrap();
        worker.join().unwrap();
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
        *shared.requested.lock().unwrap() = Some(candidate.clone());
        shared.selection.fetch_add(1, Ordering::AcqRel);
        let _ = shared.wake.try_send(());
        let deadline = Instant::now() + Duration::from_secs(5);
        while shared.import_error.lock().unwrap().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(shared.status.lock().unwrap().host_pid, Some(old_pid));
        assert_eq!(shared.status.lock().unwrap().phase, "ready");
        std::fs::write(&candidate, "normal").unwrap();
        shared.selection.fetch_add(1, Ordering::AcqRel);
        let _ = shared.wake.try_send(());
        wait_for(&shared, |s| s.detail == "角色切换完成");
        assert_ne!(shared.status.lock().unwrap().host_pid, Some(old_pid));
        shared.request("quit").unwrap();
        worker.join().unwrap();
    }
    #[test]
    fn selection_and_size_restore_after_restart() {
        let fixture = Fixture::new("normal");
        let (shared, _) = controls();
        *shared.library.lock().unwrap() = Some(fixture.0.clone());
        shared.scale.store(75, Ordering::Release);
        library::save_selection(&shared, &fixture.0.join("model"));
        let (restored, _) = controls();
        library::restore(&restored, &fixture.0, None);
        assert_eq!(restored.scale.load(Ordering::Acquire), 75);
        assert_eq!(
            *restored.requested.lock().unwrap(),
            Some(fixture.0.join("model"))
        );
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
