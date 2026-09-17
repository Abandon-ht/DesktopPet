use anyhow::{Context, Result};
use std::{process::Command, time::Duration};

fn run() -> Result<()> {
    let host = std::env::args_os()
        .nth(1)
        .context("usage: p0-supervisor HOST_EXECUTABLE")?;
    let attempts = p0_ipc_probe::supervisor::supervise(
        || Command::new(&host),
        Duration::from_secs(2),
        Duration::from_secs(1),
        3,
        Duration::from_millis(250),
    )?;
    println!(
        "{}",
        serde_json::json!({"event":"supervisor_complete","attempts":attempts,"health_checks":3})
    );
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("p0-supervisor: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
