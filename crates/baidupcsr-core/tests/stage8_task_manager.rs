use baidupcsr_core::{
    TaskError, TaskKind, TaskManager, TaskState, TransferPolicy, TransferProgress,
};

#[test]
fn download_task_tracks_progress_speed_and_completion() -> Result<(), TaskError> {
    let mut manager = TaskManager::new();
    let id = manager.enqueue_download("/remote/file.txt", "/tmp/file.txt", 1024);

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
fn cancelling_running_task_can_finish_as_cancelled_and_keeps_reason() -> Result<(), TaskError> {
    let mut manager = TaskManager::new();
    let id = manager.enqueue_download("/remote/file.txt", "/tmp/file.txt", 1024);

    manager.start(id)?;
    manager.update_progress(id, 128, 1)?;
    manager.request_cancel(id)?;
    manager.cancelled(id, "user cancelled")?;

    let task = manager
        .snapshots()
        .into_iter()
        .find(|task| task.id == id)
        .ok_or(TaskError::UnknownTask)?;
    assert_eq!(task.state, TaskState::Cancelled);
    assert_eq!(task.error.as_deref(), Some("user cancelled"));
    assert_eq!(manager.active_count(), 0);
    Ok(())
}

#[test]
fn completed_event_after_cancel_request_still_reaches_terminal_state() -> Result<(), TaskError> {
    let mut manager = TaskManager::new();
    let id = manager.enqueue_download("/remote/file.txt", "/tmp/file.txt", 1024);

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
    assert_eq!(task.error, None);
    assert_eq!(manager.active_count(), 0);
    Ok(())
}

#[test]
fn failed_task_can_be_retried_as_new_queued_task() -> Result<(), TaskError> {
    let mut manager = TaskManager::new();
    let id = manager.enqueue_download("/remote/file.txt", "/tmp/file.txt", 1024);

    manager.start(id)?;
    manager.fail(id, "network")?;
    let retry = manager.retry(id)?;

    let tasks = manager.snapshots();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].state, TaskState::Failed);
    assert_eq!(tasks[1].id, retry);
    assert_eq!(tasks[1].retry_of, Some(id));
    assert_eq!(tasks[1].state, TaskState::Queued);
    assert_eq!(tasks[1].progress.bytes_done, 0);
    Ok(())
}

#[test]
fn upload_task_records_conflict_policy_without_starting_network_work() -> Result<(), TaskError> {
    let mut manager = TaskManager::new();
    let id = manager.enqueue_upload("/tmp/local.txt", "/remote/local.txt", TransferPolicy::Rsync);
    let task = manager
        .snapshots()
        .into_iter()
        .find(|task| task.id == id)
        .ok_or(TaskError::UnknownTask)?;

    assert_eq!(
        task.kind,
        TaskKind::Upload {
            local_path: "/tmp/local.txt".to_string(),
            remote_path: "/remote/local.txt".to_string(),
            policy: TransferPolicy::Rsync,
        }
    );
    assert_eq!(task.state, TaskState::Queued);
    Ok(())
}
