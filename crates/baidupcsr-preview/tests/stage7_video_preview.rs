use baidupcsr_preview::{
    LoopbackEndpoint, PlayerBackend, PreviewKey, VideoCacheBudget, VideoChunkCache,
    VideoPreviewError, VideoPreviewSession, VideoProxyMethod, VideoProxyRequest,
    VideoProxyResponse, VideoRange, VideoToken, VideoUpstream,
};

fn key() -> PreviewKey {
    PreviewKey {
        account_uid: 100,
        fs_id: 42,
        mtime: 7,
        size: 1_000,
    }
}

fn session() -> Result<VideoPreviewSession, VideoPreviewError> {
    VideoPreviewSession::new(
        key(),
        VideoUpstream::new("https://d.pcs.baidu.com/file/private-video?token=secret")?,
        VideoToken::new("tok_0123456789abcdef")?,
        LoopbackEndpoint::new(39_177)?,
        VideoCacheBudget::new(256, 2)?,
    )
}

#[test]
fn video_session_exposes_only_loopback_token_url_to_player() -> Result<(), VideoPreviewError> {
    let session = session()?;

    assert_eq!(
        session.player_url(),
        "http://127.0.0.1:39177/preview/video/tok_0123456789abcdef"
    );
    assert!(!session.player_url().contains("baidu"));
    assert!(!format!("{:?}", session.upstream()).contains("private-video"));

    let launch = session.player_launch(PlayerBackend::Mpv)?;
    let args = launch.args().join(" ");
    assert!(args.contains("127.0.0.1:39177"));
    assert!(!args.contains("baidu"));
    assert_eq!(launch.max_memory_cache_bytes(), 512);
    assert!(launch.disk_cache_disabled());
    Ok(())
}

#[test]
fn loopback_endpoint_rejects_non_loopback_hosts() {
    assert_eq!(
        LoopbackEndpoint::from_host_port("0.0.0.0", 39_177),
        Err(VideoPreviewError::NonLoopbackBindAddress)
    );
    assert_eq!(
        LoopbackEndpoint::from_host_port("192.168.1.12", 39_177),
        Err(VideoPreviewError::NonLoopbackBindAddress)
    );
    assert!(LoopbackEndpoint::from_host_port("127.0.0.1", 39_177).is_ok());
}

#[test]
fn proxy_request_requires_matching_token_and_active_session() -> Result<(), VideoPreviewError> {
    let mut session = session()?;

    let rejected = session.handle_proxy_request(VideoProxyRequest {
        method: VideoProxyMethod::Get,
        token: "wrong".to_string(),
        range: Some(VideoRange::new(10, 20)?),
    });
    assert_eq!(rejected, Err(VideoPreviewError::InvalidVideoToken));

    let range = VideoRange::new(10, 20)?;
    let accepted = session.handle_proxy_request(VideoProxyRequest {
        method: VideoProxyMethod::Get,
        token: "tok_0123456789abcdef".to_string(),
        range: Some(range),
    })?;
    assert_eq!(accepted, VideoProxyResponse::Fetch(range));

    session.invalidate();
    let stale = session.handle_proxy_request(VideoProxyRequest {
        method: VideoProxyMethod::Head,
        token: "tok_0123456789abcdef".to_string(),
        range: None,
    });
    assert_eq!(stale, Err(VideoPreviewError::InactiveVideoSession));
    Ok(())
}

#[test]
fn proxy_head_response_reports_range_capability_without_fetching_body(
) -> Result<(), VideoPreviewError> {
    let session = session()?;

    let response = session.handle_proxy_request(VideoProxyRequest {
        method: VideoProxyMethod::Head,
        token: "tok_0123456789abcdef".to_string(),
        range: None,
    })?;

    assert_eq!(
        response,
        VideoProxyResponse::Head {
            content_length: 1_000,
            accept_ranges: true,
        }
    );
    Ok(())
}

#[test]
fn video_chunk_cache_enforces_budget_and_seek_window() -> Result<(), VideoPreviewError> {
    let budget = VideoCacheBudget::new(4, 2)?;
    let mut cache = VideoChunkCache::new(budget);

    cache.put(0, b"aaaa".to_vec())?;
    cache.put(4, b"bbbb".to_vec())?;
    cache.put(8, b"cccc".to_vec())?;

    assert!(cache.get(0).is_none());
    assert_eq!(cache.get(4), Some(&b"bbbb"[..]));
    assert_eq!(cache.get(8), Some(&b"cccc"[..]));
    assert_eq!(cache.used_bytes(), 8);

    cache.retain_window(8, 4);
    assert!(cache.get(4).is_none());
    assert_eq!(cache.get(8), Some(&b"cccc"[..]));
    assert_eq!(cache.used_bytes(), 4);
    Ok(())
}

#[test]
fn chunk_cache_rejects_chunks_larger_than_budget_chunk_size() -> Result<(), VideoPreviewError> {
    let budget = VideoCacheBudget::new(4, 2)?;
    let mut cache = VideoChunkCache::new(budget);

    assert_eq!(
        cache.put(0, b"too large".to_vec()),
        Err(VideoPreviewError::VideoChunkTooLarge)
    );
    Ok(())
}
