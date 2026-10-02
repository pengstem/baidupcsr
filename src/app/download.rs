use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::stream::{FuturesOrdered, StreamExt};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use super::fetch::RangeFetcher;
use crate::pcs::{ByteRange, PcsError};

const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(250),
    Duration::from_secs(1),
    Duration::from_secs(4),
];
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
pub struct DownloadSpec {
    pub target: PathBuf,
    pub size: u64,
    pub chunk_size: u64,
    /// How many ranges are kept in flight at once.
    pub concurrency: usize,
    /// Treat an existing complete target as done instead of an error.
    pub reuse_existing: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DownloadOutcome {
    Completed,
    Cancelled { bytes_written: u64 },
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("{0} already exists")]
    TargetExists(String),
    #[error(transparent)]
    Pcs(#[from] PcsError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Download `spec.size` bytes into `spec.target`, resuming from
/// `<target>.part`. Ranges are fetched through a sliding window of
/// `concurrency` requests and appended in file order, so the `.part` file is
/// always one contiguous prefix. Each range is retried with backoff.
pub async fn download(
    fetcher: &impl RangeFetcher,
    spec: &DownloadSpec,
    mut on_progress: impl FnMut(u64),
    cancel: &CancellationToken,
) -> Result<DownloadOutcome, DownloadError> {
    if let Some(len) = file_len(&spec.target).await? {
        return if spec.reuse_existing && len == spec.size {
            Ok(DownloadOutcome::Completed)
        } else {
            Err(DownloadError::TargetExists(
                spec.target.display().to_string(),
            ))
        };
    }
    if let Some(parent) = spec.target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let partial = partial_path(&spec.target);
    let mut offset = file_len(&partial).await?.unwrap_or(0);
    if offset > spec.size {
        tokio::fs::remove_file(&partial).await?;
        offset = 0;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&partial)
        .await?;
    on_progress(offset);

    let chunk_size = spec.chunk_size.max(1);
    let mut pending = FuturesOrdered::new();
    let mut next = offset;
    let mut last_report = Instant::now();
    loop {
        while next < spec.size && pending.len() < spec.concurrency.max(1) {
            let range = ByteRange::chunk(next, chunk_size, spec.size)?;
            next = range.end + 1;
            pending.push_back(fetch_with_retry(fetcher, range, cancel));
        }
        let result = tokio::select! {
            biased;
            () = cancel.cancelled() => None,
            result = pending.next() => match result {
                Some(result) => Some(result),
                None => break,
            },
        };
        let bytes = match result {
            Some(Ok(bytes)) => bytes,
            Some(Err(error)) if !error.is_cancelled() => return Err(error.into()),
            _ => {
                file.flush().await?;
                return Ok(DownloadOutcome::Cancelled {
                    bytes_written: offset,
                });
            }
        };
        file.write_all(&bytes).await?;
        offset += bytes.len() as u64;
        if last_report.elapsed() >= PROGRESS_INTERVAL || offset == spec.size {
            last_report = Instant::now();
            on_progress(offset);
        }
    }

    file.flush().await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(&partial, &spec.target).await?;
    Ok(DownloadOutcome::Completed)
}

async fn fetch_with_retry(
    fetcher: &impl RangeFetcher,
    range: ByteRange,
    cancel: &CancellationToken,
) -> Result<Bytes, PcsError> {
    let mut delays = RETRY_DELAYS.iter();
    loop {
        match fetcher.fetch(range, cancel).await {
            Err(error) if !error.is_cancelled() => {
                let Some(delay) = delays.next() else {
                    return Err(error);
                };
                tokio::select! {
                    () = cancel.cancelled() => return Err(PcsError::Cancelled { operation: "download" }),
                    () = tokio::time::sleep(*delay) => {}
                }
            }
            result => return result,
        }
    }
}

async fn file_len(path: &Path) -> std::io::Result<Option<u64>> {
    match tokio::fs::metadata(path).await {
        Ok(metadata) => Ok(Some(metadata.len())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn partial_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use super::*;

    /// Serves ranges of `data`, failing the first `failures` calls and
    /// recording the order ranges were requested in.
    struct MemoryFile {
        data: Vec<u8>,
        failures: AtomicUsize,
        requested: Mutex<Vec<u64>>,
        cancel_after: Option<(usize, CancellationToken)>,
    }

    impl MemoryFile {
        fn new(len: usize) -> Self {
            Self {
                data: (0..len).map(|index| (index % 251) as u8).collect(),
                failures: AtomicUsize::new(0),
                requested: Mutex::new(Vec::new()),
                cancel_after: None,
            }
        }
    }

    impl RangeFetcher for MemoryFile {
        async fn fetch(
            &self,
            range: ByteRange,
            _cancel: &CancellationToken,
        ) -> Result<Bytes, PcsError> {
            let calls = {
                let mut requested = self.requested.lock().unwrap();
                requested.push(range.start);
                requested.len()
            };
            if let Some((after, token)) = &self.cancel_after {
                if calls > *after {
                    token.cancel();
                    return Err(PcsError::Cancelled { operation: "test" });
                }
            }
            if self
                .failures
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(1)
                })
                .is_ok()
            {
                return Err(PcsError::parse("test", "flaky"));
            }
            Ok(Bytes::copy_from_slice(
                &self.data[range.start as usize..=range.end as usize],
            ))
        }
    }

    fn spec(dir: &Path, size: usize) -> DownloadSpec {
        DownloadSpec {
            target: dir.join("file.bin"),
            size: size as u64,
            chunk_size: 10,
            concurrency: 3,
            reuse_existing: false,
        }
    }

    #[tokio::test]
    async fn downloads_in_order_through_a_window() {
        let dir = tempfile::tempdir().unwrap();
        let file = MemoryFile::new(95);
        let spec = spec(dir.path(), 95);
        let mut reports = Vec::new();
        let outcome = download(
            &file,
            &spec,
            |done| reports.push(done),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(outcome, DownloadOutcome::Completed);
        assert_eq!(std::fs::read(&spec.target).unwrap(), file.data);
        assert!(!partial_path(&spec.target).exists());
        assert_eq!(reports.last(), Some(&95));
        assert_eq!(file.requested.lock().unwrap().len(), 10);
    }

    #[tokio::test(start_paused = true)]
    async fn retries_transient_failures() {
        let dir = tempfile::tempdir().unwrap();
        let file = MemoryFile::new(30);
        file.failures.store(3, Ordering::SeqCst);
        let spec = spec(dir.path(), 30);
        download(&file, &spec, |_| {}, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(std::fs::read(&spec.target).unwrap(), file.data);
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_retries() {
        let dir = tempfile::tempdir().unwrap();
        let file = MemoryFile::new(30);
        file.failures.store(usize::MAX, Ordering::SeqCst);
        let result = download(
            &file,
            &spec(dir.path(), 30),
            |_| {},
            &CancellationToken::new(),
        )
        .await;
        assert!(matches!(result, Err(DownloadError::Pcs(_))));
    }

    #[tokio::test]
    async fn resumes_from_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = MemoryFile::new(50);
        let spec = spec(dir.path(), 50);
        std::fs::write(partial_path(&spec.target), &file.data[..20]).unwrap();
        download(&file, &spec, |_| {}, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(std::fs::read(&spec.target).unwrap(), file.data);
        assert_eq!(file.requested.lock().unwrap()[0], 20);
    }

    #[tokio::test]
    async fn complete_partial_file_is_renamed_without_fetching() {
        let dir = tempfile::tempdir().unwrap();
        let file = MemoryFile::new(50);
        let spec = spec(dir.path(), 50);
        std::fs::write(partial_path(&spec.target), &file.data).unwrap();
        download(&file, &spec, |_| {}, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(std::fs::read(&spec.target).unwrap(), file.data);
        assert!(file.requested.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn existing_target_is_an_error_unless_reused() {
        let dir = tempfile::tempdir().unwrap();
        let file = MemoryFile::new(10);
        let mut spec = spec(dir.path(), 10);
        std::fs::write(&spec.target, &file.data).unwrap();
        let result = download(&file, &spec, |_| {}, &CancellationToken::new()).await;
        assert!(matches!(result, Err(DownloadError::TargetExists(_))));
        spec.reuse_existing = true;
        let outcome = download(&file, &spec, |_| {}, &CancellationToken::new()).await;
        assert_eq!(outcome.unwrap(), DownloadOutcome::Completed);
    }

    #[tokio::test]
    async fn cancel_keeps_contiguous_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancellationToken::new();
        let mut file = MemoryFile::new(100);
        file.cancel_after = Some((4, cancel.clone()));
        let spec = spec(dir.path(), 100);
        let outcome = download(&file, &spec, |_| {}, &cancel).await.unwrap();
        let DownloadOutcome::Cancelled { bytes_written } = outcome else {
            panic!("expected cancellation");
        };
        let partial = std::fs::read(partial_path(&spec.target)).unwrap();
        assert_eq!(partial.len() as u64, bytes_written);
        assert_eq!(partial, file.data[..partial.len()]);
    }
}
