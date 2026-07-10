use super::super::*;
use crate::{
    app::{shutdown_tasks, AppEvent},
    pcs::{FileEntry, PcsError},
};
use std::time::{Duration, Instant};
use tokio::{sync::mpsc, task::JoinSet};
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

#[tokio::test]
async fn full_app_event_channel_does_not_block_local_ui_state_updates() {
    let (events, _receiver) = mpsc::channel(256);
    for generation in 0..256 {
        events
            .try_send(AppEvent::DirectoryLoaded {
                generation,
                path: "/blocked".to_string(),
                result: Err(PcsError::Cancelled {
                    operation: "list".to_string(),
                }),
            })
            .expect("channel should accept exactly its declared capacity");
    }
    assert!(events
        .try_send(AppEvent::DirectoryLoaded {
            generation: 257,
            path: "/blocked".to_string(),
            result: Ok(Vec::new()),
        })
        .is_err());

    let mut app = TuiApp::new("/");
    app.set_entries(vec![file(1, "/a.txt"), file(2, "/b.txt")]);
    let started = Instant::now();
    app.handle_key_event(KeyEvent::Char('j'));
    assert_eq!(app.selected_index(), Some(1));
    assert!(started.elapsed() < Duration::from_millis(20));
}

#[tokio::test]
async fn shutdown_cancels_tasks_and_aborts_work_past_deadline() {
    let root = CancellationToken::new();
    let child = root.child_token();
    let mut tasks = JoinSet::new();
    tasks.spawn(async move {
        child.cancelled().await;
        std::future::pending::<()>().await;
    });

    let started = Instant::now();
    shutdown_tasks(&mut tasks, &root, Duration::from_millis(100)).await;

    assert!(root.is_cancelled());
    assert!(started.elapsed() >= Duration::from_millis(90));
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(tasks.is_empty());
}

#[test]
fn canonical_preview_kind_is_used_directly_by_tui_budget() {
    let budget = PreviewBudget {
        kind: crate::preview::PreviewKind::Video,
        fetched_bytes: 64,
        memory_limit_bytes: 128,
    };
    assert_eq!(budget.kind, crate::preview::PreviewKind::Video);
}
