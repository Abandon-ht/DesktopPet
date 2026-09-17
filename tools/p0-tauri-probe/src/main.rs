use anyhow::{Context, Result, ensure};
use p0_ipc_probe::supervisor::{Host, RestartBudget};
use std::{
    path::PathBuf,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use tauri::Manager;

fn run() -> Result<()> {
    let model = PathBuf::from(
        std::env::var_os("P0_MODEL").context("set P0_MODEL to the local model3.json path")?,
    )
    .canonicalize()?;
    let host = std::env::current_exe()?
        .parent()
        .context("missing executable directory")?
        .join("p0-probe");
    ensure!(host.is_file(), "build sibling p0-probe executable first");
    let auto_exit = std::env::var("P0_AUTO_EXIT_SECONDS")
        .ok()
        .map(|s| s.parse::<u64>())
        .transpose()?;
    if let Some(seconds) = auto_exit {
        ensure!(
            (1..=3600).contains(&seconds),
            "auto-exit duration must be 1–3600 seconds"
        );
    }
    let (stop_tx, stop_rx) = mpsc::sync_channel::<()>(1);
    let finished = Arc::new(AtomicBool::new(false));
    let done = finished.clone();
    let outcome = Arc::new(AtomicI32::new(0));
    let worker_outcome = outcome.clone();
    let app = tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            if let Some(seconds) = auto_exit {
                let timer_handle = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(seconds));
                    timer_handle.exit(0);
                });
            }
            std::thread::spawn(move || {
                let result = monitor(&handle, host, model, stop_rx);
                let code = if let Err(error) = result {
                    p0_ipc_probe::event_log!(
                        "{}",
                        serde_json::json!({"event":"bridge_failed","error":format!("{error:#}")})
                    );
                    1
                } else {
                    0
                };
                worker_outcome.store(code, Ordering::Release);
                done.store(true, Ordering::Release);
                p0_ipc_probe::event_log!(
                    "{}",
                    serde_json::json!({"event":"bridge_stopped","code":code})
                );
                handle.exit(code);
            });
            Ok(())
        })
        .build(tauri::generate_context!())?;
    let exit_code = app.run_return(move |_, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event
            && !finished.load(Ordering::Acquire)
        {
            api.prevent_exit();
            p0_ipc_probe::event_log!("{}", serde_json::json!({"event":"bridge_exit_requested"}));
            let _ = stop_tx.try_send(());
        }
    });
    ensure!(exit_code == 0, "bridge exited with status {exit_code}");
    // Preserve the supervisor outcome even if the platform event loop returns
    // its normal termination code for an application-requested exit.
    ensure!(
        outcome.load(Ordering::Acquire) == 0,
        "bridge supervisor failed"
    );
    Ok(())
}

fn monitor(
    app: &tauri::AppHandle,
    executable: PathBuf,
    model: PathBuf,
    stop: mpsc::Receiver<()>,
) -> Result<()> {
    let stopping = std::cell::Cell::new(false);
    let cancelled = || {
        if stop.try_recv().is_ok() {
            stopping.set(true);
        }
        stopping.get()
    };
    let mut budget = RestartBudget::new(Duration::from_millis(250));
    let mut attempt = 0;
    loop {
        if cancelled() {
            return Ok(());
        }
        attempt += 1;
        let mut command = Command::new(&executable);
        command.arg("host").arg(&model).env("P0_INPUT", "dynamic");
        let result = (|| -> Result<()> {
            let mut host = Host::start(&mut command, Duration::from_secs(5))?;
            p0_ipc_probe::event_log!(
                "{}",
                serde_json::json!({"event":"bridge_ready","host_pid":host.id(),"attempt":attempt})
            );
            if let Some(window) = app.get_webview_window("main") {
                let _ =
                    window.set_title(&format!("DesktopPet — 角色已连接（第 {attempt} 次启动）"));
            }
            loop {
                if cancelled() {
                    return host.shutdown();
                }
                match stop.recv_timeout(Duration::from_secs(1)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                        stopping.set(true);
                        return host.shutdown();
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        host.ping()?;
                    }
                }
            }
        })();
        match result {
            Ok(()) => return Ok(()),
            Err(error) => {
                p0_ipc_probe::event_log!(
                    "{}",
                    serde_json::json!({"event":"bridge_retry","attempt":attempt,"error":format!("{error:#}")})
                );
                if cancelled() {
                    return Ok(());
                }
                let delay = budget
                    .failure(Instant::now())
                    .context("restart budget exhausted")?;
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_title("DesktopPet — 正在重新连接角色");
                }
                if stop.recv_timeout(delay).is_ok() {
                    return Ok(());
                }
            }
        }
    }
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            p0_ipc_probe::event_log!("p0-tauri-probe: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
