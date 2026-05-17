use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, flag] if cmd == "doctor" && flag == "--redact" => {
            print_stage1_doctor();
            ExitCode::SUCCESS
        }
        [cmd] if cmd == "doctor" => {
            print_stage1_doctor();
            ExitCode::SUCCESS
        }
        [] => {
            println!("baidupcsr tui shell");
            println!("status: stage1 scaffold");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: baidupcsr-cli [doctor [--redact]]");
            ExitCode::from(2)
        }
    }
}

fn print_stage1_doctor() {
    println!("baidupcsr doctor");
    println!("redacted: true");
    println!("config: not loaded");
    println!("crates:");
    println!("  - {}", baidupcsr_config::crate_name());
    println!("  - {}", baidupcsr_core::crate_name());
    println!("  - {}", baidupcsr_pcs::crate_name());
    println!("  - {}", baidupcsr_preview::crate_name());
    println!("  - {}", baidupcsr_tui::crate_name());
}
