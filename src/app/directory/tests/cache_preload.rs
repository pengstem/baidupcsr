use std::fs;

use super::super::*;
use crate::pcs::FileEntry;

fn entry(fs_id: i64, path: &str, is_dir: bool) -> FileEntry {
    FileEntry {
        fs_id,
        app_id: 266719,
        path: path.to_string(),
        server_filename: path
            .rsplit('/')
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or(path)
            .to_string(),
        ctime: 1,
        mtime: 10,
        md5: String::new(),
        block_list: Vec::new(),
        size: if is_dir { 0 } else { 4096 },
        is_dir,
        has_subdir: is_dir,
    }
}

#[test]
fn directory_cache_store_round_trips_entries_in_cache_dir() -> Result<(), Box<dyn std::error::Error>>
{
    let root =
        std::env::temp_dir().join(format!("baidupcsr-stage9-5-cache-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }

    let store = DirectoryCacheStore::new(&root);
    store.write_snapshot(
        42,
        "/remote/docs",
        vec![entry(1, "/remote/docs/readme.md", false)],
        1234,
    )?;

    let snapshot = store
        .read_snapshot(42, "/remote/docs")?
        .ok_or("missing cache snapshot")?;

    assert_eq!(snapshot.saved_at_unix_seconds, 1234);
    assert_eq!(snapshot.entries[0].server_filename, "readme.md");
    assert!(root.read_dir()?.next().is_some());

    fs::remove_dir_all(&root)?;
    Ok(())
}

#[test]
fn directory_cache_store_uses_stable_fnv_snapshot_filename() {
    let store = DirectoryCacheStore::new("/tmp/baidupcsr-cache-contract");

    assert_eq!(stable_path_hash("/remote/docs"), 0x4d21_26de_0d6d_a6d6);
    assert_eq!(
        store.snapshot_path(42, "/remote/docs"),
        std::path::PathBuf::from("/tmp/baidupcsr-cache-contract/dir-42-4d2126de0d6da6d6.json")
    );
}

#[test]
fn paged_preload_planner_limits_work_to_visible_page_neighbors() {
    let entries: Vec<FileEntry> = (0..40)
        .map(|index| entry(index, &format!("/root/dir-{index:02}"), true))
        .collect();

    let plan = PagedPreloadPlanner::new(10, 1, 6, 4).plan(&entries, 22);

    assert_eq!(plan.targets.len(), 6);
    assert!(plan.targets.contains(&PreloadTarget {
        path: "/root/dir-20".to_string(),
        kind: PreloadKind::Directory,
    }));
    assert!(plan.targets.contains(&PreloadTarget {
        path: "/root/dir-25".to_string(),
        kind: PreloadKind::Directory,
    }));
    assert!(!plan
        .targets
        .iter()
        .any(|target| target.path == "/root/dir-00"));
    assert!(!plan
        .targets
        .iter()
        .any(|target| target.path == "/root/dir-39"));
}
