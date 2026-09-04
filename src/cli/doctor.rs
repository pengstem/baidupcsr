use super::*;

pub(crate) fn print_doctor(config_dir: Option<PathBuf>) {
    println!("baidupcsr doctor");
    println!("redacted: true");
    let config_path = crate::config::discover_config_file(config_dir);
    let ui_config_path = crate::config::discover_ui_config_file();
    let cache_dir = crate::config::discover_cache_dir();
    println!("config_path: {}", config_path.display());
    println!("ui_config_path: {}", ui_config_path.display());
    println!("cache_dir: {}", cache_dir.display());
    let config_permission = match fs::metadata(&config_path) {
        Ok(metadata) => config_metadata_permission_label(&metadata),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => "missing".to_string(),
        Err(err) => io_error_label(&err),
    };
    println!("config_permission: {config_permission}");
    match crate::config::load_ui_config_or_default(&ui_config_path) {
        Ok(config) => {
            println!(
                "video_preview: chunk={} chunks={} budget={}",
                config.video_preview.chunk_size_bytes,
                config.video_preview.max_chunks,
                config.video_preview.memory_limit_bytes()
            );
        }
        Err(err) => {
            println!("video_preview: unknown");
            println!("ui_config_error: {err}");
        }
    }
    println!(
        "mpv: {}",
        if program_available("mpv") {
            "present"
        } else {
            "missing"
        }
    );

    match crate::config::load_config(&config_path) {
        Ok(config) => print_loaded_config(&config),
        Err(err) => {
            println!("config: not loaded");
            println!("config_error: {err}");
            println!(
                "repair: set BAIDUPCS_GO_CONFIG_DIR to your BaiduPCS-Go config directory or run BaiduPCS-Go login first"
            );
        }
    }
}

#[cfg(unix)]
pub(crate) fn config_metadata_permission_label(metadata: &fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;

    let mode = metadata.permissions().mode() & 0o777;
    if metadata.permissions().readonly() {
        format!("readonly mode={mode:03o}")
    } else if mode & 0o077 == 0 {
        format!("owner-only mode={mode:03o}")
    } else {
        format!("too-open mode={mode:03o}")
    }
}

#[cfg(not(unix))]
pub(crate) fn config_metadata_permission_label(metadata: &fs::Metadata) -> String {
    if metadata.permissions().readonly() {
        "readonly".to_string()
    } else {
        "present".to_string()
    }
}

pub(crate) fn print_loaded_config(config: &crate::config::PcsConfig) {
    println!("config: loaded");
    println!("active_uid: {}", config.baidu_active_uid);
    println!("accounts: {}", config.baidu_user_list.len());

    match config.active_user() {
        Some(user) => {
            let auth = user.auth_presence();
            println!("workdir: {}", user.workdir);
            println!("bduss: {}", presence(auth.bduss));
            println!("stoken: {}", presence(auth.stoken));
            println!("cookies: {}", presence(auth.cookies));
            println!("access_token: {}", presence(auth.access_token));
        }
        None => {
            println!("active_user: missing");
        }
    }
}

pub(crate) const fn presence(value: bool) -> &'static str {
    if value {
        "present"
    } else {
        "missing"
    }
}
