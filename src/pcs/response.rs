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

pub(crate) fn multipart_param_body(param: &str) -> (String, String) {
    const BOUNDARY: &str = "baidupcsr-stage9-boundary";
    (
        format!("multipart/form-data; boundary={BOUNDARY}"),
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"param\"\r\n\r\n{param}\r\n--{BOUNDARY}--\r\n"
        ),
    )
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
    format!("{:X}|0", Md5Digest::finalize(md5))
}

pub(crate) fn locatedownload_sign_with(
    time: i64,
    devuid: &str,
    uid: u64,
    bduss: &str,
) -> LocateDownloadSign {
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
    let status = response.status();
    if !status.is_success() {
        let text = response_text(response, "download", cancel).await?;
        return Err(parse_remote_error("download", &text));
    }

    let limit = usize::try_from(range.read_limit_bytes()).unwrap_or(usize::MAX);
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    let mut stream = response.bytes_stream();
    while bytes.len() < limit {
        let next = tokio::select! {
            _ = cancel.cancelled() => return Err(cancelled("download")),
            next = stream.next() => next,
        };
        let Some(chunk) = next else {
            break;
        };
        let chunk = chunk.map_err(|source| PcsError::Network {
            operation: "download".to_string(),
            message: reqwest_error_label(&source),
        })?;
        let remaining = limit.saturating_sub(bytes.len());
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }
    Ok(bytes)
}

pub(crate) fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}
