use crate::app::{download_to_path, DownloadOutcome, DownloadSpec, RangeFetcher};
use crate::pcs::{ByteRange, PcsError};
use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
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
