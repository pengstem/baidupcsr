use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

#[cfg(test)]
mod tests;

use crate::pcs::FileEntry;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RefreshGeneration(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectoryStatus {
    Empty,
    Fresh,
    Refreshing,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryView {
    pub entries: Vec<FileEntry>,
    pub status: DirectoryStatus,
    pub generation: RefreshGeneration,
}

#[derive(Clone, Debug, Default)]
pub struct DirectoryCache {
    entries: HashMap<String, CacheEntry>,
}

impl DirectoryCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replace_with_fresh_entries(&mut self, path: &str, entries: Vec<FileEntry>) {
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
            entries: entry.entries.clone(),
            status: entry.status,
            generation,
        }
    }

    pub fn snapshot(&self, path: &str) -> DirectoryView {
        let entry = self.entries.get(path);
        DirectoryView {
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
    ) -> bool {
        let entry = self
            .entries
            .entry(path.to_string())
            .or_insert_with(CacheEntry::empty);

        if entry.generation != generation {
            return false;
        }

        entry.entries = new_entries;
        entry.status = DirectoryStatus::Fresh;

        true
    }

    pub fn finish_refresh_error(&mut self, path: &str, generation: RefreshGeneration) -> bool {
        let entry = self
            .entries
            .entry(path.to_string())
            .or_insert_with(CacheEntry::empty);

        if entry.generation != generation {
            return false;
        }

        entry.status = DirectoryStatus::Error;

        true
    }
}

#[derive(Clone, Debug)]
pub struct DirectoryCacheStore {
    root: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum DirectoryCacheStoreError {
    #[error("directory cache io error: {0}")]
    Io(#[source] io::Error),
    #[error("directory cache json error: {0}")]
    Json(#[source] serde_json::Error),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CachedDirectorySnapshot {
    account_uid: u64,
    path: String,
    pub saved_at_unix_seconds: u64,
    pub entries: Vec<FileEntry>,
}

impl DirectoryCacheStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
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
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
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
