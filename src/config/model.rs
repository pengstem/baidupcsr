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
pub struct PcsConfig {
    pub baidu_active_uid: u64,
    pub baidu_user_list: Vec<BaiduUser>,
    pub appid: i64,
    pub user_agent: String,
    pub pcs_ua: String,
    pub pcs_addr: String,
    pub pan_ua: String,
    pub savedir: String,
    pub enable_https: bool,
    pub proxy: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BaiduUser {
    pub uid: u64,
    pub bduss: String,
    pub stoken: String,
    pub baiduid: String,
    pub cookies: String,
    pub accesstoken: String,
    pub workdir: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthPresence {
    pub bduss: bool,
    pub stoken: bool,
    pub cookies: bool,
    pub access_token: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "snake_case")]
pub struct UiConfig {
    pub video_preview: VideoPreviewConfig,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "snake_case")]
pub struct VideoPreviewConfig {
    pub chunk_size_bytes: u64,
    pub max_chunks: usize,
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

impl Default for VideoPreviewConfig {
    fn default() -> Self {
        Self {
            chunk_size_bytes: 4 * 1024 * 1024,
            max_chunks: 32,
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
