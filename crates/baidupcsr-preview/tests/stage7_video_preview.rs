use baidupcsr_preview::{
    LoopbackEndpoint, PlayerBackend, PreviewKey, VideoCacheBudget, VideoChunkCache,
    VideoLoopbackServer, VideoPreviewError, VideoPreviewSession, VideoProxyMethod,
    VideoProxyRequest, VideoProxyResponse, VideoRange, VideoToken, VideoUpstream,
};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
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
    assert!(args.contains("--cache-on-disk=no"));
    assert!(!args.contains("--disk-cache=no"));
    assert!(args.contains("--no-config"));
    assert!(args.contains("--load-scripts=no"));
    assert!(args.contains("--osc=no"));
    assert!(args.contains("--no-terminal"));
    assert!(args.contains("--force-window=yes"));
    assert!(!args.contains("--vo=null"));
    assert!(!args.contains("--pause"));
    assert!(args.contains("--idle=yes"));
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

#[test]
fn loopback_server_serves_head_and_range_without_exposing_upstream() -> Result<(), String> {
    let fetches = Arc::new(AtomicUsize::new(0));
    let fetches_for_server = Arc::clone(&fetches);
    let token = VideoToken::new("tok_loopback_server_0001").map_err(debug)?;
    let server = VideoLoopbackServer::spawn(
        key(),
        VideoUpstream::new("https://d.pcs.baidu.com/file/private-video?secret=1").map_err(debug)?,
        token.clone(),
        VideoCacheBudget::new(64, 2).map_err(debug)?,
        move |range| {
            fetches_for_server.fetch_add(1, Ordering::AcqRel);
            bytes_for_range(range)
        },
    )
    .map_err(debug)?;

    assert!(server.player_url().contains("127.0.0.1"));
    assert!(!server.player_url().contains("baidu"));

    let head = raw_http(
        server.endpoint().port(),
        &format!(
            "HEAD /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            token.as_str()
        ),
    )?;
    assert!(head.starts_with("HTTP/1.1 200 OK"));
    assert!(head.contains("Accept-Ranges: bytes"));
    assert!(head.contains("Content-Length: 1000"));
    assert!(body(&head).is_empty());
    assert_eq!(fetches.load(Ordering::Acquire), 0);

    let get = raw_http(
        server.endpoint().port(),
        &format!(
            "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-63\r\n\r\n",
            token.as_str()
        ),
    )?;
    assert!(get.starts_with("HTTP/1.1 206 Partial Content"));
    assert!(get.contains("Content-Range: bytes 0-63/1000"));
    assert_eq!(body(&get).len(), 64);
    assert_eq!(fetches.load(Ordering::Acquire), 1);
    assert_eq!(server.stats().used_bytes, 64);
    assert!(server.stats().used_bytes <= 128);
    Ok(())
}

#[test]
fn loopback_server_rejects_wrong_token_without_fetching_upstream() -> Result<(), String> {
    let fetches = Arc::new(AtomicUsize::new(0));
    let fetches_for_server = Arc::clone(&fetches);
    let server = VideoLoopbackServer::spawn(
        key(),
        VideoUpstream::new("https://d.pcs.baidu.com/file/private-video?secret=1").map_err(debug)?,
        VideoToken::new("tok_loopback_server_0002").map_err(debug)?,
        VideoCacheBudget::new(64, 2).map_err(debug)?,
        move |range| {
            fetches_for_server.fetch_add(1, Ordering::AcqRel);
            bytes_for_range(range)
        },
    )
    .map_err(debug)?;

    let response = raw_http(
        server.endpoint().port(),
        "GET /preview/video/wrong HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-63\r\n\r\n",
    )?;

    assert!(response.starts_with("HTTP/1.1 403 Forbidden"));
    assert_eq!(fetches.load(Ordering::Acquire), 0);
    assert_eq!(server.stats().used_bytes, 0);
    Ok(())
}

#[test]
fn loopback_server_reuses_chunk_cache_and_reports_stats() -> Result<(), String> {
    let fetches = Arc::new(AtomicUsize::new(0));
    let fetches_for_server = Arc::clone(&fetches);
    let token = VideoToken::new("tok_loopback_server_0003").map_err(debug)?;
    let server = VideoLoopbackServer::spawn(
        key(),
        VideoUpstream::new("https://d.pcs.baidu.com/file/private-video?secret=1").map_err(debug)?,
        token.clone(),
        VideoCacheBudget::new(64, 2).map_err(debug)?,
        move |range| {
            fetches_for_server.fetch_add(1, Ordering::AcqRel);
            bytes_for_range(range)
        },
    )
    .map_err(debug)?;
    let request = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-63\r\n\r\n",
        token.as_str()
    );

    let first = raw_http(server.endpoint().port(), &request)?;
    let second = raw_http(server.endpoint().port(), &request)?;

    assert_eq!(body(&first), body(&second));
    assert_eq!(fetches.load(Ordering::Acquire), 1);
    assert_eq!(server.stats().upstream_fetches, 1);
    assert_eq!(server.stats().cache_hits, 1);
    assert!(server.stats().used_bytes <= 128);
    assert_eq!(server.stats().last_range_start, Some(0));
    assert_eq!(server.stats().last_range_end_inclusive, Some(63));
    assert_eq!(server.stats().window_start, Some(0));
    assert_eq!(server.stats().window_end_inclusive, Some(63));
    assert_eq!(server.stats().last_error, None);
    Ok(())
}

fn bytes_for_range(range: VideoRange) -> Result<Vec<u8>, VideoPreviewError> {
    let len = range
        .end_inclusive
        .saturating_sub(range.start)
        .saturating_add(1);
    Ok(vec![b'v'; usize::try_from(len).unwrap_or(usize::MAX)])
}

fn raw_http(port: u16, request: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|err| err.to_string())?;
    stream
        .write_all(request.as_bytes())
        .map_err(|err| err.to_string())?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|err| err.to_string())?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|err| err.to_string())?;
    Ok(response)
}

fn body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("")
}

fn debug(err: VideoPreviewError) -> String {
    format!("{err:?}")
}
