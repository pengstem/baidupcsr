use std::collections::VecDeque;
use std::time::{Duration, Instant};

const MAX_FINISHED: usize = 64;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TaskId(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskKind {
    Download,
    Open,
    Remote,
    Search,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskState {
    Running,
    Cancelling,
    Done,
    Failed(String),
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct Task {
    pub id: TaskId,
    pub kind: TaskKind,
    pub label: String,
    pub state: TaskState,
    pub done: u64,
    pub total: Option<u64>,
    /// Smoothed transfer rate in bytes per second.
    pub speed: f64,
    sample: Option<(Instant, u64)>,
}

impl Task {
    pub const fn is_active(&self) -> bool {
        matches!(self.state, TaskState::Running | TaskState::Cancelling)
    }

    pub fn fraction(&self) -> Option<f64> {
        self.total
            .filter(|total| *total > 0)
            .map(|total| (self.done as f64 / total as f64).clamp(0.0, 1.0))
    }

    pub fn eta(&self) -> Option<Duration> {
        let remaining = self.total?.saturating_sub(self.done);
        (self.speed >= 1.0).then(|| Duration::from_secs_f64(remaining as f64 / self.speed))
    }

    fn record(&mut self, done: u64, now: Instant) {
        match self.sample {
            // Resumed bytes count as done but not towards the speed.
            None => self.sample = Some((now, done)),
            Some((then, before)) => {
                let elapsed = now.duration_since(then).as_secs_f64();
                if elapsed < 0.2 {
                    self.done = done;
                    return;
                }
                let rate = done.saturating_sub(before) as f64 / elapsed;
                self.speed = if self.speed == 0.0 {
                    rate
                } else {
                    0.7 * self.speed + 0.3 * rate
                };
                self.sample = Some((now, done));
            }
        }
        self.done = done;
    }
}

/// Every task the session started: active ones plus a bounded history.
#[derive(Debug, Default)]
pub struct TaskList {
    next_id: u64,
    tasks: VecDeque<Task>,
}

impl TaskList {
    pub fn start(
        &mut self,
        kind: TaskKind,
        label: impl Into<String>,
        total: Option<u64>,
    ) -> TaskId {
        self.next_id += 1;
        let id = TaskId(self.next_id);
        self.tasks.push_back(Task {
            id,
            kind,
            label: label.into(),
            state: TaskState::Running,
            done: 0,
            total,
            speed: 0.0,
            sample: None,
        });
        id
    }

    pub fn progress(&mut self, id: TaskId, done: u64) {
        if let Some(task) = self.get_mut(id) {
            task.record(done, Instant::now());
        }
    }

    pub fn request_cancel(&mut self, id: TaskId) -> bool {
        match self.get_mut(id) {
            Some(task) if task.state == TaskState::Running => {
                task.state = TaskState::Cancelling;
                true
            }
            _ => false,
        }
    }

    /// Move a task to a terminal state and return it.
    pub fn finish(&mut self, id: TaskId, state: TaskState) -> Option<&Task> {
        let task = self.get_mut(id)?;
        if state == TaskState::Done
            && let Some(total) = task.total
        {
            task.done = total;
        }
        task.state = state;
        task.speed = 0.0;
        self.prune();
        self.tasks.iter().find(|task| task.id == id)
    }

    pub fn get(&self, id: TaskId) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == id)
    }

    /// All tasks, oldest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Task> {
        self.tasks.iter()
    }

    pub fn active(&self) -> impl DoubleEndedIterator<Item = &Task> {
        self.tasks.iter().filter(|task| task.is_active())
    }

    pub fn has_active(&self) -> bool {
        self.tasks.iter().any(Task::is_active)
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    fn get_mut(&mut self, id: TaskId) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|task| task.id == id)
    }

    fn prune(&mut self) {
        let mut finished = self.tasks.iter().filter(|task| !task.is_active()).count();
        self.tasks.retain(|task| {
            if finished > MAX_FINISHED && !task.is_active() {
                finished -= 1;
                false
            } else {
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_and_history_bound() {
        let mut tasks = TaskList::default();
        let id = tasks.start(TaskKind::Download, "a", Some(100));
        assert!(tasks.has_active());
        assert!(tasks.request_cancel(id));
        assert!(!tasks.request_cancel(id));
        tasks.finish(id, TaskState::Cancelled);
        assert!(!tasks.has_active());

        for index in 0..MAX_FINISHED + 10 {
            let id = tasks.start(TaskKind::Remote, index.to_string(), None);
            tasks.finish(id, TaskState::Done);
        }
        let running = tasks.start(TaskKind::Download, "keep", Some(1));
        assert_eq!(tasks.len(), MAX_FINISHED + 1);
        assert!(tasks.get(running).is_some_and(Task::is_active));
    }

    #[test]
    fn speed_ignores_resumed_bytes() {
        let mut task = TaskList::default();
        let id = task.start(TaskKind::Download, "a", Some(1000));
        let start = Instant::now();
        let record = |list: &mut TaskList, done, secs: f64| {
            if let Some(task) = list.get_mut(id) {
                task.record(done, start + Duration::from_secs_f64(secs));
            }
        };
        record(&mut task, 500, 0.0);
        record(&mut task, 600, 1.0);
        let task = task.get(id).unwrap();
        assert!((task.speed - 100.0).abs() < 1e-6);
        assert_eq!(task.eta(), Some(Duration::from_secs(4)));
        assert_eq!(task.fraction(), Some(0.6));
    }
}
