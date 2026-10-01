use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{CommandFactory as _, Parser, Subcommand};

use crate::app::launch::program_available;
use crate::config::{self, PcsConfig, UiConfig};

/// Terminal file manager for Baidu Netdisk, using BaiduPCS-Go's login.
#[derive(Parser)]
#[command(name = "baidupcsr-cli", version)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open the file browser (default).
    Tui,
    /// Check configuration and login without printing secrets.
    Doctor {
        /// Directory containing BaiduPCS-Go's pcs_config.json.
        #[arg(long)]
        config_dir: Option<PathBuf>,
    },
    /// Print a shell completion script.
    Completions { shell: clap_complete::Shell },
}

pub async fn run(cli: Cli) -> ExitCode {
    match cli.command.unwrap_or(Command::Tui) {
        Command::Tui => match crate::tui::terminal::run().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("terminal error: {error}");
                ExitCode::FAILURE
            }
        },
        Command::Doctor { config_dir } => doctor(config_dir),
        Command::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "baidupcsr-cli",
                &mut std::io::stdout(),
            );
            ExitCode::SUCCESS
        }
    }
}

fn doctor(config_dir: Option<PathBuf>) -> ExitCode {
    let pcs_path = config::pcs_config_path(config_dir);
    let ui_path = config::ui_config_path();
    let mut healthy = true;
    let mut report = |ok: bool, label: &str, detail: String| {
        healthy &= ok;
        println!("{} {label:<14} {detail}", if ok { "✓" } else { "✗" });
    };

    report(true, "cache", config::cache_dir().display().to_string());
    match UiConfig::load(&ui_path) {
        Ok(ui) => {
            let exists = if ui_path.exists() {
                ""
            } else {
                " (not created, using defaults)"
            };
            report(true, "ui config", format!("{}{exists}", ui_path.display()));
            report(
                program_available(&ui.player.command),
                "player",
                ui.player.command.clone(),
            );
            report(
                true,
                "downloads",
                format!(
                    "{} chunks × {} in flight",
                    crate::tui::format::size(ui.download.chunk_size),
                    ui.download.concurrency
                ),
            );
        }
        Err(error) => report(false, "ui config", error.to_string()),
    }

    match PcsConfig::load(&pcs_path) {
        Ok(pcs) => {
            report(
                true,
                "pcs config",
                format!("{} ({})", pcs_path.display(), permissions(&pcs_path)),
            );
            match pcs.active_user() {
                Some(user) => {
                    report(true, "account", format!("{} (uid {})", user.name, user.uid));
                    report(
                        user.account().is_some(),
                        "BDUSS",
                        presence(user.account().is_some()),
                    );
                    report(true, "STOKEN", presence(user.has_stoken()));
                    report(true, "workdir", user.workdir.clone());
                }
                None => report(
                    false,
                    "account",
                    "no active user — run BaiduPCS-Go login".to_string(),
                ),
            }
        }
        Err(error) => {
            report(false, "pcs config", error.to_string());
            println!("  set BAIDUPCS_GO_CONFIG_DIR or log in with BaiduPCS-Go first");
        }
    }
    if healthy {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn presence(present: bool) -> String {
    if present { "present" } else { "missing" }.to_string()
}

#[cfg(unix)]
fn permissions(path: &Path) -> String {
    use std::os::unix::fs::PermissionsExt as _;
    match std::fs::metadata(path) {
        Ok(metadata) => {
            let mode = metadata.permissions().mode() & 0o777;
            let note = if mode & 0o077 == 0 {
                "owner-only"
            } else {
                "readable by others"
            };
            format!("{note}, mode {mode:03o}")
        }
        Err(error) => error.to_string(),
    }
}

#[cfg(not(unix))]
fn permissions(_path: &Path) -> String {
    "present".to_string()
}
