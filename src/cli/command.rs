use super::*;

pub(crate) enum CommandAction {
    Help,
    Doctor { config_dir: Option<PathBuf> },
    VideoHelper,
    TuiShell,
    UsageError,
}

pub(crate) fn parse_command(args: &[String]) -> CommandAction {
    match args {
        [] => CommandAction::TuiShell,
        [flag] if matches!(flag.as_str(), "--help" | "-h") => CommandAction::Help,
        [cmd] if cmd == "tui" => CommandAction::TuiShell,
        [cmd] if cmd == VIDEO_HELPER_COMMAND => CommandAction::VideoHelper,
        [cmd] if cmd == "doctor" => CommandAction::Doctor { config_dir: None },
        [cmd, flag] if cmd == "doctor" && flag == "--redact" => {
            CommandAction::Doctor { config_dir: None }
        }
        [cmd, flag, dir_flag, dir]
            if cmd == "doctor" && flag == "--redact" && dir_flag == "--config-dir" =>
        {
            CommandAction::Doctor {
                config_dir: Some(PathBuf::from(dir)),
            }
        }
        _ => CommandAction::UsageError,
    }
}
