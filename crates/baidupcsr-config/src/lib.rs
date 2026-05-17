use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

pub const CONFIG_FILE_NAME: &str = "pcs_config.json";
pub const ENV_CONFIG_DIR: &str = "BAIDUPCS_GO_CONFIG_DIR";

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
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "failed to read config {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(f, "failed to parse config {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
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
