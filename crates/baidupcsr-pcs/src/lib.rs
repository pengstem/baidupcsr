use std::{
    fmt,
    io::Read,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use md5::{Digest as Md5Digest, Md5};
use reqwest::header::{COOKIE, RANGE, USER_AGENT};
use serde::Deserialize;
use sha1::{Digest as Sha1Digest, Sha1};
use url::form_urlencoded;

pub const fn crate_name() -> &'static str {
    "baidupcsr-pcs"
}

const PAN_APP_ID: i64 = 250528;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PcsError {
    Cancelled {
        operation: String,
    },
    Parse {
        operation: String,
        message: String,
    },
    Network {
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
            Self::Cancelled { operation } => {
                write!(f, "{operation}: cancelled")
            }
            Self::Parse { operation, message } => {
                write!(f, "{operation}: failed to parse response: {message}")
            }
            Self::Network { operation, message } => {
                write!(f, "{operation}: network error: {message}")
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
pub struct ByteRange {
    start: u64,
    end_inclusive: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PcsAuth {
    cookie_header: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockingPcsClientOptions {
    pub user_agent: String,
    pub pan_user_agent: String,
    pub proxy: Option<String>,
    pub timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct BlockingPcsClient {
    api: PcsApi,
    auth: PcsAuth,
    options: BlockingPcsClientOptions,
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

    pub fn download_request(&self, path: &str) -> PcsRequest {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("app_id", &self.options.app_id.to_string())
            .append_pair("method", "download")
            .append_pair("path", path)
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

    pub fn locatedownload_request(&self, path: &str, sign: &LocateDownloadSign) -> PcsRequest {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("ant", "1")
            .append_pair("check_blue", "1")
            .append_pair("es", "1")
            .append_pair("esl", "1")
            .append_pair("app_id", &PAN_APP_ID.to_string())
            .append_pair("method", "locatedownload")
            .append_pair("path", path)
            .append_pair("ver", "4.0")
            .append_pair("clienttype", "17")
            .append_pair("channel", "0")
            .append_pair("apn_id", "1_0")
            .append_pair("freeisp", "0")
            .append_pair("queryfree", "0")
            .append_pair("use", "0")
            .append_pair("time", &sign.time.to_string())
            .append_pair("rand", &sign.rand)
            .append_pair("devuid", &sign.devuid)
            .append_pair("cuid", &sign.devuid)
            .finish();

        PcsRequest {
            method: "POST",
            url: format!(
                "{}://{}/rest/2.0/pcs/file?{}",
                scheme(self.options.enable_https),
                self.options.pcs_addr,
                query
            ),
        }
    }
}

impl ByteRange {
    pub fn new(start: u64, end_inclusive: u64) -> Result<Self, PcsError> {
        if start > end_inclusive {
            return Err(PcsError::Network {
                operation: "download".to_string(),
                message: "range start must be before range end".to_string(),
            });
        }

        Ok(Self {
            start,
            end_inclusive,
        })
    }

    pub const fn prefix(length: u64) -> Self {
        Self {
            start: 0,
            end_inclusive: length.saturating_sub(1),
        }
    }

    pub fn try_prefix(length: u64) -> Result<Self, PcsError> {
        if length == 0 {
            return Err(PcsError::Network {
                operation: "download".to_string(),
                message: "range length must be greater than zero".to_string(),
            });
        }

        Ok(Self::prefix(length))
    }

    pub fn header_value(&self) -> String {
        format!("bytes={}-{}", self.start, self.end_inclusive)
    }

    pub fn read_limit_bytes(&self) -> u64 {
        self.end_inclusive
            .saturating_sub(self.start)
            .saturating_add(2)
    }
}

impl PcsAuth {
    pub fn from_cookie_header(cookie_header: impl Into<String>) -> Self {
        Self {
            cookie_header: cookie_header.into(),
        }
    }

    pub fn from_bduss_stoken(
        bduss: &str,
        stoken: Option<&str>,
        baiduid: Option<&str>,
    ) -> Option<Self> {
        if bduss.is_empty() {
            return None;
        }

        let mut parts = vec![format!("BDUSS={bduss}")];
        if let Some(stoken) = stoken.filter(|value| !value.is_empty()) {
            parts.push(format!("STOKEN={stoken}"));
        }
        if let Some(baiduid) = baiduid.filter(|value| !value.is_empty()) {
            parts.push(format!("BAIDUID={baiduid}"));
        }

        Some(Self {
            cookie_header: parts.join("; "),
        })
    }

    pub fn cookie_header(&self) -> &str {
        &self.cookie_header
    }
}

impl BlockingPcsClientOptions {
    pub fn new(user_agent: impl Into<String>, proxy: Option<String>) -> Self {
        let user_agent = user_agent.into();
        Self {
            pan_user_agent: user_agent.clone(),
            user_agent,
            proxy,
            timeout: Duration::from_secs(15),
        }
    }

    pub fn new_with_pan(
        user_agent: impl Into<String>,
        pan_user_agent: impl Into<String>,
        proxy: Option<String>,
    ) -> Self {
        Self {
            user_agent: user_agent.into(),
            pan_user_agent: pan_user_agent.into(),
            proxy,
            timeout: Duration::from_secs(15),
        }
    }
}

impl BlockingPcsClient {
    pub fn new(api: PcsApi, auth: PcsAuth, options: BlockingPcsClientOptions) -> Self {
        Self { api, auth, options }
    }

    pub fn list(&self, path: &str, order: OrderOptions) -> Result<Vec<FileEntry>, PcsError> {
        let request = self.api.list_request(path, order);
        let client = self.build_client("list")?;
        let builder = client
            .get(&request.url)
            .header(USER_AGENT, self.options.user_agent.as_str())
            .header(COOKIE, self.auth.cookie_header());
        if !request.method.eq_ignore_ascii_case("GET") {
            return Err(PcsError::Network {
                operation: "list".to_string(),
                message: format!("unsupported method {}", request.method),
            });
        }

        let response = builder.send().map_err(|source| PcsError::Network {
            operation: "list".to_string(),
            message: source.to_string(),
        })?;
        let status = response.status();
        let text = response.text().map_err(|source| PcsError::Network {
            operation: "list".to_string(),
            message: source.to_string(),
        })?;

        if !status.is_success() {
            return Err(parse_remote_error("list", &text));
        }

        parse_file_list(&text)
    }

    pub fn download_range(&self, path: &str, range: ByteRange) -> Result<Vec<u8>, PcsError> {
        self.download_range_with_cancel(path, range, || false)
    }

    pub fn locate_download_urls(
        &self,
        path: &str,
        uid: u64,
        bduss: &str,
    ) -> Result<Vec<String>, PcsError> {
        let sign = locatedownload_sign_with(now_unix_seconds(), &dev_uid(bduss), uid, bduss);
        let request = self.api.locatedownload_request(path, &sign);
        let client = self.build_client("locatedownload")?;
        let builder = client
            .post(&request.url)
            .header(USER_AGENT, self.options.pan_user_agent.as_str())
            .header(COOKIE, self.auth.cookie_header());
        if !request.method.eq_ignore_ascii_case("POST") {
            return Err(PcsError::Network {
                operation: "locatedownload".to_string(),
                message: format!("unsupported method {}", request.method),
            });
        }

        let response = builder.send().map_err(|source| PcsError::Network {
            operation: "locatedownload".to_string(),
            message: source.to_string(),
        })?;
        let status = response.status();
        let text = response.text().map_err(|source| PcsError::Network {
            operation: "locatedownload".to_string(),
            message: source.to_string(),
        })?;

        if !status.is_success() {
            return Err(parse_remote_error("locatedownload", &text));
        }

        let info = parse_locatedownload(&text)?;
        let urls = select_download_urls(&info, self.api.options.enable_https);
        if urls.is_empty() {
            return Err(PcsError::Parse {
                operation: "locatedownload".to_string(),
                message: "no usable download url in response".to_string(),
            });
        }

        Ok(urls)
    }

    pub fn download_range_with_cancel<F>(
        &self,
        path: &str,
        range: ByteRange,
        mut is_cancelled: F,
    ) -> Result<Vec<u8>, PcsError>
    where
        F: FnMut() -> bool,
    {
        if is_cancelled() {
            return Err(PcsError::Cancelled {
                operation: "download".to_string(),
            });
        }

        let request = self.api.download_request(path);
        let client = self.build_client("download")?;
        let builder = client
            .get(&request.url)
            .header(USER_AGENT, self.options.user_agent.as_str())
            .header(COOKIE, self.auth.cookie_header())
            .header(RANGE, range.header_value());
        if !request.method.eq_ignore_ascii_case("GET") {
            return Err(PcsError::Network {
                operation: "download".to_string(),
                message: format!("unsupported method {}", request.method),
            });
        }

        if is_cancelled() {
            return Err(PcsError::Cancelled {
                operation: "download".to_string(),
            });
        }

        let response = builder.send().map_err(|source| PcsError::Network {
            operation: "download".to_string(),
            message: source.to_string(),
        })?;

        read_bounded_response(response, range, is_cancelled)
    }

    pub fn download_url_range_with_cancel<F>(
        &self,
        url: &str,
        range: ByteRange,
        mut is_cancelled: F,
    ) -> Result<Vec<u8>, PcsError>
    where
        F: FnMut() -> bool,
    {
        if is_cancelled() {
            return Err(PcsError::Cancelled {
                operation: "download".to_string(),
            });
        }

        let client = self.build_client("download")?;
        let builder = client
            .get(url)
            .header(USER_AGENT, self.options.pan_user_agent.as_str())
            .header(COOKIE, self.auth.cookie_header())
            .header(RANGE, range.header_value());
        if is_cancelled() {
            return Err(PcsError::Cancelled {
                operation: "download".to_string(),
            });
        }

        let response = builder.send().map_err(|source| PcsError::Network {
            operation: "download".to_string(),
            message: source.to_string(),
        })?;

        read_bounded_response(response, range, is_cancelled)
    }

    fn build_client(&self, operation: &str) -> Result<reqwest::blocking::Client, PcsError> {
        let mut builder = reqwest::blocking::Client::builder().timeout(self.options.timeout);
        if let Some(proxy) = self
            .options
            .proxy
            .as_ref()
            .filter(|value| !value.is_empty())
        {
            let proxy = reqwest::Proxy::all(proxy).map_err(|source| PcsError::Network {
                operation: operation.to_string(),
                message: source.to_string(),
            })?;
            builder = builder.proxy(proxy);
        }

        builder.build().map_err(|source| PcsError::Network {
            operation: operation.to_string(),
            message: source.to_string(),
        })
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
    #[serde(default)]
    pub app_id: i64,
    pub path: String,
    pub server_filename: String,
    #[serde(default)]
    pub ctime: i64,
    #[serde(default)]
    pub mtime: i64,
    #[serde(default)]
    pub md5: String,
    #[serde(default)]
    pub block_list: Vec<String>,
    #[serde(default)]
    pub size: i64,
    #[serde(default, rename = "isdir", deserialize_with = "int_to_bool")]
    pub is_dir: bool,
    #[serde(default, rename = "ifhassubdir", deserialize_with = "int_to_bool")]
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
    #[serde(default)]
    errno: i64,
    list: Vec<FileEntry>,
}

#[derive(Deserialize)]
struct LocateDownloadResponse {
    #[serde(default)]
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

fn read_bounded_response<F>(
    response: reqwest::blocking::Response,
    range: ByteRange,
    mut is_cancelled: F,
) -> Result<Vec<u8>, PcsError>
where
    F: FnMut() -> bool,
{
    if is_cancelled() {
        return Err(PcsError::Cancelled {
            operation: "download".to_string(),
        });
    }

    let status = response.status();
    if !status.is_success() {
        let text = response.text().map_err(|source| PcsError::Network {
            operation: "download".to_string(),
            message: source.to_string(),
        })?;
        return Err(parse_remote_error("download", &text));
    }

    let mut limited = response.take(range.read_limit_bytes());
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        if is_cancelled() {
            return Err(PcsError::Cancelled {
                operation: "download".to_string(),
            });
        }

        let read = limited
            .read(&mut buffer)
            .map_err(|source| PcsError::Network {
                operation: "download".to_string(),
                message: source.to_string(),
            })?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
    }

    Ok(bytes)
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
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
