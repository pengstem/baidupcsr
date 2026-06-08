use std::{
    fmt,
    io::Read,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use md5::{Digest as Md5Digest, Md5};
use reqwest::header::{CONTENT_TYPE, COOKIE, RANGE, USER_AGENT};
use serde::{Deserialize, Serialize};
use sha1::{Digest as Sha1Digest, Sha1};
use url::form_urlencoded;

const PAN_APP_ID: i64 = 250_528;

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
struct PcsApi {
    options: PcsClientOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PcsRequest {
    url: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileOperation {
    Delete,
    Copy,
    Move,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FileOperationPair {
    pub from: String,
    pub to: String,
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
    user_agent: String,
    pan_user_agent: String,
    proxy: Option<String>,
    timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct BlockingPcsClient {
    api: PcsApi,
    auth: PcsAuth,
    options: BlockingPcsClientOptions,
}

impl PcsApi {
    fn list_request(&self, path: &str) -> PcsRequest {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("app_id", &self.options.app_id.to_string())
            .append_pair("method", "list")
            .append_pair("path", path)
            .append_pair("by", "name")
            .append_pair("order", "asc")
            .finish();

        PcsRequest {
            url: format!(
                "{}://{}/rest/2.0/pcs/file?{}",
                scheme(self.options.enable_https),
                self.options.pcs_addr,
                query
            ),
        }
    }

    fn download_request(&self, path: &str) -> PcsRequest {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("app_id", &self.options.app_id.to_string())
            .append_pair("method", "download")
            .append_pair("path", path)
            .finish();

        PcsRequest {
            url: format!(
                "{}://{}/rest/2.0/pcs/file?{}",
                scheme(self.options.enable_https),
                self.options.pcs_addr,
                query
            ),
        }
    }

    fn mkdir_request(&self, path: &str) -> PcsRequest {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("app_id", &self.options.app_id.to_string())
            .append_pair("method", "mkdir")
            .append_pair("path", path)
            .finish();

        PcsRequest {
            url: format!(
                "{}://{}/rest/2.0/pcs/file?{}",
                scheme(self.options.enable_https),
                self.options.pcs_addr,
                query
            ),
        }
    }

    fn search_request(&self, path: &str, keyword: &str, recursive: bool) -> PcsRequest {
        let recursive = if recursive { "1" } else { "0" };
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("app_id", &self.options.app_id.to_string())
            .append_pair("method", "search")
            .append_pair("path", path)
            .append_pair("wd", keyword)
            .append_pair("re", recursive)
            .finish();

        PcsRequest {
            url: format!(
                "{}://{}/rest/2.0/pcs/file?{}",
                scheme(self.options.enable_https),
                self.options.pcs_addr,
                query
            ),
        }
    }

    fn file_operation_request(&self, operation: FileOperation) -> PcsRequest {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("app_id", &self.options.app_id.to_string())
            .append_pair("method", operation.method_name())
            .finish();

        PcsRequest {
            url: format!(
                "{}://{}/rest/2.0/pcs/file?{}",
                scheme(self.options.enable_https),
                self.options.pcs_addr,
                query
            ),
        }
    }

    fn locatedownload_request(&self, path: &str, sign: &LocateDownloadSign) -> PcsRequest {
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

    pub fn prefix(length: u64) -> Result<Self, PcsError> {
        if length == 0 {
            return Err(PcsError::Network {
                operation: "download".to_string(),
                message: "range length must be greater than zero".to_string(),
            });
        }

        Ok(Self {
            start: 0,
            end_inclusive: length - 1,
        })
    }

    fn header_value(&self) -> String {
        format!("bytes={}-{}", self.start, self.end_inclusive)
    }

    const fn read_limit_bytes(&self) -> u64 {
        self.end_inclusive
            .saturating_sub(self.start)
            .saturating_add(1)
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
}

impl BlockingPcsClientOptions {
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
    pub fn new(
        api_options: PcsClientOptions,
        auth: PcsAuth,
        options: BlockingPcsClientOptions,
    ) -> Self {
        Self {
            api: PcsApi {
                options: api_options,
            },
            auth,
            options,
        }
    }

    pub fn list(&self, path: &str) -> Result<Vec<FileEntry>, PcsError> {
        let request = self.api.list_request(path);
        let client = self.build_client("list")?;
        let builder = client
            .get(&request.url)
            .header(USER_AGENT, self.options.user_agent.as_str())
            .header(COOKIE, self.auth.cookie_header.as_str());

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

        parse_file_list_for_operation("list", &text)
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
            .header(COOKIE, self.auth.cookie_header.as_str());

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

    pub fn mkdir(&self, path: &str) -> Result<(), PcsError> {
        let request = self.api.mkdir_request(path);
        self.send_file_operation_request("mkdir", &request, None)
    }

    pub fn delete_paths(&self, paths: &[String]) -> Result<(), PcsError> {
        let param = delete_param_json(paths)?;
        let request = self.api.file_operation_request(FileOperation::Delete);
        self.send_file_operation_request("delete", &request, Some(param.as_str()))
    }

    pub fn copy_paths(&self, pairs: &[FileOperationPair]) -> Result<(), PcsError> {
        let param = copy_move_param_json(pairs)?;
        let request = self.api.file_operation_request(FileOperation::Copy);
        self.send_file_operation_request("copy", &request, Some(param.as_str()))
    }

    pub fn move_paths(&self, pairs: &[FileOperationPair]) -> Result<(), PcsError> {
        let param = copy_move_param_json(pairs)?;
        let request = self.api.file_operation_request(FileOperation::Move);
        self.send_file_operation_request("move", &request, Some(param.as_str()))
    }

    pub fn search(
        &self,
        path: &str,
        keyword: &str,
        recursive: bool,
    ) -> Result<Vec<FileEntry>, PcsError> {
        let request = self.api.search_request(path, keyword, recursive);
        let client = self.build_client("search")?;
        let builder = client
            .get(&request.url)
            .header(USER_AGENT, self.options.user_agent.as_str())
            .header(COOKIE, self.auth.cookie_header.as_str());

        let response = builder.send().map_err(|source| PcsError::Network {
            operation: "search".to_string(),
            message: source.to_string(),
        })?;
        let status = response.status();
        let text = response.text().map_err(|source| PcsError::Network {
            operation: "search".to_string(),
            message: source.to_string(),
        })?;

        if !status.is_success() {
            return Err(parse_remote_error("search", &text));
        }

        parse_file_list_for_operation("search", &text)
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
            .header(COOKIE, self.auth.cookie_header.as_str())
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
            .header(COOKIE, self.auth.cookie_header.as_str())
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

    fn send_file_operation_request(
        &self,
        operation: &str,
        request: &PcsRequest,
        param: Option<&str>,
    ) -> Result<(), PcsError> {
        let client = self.build_client(operation)?;
        let mut builder = client
            .post(&request.url)
            .header(USER_AGENT, self.options.user_agent.as_str())
            .header(COOKIE, self.auth.cookie_header.as_str());
        if let Some(param) = param {
            let (content_type, body) = multipart_param_body(param);
            builder = builder.header(CONTENT_TYPE, content_type).body(body);
        }

        let response = builder.send().map_err(|source| PcsError::Network {
            operation: operation.to_string(),
            message: source.to_string(),
        })?;
        let status = response.status();
        let text = response.text().map_err(|source| PcsError::Network {
            operation: operation.to_string(),
            message: source.to_string(),
        })?;

        if !status.is_success() {
            return Err(parse_remote_error(operation, &text));
        }

        parse_file_operation_response(operation, &text)
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

impl FileOperation {
    const fn method_name(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Copy => "copy",
            Self::Move => "move",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
    #[serde(
        default,
        rename = "isdir",
        deserialize_with = "int_to_bool",
        serialize_with = "bool_to_int"
    )]
    pub is_dir: bool,
    #[serde(
        default,
        rename = "ifhassubdir",
        deserialize_with = "int_to_bool",
        serialize_with = "bool_to_int"
    )]
    pub has_subdir: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct LocateDownloadInfo {
    urls: Vec<DownloadUrlInfo>,
}

#[derive(Clone, Debug, Deserialize)]
struct DownloadUrlInfo {
    url: String,
    encrypt: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LocateDownloadSign {
    time: i64,
    rand: String,
    devuid: String,
}

fn parse_file_list_for_operation(operation: &str, data: &str) -> Result<Vec<FileEntry>, PcsError> {
    let response =
        serde_json::from_str::<ListResponse>(data).map_err(|source| PcsError::Parse {
            operation: operation.to_string(),
            message: source.to_string(),
        })?;

    if response.errno != 0 {
        return Err(PcsError::Remote {
            operation: operation.to_string(),
            code: response.errno,
            message: format!("remote {operation} error"),
        });
    }

    Ok(response.list)
}

fn parse_file_operation_response(operation: &str, data: &str) -> Result<(), PcsError> {
    let response =
        serde_json::from_str::<FileOperationResponse>(data).map_err(|source| PcsError::Parse {
            operation: operation.to_string(),
            message: source.to_string(),
        })?;

    let code = response.error_code.or(response.errno).unwrap_or(0);
    if code != 0 {
        return Err(PcsError::Remote {
            operation: operation.to_string(),
            code,
            message: response
                .error_msg
                .unwrap_or_else(|| format!("remote {operation} error")),
        });
    }

    Ok(())
}

fn delete_param_json(paths: &[String]) -> Result<String, PcsError> {
    let list = paths
        .iter()
        .map(|path| PathParam { path: path.clone() })
        .collect();
    serde_json::to_string(&PathParamList { list }).map_err(|source| PcsError::Parse {
        operation: "delete".to_string(),
        message: source.to_string(),
    })
}

fn copy_move_param_json(pairs: &[FileOperationPair]) -> Result<String, PcsError> {
    serde_json::to_string(&CopyMoveParamList { list: pairs }).map_err(|source| PcsError::Parse {
        operation: "file_operation".to_string(),
        message: source.to_string(),
    })
}

fn multipart_param_body(param: &str) -> (String, String) {
    const BOUNDARY: &str = "baidupcsr-stage9-boundary";
    (
        format!("multipart/form-data; boundary={BOUNDARY}"),
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"param\"\r\n\r\n{param}\r\n--{BOUNDARY}--\r\n"
        ),
    )
}

fn parse_locatedownload(data: &str) -> Result<LocateDownloadInfo, PcsError> {
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

fn parse_remote_error(operation: &str, data: &str) -> PcsError {
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

fn select_download_urls(info: &LocateDownloadInfo, https: bool) -> Vec<String> {
    info.urls
        .iter()
        .filter(|url| url.encrypt == 0)
        .map(|url| normalize_scheme(&url.url, https))
        .collect()
}

fn dev_uid(feature: &str) -> String {
    let mut md5 = Md5::new();
    Md5Digest::update(&mut md5, feature.as_bytes());
    format!("{:X}|0", Md5Digest::finalize(md5))
}

fn locatedownload_sign_with(time: i64, devuid: &str, uid: u64, bduss: &str) -> LocateDownloadSign {
    let mut bduss_sha1 = Sha1::new();
    Sha1Digest::update(&mut bduss_sha1, bduss.as_bytes());
    let bduss_sha1_hex = format!("{:x}", Sha1Digest::finalize(bduss_sha1));

    let mut rand_sha1 = Sha1::new();
    Sha1Digest::update(&mut rand_sha1, bduss_sha1_hex.as_bytes());
    Sha1Digest::update(&mut rand_sha1, uid.to_string().as_bytes());
    Sha1Digest::update(&mut rand_sha1, b"ebrcUYiuxaZv2XGu7KIYKxUrqfnOfpDF");
    Sha1Digest::update(&mut rand_sha1, time.to_string().as_bytes());
    Sha1Digest::update(&mut rand_sha1, devuid.as_bytes());

    LocateDownloadSign {
        time,
        rand: format!("{:x}", Sha1Digest::finalize(rand_sha1)),
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

#[derive(Deserialize)]
struct FileOperationResponse {
    errno: Option<i64>,
    error_code: Option<i64>,
    error_msg: Option<String>,
}

#[derive(Serialize)]
struct PathParam {
    path: String,
}

#[derive(Serialize)]
struct PathParamList {
    list: Vec<PathParam>,
}

#[derive(Serialize)]
struct CopyMoveParamList<'a> {
    list: &'a [FileOperationPair],
}

fn int_to_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = i64::deserialize(deserializer)?;
    Ok(value != 0)
}

fn bool_to_int<S>(value: &bool, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_i64(i64::from(*value))
}

const fn scheme(https: bool) -> &'static str {
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
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use super::{
        copy_move_param_json, delete_param_json, dev_uid, locatedownload_sign_with,
        BlockingPcsClientOptions, ByteRange, FileOperation, FileOperationPair, PcsApi,
        PcsClientOptions, PcsError,
    };

    fn fixture_path(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/api")
            .join(name)
    }

    fn fixture_string(name: &str) -> String {
        let data = fs::read_to_string(fixture_path(name));
        assert!(data.is_ok(), "fixture should be readable");
        data.unwrap_or_default()
    }

    fn api() -> PcsApi {
        PcsApi {
            options: PcsClientOptions {
                app_id: 266_719,
                pcs_addr: "pcs.baidu.com".to_string(),
                enable_https: true,
            },
        }
    }

    #[test]
    fn client_options_keep_pan_user_agent_separate() {
        let options = BlockingPcsClientOptions::new_with_pan("pcs-ua", "pan-ua", None);

        assert_eq!(options.user_agent, "pcs-ua");
        assert_eq!(options.pan_user_agent, "pan-ua");
    }

    #[test]
    fn list_request_builds_pcs_url_with_ordering() {
        let request = api().list_request("/我的资源");

        assert_eq!(
            request.url.as_str(),
            "https://pcs.baidu.com/rest/2.0/pcs/file?app_id=266719&method=list&path=%2F%E6%88%91%E7%9A%84%E8%B5%84%E6%BA%90&by=name&order=asc"
        );
    }

    #[test]
    fn parse_list_response_preserves_file_model_fields() {
        let entries =
            super::parse_file_list_for_operation("list", &fixture_string("list_root.json"));
        assert!(entries.is_ok(), "list fixture should parse");
        let Ok(entries) = entries else {
            return;
        };

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].fs_id, 9001);
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].server_filename, "hello.txt");
        assert_eq!(
            entries[1].block_list,
            vec!["5d41402abc4b2a76b9719d911017c592"]
        );
    }

    #[test]
    fn parse_list_response_accepts_real_success_defaults() -> Result<(), PcsError> {
        let response = r#"{
            "list": [
                {
                    "fs_id": 10,
                    "path": "/我的资源/example",
                    "server_filename": "example",
                    "isdir": 1
                }
            ]
        }"#;

        let files = super::parse_file_list_for_operation("list", response)?;

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].server_filename, "example");
        assert!(files[0].is_dir);
        assert_eq!(files[0].md5, "");
        assert_eq!(files[0].size, 0);
        Ok(())
    }

    #[test]
    fn parse_remote_error_keeps_remote_code_and_message() {
        let error = super::parse_remote_error("list", &fixture_string("remote_error.json"));
        assert!(matches!(
            error,
            PcsError::Remote {
                operation: _,
                code: 31066,
                message: _
            }
        ));
    }

    #[test]
    fn download_request_builds_readonly_pcs_file_url() {
        let request = api().download_request("/我的资源/notes.md");

        assert_eq!(
            request.url.as_str(),
            "https://pcs.baidu.com/rest/2.0/pcs/file?app_id=266719&method=download&path=%2F%E6%88%91%E7%9A%84%E8%B5%84%E6%BA%90%2Fnotes.md"
        );
    }

    #[test]
    fn locatedownload_request_builds_signed_pcs_url() {
        let sign = locatedownload_sign_with(
            1_571_140_066,
            "O|1E67351CCE80B2CF48DB511CD77ACD9F",
            10086,
            "test_bduss",
        );

        let request = api().locatedownload_request("/我的资源/movie.mp4", &sign);

        assert!(request
            .url
            .starts_with("https://pcs.baidu.com/rest/2.0/pcs/file?ant=1&check_blue=1"));
        assert!(request.url.contains("method=locatedownload"));
        assert!(request.url.contains("app_id=250528"));
        assert!(request
            .url
            .contains("path=%2F%E6%88%91%E7%9A%84%E8%B5%84%E6%BA%90%2Fmovie.mp4"));
        assert!(request.url.contains("time=1571140066"));
        assert!(request
            .url
            .contains("devuid=O%7C1E67351CCE80B2CF48DB511CD77ACD9F"));
    }

    #[test]
    fn locatedownload_sign_matches_reference_vector() {
        let sign = locatedownload_sign_with(
            1_571_140_066,
            "O|1E67351CCE80B2CF48DB511CD77ACD9F",
            10086,
            "test_bduss",
        );

        assert_eq!(sign.rand, "b6bb7a6f46899e181baea58798d4fdb889775c2c");
        assert_eq!(sign.time, 1_571_140_066);
        assert_eq!(sign.devuid, "O|1E67351CCE80B2CF48DB511CD77ACD9F");
    }

    #[test]
    fn devuid_is_uppercase_md5_plus_suffix() {
        assert_eq!(dev_uid("test_bduss"), "66AE65D3C5D483FC4431FC43670A4673|0");
    }

    #[test]
    fn mkdir_request_builds_post_file_url() {
        let request = api().mkdir_request("/docs/new folder");

        assert!(request
            .url
            .starts_with("https://pcs.baidu.com/rest/2.0/pcs/file?"));
        assert!(request.url.contains("method=mkdir"));
        assert!(request.url.contains("app_id=266719"));
        assert!(request.url.contains("path=%2Fdocs%2Fnew+folder"));
    }

    #[test]
    fn search_request_records_recursive_flag_and_keyword() {
        let request = api().search_request("/docs", "report 2026", true);

        assert!(request.url.contains("method=search"));
        assert!(request.url.contains("path=%2Fdocs"));
        assert!(request.url.contains("wd=report+2026"));
        assert!(request.url.contains("re=1"));
    }

    #[test]
    fn file_operation_request_builds_batch_operation_url() {
        let request = api().file_operation_request(FileOperation::Move);

        assert!(request.url.contains("method=move"));
    }

    #[test]
    fn delete_param_json_matches_go_reference_shape() -> Result<(), PcsError> {
        let json = delete_param_json(&["/a.txt".to_string(), "/dir/b.txt".to_string()])?;

        assert_eq!(
            json,
            r#"{"list":[{"path":"/a.txt"},{"path":"/dir/b.txt"}]}"#
        );
        Ok(())
    }

    #[test]
    fn copy_move_param_json_matches_go_reference_shape() -> Result<(), PcsError> {
        let json = copy_move_param_json(&[FileOperationPair {
            from: "/a.txt".to_string(),
            to: "/backup/a.txt".to_string(),
        }])?;

        assert_eq!(json, r#"{"list":[{"from":"/a.txt","to":"/backup/a.txt"}]}"#);
        Ok(())
    }

    #[test]
    fn parse_file_operation_response_accepts_errno_zero_and_rejects_remote_error(
    ) -> Result<(), String> {
        super::parse_file_operation_response("mkdir", r#"{"errno":0}"#)
            .map_err(|err| err.to_string())?;

        let err = match super::parse_file_operation_response(
            "delete",
            r#"{"error_code":31070,"error_msg":"file delete failed"}"#,
        ) {
            Ok(()) => return Err("remote delete error should fail".to_string()),
            Err(err) => err,
        };

        assert_eq!(
            err,
            PcsError::Remote {
                operation: "delete".to_string(),
                code: 31070,
                message: "file delete failed".to_string(),
            }
        );
        Ok(())
    }

    #[test]
    fn locatedownload_parser_selects_unencrypted_urls() {
        let info = super::parse_locatedownload(&fixture_string("locatedownload.json"));
        assert!(info.is_ok(), "locatedownload fixture should parse");
        let Ok(info) = info else {
            return;
        };

        let urls = super::select_download_urls(&info, true);
        assert_eq!(urls, vec!["https://fixture.invalid/download/hello.txt"]);
    }

    #[test]
    fn locatedownload_parser_accepts_success_without_errno() -> Result<(), PcsError> {
        let info = super::parse_locatedownload(
            r#"{
                "urls": [
                    {
                        "url": "http://fixture.invalid/download/movie.mp4",
                        "encrypt": 0
                    }
                ]
            }"#,
        )?;

        assert_eq!(super::select_download_urls(&info, true).len(), 1);
        Ok(())
    }

    #[test]
    fn preview_range_header_is_bounded_to_requested_bytes() -> Result<(), PcsError> {
        let range = ByteRange::prefix(16)?;

        assert_eq!(range.header_value(), "bytes=0-15");
        assert_eq!(range.read_limit_bytes(), 16);
        Ok(())
    }

    #[test]
    fn preview_range_rejects_zero_length_prefix() {
        assert!(ByteRange::prefix(0).is_err());
    }

    #[test]
    fn preview_range_header_supports_seek_offsets() -> Result<(), PcsError> {
        let range = ByteRange::new(1024, 2047)?;

        assert_eq!(range.header_value(), "bytes=1024-2047");
        assert_eq!(range.read_limit_bytes(), 1024);
        Ok(())
    }
}
