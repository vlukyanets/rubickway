use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    if std::env::args_os().len() > 1 {
        eprintln!("usage: rubickway");
        return ExitCode::from(2);
    }

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("rubickway=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    match rubickway::state::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("rubickway: {err}");
            ExitCode::FAILURE
        }
    }
}
