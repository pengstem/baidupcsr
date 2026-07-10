use super::super::*;
use crate::pcs::FileEntry;

fn file(fs_id: i64, path: &str, size: i64, mtime: i64) -> FileEntry {
    FileEntry {
        fs_id,
        app_id: 266719,
        path: path.to_string(),
        server_filename: path.trim_start_matches('/').to_string(),
        ctime: 1,
        mtime,
        md5: String::new(),
        block_list: Vec::new(),
        size,
        is_dir: false,
        has_subdir: false,
    }
}

#[test]
fn entering_directory_returns_stale_snapshot_and_starts_refresh() {
    let mut cache = DirectoryCache::new();
    cache.replace_with_fresh_entries("/docs", vec![file(1, "/docs/a.txt", 10, 100)]);

    let view = cache.enter_directory("/docs");

    assert_eq!(view.entries.len(), 1);
    assert_eq!(view.status, DirectoryStatus::Refreshing);
}

#[test]
fn stale_refresh_result_cannot_overwrite_newer_generation() {
    let mut cache = DirectoryCache::new();
    let old = cache.begin_refresh("/docs");
    let new = cache.begin_refresh("/docs");

    let old_applied = cache.finish_refresh("/docs", old, vec![file(1, "/docs/old.txt", 10, 100)]);
    let new_applied = cache.finish_refresh("/docs", new, vec![file(2, "/docs/new.txt", 10, 100)]);

    assert!(!old_applied);
    assert!(new_applied);

    let view = cache.enter_directory("/docs");
    assert_eq!(view.entries[0].path, "/docs/new.txt");
}

#[test]
fn refresh_error_preserves_cached_entries_and_marks_error() {
    let mut cache = DirectoryCache::new();
    cache.replace_with_fresh_entries("/docs", vec![file(1, "/docs/a.txt", 10, 100)]);

    let generation = cache.begin_refresh("/docs");
    let applied = cache.finish_refresh_error("/docs", generation);

    assert!(applied);

    let view = cache.snapshot("/docs");
    assert_eq!(view.status, DirectoryStatus::Error);
    assert_eq!(view.entries.len(), 1);
    assert_eq!(view.entries[0].path, "/docs/a.txt");
}
