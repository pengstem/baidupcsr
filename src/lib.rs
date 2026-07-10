use std::ffi::OsString;
use std::process::ExitCode;

pub mod app;
pub mod cli;
pub mod config;
pub mod pcs;
pub mod preview;
pub mod tui;

/// Runs the command-line application with an explicit argument iterator.
pub async fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    cli::run(args).await
}
