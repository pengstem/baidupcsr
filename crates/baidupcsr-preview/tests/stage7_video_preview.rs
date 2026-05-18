use baidupcsr_preview::{
    parse_video_range_header, LoopbackEndpoint, MpvLaunchProfile, PlaybackProgress, PlaybackState,
    PlayerBackend, PlayerLogRing, PreviewKey, RequestedVideoRange, VideoCacheBudget,
    VideoChunkCache, VideoGatewayPolicy, VideoIntervalBuffer, VideoLoopbackServer,
    VideoPreviewError, VideoPreviewSession, VideoProxyMethod, VideoProxyRequest,
    VideoProxyResponse, VideoRange, VideoToken, VideoUpstream,
};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Condvar, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

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
fn player_log_ring_keeps_recent_sanitized_lines() {
    let mut log = PlayerLogRing::new(2);
    log.push("Opening http://127.0.0.1:1234/preview/video/tok_secret");
    log.push("A: 00:00:01 / 00:20:00");
    log.push("Error: https://d.pcs.baidu.com/file/private?token=SECRET");

    assert_eq!(log.lines().len(), 2);
    assert!(log.lines()[0].contains("A: 00:00:01"));
    assert!(log.lines()[1].contains("<redacted-url>"));
    assert!(!log.lines()[1].contains("SECRET"));
}

#[test]
fn player_log_ring_redacts_token_like_fields() {
    let mut log = PlayerLogRing::new(2);
    log.push("headers BDUSS=SECRET_BDUSS token=SECRET_TOKEN cookie=SECRET_COOKIE");

    let line = log.last_line().unwrap_or_default();
    assert!(line.contains("BDUSS=<redacted>"));
    assert!(line.contains("token=<redacted>"));
    assert!(line.contains("cookie=<redacted>"));
    assert!(!line.contains("SECRET"));
}

#[test]
fn playback_status_marks_stalled_after_no_progress_timeout() {
    let status = PlaybackProgress::new(PlaybackState::Playing)
        .with_last_activity_ms(1_000)
        .status_at(7_000, 5_000);

    assert_eq!(status, PlaybackState::Stalled);
}

#[test]
fn interval_buffer_serves_overlapping_subranges() -> Result<(), VideoPreviewError> {
    let budget = VideoCacheBudget::new(16, 2)?;
    let mut buffer = VideoIntervalBuffer::new(budget);
    buffer.put(VideoRange::new(0, 15)?, b"abcdefghijklmnop".to_vec())?;

    assert_eq!(buffer.get(VideoRange::new(4, 7)?), Some(b"efgh".to_vec()));
    assert_eq!(buffer.get(VideoRange::new(12, 20)?), None);
    Ok(())
}

#[test]
fn interval_buffer_evicts_until_within_budget() -> Result<(), VideoPreviewError> {
    let budget = VideoCacheBudget::new(8, 2)?;
    let mut buffer = VideoIntervalBuffer::new(budget);
    buffer.put(VideoRange::new(0, 7)?, b"aaaaaaaa".to_vec())?;
    buffer.put(VideoRange::new(8, 15)?, b"bbbbbbbb".to_vec())?;
    buffer.put(VideoRange::new(16, 23)?, b"cccccccc".to_vec())?;

    assert_eq!(buffer.used_bytes(), 16);
    assert!(buffer.get(VideoRange::new(0, 7)?).is_none());
    assert_eq!(
        buffer.get(VideoRange::new(16, 23)?),
        Some(b"cccccccc".to_vec())
    );
    Ok(())
}

#[test]
fn range_parser_accepts_open_ended_and_suffix_ranges() -> Result<(), VideoPreviewError> {
    assert_eq!(
        parse_video_range_header("Range: bytes=4096-").transpose()?,
        Some(RequestedVideoRange::Explicit(VideoRange::new(
            4096,
            u64::MAX
        )?))
    );
    assert_eq!(
        parse_video_range_header("Range: bytes=-8192").transpose()?,
        Some(RequestedVideoRange::Suffix(8192))
    );
    Ok(())
}

#[test]
fn suffix_proxy_range_resolves_against_file_tail() -> Result<(), VideoPreviewError> {
    let session = session()?;
    let response = session.handle_proxy_request(VideoProxyRequest {
        method: VideoProxyMethod::Get,
        token: "tok_0123456789abcdef".to_string(),
        range: Some(RequestedVideoRange::Suffix(128)),
    })?;

    assert_eq!(
        response,
        VideoProxyResponse::Fetch(VideoRange::new(872, 999)?)
    );
    Ok(())
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
    assert!(!args.contains("--no-config"));
    assert!(!args.contains("--load-scripts=no"));
    assert!(!args.contains("--osc=no"));
    assert!(args.contains("--demuxer-readahead-secs=20"));
    assert!(args.contains("--cache-pause=no"));
    assert!(args.contains("--no-terminal"));
    assert!(args.contains("--force-window=immediate"));
    assert!(args.contains("--msg-level=all=warn"));
    assert!(!args.contains("--really-quiet"));
    assert!(!args.contains("--vo=null"));
    assert!(!args.contains("--ao=null"));
    assert!(!args.contains("--mute=yes"));
    assert!(!args.contains("--pause"));
    assert!(args.contains("--idle=yes"));
    assert_eq!(launch.max_memory_cache_bytes(), 512);
    assert!(launch.disk_cache_disabled());
    Ok(())
}

#[test]
fn isolated_mpv_profile_disables_user_config_scripts_and_osc() -> Result<(), VideoPreviewError> {
    let session = session()?;

    let launch =
        session.player_launch_with_profile(PlayerBackend::Mpv, MpvLaunchProfile::Isolated)?;
    let args = launch.args().join(" ");
    assert!(args.contains("127.0.0.1:39177"));
    assert!(!args.contains("baidu"));
    assert!(args.contains("--cache-on-disk=no"));
    assert!(args.contains("--no-config"));
    assert!(args.contains("--load-scripts=no"));
    assert!(args.contains("--osc=no"));
    assert!(args.contains("--no-terminal"));
    assert!(args.contains("--force-window=immediate"));
    assert!(args.contains("--msg-level=all=warn"));
    assert!(!args.contains("--really-quiet"));
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
        range: Some(VideoRange::new(10, 20)?.into()),
    });
    assert_eq!(rejected, Err(VideoPreviewError::InvalidVideoToken));

    let range = VideoRange::new(10, 20)?;
    let accepted = session.handle_proxy_request(VideoProxyRequest {
        method: VideoProxyMethod::Get,
        token: "tok_0123456789abcdef".to_string(),
        range: Some(range.into()),
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

#[test]
fn loopback_server_handles_helper_request_while_fetch_is_blocked() -> Result<(), String> {
    let fetches = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let fetches_for_server = Arc::clone(&fetches);
    let gate_for_server = Arc::clone(&gate);
    let token = VideoToken::new("tok_loopback_server_0006").map_err(debug)?;
    let server = VideoLoopbackServer::spawn(
        key(),
        VideoUpstream::new("https://d.pcs.baidu.com/file/private-video?secret=1").map_err(debug)?,
        token.clone(),
        VideoCacheBudget::new(64, 2).map_err(debug)?,
        move |range| {
            if fetches_for_server.fetch_add(1, Ordering::AcqRel) == 0 {
                let (lock, cvar) = &*gate_for_server;
                let mut released = lock
                    .lock()
                    .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
                while !*released {
                    released = cvar
                        .wait(released)
                        .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
                }
            }
            bytes_for_range(range)
        },
    )
    .map_err(debug)?;

    let first_request = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-63\r\n\r\n",
        token.as_str()
    );
    let head_request = format!(
        "HEAD /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        token.as_str()
    );
    let port = server.endpoint().port();
    let first = thread::spawn(move || raw_http(port, &first_request));

    wait_for_fetches(&fetches, 1)?;
    let head = raw_http_with_timeout(port, &head_request, Duration::from_millis(200));

    {
        let (lock, cvar) = &*gate;
        let mut released = lock.lock().map_err(|err| err.to_string())?;
        *released = true;
        cvar.notify_all();
    }

    let first = first
        .join()
        .map_err(|_| "first request thread panicked".to_string())??;
    assert!(first.starts_with("HTTP/1.1 206 Partial Content"));
    assert!(head?.starts_with("HTTP/1.1 200 OK"));
    Ok(())
}

#[test]
fn loopback_server_prefetches_forward_window_without_exceeding_budget() -> Result<(), String> {
    let fetches = Arc::new(AtomicUsize::new(0));
    let fetches_for_server = Arc::clone(&fetches);
    let token = VideoToken::new("tok_loopback_server_0004").map_err(debug)?;
    let server = VideoLoopbackServer::spawn_with_policy(
        key(),
        VideoUpstream::new("https://d.pcs.baidu.com/file/private-video?secret=1").map_err(debug)?,
        token.clone(),
        VideoCacheBudget::new(64, 4).map_err(debug)?,
        VideoGatewayPolicy { prefetch_chunks: 2 },
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
    let response = raw_http(server.endpoint().port(), &request)?;

    assert!(response.starts_with("HTTP/1.1 206 Partial Content"));
    wait_for_fetches(&fetches, 3)?;
    assert!(server.stats().used_bytes <= 256);
    assert!(server.stats().prefetch_fetches >= 2);
    Ok(())
}

#[test]
fn loopback_server_serves_prefetched_subrange_from_interval_buffer() -> Result<(), String> {
    let fetches = Arc::new(AtomicUsize::new(0));
    let fetches_for_server = Arc::clone(&fetches);
    let token = VideoToken::new("tok_loopback_server_0005").map_err(debug)?;
    let server = VideoLoopbackServer::spawn_with_policy(
        key(),
        VideoUpstream::new("https://d.pcs.baidu.com/file/private-video?secret=1").map_err(debug)?,
        token.clone(),
        VideoCacheBudget::new(64, 3).map_err(debug)?,
        VideoGatewayPolicy { prefetch_chunks: 1 },
        move |range| {
            fetches_for_server.fetch_add(1, Ordering::AcqRel);
            bytes_for_range(range)
        },
    )
    .map_err(debug)?;

    let first = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-63\r\n\r\n",
        token.as_str()
    );
    let second = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=80-95\r\n\r\n",
        token.as_str()
    );

    let _ = raw_http(server.endpoint().port(), &first)?;
    wait_for_fetches(&fetches, 2)?;
    let before = fetches.load(Ordering::Acquire);
    let response = raw_http(server.endpoint().port(), &second)?;

    assert!(response.starts_with("HTTP/1.1 206 Partial Content"));
    assert_eq!(body(&response).len(), 16);
    assert_eq!(fetches.load(Ordering::Acquire), before);
    assert!(server.stats().cache_hits >= 1);
    Ok(())
}

#[test]
fn loopback_server_does_not_prefetch_far_seek_thumbnail_requests() -> Result<(), String> {
    let fetches = Arc::new(AtomicUsize::new(0));
    let fetches_for_server = Arc::clone(&fetches);
    let token = VideoToken::new("tok_loopback_server_0007").map_err(debug)?;
    let server = VideoLoopbackServer::spawn_with_policy(
        key(),
        VideoUpstream::new("https://d.pcs.baidu.com/file/private-video?secret=1").map_err(debug)?,
        token.clone(),
        VideoCacheBudget::new(64, 4).map_err(debug)?,
        VideoGatewayPolicy { prefetch_chunks: 2 },
        move |range| {
            fetches_for_server.fetch_add(1, Ordering::AcqRel);
            bytes_for_range(range)
        },
    )
    .map_err(debug)?;

    let first = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-63\r\n\r\n",
        token.as_str()
    );
    let far_seek = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=512-575\r\n\r\n",
        token.as_str()
    );

    let _ = raw_http(server.endpoint().port(), &first)?;
    wait_for_fetches(&fetches, 3)?;
    let before = fetches.load(Ordering::Acquire);
    let response = raw_http(server.endpoint().port(), &far_seek)?;
    thread::sleep(Duration::from_millis(100));

    assert!(response.starts_with("HTTP/1.1 206 Partial Content"));
    assert_eq!(fetches.load(Ordering::Acquire), before + 1);
    Ok(())
}

fn bytes_for_range(range: VideoRange) -> Result<Vec<u8>, VideoPreviewError> {
    let len = range
        .end_inclusive
        .saturating_sub(range.start)
        .saturating_add(1);
    Ok(vec![b'v'; usize::try_from(len).unwrap_or(usize::MAX)])
}

fn wait_for_fetches(fetches: &AtomicUsize, expected: usize) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while fetches.load(Ordering::Acquire) < expected {
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for {expected} fetches, saw {}",
                fetches.load(Ordering::Acquire)
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn raw_http(port: u16, request: &str) -> Result<String, String> {
    raw_http_with_timeout(port, request, Duration::from_secs(5))
}

fn raw_http_with_timeout(port: u16, request: &str, timeout: Duration) -> Result<String, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|err| err.to_string())?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| err.to_string())?;
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
