use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TaskId(u64);

impl TaskId {
    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskKind {
    Download { local_path: String },
    LocalOpen { local_path: String },
    RemoteOperation { label: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskState {
    Queued,
    Running,
    CancelRequested,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TransferProgress {
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub speed_bytes_per_sec: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskSnapshot {
    pub id: TaskId,
    pub kind: TaskKind,
    pub state: TaskState,
    pub progress: TransferProgress,
}

#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("unknown task")]
    UnknownTask,
    #[error("invalid task state transition")]
    InvalidTransition,
    #[error("download target already exists: {0}")]
    TargetExists(String),
    #[error("download chunk size must be greater than zero")]
    InvalidChunkSize,
    #[error("partial download is larger than the remote file: {0}")]
    PartialLargerThanRemote(String),
    #[error(transparent)]
    Pcs(#[from] crate::pcs::PcsError),
    #[error("task I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug)]
pub struct TaskManager {
    next_id: u64,
    tasks: BTreeMap<TaskId, TaskRecord>,
    active_ids: BTreeSet<TaskId>,
    terminal_history: VecDeque<TaskId>,
}

const MAX_TERMINAL_TASKS: usize = 128;

impl Default for TaskManager {
    fn default() -> Self {
        Self {
            next_id: 1,
            tasks: BTreeMap::new(),
            active_ids: BTreeSet::new(),
            terminal_history: VecDeque::new(),
        }
    }
}

impl TaskManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue_download(&mut self, local_path: impl Into<String>, size_bytes: u64) -> TaskId {
        self.insert_task(
            TaskKind::Download {
                local_path: local_path.into(),
            },
            Some(size_bytes),
        )
    }

    pub fn enqueue_local_open(&mut self, local_path: impl Into<String>, size_bytes: u64) -> TaskId {
        self.insert_task(
            TaskKind::LocalOpen {
                local_path: local_path.into(),
            },
            Some(size_bytes),
        )
    }

    pub fn enqueue_remote_operation(&mut self, label: impl Into<String>) -> TaskId {
        self.insert_task(
            TaskKind::RemoteOperation {
                label: label.into(),
            },
            None,
        )
    }

    pub fn start(&mut self, id: TaskId) -> Result<(), TaskError> {
        let task = self.task_mut(id)?;
        if task.state != TaskState::Queued {
            return Err(TaskError::InvalidTransition);
        }
        task.state = TaskState::Running;
        Ok(())
    }

    pub fn update_progress(
        &mut self,
        id: TaskId,
        bytes_done: u64,
        elapsed_seconds: u64,
    ) -> Result<(), TaskError> {
        let task = self.task_mut(id)?;
        if !matches!(task.state, TaskState::Running | TaskState::CancelRequested) {
            return Err(TaskError::InvalidTransition);
        }
        task.progress.bytes_done = task
            .progress
            .bytes_total
            .map_or(bytes_done, |total| bytes_done.min(total));
        task.progress.speed_bytes_per_sec = task
            .progress
            .bytes_done
            .checked_div(elapsed_seconds)
            .unwrap_or(0);
        Ok(())
    }

    pub fn request_cancel(&mut self, id: TaskId) -> Result<(), TaskError> {
        let task = self.task_mut(id)?;
        if !matches!(task.state, TaskState::Queued | TaskState::Running) {
            return Err(TaskError::InvalidTransition);
        }
        task.state = TaskState::CancelRequested;
        Ok(())
    }

    pub fn complete(&mut self, id: TaskId) -> Result<(), TaskError> {
        {
            let task = self.task_mut(id)?;
            if !matches!(task.state, TaskState::Running | TaskState::CancelRequested) {
                return Err(TaskError::InvalidTransition);
            }
            if let Some(total) = task.progress.bytes_total {
                task.progress.bytes_done = total;
            }
            task.state = TaskState::Completed;
        }
        self.mark_terminal(id);
        Ok(())
    }

    pub fn fail(&mut self, id: TaskId) -> Result<(), TaskError> {
        {
            let task = self.task_mut(id)?;
            if task.is_terminal() {
                return Err(TaskError::InvalidTransition);
            }
            task.state = TaskState::Failed;
        }
        self.mark_terminal(id);
        Ok(())
    }

    pub fn cancelled(&mut self, id: TaskId) -> Result<(), TaskError> {
        {
            let task = self.task_mut(id)?;
            if task.state != TaskState::CancelRequested {
                return Err(TaskError::InvalidTransition);
            }
            task.state = TaskState::Cancelled;
        }
        self.mark_terminal(id);
        Ok(())
    }

    pub fn snapshots(&self) -> Vec<TaskSnapshot> {
        self.tasks
            .iter()
            .map(|(id, task)| Self::snapshot_from_record(*id, task))
            .collect()
    }

    /// Return at most `limit` snapshots, newest task first.
    ///
    /// The task panel only needs the newest few entries. Keeping this query
    /// bounded avoids cloning the complete task history on every UI refresh.
    pub fn recent_snapshots(&self, limit: usize) -> Vec<TaskSnapshot> {
        self.tasks
            .iter()
            .rev()
            .take(limit)
            .map(|(id, task)| Self::snapshot_from_record(*id, task))
            .collect()
    }

    pub fn snapshot(&self, id: TaskId) -> Option<TaskSnapshot> {
        self.tasks
            .get(&id)
            .map(|task| Self::snapshot_from_record(id, task))
    }

    pub fn active_count(&self) -> usize {
        self.active_ids.len()
    }

    pub fn latest_active_id(&self) -> Option<TaskId> {
        self.active_ids.last().copied()
    }

    pub fn latest_cancellable_id(&self) -> Option<TaskId> {
        self.active_ids.iter().rev().find_map(|id| {
            self.tasks.get(id).and_then(|task| {
                matches!(task.state, TaskState::Queued | TaskState::Running).then_some(*id)
            })
        })
    }

    fn insert_task(&mut self, kind: TaskKind, bytes_total: Option<u64>) -> TaskId {
        let id = TaskId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.tasks.insert(
            id,
            TaskRecord {
                kind,
                state: TaskState::Queued,
                progress: TransferProgress {
                    bytes_done: 0,
                    bytes_total,
                    speed_bytes_per_sec: 0,
                },
            },
        );
        self.active_ids.insert(id);
        id
    }

    fn snapshot_from_record(id: TaskId, task: &TaskRecord) -> TaskSnapshot {
        TaskSnapshot {
            id,
            kind: task.kind.clone(),
            state: task.state,
            progress: task.progress,
        }
    }

    fn mark_terminal(&mut self, id: TaskId) {
        debug_assert!(self.active_ids.remove(&id));
        self.terminal_history.push_back(id);

        while self.terminal_history.len() > MAX_TERMINAL_TASKS {
            let Some(oldest_id) = self.terminal_history.pop_front() else {
                break;
            };
            self.tasks.remove(&oldest_id);
        }
    }

    fn task_mut(&mut self, id: TaskId) -> Result<&mut TaskRecord, TaskError> {
        self.tasks.get_mut(&id).ok_or(TaskError::UnknownTask)
    }
}

#[derive(Clone, Debug)]
struct TaskRecord {
    kind: TaskKind,
    state: TaskState,
    progress: TransferProgress,
}

impl TaskRecord {
    const fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            TaskState::Completed | TaskState::Failed | TaskState::Cancelled
        )
    }
}
