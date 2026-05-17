use baidupcsr_preview::{
    PreviewKey, PreviewKind, PreviewManager, PreviewStatus, PreviewText, RequestState,
};

fn key(fs_id: i64, size: u64, mtime: i64) -> PreviewKey {
    PreviewKey {
        account_uid: 100,
        fs_id,
        mtime,
        size,
    }
}

#[test]
fn text_preview_is_memory_only_and_truncated_to_limit() {
    let mut manager = PreviewManager::new(8);
    let request = manager.start_request(key(1, 11, 1));
    let update = manager.finish_text_request(request, b"hello world");

    assert_eq!(
        update.status,
        PreviewStatus {
            kind: PreviewKind::Text,
            fetched_bytes: 8,
            memory_limit_bytes: 8,
            truncated: true,
            disk_backed: false,
        }
    );

    assert_eq!(
        manager.get(&key(1, 11, 1)),
        Some(&PreviewText {
            content: "hello wo".to_string(),
            status: update.status,
        })
    );
}

#[test]
fn cache_key_uses_account_file_mtime_and_size() {
    assert_ne!(key(1, 11, 1), key(1, 12, 1));
    assert_ne!(key(1, 11, 1), key(1, 11, 2));
}

#[test]
fn lru_evicts_oldest_preview_when_budget_is_exceeded() {
    let mut manager = PreviewManager::new(10);

    let first = manager.start_request(key(1, 5, 1));
    manager.finish_text_request(first, b"12345");
    let second = manager.start_request(key(2, 6, 1));
    manager.finish_text_request(second, b"abcdef");

    assert!(manager.get(&key(1, 5, 1)).is_none());
    assert!(manager.get(&key(2, 6, 1)).is_some());
    assert!(manager.used_bytes() <= 10);
}

#[test]
fn stale_preview_request_cannot_overwrite_newer_request() {
    let mut manager = PreviewManager::new(1024);
    let old = manager.start_request(key(1, 3, 1));
    let new = manager.start_request(key(1, 3, 1));

    let old_update = manager.finish_text_request(old, b"old");
    let new_update = manager.finish_text_request(new, b"new");

    assert_eq!(old_update.request_state, RequestState::Stale);
    assert_eq!(new_update.request_state, RequestState::Applied);
    assert_eq!(
        manager.get(&key(1, 3, 1)).map(|item| item.content.as_str()),
        Some("new")
    );
}
