use super::*;

pub fn discover_config_file(config_dir_override: Option<PathBuf>) -> PathBuf {
    let input = discovery_input_from_env(config_dir_override);
    discover_config_file_from_input(&input)
}

pub(crate) fn discover_config_file_from_input(input: &ConfigDiscoveryInput) -> PathBuf {
    if let Some(config_dir) = &input.config_dir_env {
        let config_dir = if config_dir.is_absolute() {
            config_dir.clone()
        } else {
            input.executable_dir.join(config_dir)
        };
        return config_dir.join(CONFIG_FILE_NAME);
    }

    let legacy_path = input.executable_dir.join(CONFIG_FILE_NAME);
    if legacy_path.exists() {
        return legacy_path;
    }

    match input.target_os {
        TargetOs::Windows => input
            .appdata_dir
            .as_ref()
            .map(|path| path.join("BaiduPCS-Go").join(CONFIG_FILE_NAME))
            .unwrap_or(legacy_path),
        TargetOs::Unix => input
            .home_dir
            .as_ref()
            .map(|path| {
                path.join(".config")
                    .join("BaiduPCS-Go")
                    .join(CONFIG_FILE_NAME)
            })
            .unwrap_or(legacy_path),
        TargetOs::Android => PathBuf::from("/sdcard/Download")
            .join("BaiduPCS-Go")
            .join(CONFIG_FILE_NAME),
    }
}

pub(crate) fn discovery_input_from_env(
    config_dir_override: Option<PathBuf>,
) -> ConfigDiscoveryInput {
    let executable_dir = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    ConfigDiscoveryInput {
        config_dir_env: config_dir_override
            .or_else(|| env::var_os(ENV_CONFIG_DIR).map(PathBuf::from)),
        executable_dir,
        home_dir: env::var_os("HOME").map(PathBuf::from),
        appdata_dir: env::var_os("APPDATA").map(PathBuf::from),
        target_os: current_target_os(),
    }
}

pub fn load_config(path: &Path) -> Result<PcsConfig, ConfigError> {
    let data = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&data).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

pub fn save_active_user_workdir(path: &Path, workdir: &str) -> Result<(), ConfigError> {
    let data = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut config: Value = serde_json::from_str(&data).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    let active_uid = config
        .get("baidu_active_uid")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_config(path, "missing baidu_active_uid"))?;
    let users = config
        .get_mut("baidu_user_list")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid_config(path, "missing baidu_user_list"))?;
    let active_user = users
        .iter_mut()
        .find(|user| user.get("uid").and_then(Value::as_u64) == Some(active_uid))
        .ok_or_else(|| invalid_config(path, "active user not found"))?;
    let active_user = active_user
        .as_object_mut()
        .ok_or_else(|| invalid_config(path, "active user is not an object"))?;
    if active_user.get("workdir").and_then(Value::as_str) == Some(workdir) {
        return Ok(());
    }
    active_user.insert("workdir".to_string(), Value::String(workdir.to_string()));

    let mut serialized =
        serde_json::to_string_pretty(&config).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
    serialized.push('\n');
    fs::write(path, serialized).map_err(|source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    })
}

pub fn discover_ui_config_file() -> PathBuf {
    if let Some(path) = env::var_os(ENV_UI_CONFIG).map(PathBuf::from) {
        return path;
    }

    env::var_os("HOME").map(PathBuf::from).map_or_else(
        || PathBuf::from(UI_CONFIG_FILE_NAME),
        |path| {
            path.join(".config")
                .join("baidupcsr")
                .join(UI_CONFIG_FILE_NAME)
        },
    )
}

pub fn discover_cache_dir() -> PathBuf {
    if let Some(path) = env::var_os(ENV_CACHE_DIR).map(PathBuf::from) {
        return path;
    }

    if let Some(path) = env::var_os("XDG_CACHE_HOME").map(PathBuf::from) {
        return path.join("baidupcsr");
    }

    env::var_os("HOME").map(PathBuf::from).map_or_else(
        || PathBuf::from(".baidupcsr-cache"),
        |path| path.join(".cache").join("baidupcsr"),
    )
}

pub fn load_ui_config_or_default(path: &Path) -> Result<UiConfig, ConfigError> {
    match load_ui_config(path) {
        Ok(config) => Ok(config),
        Err(ConfigError::Read { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(UiConfig::default())
        }
        Err(err) => Err(err),
    }
}

pub(crate) fn load_ui_config(path: &Path) -> Result<UiConfig, ConfigError> {
    let data = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse_ui_config(path, &data)
}

pub(crate) fn parse_ui_config(path: &Path, data: &str) -> Result<UiConfig, ConfigError> {
    let config = toml::from_str::<UiConfig>(data)
        .map_err(|source| invalid_ui_config(path, &source.to_string()))?;

    if config.video_preview.chunk_size_bytes == 0 || config.video_preview.max_chunks == 0 {
        return Err(invalid_ui_config(
            path,
            "video preview chunk budget must be greater than zero",
        ));
    }

    Ok(config)
}

pub(crate) fn invalid_ui_config(path: &Path, message: &str) -> ConfigError {
    ConfigError::InvalidUiConfig {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

pub(crate) fn invalid_config(path: &Path, message: &str) -> ConfigError {
    ConfigError::InvalidConfig {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

pub(crate) fn cookie_has_key(cookies: &str, key: &str) -> bool {
    cookies.split(';').any(|item| {
        let item = item.trim();
        item.strip_prefix(key)
            .and_then(|rest| rest.strip_prefix('='))
            .is_some_and(|value| !value.is_empty())
    })
}

pub(crate) fn current_target_os() -> TargetOs {
    if cfg!(windows) {
        TargetOs::Windows
    } else if cfg!(target_os = "android") {
        TargetOs::Android
    } else {
        TargetOs::Unix
    }
}
