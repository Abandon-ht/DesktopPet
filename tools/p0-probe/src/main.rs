mod audit;
mod capture;
mod composite;
mod input;
mod platform;
mod window;

use anyhow::{Result, bail};
use std::path::Path;

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 2 && args[0] == "host" {
        return window::run_host(Path::new(&args[1]));
    }
    // A packaged development app can supply a local asset path without embedding
    // copyrighted assets or paths in the distributable executable.
    if args.is_empty() {
        let config = std::env::current_exe()?
            .parent()
            .unwrap()
            .join("../Resources/model-path.txt");
        if config.is_file() {
            let entry = std::fs::read_to_string(config)?;
            return window::run(Path::new(entry.trim()), 180);
        }
    }
    if args.len() == 2 && args[0] == "gallery" {
        return capture::gallery(Path::new(&args[1]));
    }
    if args.len() >= 2 && args[0] == "window" {
        let seconds = args
            .get(2)
            .map(|s| s.to_string_lossy().parse::<u64>())
            .transpose()?
            .unwrap_or(60);
        return window::run(Path::new(&args[1]), seconds);
    }
    if args.len() != 2 || args[0] != "audit" {
        bail!("usage: p0-probe audit MODEL | window MODEL [SECONDS=60]");
    }
    let report = audit::audit(Path::new(&args[1]))?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            p0_ipc_probe::event_log!("p0-probe: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
