use baidupcsr_core::PagedPreloadPlanner;
use baidupcsr_pcs::FileEntry;

fn entry(fs_id: i64, path: &str, is_dir: bool) -> FileEntry {
    FileEntry {
        fs_id,
        app_id: 266719,
        path: path.to_string(),
        server_filename: path.rsplit('/').next().unwrap_or(path).to_string(),
        ctime: 1,
        mtime: 1,
        md5: String::new(),
        block_list: Vec::new(),
        size: if is_dir { 0 } else { 4096 },
        is_dir,
        has_subdir: is_dir,
    }
}

#[test]
fn paged_preload_records_bounded_work_for_large_directories() {
    let entries: Vec<FileEntry> = (0..100_000)
        .map(|index| entry(index, &format!("/huge/item-{index:06}.txt"), false))
        .collect();

    let plan = PagedPreloadPlanner::new(24, 1, 8, 8).plan(&entries, 50_000);

    assert!(plan.targets.len() <= 16);
    assert!(plan.window_end.saturating_sub(plan.window_start) <= 72);
    assert!(plan.window_start > 49_900);
    assert!(plan.window_end < 50_100);
}
