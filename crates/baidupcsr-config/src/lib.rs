use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

pub const CONFIG_FILE_NAME: &str = "pcs_config.json";
pub const ENV_CONFIG_DIR: &str = "BAIDUPCS_GO_CONFIG_DIR";
pub const UI_CONFIG_FILE_NAME: &str = "ui_config.toml";
pub const ENV_UI_CONFIG: &str = "BAIDUPCSR_UI_CONFIG";
pub const ENV_CACHE_DIR: &str = "BAIDUPCSR_CACHE_DIR";

pub const fn crate_name() -> &'static str {
    "baidupcsr-config"
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetOs {
    Unix,
    Windows,
    Android,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigDiscoveryInput {
    pub config_dir_env: Option<PathBuf>,
    pub executable_dir: PathBuf,
    pub home_dir: Option<PathBuf>,
    pub appdata_dir: Option<PathBuf>,
    pub target_os: TargetOs,
}

#[derive(Debug)]
pub enum ConfigError {
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    InvalidConfig {
        path: PathBuf,
        message: String,
    },
    InvalidUiConfig {
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "failed to read config {}: {source}", path.display())
            }
            Self::Write { path, source } => {
                write!(f, "failed to write config {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(f, "failed to parse config {}: {source}", path.display())
            }
            Self::InvalidConfig { path, message } => {
                write!(f, "invalid config {}: {message}", path.display())
            }
            Self::InvalidUiConfig { path, message } => {
                write!(f, "invalid UI config {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Write { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::InvalidConfig { .. } => None,
            Self::InvalidUiConfig { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct PcsConfig {
    pub baidu_active_uid: u64,
    pub baidu_user_list: Vec<BaiduUser>,
    pub appid: i64,
    pub cache_size: i64,
    pub max_parallel: i64,
    pub max_upload_parallel: i64,
    pub max_download_load: i64,
    pub max_upload_load: i64,
    pub max_download_rate: i64,
    pub max_upload_rate: i64,
    pub user_agent: String,
    pub pcs_ua: String,
    pub pcs_addr: String,
    pub pan_ua: String,
    pub savedir: String,
    pub enable_https: bool,
    pub fix_pcs_addr: bool,
    pub force_login_username: String,
    pub proxy: String,
    pub proxy_hostnames: String,
    pub local_addrs: String,
    pub no_check: bool,
    pub ignore_illegal: bool,
    pub u_policy: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BaiduUser {
    pub uid: u64,
    pub name: String,
    pub sex: String,
    pub age: f64,
    pub bduss: String,
    pub ptoken: String,
    pub stoken: String,
    pub baiduid: String,
    pub sboxtkn: String,
    pub cookies: String,
    pub accesstoken: String,
    pub workdir: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthPresence {
    pub bduss: bool,
    pub stoken: bool,
    pub cookies: bool,
    pub access_token: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiConfig {
    pub video_preview: VideoPreviewConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoPreviewConfig {
    pub enabled: bool,
    pub chunk_size_bytes: u64,
    pub max_chunks: usize,
    pub player_profile: VideoPlayerProfile,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoPlayerProfile {
    User,
    Isolated,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            video_preview: VideoPreviewConfig {
                enabled: true,
                chunk_size_bytes: 4 * 1024 * 1024,
                max_chunks: 32,
                player_profile: VideoPlayerProfile::User,
            },
        }
    }
}

impl BaiduUser {
    pub fn auth_presence(&self) -> AuthPresence {
        AuthPresence {
            bduss: !self.bduss.is_empty() || cookie_has_key(&self.cookies, "BDUSS"),
            stoken: !self.stoken.is_empty() || cookie_has_key(&self.cookies, "STOKEN"),
            cookies: !self.cookies.is_empty(),
            access_token: !self.accesstoken.is_empty(),
        }
    }
}

pub fn discover_config_file(input: &ConfigDiscoveryInput) -> PathBuf {
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

pub fn discovery_input_from_env(config_dir_override: Option<PathBuf>) -> ConfigDiscoveryInput {
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

    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| {
            path.join(".config")
                .join("baidupcsr")
                .join(UI_CONFIG_FILE_NAME)
        })
        .unwrap_or_else(|| PathBuf::from(UI_CONFIG_FILE_NAME))
}

pub fn discover_cache_dir() -> PathBuf {
    if let Some(path) = env::var_os(ENV_CACHE_DIR).map(PathBuf::from) {
        return path;
    }

    if let Some(path) = env::var_os("XDG_CACHE_HOME").map(PathBuf::from) {
        return path.join("baidupcsr");
    }

    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| path.join(".cache").join("baidupcsr"))
        .unwrap_or_else(|| PathBuf::from(".baidupcsr-cache"))
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

pub fn load_ui_config(path: &Path) -> Result<UiConfig, ConfigError> {
    let data = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse_ui_config(path, &data)
}

fn parse_ui_config(path: &Path, data: &str) -> Result<UiConfig, ConfigError> {
    let mut config = UiConfig::default();
    for line in data.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(invalid_ui_config(path, "expected key = value"));
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"');
        match key {
            "video_preview_enabled" => {
                config.video_preview.enabled = parse_bool(path, value)?;
            }
            "video_chunk_size_bytes" => {
                config.video_preview.chunk_size_bytes = parse_u64(path, value)?;
            }
            "video_max_chunks" => {
                config.video_preview.max_chunks = parse_usize(path, value)?;
            }
            "video_player_profile" => {
                config.video_preview.player_profile = parse_video_player_profile(path, value)?;
            }
            _ => {}
        }
    }

    if config.video_preview.chunk_size_bytes == 0 || config.video_preview.max_chunks == 0 {
        return Err(invalid_ui_config(
            path,
            "video preview chunk budget must be greater than zero",
        ));
    }

    Ok(config)
}

fn parse_bool(path: &Path, value: &str) -> Result<bool, ConfigError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid_ui_config(path, "expected boolean value")),
    }
}

fn parse_u64(path: &Path, value: &str) -> Result<u64, ConfigError> {
    value
        .parse::<u64>()
        .map_err(|_| invalid_ui_config(path, "expected unsigned integer"))
}

fn parse_usize(path: &Path, value: &str) -> Result<usize, ConfigError> {
    value
        .parse::<usize>()
        .map_err(|_| invalid_ui_config(path, "expected unsigned integer"))
}

fn parse_video_player_profile(path: &Path, value: &str) -> Result<VideoPlayerProfile, ConfigError> {
    match value {
        "user" => Ok(VideoPlayerProfile::User),
        "isolated" => Ok(VideoPlayerProfile::Isolated),
        _ => Err(invalid_ui_config(
            path,
            "expected video player profile user or isolated",
        )),
    }
}

fn invalid_ui_config(path: &Path, message: &str) -> ConfigError {
    ConfigError::InvalidUiConfig {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

fn invalid_config(path: &Path, message: &str) -> ConfigError {
    ConfigError::InvalidConfig {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

pub fn active_user(config: &PcsConfig) -> Option<&BaiduUser> {
    config
        .baidu_user_list
        .iter()
        .find(|user| user.uid == config.baidu_active_uid)
}

fn cookie_has_key(cookies: &str, key: &str) -> bool {
    cookies.split(';').any(|item| {
        let item = item.trim();
        item.strip_prefix(key)
            .and_then(|rest| rest.strip_prefix('='))
            .is_some_and(|value| !value.is_empty())
    })
}

fn current_target_os() -> TargetOs {
    if cfg!(windows) {
        TargetOs::Windows
    } else if cfg!(target_os = "android") {
        TargetOs::Android
    } else {
        TargetOs::Unix
    }
}
