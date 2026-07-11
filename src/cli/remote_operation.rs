use super::*;

pub(crate) fn parse_remote_command(
    input: &str,
    app: &crate::tui::TuiApp,
) -> Result<ParsedRemoteCommand, String> {
    let mut parts = input.split_whitespace();
    let Some(command) = parts.next() else {
        return Err("empty command".to_string());
    };
    let args: Vec<&str> = parts.collect();

    match command {
        "mkdir" => {
            let path = args
                .first()
                .ok_or_else(|| "mkdir requires a path".to_string())?;
            Ok(ParsedRemoteCommand::Operation(
                crate::app::RemoteOperationPlan::mkdir(resolve_remote_path(app.path(), path))?,
            ))
        }
        "rename" | "ren" => {
            let target_name = args
                .first()
                .ok_or_else(|| "rename requires a new name".to_string())?;
            let (from, to) = if args.len() >= 2 {
                (
                    resolve_remote_path(app.path(), target_name),
                    resolve_remote_path(app.path(), args[1]),
                )
            } else {
                let entry = app
                    .selected_entry()
                    .ok_or_else(|| "rename requires a selected entry".to_string())?;
                let to = if target_name.starts_with('/') {
                    normalize_remote_command_path(target_name)
                } else {
                    let parent_path = crate::tui::TuiApp::parent_path(entry.path.as_str())
                        .unwrap_or_else(|| "/".to_string());
                    join_remote_path(parent_path.as_str(), target_name)
                };
                (entry.path.clone(), to)
            };
            Ok(ParsedRemoteCommand::Operation(
                crate::app::RemoteOperationPlan::rename(from, to)?,
            ))
        }
        "copy" | "cp" => {
            let target_dir = args
                .last()
                .ok_or_else(|| "copy requires a target directory".to_string())?;
            let pairs = command_target_paths_or_args(app, &args)?
                .into_iter()
                .map(|path| crate::pcs::FileOperationPair {
                    to: join_remote_path(
                        resolve_remote_path(app.path(), target_dir).as_str(),
                        remote_file_name(path.as_str()).as_str(),
                    ),
                    from: path,
                })
                .collect();
            Ok(ParsedRemoteCommand::Operation(
                crate::app::RemoteOperationPlan::copy(pairs)?,
            ))
        }
        "move" | "mv" => {
            let target_dir = args
                .last()
                .ok_or_else(|| "move requires a target directory".to_string())?;
            let pairs = command_target_paths_or_args(app, &args)?
                .into_iter()
                .map(|path| crate::pcs::FileOperationPair {
                    to: join_remote_path(
                        resolve_remote_path(app.path(), target_dir).as_str(),
                        remote_file_name(path.as_str()).as_str(),
                    ),
                    from: path,
                })
                .collect();
            Ok(ParsedRemoteCommand::Operation(
                crate::app::RemoteOperationPlan::move_paths(pairs)?,
            ))
        }
        "rm" | "delete" => Ok(ParsedRemoteCommand::Operation(
            crate::app::RemoteOperationPlan::delete(if args.is_empty() {
                command_target_paths(app)?
            } else {
                args.iter()
                    .map(|path| resolve_remote_path(app.path(), path))
                    .collect()
            })?,
        )),
        "search" => {
            let mut mode = RemoteSearchMode::RecursiveList;
            let mut words = Vec::new();
            for arg in args {
                match arg {
                    "-r" | "--recursive" => mode = RemoteSearchMode::RecursiveList,
                    "--current" => mode = RemoteSearchMode::CurrentList,
                    "--api" => mode = RemoteSearchMode::PcsApi,
                    _ => words.push(arg),
                }
            }
            let keyword = words.join(" ");
            if keyword.is_empty() {
                return Err("search requires a keyword".to_string());
            }
            Ok(ParsedRemoteCommand::Search {
                path: app.path().to_string(),
                keyword,
                mode,
            })
        }
        _ => Err(format!("unknown command: {command}")),
    }
}

pub(crate) fn delete_plan_for_selection(
    app: &crate::tui::TuiApp,
) -> Result<crate::app::RemoteOperationPlan, String> {
    crate::app::RemoteOperationPlan::delete(command_target_paths(app)?)
}

pub(crate) fn rename_plan_for_selected(
    app: &crate::tui::TuiApp,
    target_name: &str,
) -> Result<Option<crate::app::RemoteOperationPlan>, String> {
    let target_name = target_name.trim();
    if target_name.is_empty() {
        return Err("empty name".to_string());
    }
    let entry = app
        .selected_entry()
        .ok_or_else(|| "no selected entry".to_string())?;
    let to = if target_name.starts_with('/') {
        normalize_remote_command_path(target_name)
    } else {
        let parent_path =
            crate::tui::TuiApp::parent_path(entry.path.as_str()).unwrap_or_else(|| "/".to_string());
        join_remote_path(parent_path.as_str(), target_name)
    };
    if to == entry.path {
        return Ok(None);
    }
    crate::app::RemoteOperationPlan::rename(entry.path.clone(), to).map(Some)
}

pub(crate) fn command_target_paths(app: &crate::tui::TuiApp) -> Result<Vec<String>, String> {
    if !app.selected_paths().is_empty() {
        return Ok(app.selected_paths().to_vec());
    }
    app.selected_entry()
        .map(|entry| vec![entry.path.clone()])
        .ok_or_else(|| "command requires a selected entry".to_string())
}

pub(crate) fn command_target_paths_or_args(
    app: &crate::tui::TuiApp,
    args: &[&str],
) -> Result<Vec<String>, String> {
    if args.len() <= 1 {
        return command_target_paths(app);
    }
    Ok(args[..args.len() - 1]
        .iter()
        .map(|path| resolve_remote_path(app.path(), path))
        .collect())
}

pub(crate) async fn execute_remote_operation(
    client: &crate::pcs::PcsClient,
    plan: &crate::app::RemoteOperationPlan,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<(), String> {
    match plan.kind() {
        crate::app::RemoteOperationKind::Mkdir { path } => client
            .mkdir(path, cancel)
            .await
            .map_err(|err| pcs_error_label(&err)),
        crate::app::RemoteOperationKind::Delete { paths } => client
            .delete_paths(paths, cancel)
            .await
            .map_err(|err| pcs_error_label(&err)),
        crate::app::RemoteOperationKind::Rename { from, to } => {
            let pair = crate::pcs::FileOperationPair {
                from: from.clone(),
                to: to.clone(),
            };
            client
                .move_paths(&[pair], cancel)
                .await
                .map_err(|err| pcs_error_label(&err))
        }
        crate::app::RemoteOperationKind::Copy { pairs } => client
            .copy_paths(pairs, cancel)
            .await
            .map_err(|err| pcs_error_label(&err)),
        crate::app::RemoteOperationKind::Move { pairs } => client
            .move_paths(pairs, cancel)
            .await
            .map_err(|err| pcs_error_label(&err)),
    }
}

pub(crate) fn resolve_remote_path(current_dir: &str, path: &str) -> String {
    if path.starts_with('/') {
        normalize_remote_command_path(path)
    } else {
        join_remote_path(current_dir, path)
    }
}

pub(crate) fn join_remote_path(dir: &str, name: &str) -> String {
    let dir = normalize_remote_command_path(dir);
    let name = name.trim_matches('/');
    if dir == "/" {
        format!("/{name}")
    } else {
        format!("{dir}/{name}")
    }
}

pub(crate) fn normalize_remote_command_path(path: &str) -> String {
    let mut path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    while path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    path
}

pub(crate) fn remote_file_name(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("remote")
        .to_string()
}

pub(crate) fn sanitize_download_file_name(file_name: &str) -> String {
    let sanitized: String = file_name
        .chars()
        .map(|ch| {
            if ch == '/' || ch == '\\' || ch.is_control() {
                '_'
            } else {
                ch
            }
        })
        .collect();
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        "download".to_string()
    } else {
        sanitized
    }
}

pub(crate) fn task_panel_item(task: &crate::app::TaskSnapshot) -> crate::tui::TaskPanelItem {
    let (kind, label) = match &task.kind {
        crate::app::TaskKind::Download { local_path, .. } => {
            ("download", file_name_label(local_path))
        }
        crate::app::TaskKind::LocalOpen { local_path, .. } => ("open", file_name_label(local_path)),
        crate::app::TaskKind::RemoteOperation { label, .. } => ("remote", label.clone()),
    };
    crate::tui::TaskPanelItem {
        id: task.id.value(),
        kind: kind.to_string(),
        state: task_state_label(task.state).to_string(),
        bytes_done: task.progress.bytes_done,
        bytes_total: task.progress.bytes_total,
        speed_bytes_per_sec: task.progress.speed_bytes_per_sec,
        label,
    }
}

pub(crate) fn file_name_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(path)
        .to_string()
}

pub(crate) const fn task_state_label(state: crate::app::TaskState) -> &'static str {
    match state {
        crate::app::TaskState::Queued => "queued",
        crate::app::TaskState::Running => "running",
        crate::app::TaskState::CancelRequested => "cancelling",
        crate::app::TaskState::Completed => "done",
        crate::app::TaskState::Failed => "failed",
        crate::app::TaskState::Cancelled => "cancelled",
    }
}

pub(crate) fn generate_video_token(
    epoch: u64,
) -> Result<crate::preview::VideoToken, crate::preview::VideoPreviewError> {
    let mut bytes = [0_u8; 16];
    if File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .is_err()
    {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0_u128, |duration| duration.as_nanos())
            .to_le_bytes();
        bytes[..8].copy_from_slice(&epoch.to_le_bytes());
        bytes[8..].copy_from_slice(&nanos[..8]);
    }
    let mut token = String::from("tok_");
    for byte in bytes {
        let _ = write!(&mut token, "{byte:02x}");
    }
    token.push('_');
    token.push_str(&epoch.to_string());
    crate::preview::VideoToken::new(token)
}

pub(crate) fn preview_key_for_entry(
    account_uid: u64,
    entry: &crate::pcs::FileEntry,
) -> crate::preview::PreviewKey {
    crate::preview::PreviewKey {
        account_uid,
        fs_id: entry.fs_id,
        mtime: entry.mtime,
        size: nonnegative_size(entry.size),
    }
}

pub(crate) fn nonnegative_size(size: i64) -> u64 {
    u64::try_from(size).unwrap_or(0)
}

pub(crate) fn pcs_user_agent_for_config(config: &crate::config::PcsConfig) -> &str {
    if !config.pcs_ua.is_empty() {
        config.pcs_ua.as_str()
    } else if !config.user_agent.is_empty() {
        config.user_agent.as_str()
    } else if !config.pan_ua.is_empty() {
        config.pan_ua.as_str()
    } else {
        "netdisk"
    }
}

pub(crate) fn pan_user_agent_for_config(config: &crate::config::PcsConfig) -> &str {
    if !config.pan_ua.is_empty() {
        config.pan_ua.as_str()
    } else if !config.user_agent.is_empty() {
        config.user_agent.as_str()
    } else if !config.pcs_ua.is_empty() {
        config.pcs_ua.as_str()
    } else {
        "netdisk"
    }
}

pub(crate) fn download_dir_for_config(config: &crate::config::PcsConfig) -> PathBuf {
    if let Some(path) = env::var_os(ENV_DOWNLOAD_DIR).map(PathBuf::from) {
        return path;
    }
    if config.savedir.is_empty() {
        env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        PathBuf::from(config.savedir.as_str())
    }
}

pub(crate) fn proxy_from_config_or_env(
    config_proxy: &str,
    env_proxy: Option<OsString>,
) -> Option<String> {
    if !config_proxy.is_empty() {
        return Some(config_proxy.to_string());
    }
    env_proxy
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.is_empty())
}

pub(crate) fn persist_workdir_if_active(
    account_uid: u64,
    config_path: &Path,
    current_path: &str,
) -> Result<(), crate::config::ConfigError> {
    if account_uid == 0 || current_path.is_empty() {
        return Ok(());
    }
    crate::config::save_active_user_workdir(config_path, current_path)
}

pub(crate) fn breadcrumb_preferred_selection(
    current_path: &str,
    target_path: &str,
) -> Option<String> {
    if current_path == target_path {
        return None;
    }
    let current = if current_path == "/" {
        "/"
    } else {
        current_path.trim_end_matches('/')
    };
    let target = if target_path == "/" {
        "/"
    } else {
        target_path.trim_end_matches('/')
    };
    if current.is_empty() || target.is_empty() {
        return None;
    }

    if target == "/" {
        let child = current.trim_start_matches('/').split('/').next()?;
        return Some(format!("/{child}"));
    }

    let rest = current.strip_prefix(target)?.strip_prefix('/')?;
    let child = rest.split('/').next()?;
    Some(format!("{target}/{child}"))
}

pub(crate) fn bduss_from_user(user: &crate::config::BaiduUser) -> Option<String> {
    if !user.bduss.is_empty() {
        return Some(user.bduss.clone());
    }

    cookie_value(user.cookies.as_str(), "BDUSS").map(str::to_string)
}

pub(crate) fn cookie_value<'a>(cookies: &'a str, key: &str) -> Option<&'a str> {
    cookies.split(';').find_map(|item| {
        let (name, value) = item.trim().split_once('=')?;
        if name == key && !value.is_empty() {
            Some(value)
        } else {
            None
        }
    })
}

pub(crate) fn pcs_error_label(err: &crate::pcs::PcsError) -> String {
    match err {
        crate::pcs::PcsError::Cancelled { .. } => "cancelled".to_string(),
        crate::pcs::PcsError::Parse { .. } => "parse".to_string(),
        crate::pcs::PcsError::Network { .. } => "network".to_string(),
        crate::pcs::PcsError::Remote { code, .. } => format!("remote_code={code}"),
    }
}

pub(crate) fn io_error_label(err: &std::io::Error) -> String {
    format!("io:{:?}", err.kind())
}
