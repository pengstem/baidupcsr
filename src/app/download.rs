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

impl RangeFetcher for crate::pcs::PcsClient {
    fn fetch_range<'a>(
        &'a self,
        path: &'a str,
        range: ByteRange,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, PcsError>> + Send + 'a>> {
        Box::pin(self.download_range(path, range, cancel))
    }
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
    let mut offset = match tokio::fs::metadata(&partial_path).await {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
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
        if chunk.is_empty() {
            return Err(TaskError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "remote download returned an empty chunk",
            )));
        }
        let requested_len = end.saturating_sub(offset).saturating_add(1);
        let write_len = usize::try_from(requested_len)
            .unwrap_or(usize::MAX)
            .min(chunk.len());
        file.write_all(&chunk[..write_len]).await?;
        offset = offset.saturating_add(u64::try_from(write_len).unwrap_or(u64::MAX));
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
