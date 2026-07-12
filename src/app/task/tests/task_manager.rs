use crate::app::{TaskError, TaskKind, TaskManager, TaskState, TransferProgress};

#[test]
fn download_task_tracks_progress_speed_and_completion() -> Result<(), TaskError> {
    let mut manager = TaskManager::new();
    let id = manager.enqueue_download("/tmp/file.txt", 1024);

    manager.start(id)?;
    manager.update_progress(id, 512, 2)?;
    manager.complete(id)?;

    let tasks = manager.snapshots();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, id);
    assert_eq!(tasks[0].state, TaskState::Completed);
    assert_eq!(
        tasks[0].progress,
        TransferProgress {
            bytes_done: 1024,
            bytes_total: Some(1024),
            speed_bytes_per_sec: 256,
        }
    );
    assert_eq!(manager.active_count(), 0);
    Ok(())
}

#[test]
fn cancelling_running_task_can_finish_as_cancelled() -> Result<(), TaskError> {
    let mut manager = TaskManager::new();
    let id = manager.enqueue_download("/tmp/file.txt", 1024);

    manager.start(id)?;
    manager.update_progress(id, 128, 1)?;
    manager.request_cancel(id)?;
    manager.cancelled(id)?;

    let task = manager
        .snapshots()
        .into_iter()
        .find(|task| task.id == id)
        .ok_or(TaskError::UnknownTask)?;
    assert_eq!(task.state, TaskState::Cancelled);
    assert_eq!(manager.active_count(), 0);
    Ok(())
}

#[test]
fn completed_event_after_cancel_request_still_reaches_terminal_state() -> Result<(), TaskError> {
    let mut manager = TaskManager::new();
    let id = manager.enqueue_download("/tmp/file.txt", 1024);

    manager.start(id)?;
    manager.update_progress(id, 1024, 2)?;
    manager.request_cancel(id)?;
    manager.complete(id)?;

    let task = manager
        .snapshots()
        .into_iter()
        .find(|task| task.id == id)
        .ok_or(TaskError::UnknownTask)?;
    assert_eq!(task.state, TaskState::Completed);
    assert_eq!(manager.active_count(), 0);
    Ok(())
}

#[test]
fn local_open_task_tracks_temp_file_target_and_size() -> Result<(), TaskError> {
    let mut manager = TaskManager::new();
    let id = manager.enqueue_local_open("/tmp/baidupcsr-open/photo.jpg", 4096);
    let task = manager
        .snapshots()
        .into_iter()
        .find(|task| task.id == id)
        .ok_or(TaskError::UnknownTask)?;

    assert_eq!(
        task.kind,
        TaskKind::LocalOpen {
            local_path: "/tmp/baidupcsr-open/photo.jpg".to_string(),
        }
    );
    assert_eq!(task.progress.bytes_total, Some(4096));
    Ok(())
}

#[test]
fn task_manager_recent_snapshots_returns_latest_ids_first() -> Result<(), TaskError> {
    let mut manager = TaskManager::new();
    let ids = (0..5)
        .map(|index| manager.enqueue_remote_operation(format!("operation-{index}")))
        .collect::<Vec<_>>();

    let recent = manager.recent_snapshots(3);
    assert_eq!(
        recent.iter().map(|task| task.id).collect::<Vec<_>>(),
        vec![ids[4], ids[3], ids[2]]
    );
    assert!(manager.recent_snapshots(0).is_empty());
    Ok(())
}

#[test]
fn task_manager_bounds_terminal_history_and_keeps_recent_order() -> Result<(), TaskError> {
    let mut manager = TaskManager::new();
    let mut ids = Vec::new();
    for index in 0..256 {
        let id = manager.enqueue_download(format!("/tmp/task-{index}"), 1);
        manager.start(id)?;
        manager.complete(id)?;
        ids.push(id);
    }

    let snapshots = manager.snapshots();
    assert!(
        snapshots.len() < ids.len(),
        "terminal task history must stay bounded"
    );
    assert_eq!(manager.active_count(), 0);
    assert_eq!(
        manager
            .recent_snapshots(3)
            .iter()
            .map(|task| task.id)
            .collect::<Vec<_>>(),
        vec![ids[255], ids[254], ids[253]]
    );
    Ok(())
}

#[test]
fn task_manager_never_evicts_active_tasks_when_trimming_history() -> Result<(), TaskError> {
    let mut manager = TaskManager::new();
    let running = manager.enqueue_download("/tmp/running", 1);
    manager.start(running)?;
    let queued = manager.enqueue_remote_operation("queued");

    for index in 0..256 {
        let id = manager.enqueue_download(format!("/tmp/completed-{index}"), 1);
        manager.start(id)?;
        manager.complete(id)?;
    }

    let snapshots = manager.snapshots();
    assert!(snapshots.iter().any(|task| task.id == running));
    assert!(snapshots.iter().any(|task| task.id == queued));
    assert_eq!(manager.active_count(), 2);
    Ok(())
}

#[test]
fn task_manager_active_count_tracks_each_state_transition() -> Result<(), TaskError> {
    let mut manager = TaskManager::new();
    let queued = manager.enqueue_remote_operation("queued");
    assert_eq!(manager.active_count(), 1);
    assert_eq!(manager.latest_active_id(), Some(queued));

    let running = manager.enqueue_download("/tmp/running", 1);
    manager.start(running)?;
    assert_eq!(manager.active_count(), 2);
    assert_eq!(manager.latest_active_id(), Some(running));
    manager.request_cancel(running)?;
    assert_eq!(manager.active_count(), 2);
    manager.cancelled(running)?;
    assert_eq!(manager.active_count(), 1);
    assert_eq!(manager.latest_active_id(), Some(queued));

    manager.fail(queued)?;
    assert_eq!(manager.active_count(), 0);
    assert_eq!(manager.latest_active_id(), None);
    Ok(())
}
