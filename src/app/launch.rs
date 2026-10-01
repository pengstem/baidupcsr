//! Starting external programs: the video player and the system file opener.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;

use crate::config::PlayerConfig;

/// Play a direct download link. For mpv, ffmpeg is told to request bounded
/// ranges: Baidu's download hosts answer open-ended `bytes=N-` with 403.
pub fn spawn_player(
    config: &PlayerConfig,
    url: &str,
    user_agent: &str,
    title: &str,
) -> std::io::Result<tokio::process::Child> {
    let mut command = tokio::process::Command::new(&config.command);
    if is_mpv(&config.command) {
        command.args([
            format!("--user-agent={user_agent}"),
            "--stream-lavf-o=request_size=4194304,initial_request_size=1048576".to_string(),
            "--cache=yes".to_string(),
            "--demuxer-max-bytes=512MiB".to_string(),
            format!("--force-media-title={title}"),
        ]);
    }
    command
        .args(&config.args)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    command.process_group(0);
    command.spawn()
}

fn is_mpv(command: &str) -> bool {
    Path::new(command)
        .file_stem()
        .and_then(OsStr::to_str)
        .is_some_and(|stem| stem == "mpv")
}

/// Open a local file with the desktop's default application.
pub fn open_with_system(path: &Path) -> Result<(), String> {
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("open", &[])]
    } else if cfg!(windows) {
        &[("cmd", &["/C", "start", ""])]
    } else {
        &[("xdg-open", &[]), ("gio", &["open"])]
    };
    let mut last_error = "no opener available".to_string();
    for (program, args) in candidates {
        match std::process::Command::new(program)
            .args(*args)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(_) => return Ok(()),
            Err(error) => last_error = format!("{program}: {error}"),
        }
    }
    Err(last_error)
}

pub fn program_available(program: &str) -> bool {
    if program.contains(std::path::MAIN_SEPARATOR) {
        return Path::new(program).is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}
