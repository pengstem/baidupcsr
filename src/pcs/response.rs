use super::*;

impl FileOperation {
    pub(crate) const fn method_name(self) -> &'static str {
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
pub(crate) struct LocateDownloadInfo {
    pub(crate) urls: Vec<DownloadUrlInfo>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct DownloadUrlInfo {
    pub(crate) url: String,
    pub(crate) encrypt: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocateDownloadSign {
    pub(crate) time: i64,
    pub(crate) rand: String,
    pub(crate) devuid: String,
}

pub(crate) fn parse_file_list_for_operation(
    operation: &str,
    data: &str,
) -> Result<Vec<FileEntry>, PcsError> {
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

pub(crate) fn parse_file_operation_response(operation: &str, data: &str) -> Result<(), PcsError> {
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

pub(crate) fn delete_param_json(paths: &[String]) -> Result<String, PcsError> {
    let list = paths
        .iter()
        .map(|path| PathParam { path: path.clone() })
        .collect();
    serde_json::to_string(&PathParamList { list }).map_err(|source| PcsError::Parse {
        operation: "delete".to_string(),
        message: source.to_string(),
    })
}

pub(crate) fn copy_move_param_json(pairs: &[FileOperationPair]) -> Result<String, PcsError> {
    serde_json::to_string(&CopyMoveParamList { list: pairs }).map_err(|source| PcsError::Parse {
        operation: "file_operation".to_string(),
        message: source.to_string(),
    })
}

pub(crate) fn parse_locatedownload(data: &str) -> Result<LocateDownloadInfo, PcsError> {
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

pub(crate) fn parse_remote_error(operation: &str, data: &str) -> PcsError {
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

pub(crate) fn select_download_urls(info: &LocateDownloadInfo, https: bool) -> Vec<String> {
    info.urls
        .iter()
        .filter(|url| url.encrypt == 0)
        .map(|url| normalize_scheme(&url.url, https))
        .collect()
}

pub(crate) fn dev_uid(feature: &str) -> String {
    let mut md5 = Md5::new();
    Md5Digest::update(&mut md5, feature.as_bytes());
    format!("{}|0", hex::encode_upper(Md5Digest::finalize(md5)))
}

pub(crate) fn locatedownload_sign_with(
    time: i64,
    devuid: &str,
    uid: u64,
    bduss: &str,
) -> LocateDownloadSign {
    let mut bduss_sha1 = Sha1::new();
    Sha1Digest::update(&mut bduss_sha1, bduss.as_bytes());
    let bduss_sha1_hex = hex::encode(Sha1Digest::finalize(bduss_sha1));

    let mut rand_sha1 = Sha1::new();
    Sha1Digest::update(&mut rand_sha1, bduss_sha1_hex.as_bytes());
    Sha1Digest::update(&mut rand_sha1, uid.to_string().as_bytes());
    Sha1Digest::update(&mut rand_sha1, b"ebrcUYiuxaZv2XGu7KIYKxUrqfnOfpDF");
    Sha1Digest::update(&mut rand_sha1, time.to_string().as_bytes());
    Sha1Digest::update(&mut rand_sha1, devuid.as_bytes());

    LocateDownloadSign {
        time,
        rand: hex::encode(Sha1Digest::finalize(rand_sha1)),
        devuid: devuid.to_string(),
    }
}

#[derive(Deserialize)]
pub(crate) struct ListResponse {
    #[serde(default)]
    pub(crate) errno: i64,
    pub(crate) list: Vec<FileEntry>,
}

#[derive(Deserialize)]
pub(crate) struct LocateDownloadResponse {
    #[serde(default)]
    pub(crate) errno: i64,
    pub(crate) urls: Vec<DownloadUrlInfo>,
}

#[derive(Deserialize)]
pub(crate) struct RemoteErrorResponse {
    pub(crate) errno: Option<i64>,
    pub(crate) error_code: Option<i64>,
    pub(crate) error_msg: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct FileOperationResponse {
    pub(crate) errno: Option<i64>,
    pub(crate) error_code: Option<i64>,
    pub(crate) error_msg: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct PathParam {
    pub(crate) path: String,
}

#[derive(Serialize)]
pub(crate) struct PathParamList {
    pub(crate) list: Vec<PathParam>,
}

#[derive(Serialize)]
pub(crate) struct CopyMoveParamList<'a> {
    pub(crate) list: &'a [FileOperationPair],
}

pub(crate) fn int_to_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = i64::deserialize(deserializer)?;
    Ok(value != 0)
}

pub(crate) fn bool_to_int<S>(value: &bool, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_i64(i64::from(*value))
}

pub(crate) const fn scheme(https: bool) -> &'static str {
    if https {
        "https"
    } else {
        "http"
    }
}

pub(crate) fn normalize_scheme(raw: &str, https: bool) -> String {
    match raw.split_once("://") {
        Some((_, rest)) => format!("{}://{}", scheme(https), rest),
        None => raw.to_string(),
    }
}

pub(crate) async fn read_bounded_response(
    response: reqwest::Response,
    range: ByteRange,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, PcsError> {
    if cancel.is_cancelled() {
        return Err(cancelled("download"));
    }
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(download_protocol_error(format!(
            "expected HTTP 206 Partial Content, received {}",
            response.status()
        )));
    }

    validate_content_range(response.headers(), range)?;
    let requested_len = range.read_limit_bytes();
    if let Some(content_length) = response.headers().get(reqwest::header::CONTENT_LENGTH) {
        let content_length = content_length
            .to_str()
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| download_protocol_error("invalid Content-Length header"))?;
        if content_length != requested_len {
            return Err(download_protocol_error(format!(
                "Content-Length {content_length} does not match requested range length {requested_len}"
            )));
        }
    }

    let capacity = usize::try_from(requested_len)
        .map_err(|_| download_protocol_error("requested range is too large for this platform"))?;
    let mut bytes = Vec::with_capacity(capacity.min(64 * 1024));
    let mut stream = response.bytes_stream();
    loop {
        let next = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled("download")),
            next = stream.next() => next,
        };
        let Some(chunk) = next else {
            break;
        };
        let chunk = chunk.map_err(|source| PcsError::Network {
            operation: "download".to_string(),
            message: error_chain_message(&source),
        })?;
        if bytes.len().saturating_add(chunk.len()) > capacity {
            return Err(download_protocol_error(format!(
                "response body exceeds requested range length {requested_len}"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.len() != capacity {
        return Err(download_protocol_error(format!(
            "response body length {} does not match requested range length {requested_len}",
            bytes.len()
        )));
    }
    Ok(bytes)
}

fn validate_content_range(
    headers: &reqwest::header::HeaderMap,
    requested: ByteRange,
) -> Result<(), PcsError> {
    let value = headers
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| download_protocol_error("missing or invalid Content-Range header"))?;
    let (unit, value) = value
        .split_once(char::is_whitespace)
        .ok_or_else(|| download_protocol_error("invalid Content-Range header"))?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return Err(download_protocol_error("Content-Range unit must be bytes"));
    }
    let (bounds, total) = value
        .trim()
        .split_once('/')
        .ok_or_else(|| download_protocol_error("invalid Content-Range header"))?;
    let (start, end) = bounds
        .split_once('-')
        .ok_or_else(|| download_protocol_error("invalid Content-Range bounds"))?;
    let start = start
        .parse::<u64>()
        .map_err(|_| download_protocol_error("invalid Content-Range start"))?;
    let end = end
        .parse::<u64>()
        .map_err(|_| download_protocol_error("invalid Content-Range end"))?;
    if start != requested.start() || end != requested.end_inclusive() {
        return Err(download_protocol_error(format!(
            "Content-Range {start}-{end} does not match requested range {}-{}",
            requested.start(),
            requested.end_inclusive()
        )));
    }
    if total != "*" {
        let total = total
            .parse::<u64>()
            .map_err(|_| download_protocol_error("invalid Content-Range total"))?;
        if total <= end {
            return Err(download_protocol_error(
                "Content-Range total must be greater than its end offset",
            ));
        }
    }
    Ok(())
}

fn download_protocol_error(message: impl Into<String>) -> PcsError {
    PcsError::Network {
        operation: "download".to_string(),
        message: message.into(),
    }
}

pub(crate) fn now_unix_seconds() -> i64 {
    UNIX_EPOCH
        .elapsed()
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}
