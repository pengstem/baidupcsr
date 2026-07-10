use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    baidupcsr::run(std::env::args_os()).await
}
