//! Configuration: the BaiduPCS-Go account file (read, plus `workdir` written
//! back) and baidupcsr's own `ui_config.toml`.

use std::env;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use atomic_write_file::AtomicWriteFile;
use serde::Deserialize;
use serde_json::Value;

use crate::app::fetch::Account;
use crate::pcs::{PcsClient, PcsSettings};

const PCS_CONFIG_FILE: &str = "pcs_config.json";
const ENV_PCS_CONFIG_DIR: &str = "BAIDUPCS_GO_CONFIG_DIR";
const ENV_UI_CONFIG: &str = "BAIDUPCSR_UI_CONFIG";
const ENV_CACHE_DIR: &str = "BAIDUPCSR_CACHE_DIR";
const ENV_DOWNLOAD_DIR: &str = "BAIDUPCSR_DOWNLOAD_DIR";
const ENV_PROXY: &str = "BAIDUPCSR_PROXY";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot write {}: {source}", path.display())]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid {}: {message}", path.display())]
    Invalid { path: PathBuf, message: String },
}

impl ConfigError {
    fn invalid(path: &Path, message: impl ToString) -> Self {
        Self::Invalid {
            path: path.to_path_buf(),
            message: message.to_string(),
        }
    }
}

/// BaiduPCS-Go's `pcs_config.json`. Only the fields we use; missing fields
/// fall back to defaults so older or newer files still load.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
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

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct BaiduUser {
    pub uid: u64,
    pub name: String,
    pub bduss: String,
    pub stoken: String,
    pub baiduid: String,
    pub cookies: String,
    pub accesstoken: String,
    pub workdir: String,
}

impl PcsConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let data = std::fs::read(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        serde_json::from_slice(&data).map_err(|error| ConfigError::invalid(path, error))
    }

    pub fn active_user(&self) -> Option<&BaiduUser> {
        self.baidu_user_list
            .iter()
            .find(|user| user.uid == self.baidu_active_uid)
    }

    /// First non-empty user agent in order of preference.
    fn user_agent<'a>(&'a self, preferred: &'a str) -> String {
        [preferred, &self.user_agent, &self.pan_ua, &self.pcs_ua]
            .into_iter()
            .find(|ua| !ua.is_empty())
            .unwrap_or("netdisk")
            .to_string()
    }

    pub fn client(&self, user: &BaiduUser, proxy: Option<String>) -> Result<PcsClient, String> {
        let cookie = user
            .cookie_header()
            .ok_or("account has no BDUSS or cookies")?;
        let settings = PcsSettings {
            app_id: self.appid,
            pcs_addr: if self.pcs_addr.is_empty() {
                "pcs.baidu.com".to_string()
            } else {
                self.pcs_addr.clone()
            },
            https: self.enable_https,
            pcs_ua: self.user_agent(&self.pcs_ua),
            pan_ua: self.user_agent(&self.pan_ua),
            proxy: proxy.or_else(|| Some(self.proxy.clone()).filter(|proxy| !proxy.is_empty())),
        };
        PcsClient::new(settings, cookie).map_err(|error| error.to_string())
    }
}

impl BaiduUser {
    fn cookie_header(&self) -> Option<String> {
        if !self.cookies.is_empty() {
            return Some(self.cookies.clone());
        }
        if self.bduss.is_empty() {
            return None;
        }
        let parts = [
            ("BDUSS", &self.bduss),
            ("STOKEN", &self.stoken),
            ("BAIDUID", &self.baiduid),
        ];
        Some(
            parts
                .iter()
                .filter(|(_, value)| !value.is_empty())
                .map(|(name, value)| cookie::Cookie::new(*name, value.as_str()).to_string())
                .collect::<Vec<_>>()
                .join("; "),
        )
    }

    pub fn cookie(&self, name: &str) -> Option<&str> {
        cookie::Cookie::split_parse(self.cookies.as_str()).find_map(|cookie| {
            let cookie = cookie.ok()?;
            let value = cookie.value_raw()?;
            (cookie.name_raw()? == name && !value.is_empty()).then_some(value)
        })
    }

    pub fn account(&self) -> Option<Account> {
        let bduss = if self.bduss.is_empty() {
            self.cookie("BDUSS")?.to_string()
        } else {
            self.bduss.clone()
        };
        (self.uid != 0).then_some(Account {
            uid: self.uid,
            bduss,
        })
    }

    pub fn has_stoken(&self) -> bool {
        !self.stoken.is_empty() || self.cookie("STOKEN").is_some()
    }
}

/// Persist the active user's working directory, like BaiduPCS-Go's `cd`.
/// Edits the JSON in place so fields we do not model survive.
pub fn save_workdir(path: &Path, workdir: &str) -> Result<(), ConfigError> {
    let data = std::fs::read(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut config: Value =
        serde_json::from_slice(&data).map_err(|error| ConfigError::invalid(path, error))?;
    let active_uid = config.get("baidu_active_uid").and_then(Value::as_u64);
    let user = config
        .get_mut("baidu_user_list")
        .and_then(Value::as_array_mut)
        .and_then(|users| {
            users
                .iter_mut()
                .find(|user| user.get("uid").and_then(Value::as_u64) == active_uid)
        })
        .and_then(Value::as_object_mut)
        .ok_or_else(|| ConfigError::invalid(path, "active user not found"))?;
    if user.get("workdir").and_then(Value::as_str) == Some(workdir) {
        return Ok(());
    }
    user.insert("workdir".to_string(), Value::String(workdir.to_string()));

    let write_error = |source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    };
    let mut serialized =
        serde_json::to_string_pretty(&config).map_err(|error| ConfigError::invalid(path, error))?;
    serialized.push('\n');
    let mut file = AtomicWriteFile::open(path).map_err(write_error)?;
    file.write_all(serialized.as_bytes()).map_err(write_error)?;
    file.commit().map_err(write_error)
}

/// `ui_config.toml`. Every section and field is optional.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    pub ui: UiSection,
    pub download: DownloadConfig,
    pub network: NetworkConfig,
    pub player: PlayerConfig,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct UiSection {
    pub icons: IconSet,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IconSet {
    /// Nerd Font glyphs.
    #[default]
    Nerd,
    /// Plain text, for terminals without a Nerd Font.
    Ascii,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct DownloadConfig {
    /// Overrides BaiduPCS-Go's `savedir`.
    pub dir: Option<PathBuf>,
    pub chunk_size: u64,
    pub concurrency: usize,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            dir: None,
            chunk_size: 4 * 1024 * 1024,
            concurrency: 8,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkConfig {
    pub proxy: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PlayerConfig {
    pub command: String,
    /// Extra arguments placed before the URL.
    pub args: Vec<String>,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            command: "mpv".to_string(),
            args: Vec::new(),
        }
    }
}

impl UiConfig {
    /// Load the file if it exists, then apply environment overrides.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let mut config = match std::fs::read_to_string(path) {
            Ok(data) => Self::parse(path, &data)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(source) => {
                return Err(ConfigError::Read {
                    path: path.to_path_buf(),
                    source,
                })
            }
        };
        if let Some(dir) = env::var_os(ENV_DOWNLOAD_DIR) {
            config.download.dir = Some(PathBuf::from(dir));
        }
        if let Some(proxy) = env::var(ENV_PROXY).ok().filter(|proxy| !proxy.is_empty()) {
            config.network.proxy = Some(proxy);
        }
        Ok(config)
    }

    fn parse(path: &Path, data: &str) -> Result<Self, ConfigError> {
        let config: Self =
            toml::from_str(data).map_err(|error| ConfigError::invalid(path, error))?;
        if config.download.chunk_size < 64 * 1024 {
            return Err(ConfigError::invalid(
                path,
                "download.chunk_size must be at least 65536",
            ));
        }
        if !(1..=32).contains(&config.download.concurrency) {
            return Err(ConfigError::invalid(
                path,
                "download.concurrency must be 1..=32",
            ));
        }
        Ok(config)
    }

    pub fn download_dir(&self, pcs: &PcsConfig) -> PathBuf {
        self.download
            .dir
            .clone()
            .or_else(|| Some(PathBuf::from(&pcs.savedir)).filter(|dir| !dir.as_os_str().is_empty()))
            .or_else(dirs::download_dir)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

/// Where BaiduPCS-Go keeps `pcs_config.json`.
pub fn pcs_config_path(dir_override: Option<PathBuf>) -> PathBuf {
    let dir = dir_override
        .or_else(|| env::var_os(ENV_PCS_CONFIG_DIR).map(PathBuf::from))
        .unwrap_or_else(|| {
            let base = if cfg!(windows) {
                dirs::config_dir()
            } else {
                dirs::home_dir().map(|home| home.join(".config"))
            };
            base.unwrap_or_default().join("BaiduPCS-Go")
        });
    dir.join(PCS_CONFIG_FILE)
}

pub fn ui_config_path() -> PathBuf {
    env::var_os(ENV_UI_CONFIG).map_or_else(
        || {
            dirs::config_dir()
                .unwrap_or_default()
                .join("baidupcsr")
                .join("ui_config.toml")
        },
        PathBuf::from,
    )
}

pub fn cache_dir() -> PathBuf {
    env::var_os(ENV_CACHE_DIR).map_or_else(
        || dirs::cache_dir().unwrap_or_default().join("baidupcsr"),
        PathBuf::from,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_pcs_config_loads_with_defaults() {
        let config: PcsConfig = serde_json::from_str(
            r#"{"baidu_active_uid":1,"baidu_user_list":[{"uid":1,"cookies":"BDUSS=abc; STOKEN=s"}]}"#,
        )
        .unwrap();
        let user = config.active_user().unwrap();
        assert_eq!(user.account().unwrap().bduss, "abc");
        assert!(user.has_stoken());
        assert_eq!(config.user_agent(&config.pan_ua), "netdisk");
    }

    #[test]
    fn cookie_header_from_bduss() {
        let user = BaiduUser {
            bduss: "b".into(),
            stoken: "s".into(),
            ..BaiduUser::default()
        };
        assert_eq!(user.cookie_header().as_deref(), Some("BDUSS=b; STOKEN=s"));
        assert_eq!(BaiduUser::default().cookie_header(), None);
    }

    #[test]
    fn ui_config_sections_are_optional_and_validated() {
        let path = Path::new("ui_config.toml");
        assert_eq!(UiConfig::parse(path, "").unwrap(), UiConfig::default());
        let config = UiConfig::parse(
            path,
            "[ui]\nicons = \"ascii\"\n[download]\nconcurrency = 4\n[player]\nargs = [\"--fs\"]\n",
        )
        .unwrap();
        assert_eq!(config.ui.icons, IconSet::Ascii);
        assert_eq!(config.download.concurrency, 4);
        assert_eq!(config.download.chunk_size, 4 * 1024 * 1024);
        assert_eq!(config.player.command, "mpv");
        assert!(UiConfig::parse(path, "[download]\nconcurrency = 0\n").is_err());
        assert!(UiConfig::parse(path, "[nope]\n").is_err());
    }

    #[test]
    fn save_workdir_keeps_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PCS_CONFIG_FILE);
        std::fs::write(
            &path,
            r#"{"baidu_active_uid":1,"extra":true,"baidu_user_list":[{"uid":1,"workdir":"/"}]}"#,
        )
        .unwrap();
        save_workdir(&path, "/docs").unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["baidu_user_list"][0]["workdir"], "/docs");
        assert_eq!(saved["extra"], true);
    }
}
