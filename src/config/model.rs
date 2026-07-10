use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TargetOs {
    Unix,
    Windows,
    Android,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConfigDiscoveryInput {
    pub(crate) config_dir_env: Option<PathBuf>,
    pub(crate) executable_dir: PathBuf,
    pub(crate) home_dir: Option<PathBuf>,
    pub(crate) appdata_dir: Option<PathBuf>,
    pub(crate) target_os: TargetOs,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config {path}: {source}", path = path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write config {path}: {source}", path = path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config {path}: {source}", path = path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid config {path}: {message}", path = path.display())]
    InvalidConfig { path: PathBuf, message: String },
    #[error("invalid UI config {path}: {message}", path = path.display())]
    InvalidUiConfig { path: PathBuf, message: String },
}

#[derive(Clone, Debug, Deserialize)]
#[allow(
    dead_code,
    reason = "deserialized fields preserve BaiduPCS-Go config compatibility"
)]
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
#[allow(
    dead_code,
    reason = "deserialized fields preserve BaiduPCS-Go account compatibility"
)]
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

impl PcsConfig {
    pub fn active_user(&self) -> Option<&BaiduUser> {
        self.baidu_user_list
            .iter()
            .find(|user| user.uid == self.baidu_active_uid)
    }
}

impl VideoPreviewConfig {
    pub fn memory_limit_bytes(&self) -> u64 {
        self.chunk_size_bytes
            .saturating_mul(u64::try_from(self.max_chunks).unwrap_or(u64::MAX))
    }
}

impl VideoPlayerProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Isolated => "isolated",
        }
    }
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
