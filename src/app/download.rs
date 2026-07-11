use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::pcs::{ByteRange, PcsError};

use super::{TaskError, TransferProgress};

const DEFAULT_CHUNK_SIZE: u64 = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadSpec {
    pub remote_path: String,
    pub target: PathBuf,
    pub size: u64,
    pub chunk_size: u64,
    pub existing_policy: ExistingTargetPolicy,
}

impl DownloadSpec {
    pub fn new(remote_path: impl Into<String>, target: PathBuf, size: u64) -> Self {
        Self {
            remote_path: remote_path.into(),
            target,
            size,
            chunk_size: DEFAULT_CHUNK_SIZE,
            existing_policy: ExistingTargetPolicy::FailIfExists,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExistingTargetPolicy {
    FailIfExists,
    ReuseIfComplete,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DownloadOutcome {
    Completed {
        target: PathBuf,
        bytes_written: u64,
    },
    Cancelled {
        partial_path: PathBuf,
        bytes_written: u64,
    },
}

pub trait RangeFetcher: Send + Sync {
    fn fetch_range<'a>(
        &'a self,
        path: &'a str,
        range: ByteRange,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, PcsError>> + Send + 'a>>;
}

pub async fn download_to_path<F: RangeFetcher>(
    fetcher: &F,
    spec: &DownloadSpec,
    progress: mpsc::Sender<TransferProgress>,
    cancel: CancellationToken,
) -> Result<DownloadOutcome, TaskError> {
    if spec.chunk_size == 0 {
        return Err(TaskError::InvalidChunkSize);
    }
    match tokio::fs::metadata(&spec.target).await {
        Ok(metadata)
            if spec.existing_policy == ExistingTargetPolicy::ReuseIfComplete
                && metadata.len() == spec.size =>
        {
            return Ok(DownloadOutcome::Completed {
                target: spec.target.clone(),
                bytes_written: spec.size,
            });
        }
        Ok(_) if spec.existing_policy == ExistingTargetPolicy::ReuseIfComplete => {
            tokio::fs::remove_file(&spec.target).await?;
        }
        Ok(_) => return Err(TaskError::TargetExists(spec.target.display().to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(TaskError::Io(error)),
    }

    if let Some(parent) = spec
        .target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent).await?;
    }

    let partial_path = partial_path(&spec.target);
    let (mut offset, mut partial_exists) = match tokio::fs::metadata(&partial_path).await {
        Ok(metadata) => (metadata.len(), true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (0, false),
        Err(error) => return Err(TaskError::Io(error)),
    };
    if offset > spec.size {
        if spec.existing_policy == ExistingTargetPolicy::FailIfExists {
            return Err(TaskError::PartialLargerThanRemote(
                partial_path.display().to_string(),
            ));
        }
        tokio::fs::remove_file(&partial_path).await?;
        offset = 0;
        partial_exists = false;
    }
    if offset == spec.size && partial_exists {
        tokio::fs::remove_file(&partial_path).await?;
        offset = 0;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&partial_path)
        .await?;
    while offset < spec.size {
        if cancel.is_cancelled() {
            file.flush().await?;
            return Ok(DownloadOutcome::Cancelled {
                partial_path,
                bytes_written: offset,
            });
        }
        let end = offset
            .saturating_add(spec.chunk_size)
            .min(spec.size)
            .saturating_sub(1);
        let chunk = match fetcher
            .fetch_range(&spec.remote_path, ByteRange::new(offset, end)?, &cancel)
            .await
        {
            Ok(chunk) => chunk,
            Err(PcsError::Cancelled { .. }) => {
                file.flush().await?;
                return Ok(DownloadOutcome::Cancelled {
                    partial_path,
                    bytes_written: offset,
                });
            }
            Err(error) => return Err(TaskError::Pcs(error)),
        };
        let requested_len = end.saturating_sub(offset).saturating_add(1);
        let actual_len = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
        if actual_len != requested_len {
            return Err(TaskError::Pcs(PcsError::Network {
                operation: "download".to_string(),
                message: format!(
                    "range fetch returned {actual_len} bytes for a {requested_len}-byte request"
                ),
            }));
        }
        file.write_all(&chunk).await?;
        offset = offset.saturating_add(requested_len);
        let update = TransferProgress {
            bytes_done: offset,
            bytes_total: Some(spec.size),
            speed_bytes_per_sec: 0,
        };
        tokio::select! {
            biased;
            _ = progress.send(update) => {}
            _ = cancel.cancelled() => {
                let _ = progress.try_send(update);
            }
        }
        if cancel.is_cancelled() {
            file.flush().await?;
            return Ok(DownloadOutcome::Cancelled {
                partial_path,
                bytes_written: offset,
            });
        }
    }
    file.flush().await?;
    drop(file);
    tokio::fs::rename(&partial_path, &spec.target).await?;
    Ok(DownloadOutcome::Completed {
        target: spec.target.clone(),
        bytes_written: offset,
    })
}

fn partial_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
}
