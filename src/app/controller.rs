use std::collections::BTreeMap;

use crate::pcs::{FileEntry, PcsError};
use crate::preview::{PreviewError, PreviewKey, PreviewPayload};

use super::{TaskError, TaskId, TaskSnapshot};

pub type Generation = u64;

#[derive(Debug)]
pub enum AppCommand {
    Refresh { path: String },
    SelectionChanged { entry: Option<FileEntry> },
    CancelTask { task_id: TaskId },
    Tick,
    Quit,
}

#[derive(Debug)]
pub enum AppEvent {
    DirectoryLoaded {
        generation: Generation,
        path: String,
        result: Result<Vec<FileEntry>, PcsError>,
    },
    PreviewLoaded {
        generation: Generation,
        key: PreviewKey,
        result: Result<PreviewPayload, PreviewError>,
    },
    TaskChanged {
        task: TaskSnapshot,
    },
    Failed {
        operation: &'static str,
        error: AppError,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppEffect {
    LoadDirectory {
        generation: Generation,
        path: String,
    },
    LoadPreview {
        generation: Generation,
        key: PreviewKey,
    },
    CancelDirectory,
    CancelPreview,
    CancelTask {
        task_id: TaskId,
    },
    Shutdown,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Pcs(#[from] PcsError),
    #[error(transparent)]
    Preview(#[from] PreviewError),
    #[error(transparent)]
    Task(#[from] TaskError),
    #[error("{0}")]
    Runtime(String),
}

#[derive(Debug)]
pub struct AppController {
    account_uid: u64,
    current_path: String,
    directory_entries: Vec<FileEntry>,
    preview_payload: Option<PreviewPayload>,
    directory_generation: Generation,
    preview_generation: Generation,
    tasks: BTreeMap<TaskId, TaskSnapshot>,
    last_failure: Option<String>,
    quitting: bool,
}

impl AppController {
    pub fn new(initial_path: impl Into<String>) -> Self {
        Self {
            account_uid: 0,
            current_path: initial_path.into(),
            directory_entries: Vec::new(),
            preview_payload: None,
            directory_generation: 0,
            preview_generation: 0,
            tasks: BTreeMap::new(),
            last_failure: None,
            quitting: false,
        }
    }

    pub const fn with_account_uid(mut self, account_uid: u64) -> Self {
        self.account_uid = account_uid;
        self
    }

    pub fn dispatch(&mut self, command: AppCommand) -> Vec<AppEffect> {
        match command {
            AppCommand::Refresh { path } => {
                self.directory_generation = self.directory_generation.saturating_add(1);
                vec![
                    AppEffect::CancelDirectory,
                    AppEffect::LoadDirectory {
                        generation: self.directory_generation,
                        path,
                    },
                ]
            }
            AppCommand::SelectionChanged { entry } => {
                self.preview_generation = self.preview_generation.saturating_add(1);
                self.preview_payload = None;
                let mut effects = vec![AppEffect::CancelPreview];
                if let Some(entry) = entry {
                    effects.push(AppEffect::LoadPreview {
                        generation: self.preview_generation,
                        key: PreviewKey {
                            account_uid: self.account_uid,
                            fs_id: entry.fs_id,
                            mtime: entry.mtime,
                            size: u64::try_from(entry.size).unwrap_or(0),
                        },
                    });
                }
                effects
            }
            AppCommand::CancelTask { task_id } => vec![AppEffect::CancelTask { task_id }],
            AppCommand::Tick => Vec::new(),
            AppCommand::Quit => {
                self.quitting = true;
                vec![AppEffect::Shutdown]
            }
        }
    }

    pub fn reduce(&mut self, event: AppEvent) -> Vec<AppEffect> {
        match event {
            AppEvent::DirectoryLoaded {
                generation,
                path,
                result: Ok(entries),
            } if generation == self.directory_generation => {
                self.current_path = path;
                self.directory_entries = entries;
            }
            AppEvent::PreviewLoaded {
                generation,
                result: Ok(payload),
                ..
            } if generation == self.preview_generation => {
                self.preview_payload = Some(payload);
            }
            AppEvent::DirectoryLoaded {
                generation,
                result: Err(error),
                ..
            } if generation == self.directory_generation => {
                self.last_failure = Some(error.to_string());
            }
            AppEvent::PreviewLoaded {
                generation,
                result: Err(error),
                ..
            } if generation == self.preview_generation => {
                self.last_failure = Some(error.to_string());
            }
            AppEvent::TaskChanged { task } => {
                self.tasks.insert(task.id, task);
            }
            AppEvent::Failed { operation, error } => {
                self.last_failure = Some(format!("{operation}: {error}"));
            }
            AppEvent::DirectoryLoaded { .. } | AppEvent::PreviewLoaded { .. } => {}
        }
        Vec::new()
    }

    pub const fn directory_generation(&self) -> Generation {
        self.directory_generation
    }

    pub const fn preview_generation(&self) -> Generation {
        self.preview_generation
    }

    pub const fn is_quitting(&self) -> bool {
        self.quitting
    }

    pub fn directory_entries(&self) -> &[FileEntry] {
        &self.directory_entries
    }

    pub const fn preview_payload(&self) -> Option<&PreviewPayload> {
        self.preview_payload.as_ref()
    }

    pub fn task_snapshot(&self, id: TaskId) -> Option<&TaskSnapshot> {
        self.tasks.get(&id)
    }

    pub fn last_failure(&self) -> Option<&str> {
        self.last_failure.as_deref()
    }
}
