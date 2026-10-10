use desktop_pet_ax650::{Config, HELP, run};
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{HELP}");
        return ExitCode::SUCCESS;
    }
    let config = match Config::parse(arguments) {
        Ok(config) => config,
        Err(error) => {
            eprintln!(
                "{}",
                serde_json::json!({"event":"configuration_error","error":format!("{error:#}")})
            );
            return ExitCode::from(2);
        }
    };
    match run(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!(
                "{}",
                serde_json::json!({"event":"runtime_error","error":format!("{error:#}")})
            );
            ExitCode::FAILURE
        }
    }
}
