//! Application-level tests use a protocol fixture, never an actual desktop.
#![cfg(unix)]
use serde_json::Value;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

struct Fixture {
    directory: TempDir,
    host: PathBuf,
    model: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let host = directory.path().join("fake-host");
        fs::write(&host, include_bytes!("fake_host.py")).unwrap();
        fs::set_permissions(&host, fs::Permissions::from_mode(0o755)).unwrap();
        let model = directory.path().join("manifest.json");
        let template = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/demo/pack-template/manifest.v4.example.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(template).unwrap()).unwrap();
        manifest["touch_reactions"] =
            serde_json::json!({"boundary_first":[{"expression":"happy","duration_ms":1200}]});
        fs::write(&model, serde_json::to_vec(&manifest).unwrap()).unwrap();
        Self {
            directory,
            host,
            model,
        }
    }
    fn command(&self, run: &str, mode: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_desktop-pet-ax650"));
        command
            .arg("--model")
            .arg(&self.model)
            .arg("--host")
            .arg(&self.host)
            .arg("--data-dir")
            .arg(self.directory.path().join("data"))
            .arg("--log-dir")
            .arg(self.directory.path().join(run))
            .env("AX650_TEST_MODE", mode)
            .env("AX650_TEST_RECORD", self.record(run))
            .env("DISPLAY", ":42")
            .env("XAUTHORITY", "/test/auth")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
    fn record(&self, run: &str) -> PathBuf {
        self.directory.path().join(format!("{run}-host.jsonl"))
    }
    fn run(&self, run: &str, mode: &str, input: &str, extra: &[&str]) -> Output {
        let mut command = self.command(run, mode);
        command.args(extra);
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            !output.stdout.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
    fn events(&self, run: &str) -> Vec<Value> {
        records(self.directory.path().join(run).join("parent.jsonl"))
    }
}
fn records(path: impl AsRef<Path>) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn event<'a>(events: &'a [Value], name: &str) -> &'a Value {
    events
        .iter()
        .rev()
        .find(|row| row["event"] == name)
        .unwrap()
}
fn assert_gone(pid: u32) {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output()
        .unwrap();
    assert!(
        output.stdout.is_empty(),
        "host {pid} survived parent cleanup"
    );
}
fn wait_host(fixture: &Fixture, run: &str, child: &mut Child) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(text) = fs::read_to_string(fixture.record(run)) {
            let rows: Vec<Value> = text
                .lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect();
            if rows.iter().any(|row| row["request"]["type"] == "ping") {
                return rows;
            }
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("runtime exited early: {status}");
        }
        assert!(Instant::now() < deadline, "runtime did not finish startup");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn care_request_deduplication_scale_restore_and_backend_environment() {
    let fixture = Fixture::new();
    let output = fixture.run(
        "first",
        "normal",
        "scale 75\nfeed same\nfeed same\nplay same\nstatus\nquit\n",
        &[],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = fixture.events("first");
    let status = event(&events, "status");
    assert_eq!(status["scale"], 75);
    assert_eq!(status["care"]["food"], 2);
    assert_eq!(
        events.iter().filter(|row| row["event"] == "care").count(),
        2
    );
    assert!(
        events
            .iter()
            .any(|row| row["event"] == "care" && row["replayed"] == true)
    );
    assert!(
        events
            .iter()
            .any(|row| row["event"] == "command_error" && row["request_id"] == "same")
    );
    let host = records(fixture.record("first"));
    let started = &host[0];
    assert_eq!(started["started"], started["sid"]);
    assert_eq!(started["started"], started["pgrp"]);
    assert_eq!(started["environment"]["WGPU_BACKEND"], "gl");
    assert_eq!(started["environment"]["DESKTOPPET_X11_GL_ALPHA"], "1");
    assert_eq!(started["environment"]["LIBGL_ALWAYS_SOFTWARE"], "1");
    assert_eq!(started["environment"]["DESKTOPPET_MAX_FPS"], "20");
    assert_eq!(started["environment"]["DISPLAY"], ":42");
    assert_eq!(started["environment"]["XAUTHORITY"], "/test/auth");
    assert_eq!(
        host.iter()
            .filter(|row| row["request"]["payload"]["type"] == "play_feedback"
                && row["request"]["payload"]["payload"] == "feed")
            .count(),
        1
    );
    assert_gone(started["started"].as_u64().unwrap() as u32);
    assert!(
        fs::read_to_string(fixture.directory.path().join("first/host.stderr.log"))
            .unwrap()
            .contains("AX650 fake host initialized")
    );
    let output = fixture.run("second", "normal", "status\nquit\n", &[]);
    assert!(output.status.success());
    let events = fixture.events("second");
    assert_eq!(event(&events, "status")["scale"], 75);
    assert_eq!(event(&events, "status")["care"]["food"], 2);
}

#[test]
fn eof_keeps_service_running_and_duplicate_click_has_one_durable_effect() {
    let fixture = Fixture::new();
    let started = Instant::now();
    let output = fixture.run("touch", "duplicate_touch", "", &["--seconds", "1"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(started.elapsed() >= Duration::from_millis(900));
    let events = fixture.events("touch");
    assert!(event(&events, "stdin_eof")["continuing"].as_bool().unwrap());
    let touches: Vec<_> = events
        .iter()
        .filter(|row| row["event"] == "touch")
        .collect();
    assert_eq!(touches.len(), 2);
    assert_eq!(touches[0]["outcome"]["replayed"], false);
    assert_eq!(touches[1]["outcome"]["replayed"], true);
    assert_eq!(event(&events, "stopped")["care"]["needs"]["intimacy"], 15);
    let host = records(fixture.record("touch"));
    assert_eq!(
        host.iter()
            .filter(|row| row["request"]["payload"]["type"] == "play_touch_cue")
            .count(),
        1
    );
}

#[test]
fn care_commit_survives_render_failure_without_repeating_inventory_change() {
    let fixture = Fixture::new();
    let failed = fixture.run("failed", "crash_feedback", "feed durable-feed\n", &[]);
    assert_eq!(failed.status.code(), Some(1));
    assert_eq!(event(&fixture.events("failed"), "care")["care"]["food"], 2);
    assert_eq!(
        event(&fixture.events("failed"), "result")["completed"],
        false
    );
    let recovered = fixture.run(
        "recovered",
        "normal",
        "feed durable-feed\nstatus\nquit\n",
        &[],
    );
    assert!(recovered.status.success());
    let events = fixture.events("recovered");
    assert_eq!(event(&events, "care")["replayed"], true);
    assert_eq!(event(&events, "status")["care"]["food"], 2);
    assert!(
        !records(fixture.record("recovered"))
            .iter()
            .any(|row| row["request"]["payload"]["type"] == "play_feedback")
    );
}

#[test]
fn host_failure_stops_once_and_reaps_child() {
    let fixture = Fixture::new();
    for mode in ["wrong_version", "crash_poll", "stall_exit"] {
        let output = fixture.run(
            mode,
            mode,
            if mode == "stall_exit" { "quit\n" } else { "" },
            &[],
        );
        assert_eq!(output.status.code(), Some(1));
        let host = records(fixture.record(mode));
        assert_eq!(
            host.iter()
                .filter(|row| row.get("started").is_some())
                .count(),
            1
        );
        assert_gone(host[0]["started"].as_u64().unwrap() as u32);
        assert!(
            fixture
                .directory
                .path()
                .join(mode)
                .join("host.stderr.log")
                .is_file()
        );
        assert_eq!(event(&fixture.events(mode), "result")["completed"], false);
    }
}

#[test]
fn sigint_sigterm_and_sighup_use_protocol_shutdown() {
    let fixture = Fixture::new();
    for (run, signal) in [
        ("int", libc::SIGINT),
        ("term", libc::SIGTERM),
        ("hup", libc::SIGHUP),
    ] {
        let mut child = fixture.command(run, "normal").spawn().unwrap();
        let host = wait_host(&fixture, run, &mut child);
        assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            records(fixture.record(run))
                .iter()
                .any(|row| row["request"]["type"] == "shutdown")
        );
        assert_eq!(
            event(&fixture.events(run), "stopping")["reason"],
            format!("signal_{signal}")
        );
        assert_gone(host[0]["started"].as_u64().unwrap() as u32);
    }
}

#[test]
fn invalid_configuration_exits_before_launching_host() {
    let fixture = Fixture::new();
    let output = fixture
        .command("invalid", "normal")
        .args(["--fps", "0"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!fixture.record("invalid").exists());
}

#[test]
fn second_process_cannot_share_care_and_placement_directory() {
    let fixture = Fixture::new();
    let mut active = fixture.command("active", "normal").spawn().unwrap();
    let host = wait_host(&fixture, "active", &mut active);
    let output = fixture.command("conflict", "normal").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!fixture.record("conflict").exists());
    assert!(
        event(&fixture.events("conflict"), "result")["error"]
            .as_str()
            .unwrap()
            .contains("already in use")
    );
    assert!(active.try_wait().unwrap().is_none());
    assert_eq!(unsafe { libc::kill(active.id() as i32, libc::SIGTERM) }, 0);
    assert!(active.wait_with_output().unwrap().status.success());
    assert_gone(host[0]["started"].as_u64().unwrap() as u32);
}
