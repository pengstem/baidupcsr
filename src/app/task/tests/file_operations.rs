use crate::app::{RemoteOperationPlan, RemotePathPair, TaskError, TaskKind, TaskManager};

#[test]
fn delete_plan_is_destructive_and_reports_related_directories() -> Result<(), String> {
    let plan = RemoteOperationPlan::delete(vec![
        "/docs/a.txt".to_string(),
        "/docs/sub/b.txt".to_string(),
    ])?;

    assert!(plan.is_destructive());
    assert_eq!(
        plan.affected_dirs(),
        &["/docs".to_string(), "/docs/sub".to_string()]
    );
    assert_eq!(
        plan.dry_run_lines(),
        &[
            "delete 2 remote entries".to_string(),
            "/docs/a.txt".to_string(),
            "/docs/sub/b.txt".to_string(),
        ]
    );
    Ok(())
}

#[test]
fn mkdir_plan_is_non_destructive_and_refreshes_parent() -> Result<(), String> {
    let plan = RemoteOperationPlan::mkdir("/docs/new")?;

    assert!(!plan.is_destructive());
    assert_eq!(plan.affected_dirs(), &["/docs".to_string()]);
    assert_eq!(plan.dry_run_lines()[0], "mkdir /docs/new");
    Ok(())
}

#[test]
fn copy_and_move_plans_deduplicate_related_directories() -> Result<(), String> {
    let pairs = vec![
        RemotePathPair {
            from: "/docs/a.txt".to_string(),
            to: "/backup/a.txt".to_string(),
        },
        RemotePathPair {
            from: "/docs/b.txt".to_string(),
            to: "/backup/b.txt".to_string(),
        },
    ];

    let copy = RemoteOperationPlan::copy(pairs.clone())?;
    let move_plan = RemoteOperationPlan::move_paths(pairs)?;

    assert!(!copy.is_destructive());
    assert!(!move_plan.is_destructive());
    assert_eq!(
        copy.affected_dirs(),
        &["/backup".to_string(), "/docs".to_string()]
    );
    assert_eq!(
        move_plan.affected_dirs(),
        &["/backup".to_string(), "/docs".to_string()]
    );
    Ok(())
}

#[test]
fn empty_operation_targets_are_rejected() {
    assert!(RemoteOperationPlan::delete(Vec::new()).is_err());
    assert!(RemoteOperationPlan::copy(Vec::new()).is_err());
    assert!(RemoteOperationPlan::move_paths(Vec::new()).is_err());
}

#[test]
fn task_manager_can_track_remote_operation_tasks() -> Result<(), TaskError> {
    let mut manager = TaskManager::new();
    let id = manager.enqueue_remote_operation("delete");

    manager.start(id)?;
    manager.complete(id)?;
    let task = manager.snapshots().remove(0);

    assert_eq!(
        task.kind,
        TaskKind::RemoteOperation {
            label: "delete".to_string(),
        }
    );
    assert_eq!(manager.active_count(), 0);
    Ok(())
}
