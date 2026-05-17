use std::fmt;

use md5::{Digest as Md5Digest, Md5};
use serde::Deserialize;
use sha1::{Digest as Sha1Digest, Sha1};
use url::form_urlencoded;

pub const fn crate_name() -> &'static str {
    "baidupcsr-pcs"
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PcsError {
    Parse {
        operation: String,
        message: String,
    },
    Remote {
        operation: String,
        code: i64,
        message: String,
    },
}

impl fmt::Display for PcsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse { operation, message } => {
                write!(f, "{operation}: failed to parse response: {message}")
            }
            Self::Remote {
                operation,
                code,
                message,
            } => {
                write!(f, "{operation}: remote error {code}: {message}")
            }
        }
    }
}

impl std::error::Error for PcsError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PcsClientOptions {
    pub app_id: i64,
    pub pcs_addr: String,
    pub enable_https: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PcsApi {
    options: PcsClientOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PcsRequest {
    pub method: &'static str,
    pub url: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderBy {
    Name,
    Time,
    Size,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderDirection {
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrderOptions {
    pub by: OrderBy,
    pub direction: OrderDirection,
}

impl PcsApi {
    pub fn new(options: PcsClientOptions) -> Self {
        Self { options }
    }

    pub fn list_request(&self, path: &str, order: OrderOptions) -> PcsRequest {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("app_id", &self.options.app_id.to_string())
            .append_pair("method", "list")
            .append_pair("path", path)
            .append_pair("by", order.by.as_str())
            .append_pair("order", order.direction.as_str())
            .finish();

        PcsRequest {
            method: "GET",
            url: format!(
                "{}://{}/rest/2.0/pcs/file?{}",
                scheme(self.options.enable_https),
                self.options.pcs_addr,
                query
            ),
        }
    }
}

impl OrderBy {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Time => "time",
            Self::Size => "size",
        }
    }
}

impl OrderDirection {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct FileEntry {
    pub fs_id: i64,
    pub app_id: i64,
    pub path: String,
    pub server_filename: String,
    pub ctime: i64,
    pub mtime: i64,
    pub md5: String,
    #[serde(default)]
    pub block_list: Vec<String>,
    pub size: i64,
    #[serde(rename = "isdir", deserialize_with = "int_to_bool")]
    pub is_dir: bool,
    #[serde(rename = "ifhassubdir", deserialize_with = "int_to_bool")]
    pub has_subdir: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LocateDownloadInfo {
    pub urls: Vec<DownloadUrlInfo>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DownloadUrlInfo {
    pub url: String,
    pub encrypt: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocateDownloadSign {
    pub time: i64,
    pub rand: String,
    pub devuid: String,
}

impl LocateDownloadSign {
    pub fn url_param(&self) -> String {
        format!(
            "time={}&rand={}&devuid={}&cuid={}",
            self.time, self.rand, self.devuid, self.devuid
        )
    }
}

pub fn parse_file_list(data: &str) -> Result<Vec<FileEntry>, PcsError> {
    let response =
        serde_json::from_str::<ListResponse>(data).map_err(|source| PcsError::Parse {
            operation: "list".to_string(),
            message: source.to_string(),
        })?;

    if response.errno != 0 {
        return Err(PcsError::Remote {
            operation: "list".to_string(),
            code: response.errno,
            message: "remote list error".to_string(),
        });
    }

    Ok(response.list)
}

pub fn parse_locatedownload(data: &str) -> Result<LocateDownloadInfo, PcsError> {
    let response =
        serde_json::from_str::<LocateDownloadResponse>(data).map_err(|source| PcsError::Parse {
            operation: "locatedownload".to_string(),
            message: source.to_string(),
        })?;

    if response.errno != 0 {
        return Err(PcsError::Remote {
            operation: "locatedownload".to_string(),
            code: response.errno,
            message: "remote locatedownload error".to_string(),
        });
    }

    Ok(LocateDownloadInfo {
        urls: response.urls,
    })
}

pub fn parse_remote_error(operation: &str, data: &str) -> PcsError {
    match serde_json::from_str::<RemoteErrorResponse>(data) {
        Ok(response) => PcsError::Remote {
            operation: operation.to_string(),
            code: response.error_code.or(response.errno).unwrap_or(-1),
            message: response
                .error_msg
                .unwrap_or_else(|| "remote error".to_string()),
        },
        Err(source) => PcsError::Parse {
            operation: operation.to_string(),
            message: source.to_string(),
        },
    }
}

pub fn select_download_urls(info: &LocateDownloadInfo, https: bool) -> Vec<String> {
    info.urls
        .iter()
        .filter(|url| url.encrypt == 0)
        .map(|url| normalize_scheme(&url.url, https))
        .collect()
}

pub fn dev_uid(feature: &str) -> String {
    let mut md5 = Md5::new();
    Md5Digest::update(&mut md5, feature.as_bytes());
    format!("{}|0", hex_upper(&Md5Digest::finalize(md5)))
}

pub fn locatedownload_sign_with(
    time: i64,
    devuid: &str,
    uid: u64,
    bduss: &str,
) -> LocateDownloadSign {
    let mut bduss_sha1 = Sha1::new();
    Sha1Digest::update(&mut bduss_sha1, bduss.as_bytes());
    let bduss_sha1_hex = hex_lower(&Sha1Digest::finalize(bduss_sha1));

    let mut rand_sha1 = Sha1::new();
    Sha1Digest::update(&mut rand_sha1, bduss_sha1_hex.as_bytes());
    Sha1Digest::update(&mut rand_sha1, uid.to_string().as_bytes());
    Sha1Digest::update(&mut rand_sha1, b"ebrcUYiuxaZv2XGu7KIYKxUrqfnOfpDF");
    Sha1Digest::update(&mut rand_sha1, time.to_string().as_bytes());
    Sha1Digest::update(&mut rand_sha1, devuid.as_bytes());

    LocateDownloadSign {
        time,
        rand: hex_lower(&Sha1Digest::finalize(rand_sha1)),
        devuid: devuid.to_string(),
    }
}

#[derive(Deserialize)]
struct ListResponse {
    errno: i64,
    list: Vec<FileEntry>,
}

#[derive(Deserialize)]
struct LocateDownloadResponse {
    errno: i64,
    urls: Vec<DownloadUrlInfo>,
}

#[derive(Deserialize)]
struct RemoteErrorResponse {
    errno: Option<i64>,
    error_code: Option<i64>,
    error_msg: Option<String>,
}

fn int_to_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = i64::deserialize(deserializer)?;
    Ok(value != 0)
}

fn scheme(https: bool) -> &'static str {
    if https {
        "https"
    } else {
        "http"
    }
}

fn normalize_scheme(raw: &str, https: bool) -> String {
    match raw.split_once("://") {
        Some((_, rest)) => format!("{}://{}", scheme(https), rest),
        None => raw.to_string(),
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        push_hex(&mut out, *byte, false);
    }
    out
}

fn hex_upper(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        push_hex(&mut out, *byte, true);
    }
    out
}

fn push_hex(out: &mut String, byte: u8, upper: bool) {
    let alphabet = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    out.push(char::from(alphabet[usize::from(byte >> 4)]));
    out.push(char::from(alphabet[usize::from(byte & 0x0f)]));
}
