use md5::{Digest as _, Md5};
use serde::{Deserialize, Serialize};
use sha1::Sha1;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PcsError {
    #[error("{operation}: cancelled")]
    Cancelled { operation: &'static str },
    #[error("{operation}: {message}")]
    Network {
        operation: &'static str,
        message: String,
    },
    #[error("{operation}: bad response: {message}")]
    Parse {
        operation: &'static str,
        message: String,
    },
    #[error("{operation}: remote error {code}: {message}")]
    Remote {
        operation: &'static str,
        code: i64,
        message: String,
    },
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),
}

impl PcsError {
    pub const fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }

    pub(crate) fn network(operation: &'static str, error: &dyn std::error::Error) -> Self {
        Self::Network {
            operation,
            message: error_chain(error),
        }
    }

    pub(crate) fn parse(operation: &'static str, message: impl ToString) -> Self {
        Self::Parse {
            operation,
            message: message.to_string(),
        }
    }

    pub(crate) fn http(operation: &'static str, status: impl std::fmt::Display) -> Self {
        Self::Network {
            operation,
            message: format!("HTTP {status}"),
        }
    }
}

fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !cause_text.is_empty() && !message.ends_with(&cause_text) {
            message.push_str(": ");
            message.push_str(&cause_text);
        }
        source = cause.source();
    }
    message
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileEntry {
    pub fs_id: i64,
    pub path: String,
    pub server_filename: String,
    #[serde(default)]
    pub mtime: i64,
    #[serde(default)]
    pub size: i64,
    #[serde(
        default,
        rename = "isdir",
        deserialize_with = "int_to_bool",
        serialize_with = "bool_to_int"
    )]
    pub is_dir: bool,
}

impl FileEntry {
    pub fn size_bytes(&self) -> u64 {
        u64::try_from(self.size).unwrap_or(0)
    }
}

fn int_to_bool<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Ok(i64::deserialize(deserializer)? != 0)
}

fn bool_to_int<S: serde::Serializer>(value: &bool, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_i64(i64::from(*value))
}

/// An inclusive byte range. Baidu download hosts reject open-ended ranges,
/// so every request carries an explicit end.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

impl ByteRange {
    pub fn new(start: u64, end: u64) -> Result<Self, PcsError> {
        if start > end {
            return Err(PcsError::InvalidArgument("range start is after range end"));
        }
        Ok(Self { start, end })
    }

    /// The range `[start, start + len)` clipped to `size`.
    pub fn chunk(start: u64, len: u64, size: u64) -> Result<Self, PcsError> {
        if len == 0 || start >= size {
            return Err(PcsError::InvalidArgument("empty range"));
        }
        Self::new(start, start.saturating_add(len).min(size) - 1)
    }

    #[allow(clippy::len_without_is_empty)] // never empty: end is inclusive
    pub const fn len(self) -> u64 {
        self.end - self.start + 1
    }

    pub(crate) fn header_value(self) -> String {
        format!("bytes={}-{}", self.start, self.end)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default)]
pub struct Quota {
    pub total: u64,
    pub used: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileOperation {
    Delete,
    Copy,
    Move,
}

impl FileOperation {
    pub(crate) const fn method(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Copy => "copy",
            Self::Move => "move",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FileOperationPair {
    pub from: String,
    pub to: String,
}

/// Error fields Baidu puts in every JSON response, under two spellings.
#[derive(Deserialize)]
pub(crate) struct ErrorFields {
    errno: Option<i64>,
    error_code: Option<i64>,
    error_msg: Option<String>,
    errmsg: Option<String>,
}

impl ErrorFields {
    /// The remote error a response `body` reports, if it is JSON with a
    /// non-zero errno.
    pub(crate) fn from_body(operation: &'static str, body: &[u8]) -> Option<PcsError> {
        serde_json::from_slice::<Self>(body)
            .ok()?
            .into_error(operation)
    }

    pub(crate) fn into_error(self, operation: &'static str) -> Option<PcsError> {
        let code = self.error_code.or(self.errno).unwrap_or(0);
        (code != 0).then(|| PcsError::Remote {
            operation,
            code,
            message: self
                .error_msg
                .or(self.errmsg)
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| remote_code_hint(code).to_string()),
        })
    }
}

fn remote_code_hint(code: i64) -> &'static str {
    match code {
        -6 | 110 | 111 => "login expired, refresh BDUSS",
        -7 | 31066 => "file does not exist",
        -8 | 31061 => "file already exists",
        -9 => "file not found or no permission",
        31045 | 31326 => "access denied, try again later",
        _ => "request rejected",
    }
}

#[derive(Deserialize)]
pub(crate) struct ListResponse {
    #[serde(default)]
    pub(crate) list: Vec<FileEntry>,
}

#[derive(Deserialize)]
pub(crate) struct LocateDownloadResponse {
    #[serde(default)]
    pub(crate) urls: Vec<DownloadUrl>,
}

#[derive(Deserialize)]
pub(crate) struct DownloadUrl {
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) encrypt: i64,
}

#[derive(Deserialize)]
pub(crate) struct Ignored {}

#[derive(Serialize)]
pub(crate) struct ParamList<T> {
    pub(crate) list: T,
}

#[derive(Serialize)]
pub(crate) struct PathParam<'a> {
    pub(crate) path: &'a str,
}

pub(crate) struct LocateSign {
    pub(crate) time: i64,
    pub(crate) rand: String,
    pub(crate) devuid: String,
}

/// Signature for `locatedownload`, as computed by the official netdisk client.
pub(crate) fn locate_sign(time: i64, uid: u64, bduss: &str) -> LocateSign {
    let devuid = format!("{}|0", hex::encode_upper(Md5::digest(bduss.as_bytes())));
    let bduss_sha1 = hex::encode(Sha1::digest(bduss.as_bytes()));
    let mut hasher = Sha1::new();
    hasher.update(bduss_sha1.as_bytes());
    hasher.update(uid.to_string().as_bytes());
    hasher.update(b"ebrcUYiuxaZv2XGu7KIYKxUrqfnOfpDF");
    hasher.update(time.to_string().as_bytes());
    hasher.update(devuid.as_bytes());
    LocateSign {
        time,
        rand: hex::encode(hasher.finalize()),
        devuid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_is_clipped_to_size() {
        let range = ByteRange::chunk(8, 4, 10).unwrap();
        assert_eq!((range.start, range.end, range.len()), (8, 9, 2));
        assert!(ByteRange::chunk(10, 4, 10).is_err());
    }

    #[test]
    fn list_response_parses_entries_and_flags() {
        let json = r#"{"errno":0,"list":[{"fs_id":1,"path":"/a","server_filename":"a","isdir":1,"size":0},
            {"fs_id":2,"path":"/b.txt","server_filename":"b.txt","isdir":0,"size":12,"mtime":5}]}"#;
        let list = serde_json::from_str::<ListResponse>(json).unwrap().list;
        assert!(list[0].is_dir);
        assert_eq!(list[1].size_bytes(), 12);
    }

    #[test]
    fn errno_becomes_remote_error_with_message() {
        let fields: ErrorFields = serde_json::from_str(r#"{"errno":-9}"#).unwrap();
        let error = fields.into_error("list").unwrap();
        assert!(error.to_string().contains("not found"));
        let ok: ErrorFields = serde_json::from_str(r#"{"errno":0}"#).unwrap();
        assert!(ok.into_error("list").is_none());
    }

    #[test]
    fn locate_sign_is_stable() {
        let sign = locate_sign(1_700_000_000, 42, "bduss");
        assert_eq!(sign.devuid.len(), 34);
        assert!(sign.devuid.ends_with("|0"));
        assert_eq!(sign.rand, locate_sign(1_700_000_000, 42, "bduss").rand);
        assert_ne!(sign.rand, locate_sign(1_700_000_001, 42, "bduss").rand);
    }
}
