use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt, fs, io,
    path::{Path, PathBuf},
};

use baidupcsr_pcs::FileEntry;
use serde::{Deserialize, Serialize};

pub const fn crate_name() -> &'static str {
    "baidupcsr-core"
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RefreshGeneration(u64);

impl RefreshGeneration {
    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectoryStatus {
    Empty,
    Fresh,
    Refreshing,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DirectoryChange {
    Added { fs_id: i64 },
    Removed { fs_id: i64 },
    Modified { fs_id: i64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryView {
    pub path: String,
    pub entries: Vec<FileEntry>,
    pub status: DirectoryStatus,
    pub generation: RefreshGeneration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefreshResult {
    pub applied: bool,
    pub changes: Vec<DirectoryChange>,
}

#[derive(Clone, Debug, Default)]
pub struct DirectoryCache {
    entries: HashMap<String, CacheEntry>,
}

impl DirectoryCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seed(&mut self, path: &str, entries: Vec<FileEntry>) {
        self.entries.insert(
            path.to_string(),
            CacheEntry {
                entries,
                status: DirectoryStatus::Fresh,
                generation: RefreshGeneration(0),
            },
        );
    }

    pub fn enter_directory(&mut self, path: &str) -> DirectoryView {
        let generation = self.begin_refresh(path);
        let entry = self
            .entries
            .entry(path.to_string())
            .or_insert_with(CacheEntry::empty);

        DirectoryView {
            path: path.to_string(),
            entries: entry.entries.clone(),
            status: entry.status,
            generation,
        }
    }

    pub fn snapshot(&self, path: &str) -> DirectoryView {
        let entry = self.entries.get(path);
        DirectoryView {
            path: path.to_string(),
            entries: entry.map_or_else(Vec::new, |entry| entry.entries.clone()),
            status: entry.map_or(DirectoryStatus::Empty, |entry| entry.status),
            generation: entry.map_or(RefreshGeneration(0), |entry| entry.generation),
        }
    }

    pub fn begin_refresh(&mut self, path: &str) -> RefreshGeneration {
        let entry = self
            .entries
            .entry(path.to_string())
            .or_insert_with(CacheEntry::empty);
        entry.generation = RefreshGeneration(entry.generation.0 + 1);
        entry.status = DirectoryStatus::Refreshing;
        entry.generation
    }

    pub fn finish_refresh(
        &mut self,
        path: &str,
        generation: RefreshGeneration,
        new_entries: Vec<FileEntry>,
    ) -> RefreshResult {
        let entry = self
            .entries
            .entry(path.to_string())
            .or_insert_with(CacheEntry::empty);

        if entry.generation != generation {
            return RefreshResult {
                applied: false,
                changes: Vec::new(),
            };
        }

        let changes = diff_entries(&entry.entries, &new_entries);
        entry.entries = new_entries;
        entry.status = DirectoryStatus::Fresh;

        RefreshResult {
            applied: true,
            changes,
        }
    }

    pub fn finish_refresh_error(
        &mut self,
        path: &str,
        generation: RefreshGeneration,
    ) -> RefreshResult {
        let entry = self
            .entries
            .entry(path.to_string())
            .or_insert_with(CacheEntry::empty);

        if entry.generation != generation {
            return RefreshResult {
                applied: false,
                changes: Vec::new(),
            };
        }

        entry.status = DirectoryStatus::Error;

        RefreshResult {
            applied: true,
            changes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DirectoryCacheStore {
    root: PathBuf,
}

#[derive(Debug)]
pub enum DirectoryCacheStoreError {
    Io(io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for DirectoryCacheStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "directory cache io error: {err}"),
            Self::Json(err) => write!(f, "directory cache json error: {err}"),
        }
    }
}

impl std::error::Error for DirectoryCacheStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Json(err) => Some(err),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CachedDirectorySnapshot {
    pub account_uid: u64,
    pub path: String,
    pub saved_at_unix_seconds: u64,
    pub entries: Vec<FileEntry>,
}

impl DirectoryCacheStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn write_snapshot(
        &self,
        account_uid: u64,
        path: &str,
        entries: Vec<FileEntry>,
        saved_at_unix_seconds: u64,
    ) -> Result<(), DirectoryCacheStoreError> {
        fs::create_dir_all(&self.root).map_err(DirectoryCacheStoreError::Io)?;
        let snapshot = CachedDirectorySnapshot {
            account_uid,
            path: path.to_string(),
            saved_at_unix_seconds,
            entries,
        };
        let data = serde_json::to_vec(&snapshot).map_err(DirectoryCacheStoreError::Json)?;
        let target = self.snapshot_path(account_uid, path);
        let temporary = target.with_extension("json.tmp");
        fs::write(&temporary, data).map_err(DirectoryCacheStoreError::Io)?;
        fs::rename(&temporary, target).map_err(DirectoryCacheStoreError::Io)
    }

    pub fn read_snapshot(
        &self,
        account_uid: u64,
        path: &str,
    ) -> Result<Option<CachedDirectorySnapshot>, DirectoryCacheStoreError> {
        let target = self.snapshot_path(account_uid, path);
        let data = match fs::read(target) {
            Ok(data) => data,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(DirectoryCacheStoreError::Io(err)),
        };
        let snapshot: CachedDirectorySnapshot =
            serde_json::from_slice(&data).map_err(DirectoryCacheStoreError::Json)?;
        if snapshot.account_uid == account_uid && snapshot.path == path {
            Ok(Some(snapshot))
        } else {
            Ok(None)
        }
    }

    fn snapshot_path(&self, account_uid: u64, path: &str) -> PathBuf {
        self.root.join(format!(
            "dir-{account_uid}-{:016x}.json",
            stable_path_hash(path)
        ))
    }
}

fn stable_path_hash(path: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PreloadKind {
    Directory,
    Preview,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PreloadTarget {
    pub path: String,
    pub kind: PreloadKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PagedPreloadPlan {
    pub targets: Vec<PreloadTarget>,
    pub window_start: usize,
    pub window_end: usize,
    pub considered_entries: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PagedPreloadPlanner {
    visible_rows: usize,
    page_radius: usize,
    max_directories: usize,
    max_previews: usize,
}

impl PagedPreloadPlanner {
    pub const fn new(
        visible_rows: usize,
        page_radius: usize,
        max_directories: usize,
        max_previews: usize,
    ) -> Self {
        Self {
            visible_rows,
            page_radius,
            max_directories,
            max_previews,
        }
    }

    pub fn plan(&self, entries: &[FileEntry], selected_index: usize) -> PagedPreloadPlan {
        if entries.is_empty() || self.visible_rows == 0 {
            return PagedPreloadPlan {
                targets: Vec::new(),
                window_start: 0,
                window_end: 0,
                considered_entries: 0,
            };
        }
        let selected = selected_index.min(entries.len().saturating_sub(1));
        let page_start = selected / self.visible_rows * self.visible_rows;
        let page_end = page_start
            .saturating_add(self.visible_rows)
            .min(entries.len());
        let window_start = page_start.saturating_sub(self.page_radius * self.visible_rows);
        let window_end = page_end
            .saturating_add(self.page_radius * self.visible_rows)
            .min(entries.len());

        let mut targets = Vec::new();
        self.push_targets(entries, page_start, page_end, &mut targets);
        if targets.len() < self.max_directories.saturating_add(self.max_previews) {
            self.push_targets(entries, window_start, page_start, &mut targets);
            self.push_targets(entries, page_end, window_end, &mut targets);
        }

        PagedPreloadPlan {
            targets,
            window_start,
            window_end,
            considered_entries: window_end.saturating_sub(window_start),
        }
    }

    fn push_targets(
        &self,
        entries: &[FileEntry],
        start: usize,
        end: usize,
        targets: &mut Vec<PreloadTarget>,
    ) {
        let mut directories = targets
            .iter()
            .filter(|target| target.kind == PreloadKind::Directory)
            .count();
        let mut previews = targets
            .iter()
            .filter(|target| target.kind == PreloadKind::Preview)
            .count();

        for entry in &entries[start..end] {
            if entry.is_dir {
                if directories >= self.max_directories {
                    continue;
                }
                let target = PreloadTarget {
                    path: entry.path.clone(),
                    kind: PreloadKind::Directory,
                };
                if !targets.contains(&target) {
                    targets.push(target);
                    directories += 1;
                }
            } else if previews < self.max_previews && is_light_preview_candidate(entry) {
                let target = PreloadTarget {
                    path: entry.path.clone(),
                    kind: PreloadKind::Preview,
                };
                if !targets.contains(&target) {
                    targets.push(target);
                    previews += 1;
                }
            }
        }
    }
}

fn is_light_preview_candidate(entry: &FileEntry) -> bool {
    let name = entry.server_filename.to_ascii_lowercase();
    matches!(
        name.rsplit_once('.').map(|(_, ext)| ext),
        Some("txt" | "md" | "json" | "toml" | "yaml" | "yml" | "png" | "jpg" | "jpeg")
    )
}

#[derive(Clone, Debug)]
struct CacheEntry {
    entries: Vec<FileEntry>,
    status: DirectoryStatus,
    generation: RefreshGeneration,
}

impl CacheEntry {
    const fn empty() -> Self {
        Self {
            entries: Vec::new(),
            status: DirectoryStatus::Empty,
            generation: RefreshGeneration(0),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SessionState {
    current_path: Option<String>,
    recent_paths: Vec<String>,
    favorite_paths: Vec<String>,
}

impl SessionState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn visit(&mut self, path: &str) {
        self.current_path = Some(path.to_string());
        self.recent_paths.retain(|item| item != path);
        self.recent_paths.insert(0, path.to_string());
    }

    pub fn toggle_favorite(&mut self, path: &str) {
        if self.favorite_paths.iter().any(|item| item == path) {
            self.favorite_paths.retain(|item| item != path);
        } else {
            self.favorite_paths.push(path.to_string());
        }
    }

    pub fn current_path(&self) -> Option<&str> {
        self.current_path.as_deref()
    }

    pub fn recent_paths(&self) -> Vec<&str> {
        self.recent_paths.iter().map(String::as_str).collect()
    }

    pub fn favorite_paths(&self) -> Vec<&str> {
        self.favorite_paths.iter().map(String::as_str).collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TaskId(u64);

impl TaskId {
    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransferPolicy {
    Skip,
    Overwrite,
    Rsync,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskKind {
    Download {
        remote_path: String,
        local_path: String,
        size_bytes: u64,
    },
    LocalOpen {
        remote_path: String,
        local_path: String,
        size_bytes: u64,
    },
    Upload {
        local_path: String,
        remote_path: String,
        policy: TransferPolicy,
    },
    RemoteOperation {
        label: String,
        item_count: usize,
    },
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
    pub error: Option<String>,
    pub retry_of: Option<TaskId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskError {
    UnknownTask,
    InvalidTransition,
}

#[derive(Clone, Debug)]
pub struct TaskManager {
    next_id: u64,
    tasks: BTreeMap<TaskId, TaskRecord>,
}

impl Default for TaskManager {
    fn default() -> Self {
        Self {
            next_id: 1,
            tasks: BTreeMap::new(),
        }
    }
}

impl TaskManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue_download(
        &mut self,
        remote_path: impl Into<String>,
        local_path: impl Into<String>,
        size_bytes: u64,
    ) -> TaskId {
        self.insert_task(
            TaskKind::Download {
                remote_path: remote_path.into(),
                local_path: local_path.into(),
                size_bytes,
            },
            Some(size_bytes),
            None,
        )
    }

    pub fn enqueue_local_open(
        &mut self,
        remote_path: impl Into<String>,
        local_path: impl Into<String>,
        size_bytes: u64,
    ) -> TaskId {
        self.insert_task(
            TaskKind::LocalOpen {
                remote_path: remote_path.into(),
                local_path: local_path.into(),
                size_bytes,
            },
            Some(size_bytes),
            None,
        )
    }

    pub fn enqueue_upload(
        &mut self,
        local_path: impl Into<String>,
        remote_path: impl Into<String>,
        policy: TransferPolicy,
    ) -> TaskId {
        self.insert_task(
            TaskKind::Upload {
                local_path: local_path.into(),
                remote_path: remote_path.into(),
                policy,
            },
            None,
            None,
        )
    }

    pub fn enqueue_remote_operation(
        &mut self,
        label: impl Into<String>,
        item_count: usize,
    ) -> TaskId {
        self.insert_task(
            TaskKind::RemoteOperation {
                label: label.into(),
                item_count,
            },
            None,
            None,
        )
    }

    pub fn start(&mut self, id: TaskId) -> Result<(), TaskError> {
        let task = self.task_mut(id)?;
        if task.state != TaskState::Queued {
            return Err(TaskError::InvalidTransition);
        }
        task.state = TaskState::Running;
        task.error = None;
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
        let task = self.task_mut(id)?;
        if !matches!(task.state, TaskState::Running | TaskState::CancelRequested) {
            return Err(TaskError::InvalidTransition);
        }
        if let Some(total) = task.progress.bytes_total {
            task.progress.bytes_done = total;
        }
        task.state = TaskState::Completed;
        task.error = None;
        Ok(())
    }

    pub fn fail(&mut self, id: TaskId, error: impl Into<String>) -> Result<(), TaskError> {
        let task = self.task_mut(id)?;
        if task.is_terminal() {
            return Err(TaskError::InvalidTransition);
        }
        task.state = TaskState::Failed;
        task.error = Some(error.into());
        Ok(())
    }

    pub fn cancelled(&mut self, id: TaskId, reason: impl Into<String>) -> Result<(), TaskError> {
        let task = self.task_mut(id)?;
        if task.state != TaskState::CancelRequested {
            return Err(TaskError::InvalidTransition);
        }
        task.state = TaskState::Cancelled;
        task.error = Some(reason.into());
        Ok(())
    }

    pub fn retry(&mut self, id: TaskId) -> Result<TaskId, TaskError> {
        let task = self.task(id)?;
        if !matches!(task.state, TaskState::Failed | TaskState::Cancelled) {
            return Err(TaskError::InvalidTransition);
        }
        Ok(self.insert_task(task.kind.clone(), task.progress.bytes_total, Some(id)))
    }

    pub fn snapshots(&self) -> Vec<TaskSnapshot> {
        self.tasks
            .iter()
            .map(|(id, task)| TaskSnapshot {
                id: *id,
                kind: task.kind.clone(),
                state: task.state,
                progress: task.progress,
                error: task.error.clone(),
                retry_of: task.retry_of,
            })
            .collect()
    }

    pub fn active_count(&self) -> usize {
        self.tasks
            .values()
            .filter(|task| !task.is_terminal())
            .count()
    }

    fn insert_task(
        &mut self,
        kind: TaskKind,
        bytes_total: Option<u64>,
        retry_of: Option<TaskId>,
    ) -> TaskId {
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
                error: None,
                retry_of,
            },
        );
        id
    }

    fn task(&self, id: TaskId) -> Result<&TaskRecord, TaskError> {
        self.tasks.get(&id).ok_or(TaskError::UnknownTask)
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
    error: Option<String>,
    retry_of: Option<TaskId>,
}

impl TaskRecord {
    const fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            TaskState::Completed | TaskState::Failed | TaskState::Cancelled
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemotePathPair {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoteOperationKind {
    Mkdir { path: String },
    Delete { paths: Vec<String> },
    Rename { from: String, to: String },
    Copy { pairs: Vec<RemotePathPair> },
    Move { pairs: Vec<RemotePathPair> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteOperationPlan {
    kind: RemoteOperationKind,
    destructive: bool,
    affected_dirs: Vec<String>,
    dry_run_lines: Vec<String>,
}

impl RemoteOperationPlan {
    pub fn mkdir(path: impl Into<String>) -> Result<Self, String> {
        let path = normalize_remote_path(path.into())?;
        let affected_dirs = related_dirs_for_paths([path.as_str()]);
        Ok(Self {
            dry_run_lines: vec![format!("mkdir {path}")],
            kind: RemoteOperationKind::Mkdir { path },
            destructive: false,
            affected_dirs,
        })
    }

    pub fn delete(paths: Vec<String>) -> Result<Self, String> {
        let paths = normalize_nonempty_paths(paths)?;
        let mut dry_run_lines = vec![format!("delete {} remote entries", paths.len())];
        dry_run_lines.extend(paths.iter().cloned());
        let affected_dirs = related_dirs_for_paths(paths.iter().map(String::as_str));
        Ok(Self {
            kind: RemoteOperationKind::Delete { paths },
            destructive: true,
            affected_dirs,
            dry_run_lines,
        })
    }

    pub fn rename(from: impl Into<String>, to: impl Into<String>) -> Result<Self, String> {
        let from = normalize_remote_path(from.into())?;
        let to = normalize_remote_path(to.into())?;
        let affected_dirs = related_dirs_for_paths([from.as_str(), to.as_str()]);
        Ok(Self {
            dry_run_lines: vec![format!("rename {from} -> {to}")],
            kind: RemoteOperationKind::Rename { from, to },
            destructive: false,
            affected_dirs,
        })
    }

    pub fn copy(pairs: Vec<RemotePathPair>) -> Result<Self, String> {
        Self::copy_or_move("copy", pairs, false)
    }

    pub fn move_paths(pairs: Vec<RemotePathPair>) -> Result<Self, String> {
        Self::copy_or_move("move", pairs, true)
    }

    pub const fn kind(&self) -> &RemoteOperationKind {
        &self.kind
    }

    pub const fn is_destructive(&self) -> bool {
        self.destructive
    }

    pub fn affected_dirs(&self) -> &[String] {
        &self.affected_dirs
    }

    pub fn dry_run_lines(&self) -> &[String] {
        &self.dry_run_lines
    }

    pub fn label(&self) -> &'static str {
        match &self.kind {
            RemoteOperationKind::Mkdir { .. } => "mkdir",
            RemoteOperationKind::Delete { .. } => "delete",
            RemoteOperationKind::Rename { .. } => "rename",
            RemoteOperationKind::Copy { .. } => "copy",
            RemoteOperationKind::Move { .. } => "move",
        }
    }

    pub fn item_count(&self) -> usize {
        match &self.kind {
            RemoteOperationKind::Mkdir { .. } | RemoteOperationKind::Rename { .. } => 1,
            RemoteOperationKind::Delete { paths } => paths.len(),
            RemoteOperationKind::Copy { pairs } | RemoteOperationKind::Move { pairs } => {
                pairs.len()
            }
        }
    }

    fn copy_or_move(
        label: &'static str,
        pairs: Vec<RemotePathPair>,
        is_move: bool,
    ) -> Result<Self, String> {
        let pairs = normalize_nonempty_pairs(pairs)?;
        let mut dry_run_lines = vec![format!("{label} {} remote entries", pairs.len())];
        dry_run_lines.extend(
            pairs
                .iter()
                .map(|pair| format!("{} -> {}", pair.from, pair.to)),
        );
        let affected_dirs = related_dirs_for_paths(
            pairs
                .iter()
                .flat_map(|pair| [pair.from.as_str(), pair.to.as_str()]),
        );
        Ok(Self {
            kind: if is_move {
                RemoteOperationKind::Move { pairs }
            } else {
                RemoteOperationKind::Copy { pairs }
            },
            destructive: false,
            affected_dirs,
            dry_run_lines,
        })
    }
}

fn normalize_nonempty_paths(paths: Vec<String>) -> Result<Vec<String>, String> {
    if paths.is_empty() {
        return Err("operation requires at least one remote path".to_string());
    }
    paths.into_iter().map(normalize_remote_path).collect()
}

fn normalize_nonempty_pairs(pairs: Vec<RemotePathPair>) -> Result<Vec<RemotePathPair>, String> {
    if pairs.is_empty() {
        return Err("operation requires at least one remote path pair".to_string());
    }
    pairs
        .into_iter()
        .map(|pair| {
            Ok(RemotePathPair {
                from: normalize_remote_path(pair.from)?,
                to: normalize_remote_path(pair.to)?,
            })
        })
        .collect()
}

fn normalize_remote_path(path: String) -> Result<String, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("remote path must not be empty".to_string());
    }
    let mut normalized = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    Ok(normalized)
}

fn related_dirs_for_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let dirs: BTreeSet<String> = paths.into_iter().map(remote_parent_path).collect();
    dirs.into_iter().collect()
}

fn remote_parent_path(path: &str) -> String {
    let path = path.trim_end_matches('/');
    if path.is_empty() || path == "/" {
        return "/".to_string();
    }
    match path.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(index) => path[..index].to_string(),
    }
}

fn diff_entries(old_entries: &[FileEntry], new_entries: &[FileEntry]) -> Vec<DirectoryChange> {
    let old_by_id = entries_by_id(old_entries);
    let new_by_id = entries_by_id(new_entries);

    let mut changes = Vec::new();

    for fs_id in new_by_id.keys() {
        if !old_by_id.contains_key(fs_id) {
            changes.push(DirectoryChange::Added { fs_id: *fs_id });
        }
    }

    for fs_id in old_by_id.keys() {
        if !new_by_id.contains_key(fs_id) {
            changes.push(DirectoryChange::Removed { fs_id: *fs_id });
        }
    }

    for (fs_id, old_entry) in &old_by_id {
        if let Some(new_entry) = new_by_id.get(fs_id) {
            if entry_changed(old_entry, new_entry) {
                changes.push(DirectoryChange::Modified { fs_id: *fs_id });
            }
        }
    }

    changes
}

fn entries_by_id(entries: &[FileEntry]) -> BTreeMap<i64, &FileEntry> {
    entries.iter().map(|entry| (entry.fs_id, entry)).collect()
}

fn entry_changed(old_entry: &FileEntry, new_entry: &FileEntry) -> bool {
    old_entry.path != new_entry.path
        || old_entry.server_filename != new_entry.server_filename
        || old_entry.mtime != new_entry.mtime
        || old_entry.size != new_entry.size
        || old_entry.md5 != new_entry.md5
        || old_entry.is_dir != new_entry.is_dir
}
