use super::super::*;
use crate::app::{AppCommand, AppController, AppEvent};
use crate::pcs::{ByteRange, FileEntry, PcsError};
use crate::preview::{PreviewKey, PreviewKind, PreviewPayload};
use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn file(fs_id: i64, path: &str) -> FileEntry {
    FileEntry {
        fs_id,
        app_id: 266719,
        path: path.to_string(),
        server_filename: path.rsplit('/').next().unwrap_or(path).to_string(),
        ctime: 1,
        mtime: 1,
        md5: String::new(),
        block_list: Vec::new(),
        size: 1,
        is_dir: false,
        has_subdir: false,
    }
}

#[test]
fn refresh_commands_advance_directory_generation_monotonically() {
    let mut controller = AppController::default();
    let initial = controller.directory_generation();
    controller.dispatch(AppCommand::Refresh {
        path: "/docs".to_string(),
    });
    let first = controller.directory_generation();
    controller.dispatch(AppCommand::Refresh {
        path: "/photos".to_string(),
    });
    let second = controller.directory_generation();

    assert!(first > initial);
    assert!(second > first);
}

#[test]
fn selection_changes_advance_preview_generation_monotonically() {
    let mut controller = AppController::default();
    let initial = controller.preview_generation();
    controller.dispatch(AppCommand::SelectionChanged {
        entry: Some(file(1, "/docs/a.txt")),
    });
    let first = controller.preview_generation();
    controller.dispatch(AppCommand::SelectionChanged {
        entry: Some(file(2, "/docs/b.txt")),
    });
    let second = controller.preview_generation();

    assert!(first > initial);
    assert!(second > first);
}

#[test]
fn stale_directory_loaded_event_cannot_replace_current_entries() {
    let mut controller = AppController::new("/");
    controller.dispatch(AppCommand::Refresh {
        path: "/old".to_string(),
    });
    let stale_generation = controller.directory_generation();
    controller.dispatch(AppCommand::Refresh {
        path: "/current".to_string(),
    });
    let current_generation = controller.directory_generation();

    let stale_effects = controller.reduce(AppEvent::DirectoryLoaded {
        generation: stale_generation,
        path: "/old".to_string(),
        result: Ok(vec![file(1, "/old/stale.txt")]),
    });
    assert!(stale_effects.is_empty());
    assert!(controller.directory_entries().is_empty());

    controller.reduce(AppEvent::DirectoryLoaded {
        generation: current_generation,
        path: "/current".to_string(),
        result: Ok(vec![file(2, "/current/fresh.txt")]),
    });
    assert_eq!(
        controller.directory_entries(),
        &[file(2, "/current/fresh.txt")]
    );
}

#[test]
fn stale_preview_loaded_event_cannot_replace_current_payload() {
    let mut controller = AppController::new("/");
    controller.dispatch(AppCommand::SelectionChanged {
        entry: Some(file(1, "/old.txt")),
    });
    let stale_generation = controller.preview_generation();
    controller.dispatch(AppCommand::SelectionChanged {
        entry: Some(file(2, "/current.txt")),
    });
    let current_generation = controller.preview_generation();
    let stale_key = PreviewKey {
        account_uid: 100,
        fs_id: 1,
        mtime: 1,
        size: 1,
    };
    let current_key = PreviewKey {
        account_uid: 100,
        fs_id: 2,
        mtime: 1,
        size: 1,
    };

    let stale_effects = controller.reduce(AppEvent::PreviewLoaded {
        generation: stale_generation,
        key: stale_key,
        result: Ok(PreviewPayload::metadata(PreviewKind::Metadata, "stale")),
    });
    assert!(stale_effects.is_empty());
    assert!(controller.preview_payload().is_none());

    controller.reduce(AppEvent::PreviewLoaded {
        generation: current_generation,
        key: current_key,
        result: Ok(PreviewPayload::metadata(PreviewKind::Metadata, "fresh")),
    });
    assert_eq!(
        controller.preview_payload(),
        Some(&PreviewPayload::metadata(PreviewKind::Metadata, "fresh"))
    );
}

#[test]
fn quit_command_marks_controller_quitting_immediately() {
    let mut controller = AppController::default();
    assert!(!controller.is_quitting());
    controller.dispatch(AppCommand::Quit);
    assert!(controller.is_quitting());
}

struct FakeRangeFetcher {
    calls: AtomicUsize,
    cancel_after_first: bool,
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
        panic!("cancelled download must return DownloadOutcome::Cancelled");
    };
    assert_eq!(bytes_written, 4);
    assert_ne!(partial_path, target);
    assert_eq!(tokio::fs::read(&partial_path).await?, vec![1; 4]);
    assert_eq!(
        progress_rx.recv().await.expect("progress event").bytes_done,
        4
    );
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
    assert_eq!(last.expect("final progress").bytes_done, 8);
    tokio::fs::remove_file(target).await?;
    Ok(())
}
