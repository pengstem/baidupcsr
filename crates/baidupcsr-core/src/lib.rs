use std::collections::{BTreeMap, HashMap};

use baidupcsr_pcs::FileEntry;

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
