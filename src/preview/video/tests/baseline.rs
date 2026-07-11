use super::super::*;

fn key() -> PreviewKey {
    PreviewKey {
        account_uid: 100,
        fs_id: 42,
        mtime: 7,
        size: 1_000,
    }
}

fn session() -> Result<VideoPreviewSession, VideoPreviewError> {
    Ok(VideoPreviewSession::new(
        key(),
        VideoToken::new("tok_0123456789abcdef")?,
        39_177,
        VideoCacheBudget::new(256, 2)?,
    ))
}

#[test]
fn video_session_exposes_only_loopback_token_url_to_player() -> Result<(), VideoPreviewError> {
    let session = session()?;

    assert_eq!(
        session.player_url(),
        "http://127.0.0.1:39177/preview/video/tok_0123456789abcdef"
    );
    assert!(!session.player_url().contains("baidu"));

    let args = session
        .mpv_args_with_profile(MpvLaunchProfile::User)
        .join(" ");
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
    assert!(args.contains("--demuxer-max-bytes=512"));
    Ok(())
}

#[test]
fn isolated_mpv_profile_disables_user_config_scripts_and_osc() -> Result<(), VideoPreviewError> {
    let session = session()?;

    let args = session
        .mpv_args_with_profile(MpvLaunchProfile::Isolated)
        .join(" ");
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
    assert!(args.contains("--demuxer-max-bytes=512"));
    Ok(())
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

    assert_eq!(buffer.used_bytes, 16);
    assert!(buffer.get(VideoRange::new(0, 7)?).is_none());
    assert_eq!(
        buffer.get(VideoRange::new(16, 23)?),
        Some(b"cccccccc".to_vec())
    );
    Ok(())
}

#[test]
fn open_ended_proxy_range_uses_bounded_playback_chunk() -> Result<(), VideoPreviewError> {
    let session = session()?;
    let response = session.handle_proxy_request(&VideoProxyRequest {
        method: VideoProxyMethod::Get,
        token: "tok_0123456789abcdef".to_string(),
        range: Some(RequestedVideoRange::OpenEnded(100)),
    })?;

    assert_eq!(
        response,
        VideoProxyResponse::Fetch {
            range: VideoRange::new(100, 355)?,
            force_prefetch: true,
        }
    );
    Ok(())
}

#[test]
fn explicit_proxy_range_can_use_full_cache_budget() -> Result<(), VideoPreviewError> {
    let session = session()?;
    let response = session.handle_proxy_request(&VideoProxyRequest {
        method: VideoProxyMethod::Get,
        token: "tok_0123456789abcdef".to_string(),
        range: Some(VideoRange::new(100, 900)?.into()),
    })?;

    assert_eq!(
        response,
        VideoProxyResponse::Fetch {
            range: VideoRange::new(100, 611)?,
            force_prefetch: false,
        }
    );
    Ok(())
}

#[test]
fn suffix_proxy_range_resolves_against_file_tail() -> Result<(), VideoPreviewError> {
    let session = session()?;
    let response = session.handle_proxy_request(&VideoProxyRequest {
        method: VideoProxyMethod::Get,
        token: "tok_0123456789abcdef".to_string(),
        range: Some(RequestedVideoRange::Suffix(128)),
    })?;

    assert_eq!(
        response,
        VideoProxyResponse::Fetch {
            range: VideoRange::new(872, 999)?,
            force_prefetch: false,
        }
    );
    Ok(())
}

#[test]
fn range_parser_accepts_open_ended_and_suffix_ranges() -> Result<(), VideoPreviewError> {
    assert_eq!(
        parse_video_range_header("Range: bytes=4096-").transpose()?,
        Some(RequestedVideoRange::OpenEnded(4096))
    );
    assert_eq!(
        parse_video_range_header("Range: bytes=-8192").transpose()?,
        Some(RequestedVideoRange::Suffix(8192))
    );
    Ok(())
}

#[test]
fn proxy_request_requires_matching_token() -> Result<(), VideoPreviewError> {
    let session = session()?;

    let rejected = session.handle_proxy_request(&VideoProxyRequest {
        method: VideoProxyMethod::Get,
        token: "wrong".to_string(),
        range: Some(VideoRange::new(10, 20)?.into()),
    });
    assert_eq!(rejected, Err(VideoPreviewError::InvalidVideoToken));

    let range = VideoRange::new(10, 20)?;
    let accepted = session.handle_proxy_request(&VideoProxyRequest {
        method: VideoProxyMethod::Get,
        token: "tok_0123456789abcdef".to_string(),
        range: Some(range.into()),
    })?;
    assert_eq!(
        accepted,
        VideoProxyResponse::Fetch {
            range,
            force_prefetch: false,
        }
    );
    Ok(())
}

#[test]
fn proxy_head_response_reports_range_capability_without_fetching_body(
) -> Result<(), VideoPreviewError> {
    let session = session()?;

    let response = session.handle_proxy_request(&VideoProxyRequest {
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
