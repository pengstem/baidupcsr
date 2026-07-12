use crate::app::{download_to_path, DownloadOutcome, DownloadSpec, RangeFetcher};
use crate::pcs::{ByteRange, PcsError};
use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard,
    },
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct FakeRangeFetcher {
    calls: AtomicUsize,
    cancel_after_first: bool,
}

struct FixedPayloadRangeFetcher {
    calls: AtomicUsize,
    payload: Vec<u8>,
}

#[derive(Default)]
struct ParallelFetchState {
    active: AtomicUsize,
    max_inflight: AtomicUsize,
    calls: Mutex<Vec<u64>>,
    completions: Mutex<Vec<u64>>,
}

struct ParallelRangeFetcher {
    state: Arc<ParallelFetchState>,
    fail_start: Option<u64>,
}

fn lock_state<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn update_max_inflight(state: &ParallelFetchState, active: usize) {
    let mut previous = state.max_inflight.load(Ordering::Acquire);
    while active > previous {
        match state.max_inflight.compare_exchange(
            previous,
            active,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(current) => previous = current,
        }
    }
}

impl RangeFetcher for ParallelRangeFetcher {
    fn fetch_range<'a>(
        &'a self,
        _path: &'a str,
        range: ByteRange,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, PcsError>> + Send + 'a>> {
        Box::pin(async move {
            lock_state(&self.state.calls).push(range.start);
            let active = self.state.active.fetch_add(1, Ordering::AcqRel) + 1;
            update_max_inflight(&self.state, active);

            let delay = if range.start == 0 {
                std::time::Duration::from_millis(100)
            } else {
                std::time::Duration::from_millis(5)
            };
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = cancel.cancelled() => {
                    self.state.active.fetch_sub(1, Ordering::AcqRel);
                    return Err(PcsError::Cancelled { operation: "download".to_string() });
                }
            }

            if self.fail_start == Some(range.start) {
                self.state.active.fetch_sub(1, Ordering::AcqRel);
                return Err(PcsError::Network {
                    operation: "download".to_string(),
                    message: format!("fixture failed at range {}", range.start),
                });
            }
            lock_state(&self.state.completions).push(range.start);
            self.state.active.fetch_sub(1, Ordering::AcqRel);
            let length = usize::try_from(range.read_limit_bytes()).unwrap_or(usize::MAX);
            let marker = u8::try_from(range.start / 4 + 1).unwrap_or(u8::MAX);
            Ok(vec![marker; length])
        })
    }
}

impl RangeFetcher for FixedPayloadRangeFetcher {
    fn fetch_range<'a>(
        &'a self,
        _path: &'a str,
        _range: ByteRange,
        _cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, PcsError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::AcqRel);
            Ok(self.payload.clone())
        })
    }
}

impl RangeFetcher for FakeRangeFetcher {
    fn fetch_range<'a>(
        &'a self,
        _path: &'a str,
        range: ByteRange,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, PcsError>> + Send + 'a>> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::AcqRel);
            if cancel.is_cancelled() {
                return Err(PcsError::Cancelled {
                    operation: "download".to_string(),
                });
            }
            let data = vec![u8::try_from(call + 1).unwrap_or(0); range.read_limit_bytes() as usize];
            if self.cancel_after_first && call == 0 {
                cancel.cancel();
            }
            Ok(data)
        })
    }
}

fn temp_target(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("baidupcsr-{name}-{}", std::process::id()))
}

fn partial_target(target: &std::path::Path) -> PathBuf {
    let mut name = target.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
}

async fn malformed_chunk_is_rejected_before_append(
    name: &str,
    payload: Vec<u8>,
) -> Result<(), Box<dyn std::error::Error>> {
    let target = temp_target(name);
    let partial = partial_target(&target);
    let _ = tokio::fs::remove_file(&target).await;
    let _ = tokio::fs::remove_file(&partial).await;
    let verified_prefix = vec![0x5a; 4];
    tokio::fs::write(&partial, &verified_prefix).await?;

    let mut spec = DownloadSpec::new("/remote.bin", target.clone(), 8);
    spec.chunk_size = 4;
    let fetcher = FixedPayloadRangeFetcher {
        calls: AtomicUsize::new(0),
        payload,
    };
    let (progress_tx, _) = mpsc::channel(4);
    let result = download_to_path(&fetcher, &spec, progress_tx, CancellationToken::new()).await;

    assert!(result.is_err());
    assert!(!target.exists());
    assert_eq!(tokio::fs::read(&partial).await?, verified_prefix);
    tokio::fs::remove_file(partial).await?;
    Ok(())
}

#[tokio::test]
async fn download_fetches_bounded_parallel_ranges_and_writes_them_in_offset_order(
) -> Result<(), Box<dyn std::error::Error>> {
    let target = temp_target("parallel-download.bin");
    let partial = partial_target(&target);
    let _ = tokio::fs::remove_file(&target).await;
    let _ = tokio::fs::remove_file(&partial).await;

    let state = Arc::new(ParallelFetchState::default());
    let fetcher = ParallelRangeFetcher {
        state: Arc::clone(&state),
        fail_start: None,
    };
    let mut spec = DownloadSpec::new("/remote.bin", target.clone(), 32);
    spec.chunk_size = 4;
    let (progress_tx, _) = mpsc::channel(32);
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        download_to_path(&fetcher, &spec, progress_tx, CancellationToken::new()),
    )
    .await??;

    assert_eq!(
        outcome,
        DownloadOutcome::Completed {
            target: target.clone(),
            bytes_written: 32,
        }
    );
    assert!(state.max_inflight.load(Ordering::Acquire) > 1);
    assert!(state.max_inflight.load(Ordering::Acquire) <= 4);
    let completions = lock_state(&state.completions).clone();
    let first_nonzero = completions
        .iter()
        .position(|start| *start != 0)
        .ok_or("parallel fixture did not complete a later range")?;
    let zero_completion = completions
        .iter()
        .position(|start| *start == 0)
        .ok_or("parallel fixture did not complete the first range")?;
    assert!(first_nonzero < zero_completion);

    let expected = (0..8)
        .flat_map(|index| std::iter::repeat_n(index + 1, 4))
        .map(|marker| marker as u8)
        .collect::<Vec<_>>();
    assert_eq!(tokio::fs::read(&target).await?, expected);
    assert!(!partial.exists());
    tokio::fs::remove_file(target).await?;
    Ok(())
}

#[tokio::test]
async fn download_cancellation_keeps_only_the_continuous_completed_prefix(
) -> Result<(), Box<dyn std::error::Error>> {
    let target = temp_target("parallel-cancel-download.bin");
    let partial = partial_target(&target);
    let _ = tokio::fs::remove_file(&target).await;
    let _ = tokio::fs::remove_file(&partial).await;

    let state = Arc::new(ParallelFetchState::default());
    let fetcher = ParallelRangeFetcher {
        state: Arc::clone(&state),
        fail_start: None,
    };
    let mut spec = DownloadSpec::new("/remote.bin", target.clone(), 32);
    spec.chunk_size = 4;
    let cancel = CancellationToken::new();
    let (progress_tx, _) = mpsc::channel(32);
    let task = tokio::spawn({
        let cancel = cancel.clone();
        async move { download_to_path(&fetcher, &spec, progress_tx, cancel).await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let calls_started = lock_state(&state.calls).len() >= 3;
            let first_range_completed = lock_state(&state.completions).contains(&0);
            if calls_started && first_range_completed {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;
    cancel.cancel();
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(2), task).await??;
    let DownloadOutcome::Cancelled {
        partial_path,
        bytes_written,
    } = outcome?
    else {
        return Err("cancelled parallel download must report cancellation".into());
    };
    assert_eq!(partial_path, partial);
    let bytes = tokio::fs::read(&partial).await?;
    assert_eq!(bytes_written, u64::try_from(bytes.len())?);
    assert_eq!(bytes.len() % 4, 0);
    for (index, chunk) in bytes.chunks_exact(4).enumerate() {
        assert_eq!(chunk, vec![index as u8 + 1; 4]);
    }
    assert!(!target.exists());
    tokio::fs::remove_file(partial).await?;
    Ok(())
}

#[tokio::test]
async fn download_block_failure_keeps_partial_and_never_renames_target(
) -> Result<(), Box<dyn std::error::Error>> {
    let target = temp_target("parallel-failed-download.bin");
    let partial = partial_target(&target);
    let _ = tokio::fs::remove_file(&target).await;
    let _ = tokio::fs::remove_file(&partial).await;

    let state = Arc::new(ParallelFetchState::default());
    let fetcher = ParallelRangeFetcher {
        state,
        fail_start: Some(8),
    };
    let mut spec = DownloadSpec::new("/remote.bin", target.clone(), 32);
    spec.chunk_size = 4;
    let (progress_tx, _) = mpsc::channel(32);
    let result = download_to_path(&fetcher, &spec, progress_tx, CancellationToken::new()).await;

    assert!(result.is_err());
    assert!(!target.exists());
    assert!(partial.exists());
    let bytes = tokio::fs::read(&partial).await?;
    assert!(bytes.len() < 32);
    assert_eq!(bytes.len() % 4, 0);
    for (index, chunk) in bytes.chunks_exact(4).enumerate() {
        assert_eq!(chunk, vec![index as u8 + 1; 4]);
    }
    tokio::fs::remove_file(partial).await?;
    Ok(())
}

#[tokio::test]
async fn download_rejects_short_range_payload_before_appending(
) -> Result<(), Box<dyn std::error::Error>> {
    malformed_chunk_is_rejected_before_append("short-range-payload.bin", vec![0x11; 3]).await
}

#[tokio::test]
async fn download_rejects_long_range_payload_before_appending(
) -> Result<(), Box<dyn std::error::Error>> {
    malformed_chunk_is_rejected_before_append("long-range-payload.bin", vec![0x22; 5]).await
}

#[tokio::test]
async fn unverified_full_length_partial_is_never_published_without_network_verification(
) -> Result<(), Box<dyn std::error::Error>> {
    let target = temp_target("unverified-complete-partial.bin");
    let partial = partial_target(&target);
    let _ = tokio::fs::remove_file(&target).await;
    let _ = tokio::fs::remove_file(&partial).await;
    tokio::fs::write(&partial, [0x33; 4]).await?;

    let mut spec = DownloadSpec::new("/remote.bin", target.clone(), 4);
    spec.chunk_size = 4;
    let fetcher = FixedPayloadRangeFetcher {
        calls: AtomicUsize::new(0),
        payload: vec![0x44; 4],
    };
    let (progress_tx, _) = mpsc::channel(4);
    let result = download_to_path(&fetcher, &spec, progress_tx, CancellationToken::new()).await;
    let network_calls = fetcher.calls.load(Ordering::Acquire);

    assert!(result.is_err() || network_calls > 0);
    if network_calls == 0 {
        assert!(!target.exists());
    }
    let _ = tokio::fs::remove_file(&target).await;
    let _ = tokio::fs::remove_file(&partial).await;
    Ok(())
}

#[tokio::test]
async fn download_cancel_preserves_partial_file_and_reports_written_bytes(
) -> Result<(), Box<dyn std::error::Error>> {
    let target = temp_target("cancel-download.bin");
    let _ = tokio::fs::remove_file(&target).await;
    let mut spec = DownloadSpec::new("/remote.bin", target.clone(), 8);
    spec.chunk_size = 4;
    let fetcher = FakeRangeFetcher {
        calls: AtomicUsize::new(0),
        cancel_after_first: true,
    };
    let (progress_tx, mut progress_rx) = mpsc::channel(4);
    let outcome = download_to_path(&fetcher, &spec, progress_tx, CancellationToken::new()).await?;

    let DownloadOutcome::Cancelled {
        partial_path,
        bytes_written,
    } = outcome
    else {
        return Err("cancelled download must return DownloadOutcome::Cancelled".into());
    };
    assert_eq!(bytes_written, 4);
    assert_eq!(
        partial_path.to_string_lossy(),
        format!("{}.part", target.to_string_lossy())
    );
    assert_eq!(tokio::fs::read(&partial_path).await?, vec![1; 4]);
    let progress = progress_rx
        .recv()
        .await
        .ok_or_else(|| std::io::Error::other("missing progress event"))?;
    assert_eq!(progress.bytes_done, 4);
    assert!(!target.exists());
    tokio::fs::remove_file(partial_path).await?;
    Ok(())
}

#[tokio::test]
async fn completed_download_is_atomically_published_at_target(
) -> Result<(), Box<dyn std::error::Error>> {
    let target = temp_target("complete-download.bin");
    let _ = tokio::fs::remove_file(&target).await;
    let mut spec = DownloadSpec::new("/remote.bin", target.clone(), 8);
    spec.chunk_size = 4;
    let fetcher = FakeRangeFetcher {
        calls: AtomicUsize::new(0),
        cancel_after_first: false,
    };
    let (progress_tx, mut progress_rx) = mpsc::channel(4);
    let outcome = download_to_path(&fetcher, &spec, progress_tx, CancellationToken::new()).await?;

    assert_eq!(
        outcome,
        DownloadOutcome::Completed {
            target: target.clone(),
            bytes_written: 8
        }
    );
    assert_eq!(
        tokio::fs::read(&target).await?,
        vec![1, 1, 1, 1, 2, 2, 2, 2]
    );
    let mut last = None;
    while let Some(progress) = progress_rx.recv().await {
        last = Some(progress);
    }
    let last = last.ok_or_else(|| std::io::Error::other("missing final progress"))?;
    assert_eq!(last.bytes_done, 8);
    tokio::fs::remove_file(target).await?;
    Ok(())
}
