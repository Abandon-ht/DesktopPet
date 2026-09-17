use std::io;

fn main() -> std::process::ExitCode {
    match p0_ipc_probe::server::serve(
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        |_| Ok(()),
    ) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            // Never mix diagnostics with stdout protocol frames.
            eprintln!("p0-ipc-probe: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
