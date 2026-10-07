use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use atomic_write_file::AtomicWriteFile;
use serde::{Deserialize, Serialize};

use crate::pcs::FileEntry;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortKey {
    Name,
    Modified,
    Size,
    Type,
}

impl SortKey {
    pub const ALL: [Self; 4] = [Self::Name, Self::Modified, Self::Size, Self::Type];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Modified => "modified",
            Self::Size => "size",
            Self::Type => "type",
        }
    }

    pub const fn hotkey(self) -> char {
        match self {
            Self::Name => 'n',
            Self::Modified => 'm',
            Self::Size => 's',
            Self::Type => 't',
        }
    }

    /// Newest and largest first is the useful default for those keys.
    const fn default_ascending(self) -> bool {
        matches!(self, Self::Name | Self::Type)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SortMode {
    pub key: SortKey,
    pub ascending: bool,
}

impl Default for SortMode {
    fn default() -> Self {
        Self {
            key: SortKey::Name,
            ascending: true,
        }
    }
}

impl SortMode {
    /// Choosing the current key again flips the direction.
    pub fn choose(self, key: SortKey) -> Self {
        if self.key == key {
            Self {
                key,
                ascending: !self.ascending,
            }
        } else {
            Self {
                key,
                ascending: key.default_ascending(),
            }
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SortValue {
    Number(i64, String),
    Text(String, String),
}

/// Sort directories first, then by `mode` within each group.
pub fn sort_entries(entries: &mut [FileEntry], mode: SortMode) {
    entries.sort_by_cached_key(|entry| {
        let name = entry.server_filename.to_lowercase();
        let value = match mode.key {
            SortKey::Name => SortValue::Text(name, entry.server_filename.clone()),
            SortKey::Modified => SortValue::Number(entry.mtime, name),
            SortKey::Size => SortValue::Number(entry.size, name),
            SortKey::Type => {
                let extension = name
                    .rsplit_once('.')
                    .map(|(_, extension)| extension.to_string())
                    .unwrap_or_default();
                SortValue::Text(extension, name)
            }
        };
        (!entry.is_dir, value)
    });
    if !mode.ascending {
        let split = entries.partition_point(|entry| entry.is_dir);
        entries[..split].reverse();
        entries[split..].reverse();
    }
}

#[derive(Clone, Debug, Default)]
pub struct Listing {
    pub entries: Vec<FileEntry>,
    /// Entries were confirmed by the server this session (not only from disk).
    pub fresh: bool,
    pub refreshing: bool,
    pub error: Option<String>,
    pub loaded_at: Option<SystemTime>,
    generation: u64,
}

impl Listing {
    pub const fn is_loaded(&self) -> bool {
        self.loaded_at.is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Generation(u64);

/// In-memory directory listings, kept sorted by the current [`SortMode`].
#[derive(Debug, Default)]
pub struct DirectoryCache {
    listings: HashMap<String, Listing>,
    sort: SortMode,
}

impl DirectoryCache {
    pub fn get(&self, path: &str) -> Option<&Listing> {
        self.listings.get(path)
    }

    pub fn entries(&self, path: &str) -> &[FileEntry] {
        self.listings
            .get(path)
            .map_or(&[], |listing| listing.entries.as_slice())
    }

    pub const fn sort(&self) -> SortMode {
        self.sort
    }

    pub fn set_sort(&mut self, sort: SortMode) {
        self.sort = sort;
        for listing in self.listings.values_mut() {
            sort_entries(&mut listing.entries, sort);
        }
    }

    /// Whether `path` should be (re)fetched: never loaded this session and
    /// not already being fetched.
    pub fn needs_fetch(&self, path: &str) -> bool {
        self.listings
            .get(path)
            .is_none_or(|listing| !listing.fresh && !listing.refreshing)
    }

    pub fn begin_refresh(&mut self, path: &str) -> Generation {
        let listing = self.listings.entry(path.to_string()).or_default();
        listing.generation += 1;
        listing.refreshing = true;
        Generation(listing.generation)
    }

    /// Apply a fetch result unless a newer fetch was started meanwhile.
    pub fn finish_refresh(
        &mut self,
        path: &str,
        generation: Generation,
        result: Result<Vec<FileEntry>, String>,
    ) -> bool {
        let sort = self.sort;
        let Some(listing) = self.listings.get_mut(path) else {
            return false;
        };
        if listing.generation != generation.0 {
            return false;
        }
        listing.refreshing = false;
        match result {
            Ok(mut entries) => {
                sort_entries(&mut entries, sort);
                listing.entries = entries;
                listing.fresh = true;
                listing.error = None;
                listing.loaded_at = Some(SystemTime::now());
            }
            Err(error) => listing.error = Some(error),
        }
        true
    }

    /// Seed a listing from the disk cache; it stays stale until refreshed.
    pub fn insert_stale(&mut self, path: &str, mut entries: Vec<FileEntry>, saved_at: SystemTime) {
        if self.listings.get(path).is_some_and(Listing::is_loaded) {
            return;
        }
        sort_entries(&mut entries, self.sort);
        let listing = self.listings.entry(path.to_string()).or_default();
        listing.entries = entries;
        listing.loaded_at = Some(saved_at);
    }

    /// Mark listings as stale so they are fetched again on the next visit.
    pub fn invalidate(&mut self, path: &str) {
        if let Some(listing) = self.listings.get_mut(path) {
            listing.fresh = false;
        }
    }
}

/// Listings persisted between sessions so directories show instantly.
#[derive(Clone, Debug)]
pub struct DirectoryStore {
    root: PathBuf,
    account_uid: u64,
}

#[derive(Deserialize, Serialize)]
struct Snapshot {
    account_uid: u64,
    path: String,
    saved_at_unix_seconds: u64,
    entries: Vec<FileEntry>,
}

impl DirectoryStore {
    pub fn new(root: PathBuf, account_uid: u64) -> Self {
        Self { root, account_uid }
    }

    pub fn read(&self, path: &str) -> Option<(Vec<FileEntry>, SystemTime)> {
        let data = std::fs::read(self.file(path)).ok()?;
        let snapshot: Snapshot = serde_json::from_slice(&data).ok()?;
        (snapshot.account_uid == self.account_uid && snapshot.path == path).then(|| {
            let saved_at =
                UNIX_EPOCH + std::time::Duration::from_secs(snapshot.saved_at_unix_seconds);
            (snapshot.entries, saved_at)
        })
    }

    pub fn write(&self, path: &str, entries: Vec<FileEntry>) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let snapshot = Snapshot {
            account_uid: self.account_uid,
            path: path.to_string(),
            saved_at_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs()),
            entries,
        };
        let data = serde_json::to_vec(&snapshot)?;
        let mut file = AtomicWriteFile::open(self.file(path))?;
        file.write_all(&data)?;
        file.commit()
    }

    fn file(&self, path: &str) -> PathBuf {
        Path::new(&self.root).join(format!(
            "dir-{}-{:016x}.json",
            self.account_uid,
            fnv1a(path)
        ))
    }
}

/// Stable across builds, unlike `std`'s `DefaultHasher`.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool, size: i64, mtime: i64) -> FileEntry {
        FileEntry {
            fs_id: 0,
            path: format!("/{name}"),
            server_filename: name.to_string(),
            mtime,
            size,
            is_dir,
        }
    }

    fn names(entries: &[FileEntry]) -> Vec<&str> {
        entries
            .iter()
            .map(|entry| entry.server_filename.as_str())
            .collect()
    }

    #[test]
    fn sorts_dirs_first_in_both_directions() {
        let mut entries = vec![
            entry("b.txt", false, 3, 1),
            entry("Z", true, 0, 5),
            entry("a.mkv", false, 9, 2),
            entry("c", true, 0, 1),
        ];
        sort_entries(&mut entries, SortMode::default());
        assert_eq!(names(&entries), ["c", "Z", "a.mkv", "b.txt"]);
        let by_size = SortMode::default().choose(SortKey::Size);
        assert!(!by_size.ascending);
        sort_entries(&mut entries, by_size);
        assert_eq!(names(&entries), ["Z", "c", "a.mkv", "b.txt"]);
        assert!(by_size.choose(SortKey::Size).ascending);
    }

    #[test]
    fn stale_results_are_ignored() {
        let mut cache = DirectoryCache::default();
        assert!(cache.needs_fetch("/a"));
        let first = cache.begin_refresh("/a");
        assert!(!cache.needs_fetch("/a"));
        let second = cache.begin_refresh("/a");
        assert!(!cache.finish_refresh("/a", first, Ok(vec![entry("x", false, 1, 1)])));
        assert!(cache.finish_refresh("/a", second, Ok(vec![entry("y", false, 1, 1)])));
        assert_eq!(names(cache.entries("/a")), ["y"]);
        assert!(!cache.needs_fetch("/a"));
        cache.invalidate("/a");
        assert!(cache.needs_fetch("/a"));
    }

    #[test]
    fn store_round_trips_per_account() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectoryStore::new(dir.path().to_path_buf(), 7);
        store.write("/a", vec![entry("x", false, 1, 1)]).unwrap();
        let (entries, _) = store.read("/a").unwrap();
        assert_eq!(names(&entries), ["x"]);
        assert!(
            DirectoryStore::new(dir.path().to_path_buf(), 8)
                .read("/a")
                .is_none()
        );
    }
}
