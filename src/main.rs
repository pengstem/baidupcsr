use std::process::ExitCode;

use clap::Parser as _;

fn main() -> ExitCode {
    let cli = baidupcsr::cli::Cli::parse();
    // Reading the local UTC offset is only reliable before threads exist.
    baidupcsr::tui::format::init_local_offset();
    match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime.block_on(baidupcsr::cli::run(cli)),
        Err(error) => {
            eprintln!("cannot start runtime: {error}");
            ExitCode::FAILURE
        }
    }
}
