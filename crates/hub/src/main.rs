//! `hub` — binary entry point. All behavior lives in the `hub` library.

use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args = hub::Args::parse();
    match hub::run(&args) {
        Ok(()) => ExitCode::from(hub::EXIT_OK),
        Err(err) => hub::exit_code(&err),
    }
}
