mod animation;
mod audit;
mod ax;
mod composite;
pub mod external_snap;
mod input;
mod metrics;
mod placement;
mod platform;
mod snap;
mod window;
fn main() -> std::process::ExitCode {
    let result = (|| -> anyhow::Result<()> {
        let mut args = std::env::args_os().skip(1);
        let entry = args
            .next()
            .ok_or_else(|| anyhow::anyhow!("usage: avatar-host-2d MODEL3_JSON"))?;
        anyhow::ensure!(args.next().is_none(), "unexpected argument");
        window::run(std::path::Path::new(&entry))
    })();
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("avatar-host-2d: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
