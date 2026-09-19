use pet_ipc::supervisor::{Host, RestartBudget, supervise};
use std::{
    process::Command,
    time::{Duration, Instant},
};

fn fixture(mode: &str) -> Command {
    let mut command = Command::new("python3");
    command
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fault_host.py"))
        .arg(mode);
    command
}

fn assert_gone(pid: u32) {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output()
        .unwrap();
    assert!(output.stdout.is_empty(), "host {pid} remains after cleanup");
}

#[test]
fn real_host_handshake_heartbeat_shutdown() {
    let mut host = Host::start(
        &mut Command::new(env!("CARGO_BIN_EXE_pet-ipc")),
        Duration::from_secs(2),
    )
    .unwrap();
    let pid = host.id();
    host.ping().unwrap();
    host.shutdown().unwrap();
    assert_gone(pid);
}

#[test]
fn handshake_has_deadline() {
    let now = Instant::now();
    let result = Host::start(&mut fixture("stall_handshake"), Duration::from_millis(300));
    assert!(result.err().unwrap().to_string().contains("timeout"));
    assert!(now.elapsed() < Duration::from_secs(3));
}

#[test]
fn heartbeat_timeout_reaps_child_and_invalidates_session() {
    let mut host = Host::start(&mut fixture("stall_ping"), Duration::from_secs(1)).unwrap();
    let pid = host.id();
    assert!(host.ping().unwrap_err().to_string().contains("timeout"));
    assert_gone(pid);
    assert!(host.ping().unwrap_err().to_string().contains("stopped"));
}

#[test]
fn response_identity_and_size_are_checked() {
    for (mode, reason) in [
        ("wrong_version", "protocol mismatch"),
        ("wrong_request", "request mismatch"),
        ("oversized", "frame_too_large"),
    ] {
        let error = Host::start(&mut fixture(mode), Duration::from_secs(2))
            .err()
            .unwrap();
        assert!(format!("{error:#}").contains(reason), "{error:#}");
    }
}

#[test]
fn acknowledged_shutdown_still_requires_process_exit() {
    let host = Host::start(&mut fixture("stall_exit"), Duration::from_secs(1)).unwrap();
    let pid = host.id();
    assert!(
        host.shutdown()
            .unwrap_err()
            .to_string()
            .contains("exit timeout")
    );
    assert_gone(pid);
}

#[test]
fn drop_cleans_up_host() {
    let host = Host::start(&mut fixture("normal"), Duration::from_secs(2)).unwrap();
    let pid = host.id();
    drop(host);
    assert_gone(pid);
}

#[test]
fn crash_is_restarted_and_new_session_works() {
    let mut launches = 0;
    let attempts = supervise(
        || {
            launches += 1;
            fixture(if launches == 1 {
                "crash_ping"
            } else {
                "normal"
            })
        },
        Duration::from_secs(2),
        Duration::ZERO,
        2,
        Duration::from_millis(1),
    )
    .unwrap();
    assert_eq!(attempts, 2);
}

#[test]
fn three_failures_stop_restart_loop() {
    let mut launches = 0;
    let error = supervise(
        || {
            launches += 1;
            fixture("crash_ping")
        },
        Duration::from_secs(2),
        Duration::ZERO,
        1,
        Duration::from_millis(1),
    )
    .unwrap_err();
    assert_eq!(launches, 3);
    assert!(error.to_string().contains("restart budget exhausted"));
}

#[test]
fn rolling_budget_expires_and_backoff_increases() {
    let mut budget = RestartBudget::new(Duration::from_millis(100));
    let now = Instant::now();
    assert_eq!(budget.failure(now), Some(Duration::from_millis(100)));
    assert_eq!(
        budget.failure(now + Duration::from_secs(1)),
        Some(Duration::from_millis(200))
    );
    assert_eq!(budget.failure(now + Duration::from_secs(2)), None);
    assert_eq!(
        budget.failure(now + Duration::from_secs(62)),
        Some(Duration::from_millis(100))
    );
}
