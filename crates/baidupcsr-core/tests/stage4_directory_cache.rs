use baidupcsr_core::{DirectoryCache, DirectoryChange, DirectoryStatus, SessionState};
use baidupcsr_pcs::FileEntry;

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
    cache.seed("/docs", vec![file(1, "/docs/a.txt", 10, 100)]);

    let view = cache.enter_directory("/docs");

    assert_eq!(view.path, "/docs");
    assert_eq!(view.entries.len(), 1);
    assert_eq!(view.status, DirectoryStatus::Refreshing);
    assert_eq!(view.generation.value(), 1);
}

#[test]
fn stale_refresh_result_cannot_overwrite_newer_generation() {
    let mut cache = DirectoryCache::new();
    let old = cache.begin_refresh("/docs");
    let new = cache.begin_refresh("/docs");

    let applied_old = cache.finish_refresh("/docs", old, vec![file(1, "/docs/old.txt", 10, 100)]);
    let applied_new = cache.finish_refresh("/docs", new, vec![file(2, "/docs/new.txt", 10, 100)]);

    assert!(!applied_old.applied);
    assert!(applied_new.applied);

    let view = cache.enter_directory("/docs");
    assert_eq!(view.entries[0].path, "/docs/new.txt");
}

#[test]
fn refresh_reports_added_removed_and_modified_entries() {
    let mut cache = DirectoryCache::new();
    cache.seed(
        "/docs",
        vec![
            file(1, "/docs/old.txt", 10, 100),
            file(2, "/docs/changed.txt", 10, 100),
        ],
    );

    let generation = cache.begin_refresh("/docs");
    let result = cache.finish_refresh(
        "/docs",
        generation,
        vec![
            file(2, "/docs/changed.txt", 11, 101),
            file(3, "/docs/new.txt", 10, 100),
        ],
    );

    assert_eq!(
        result.changes,
        vec![
            DirectoryChange::Added { fs_id: 3 },
            DirectoryChange::Removed { fs_id: 1 },
            DirectoryChange::Modified { fs_id: 2 },
        ]
    );
}

#[test]
fn session_state_tracks_recent_and_favorite_paths() {
    let mut session = SessionState::new();

    session.visit("/one");
    session.visit("/two");
    session.visit("/one");
    session.toggle_favorite("/one");

    assert_eq!(session.current_path(), Some("/one"));
    assert_eq!(session.recent_paths(), vec!["/one", "/two"]);
    assert_eq!(session.favorite_paths(), vec!["/one"]);
}
