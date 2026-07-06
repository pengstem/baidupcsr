use baidupcsr_preview::{
    PreviewKey, VideoCacheBudget, VideoLoopbackServer, VideoPreviewError, VideoRange, VideoToken,
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

#[test]
fn loopback_server_serves_head_and_range_through_loopback_port() -> Result<(), String> {
    let fetches = Arc::new(AtomicUsize::new(0));
    let fetches_for_server = Arc::clone(&fetches);
    let token = "tok_loopback_server_0001";
    let server = VideoLoopbackServer::spawn_with_prefetch(
        key(),
        VideoToken::new(token).map_err(debug)?,
        VideoCacheBudget::new(64, 2).map_err(debug)?,
        0,
        move |range| {
            fetches_for_server.fetch_add(1, Ordering::AcqRel);
            Ok(bytes_for_range(range))
        },
    )
    .map_err(debug)?;

    assert!(server.port() > 0);

    let head = raw_http(
        server.port(),
        &format!(
            "HEAD /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            token
        ),
    )?;
    assert!(head.starts_with("HTTP/1.1 200 OK"));
    assert!(head.contains("Accept-Ranges: bytes"));
    assert!(head.contains("Content-Length: 1000"));
    assert!(body(&head).is_empty());
    assert_eq!(fetches.load(Ordering::Acquire), 0);

    let get = raw_http(
        server.port(),
        &format!(
            "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-63\r\n\r\n",
            token
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
fn loopback_server_serves_open_ended_far_seek_as_bounded_chunk() -> Result<(), String> {
    let fetches = Arc::new(Mutex::new(Vec::new()));
    let fetches_for_server = Arc::clone(&fetches);
    let token = "tok_loopback_server_0008";
    let server = VideoLoopbackServer::spawn_with_prefetch(
        key(),
        VideoToken::new(token).map_err(debug)?,
        VideoCacheBudget::new(64, 4).map_err(debug)?,
        0,
        move |range| {
            fetches_for_server
                .lock()
                .map_err(|_| VideoPreviewError::LoopbackServerIo)?
                .push(range);
            Ok(bytes_for_range(range))
        },
    )
    .map_err(debug)?;

    let request = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=512-\r\n\r\n",
        token
    );
    let response = raw_http(server.port(), &request)?;

    assert!(response.starts_with("HTTP/1.1 206 Partial Content"));
    assert!(response.contains("Content-Range: bytes 512-575/1000"));
    assert_eq!(body(&response).len(), 64);
    assert_eq!(
        fetches.lock().map_err(|err| err.to_string())?.as_slice(),
        &[VideoRange::new(512, 575).map_err(debug)?]
    );
    Ok(())
}

#[test]
fn loopback_server_prefetches_after_open_ended_far_seek() -> Result<(), String> {
    let fetches = Arc::new(AtomicUsize::new(0));
    let fetches_for_server = Arc::clone(&fetches);
    let token = "tok_loopback_server_0009";
    let server = VideoLoopbackServer::spawn_with_prefetch(
        key(),
        VideoToken::new(token).map_err(debug)?,
        VideoCacheBudget::new(64, 4).map_err(debug)?,
        2,
        move |range| {
            fetches_for_server.fetch_add(1, Ordering::AcqRel);
            Ok(bytes_for_range(range))
        },
    )
    .map_err(debug)?;

    let request = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=512-\r\n\r\n",
        token
    );
    let response = raw_http(server.port(), &request)?;

    assert!(response.starts_with("HTTP/1.1 206 Partial Content"));
    assert_eq!(body(&response).len(), 64);
    wait_for_fetches(&fetches, 3)?;
    Ok(())
}

#[test]
fn loopback_server_prefetches_after_open_ended_far_seek_with_existing_window() -> Result<(), String>
{
    let fetches = Arc::new(AtomicUsize::new(0));
    let fetches_for_server = Arc::clone(&fetches);
    let token = "tok_loopback_server_0011";
    let server = VideoLoopbackServer::spawn_with_prefetch(
        key(),
        VideoToken::new(token).map_err(debug)?,
        VideoCacheBudget::new(64, 6).map_err(debug)?,
        2,
        move |range| {
            fetches_for_server.fetch_add(1, Ordering::AcqRel);
            Ok(bytes_for_range(range))
        },
    )
    .map_err(debug)?;

    let first = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-63\r\n\r\n",
        token
    );
    let far_seek = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=512-\r\n\r\n",
        token
    );

    let first_response = raw_http(server.port(), &first)?;
    assert!(first_response.starts_with("HTTP/1.1 206 Partial Content"));
    wait_for_fetches(&fetches, 3)?;

    let far_response = raw_http(server.port(), &far_seek)?;
    assert!(far_response.starts_with("HTTP/1.1 206 Partial Content"));
    assert_eq!(body(&far_response).len(), 64);
    wait_for_fetches(&fetches, 6)?;
    Ok(())
}

#[test]
fn loopback_server_rejects_wrong_token_without_fetching_upstream() -> Result<(), String> {
    let fetches = Arc::new(AtomicUsize::new(0));
    let fetches_for_server = Arc::clone(&fetches);
    let server = VideoLoopbackServer::spawn_with_prefetch(
        key(),
        VideoToken::new("tok_loopback_server_0002").map_err(debug)?,
        VideoCacheBudget::new(64, 2).map_err(debug)?,
        0,
        move |range| {
            fetches_for_server.fetch_add(1, Ordering::AcqRel);
            Ok(bytes_for_range(range))
        },
    )
    .map_err(debug)?;

    let response = raw_http(
        server.port(),
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
    let token = "tok_loopback_server_0003";
    let server = VideoLoopbackServer::spawn_with_prefetch(
        key(),
        VideoToken::new(token).map_err(debug)?,
        VideoCacheBudget::new(64, 2).map_err(debug)?,
        0,
        move |range| {
            fetches_for_server.fetch_add(1, Ordering::AcqRel);
            Ok(bytes_for_range(range))
        },
    )
    .map_err(debug)?;
    let request = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-63\r\n\r\n",
        token
    );

    let first = raw_http(server.port(), &request)?;
    let second = raw_http(server.port(), &request)?;

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
fn loopback_server_waits_for_matching_inflight_prefetch() -> Result<(), String> {
    let ranges = Arc::new(Mutex::new(Vec::new()));
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let ranges_for_server = Arc::clone(&ranges);
    let gate_for_server = Arc::clone(&gate);
    let token = "tok_loopback_server_0010";
    let server = VideoLoopbackServer::spawn_with_prefetch(
        key(),
        VideoToken::new(token).map_err(debug)?,
        VideoCacheBudget::new(64, 4).map_err(debug)?,
        1,
        move |range| {
            ranges_for_server
                .lock()
                .map_err(|_| VideoPreviewError::LoopbackServerIo)?
                .push(range);
            if range == VideoRange::new(64, 127)? {
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
            Ok(bytes_for_range(range))
        },
    )
    .map_err(debug)?;

    let first_request = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-63\r\n\r\n",
        token
    );
    let second_request = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=64-127\r\n\r\n",
        token
    );
    let _ = raw_http(server.port(), &first_request)?;
    wait_for_range(&ranges, VideoRange::new(64, 127).map_err(debug)?)?;

    let port = server.port();
    let second = thread::spawn(move || raw_http(port, &second_request));
    thread::sleep(Duration::from_millis(50));
    {
        let (lock, cvar) = &*gate;
        let mut released = lock.lock().map_err(|_| "gate lock poisoned".to_string())?;
        *released = true;
        cvar.notify_all();
    }
    let response = second
        .join()
        .map_err(|_| "second request thread panicked".to_string())??;

    assert!(response.starts_with("HTTP/1.1 206 Partial Content"));
    assert_eq!(body(&response).len(), 64);
    let fetched_ranges = ranges.lock().map_err(|err| err.to_string())?;
    let prefetched_range = VideoRange::new(64, 127).map_err(debug)?;
    assert_eq!(
        fetched_ranges
            .iter()
            .filter(|range| **range == prefetched_range)
            .count(),
        1
    );
    assert_eq!(server.stats().cache_hits, 1);
    Ok(())
}

#[test]
fn loopback_server_handles_helper_request_while_fetch_is_blocked() -> Result<(), String> {
    let fetches = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let fetches_for_server = Arc::clone(&fetches);
    let gate_for_server = Arc::clone(&gate);
    let token = "tok_loopback_server_0006";
    let server = VideoLoopbackServer::spawn_with_prefetch(
        key(),
        VideoToken::new(token).map_err(debug)?,
        VideoCacheBudget::new(64, 2).map_err(debug)?,
        0,
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
            Ok(bytes_for_range(range))
        },
    )
    .map_err(debug)?;

    let first_request = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-63\r\n\r\n",
        token
    );
    let head_request = format!(
        "HEAD /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        token
    );
    let port = server.port();
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
    let token = "tok_loopback_server_0004";
    let server = VideoLoopbackServer::spawn_with_prefetch(
        key(),
        VideoToken::new(token).map_err(debug)?,
        VideoCacheBudget::new(64, 4).map_err(debug)?,
        2,
        move |range| {
            fetches_for_server.fetch_add(1, Ordering::AcqRel);
            Ok(bytes_for_range(range))
        },
    )
    .map_err(debug)?;

    let request = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-63\r\n\r\n",
        token
    );
    let response = raw_http(server.port(), &request)?;

    assert!(response.starts_with("HTTP/1.1 206 Partial Content"));
    wait_for_fetches(&fetches, 3)?;
    assert!(server.stats().used_bytes <= 256);
    assert!(server.stats().prefetch_fetches >= 2);
    Ok(())
}

#[test]
fn loopback_server_prefetches_forward_window_in_parallel() -> Result<(), String> {
    let active_prefetches = Arc::new(AtomicUsize::new(0));
    let max_active_prefetches = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let active_for_server = Arc::clone(&active_prefetches);
    let max_for_server = Arc::clone(&max_active_prefetches);
    let gate_for_server = Arc::clone(&gate);
    let token = "tok_loopback_server_0012";
    let server = VideoLoopbackServer::spawn_with_prefetch(
        key(),
        VideoToken::new(token).map_err(debug)?,
        VideoCacheBudget::new(64, 5).map_err(debug)?,
        3,
        move |range| {
            if range.start >= 64 {
                let active = active_for_server
                    .fetch_add(1, Ordering::AcqRel)
                    .saturating_add(1);
                record_max_active(&max_for_server, active);
                let (lock, cvar) = &*gate_for_server;
                let mut released = lock
                    .lock()
                    .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
                while !*released {
                    released = cvar
                        .wait(released)
                        .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
                }
                active_for_server.fetch_sub(1, Ordering::AcqRel);
            }
            Ok(bytes_for_range(range))
        },
    )
    .map_err(debug)?;

    let request = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-63\r\n\r\n",
        token
    );
    let response = raw_http(server.port(), &request)?;
    assert!(response.starts_with("HTTP/1.1 206 Partial Content"));

    wait_for_fetches(&active_prefetches, 3)?;
    {
        let (lock, cvar) = &*gate;
        let mut released = lock.lock().map_err(|err| err.to_string())?;
        *released = true;
        cvar.notify_all();
    }
    wait_for_prefetches(&server, 3)?;
    assert!(max_active_prefetches.load(Ordering::Acquire) >= 3);
    Ok(())
}

#[test]
fn loopback_server_serves_prefetched_subrange_from_interval_buffer() -> Result<(), String> {
    let fetches = Arc::new(AtomicUsize::new(0));
    let fetches_for_server = Arc::clone(&fetches);
    let token = "tok_loopback_server_0005";
    let server = VideoLoopbackServer::spawn_with_prefetch(
        key(),
        VideoToken::new(token).map_err(debug)?,
        VideoCacheBudget::new(64, 3).map_err(debug)?,
        1,
        move |range| {
            fetches_for_server.fetch_add(1, Ordering::AcqRel);
            Ok(bytes_for_range(range))
        },
    )
    .map_err(debug)?;

    let first = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-63\r\n\r\n",
        token
    );
    let second = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=80-95\r\n\r\n",
        token
    );

    let _ = raw_http(server.port(), &first)?;
    wait_for_fetches(&fetches, 2)?;
    let before = fetches.load(Ordering::Acquire);
    let response = raw_http(server.port(), &second)?;

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
    let token = "tok_loopback_server_0007";
    let server = VideoLoopbackServer::spawn_with_prefetch(
        key(),
        VideoToken::new(token).map_err(debug)?,
        VideoCacheBudget::new(64, 4).map_err(debug)?,
        2,
        move |range| {
            fetches_for_server.fetch_add(1, Ordering::AcqRel);
            Ok(bytes_for_range(range))
        },
    )
    .map_err(debug)?;

    let first = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-63\r\n\r\n",
        token
    );
    let far_seek = format!(
        "GET /preview/video/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=512-575\r\n\r\n",
        token
    );

    let _ = raw_http(server.port(), &first)?;
    wait_for_fetches(&fetches, 3)?;
    let before = fetches.load(Ordering::Acquire);
    let response = raw_http(server.port(), &far_seek)?;
    thread::sleep(Duration::from_millis(100));

    assert!(response.starts_with("HTTP/1.1 206 Partial Content"));
    assert_eq!(fetches.load(Ordering::Acquire), before + 1);
    Ok(())
}

fn bytes_for_range(range: VideoRange) -> Vec<u8> {
    let len = range
        .end_inclusive
        .saturating_sub(range.start)
        .saturating_add(1);
    vec![b'v'; usize::try_from(len).unwrap_or(usize::MAX)]
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

fn wait_for_prefetches(server: &VideoLoopbackServer, expected: u64) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while server.stats().prefetch_fetches < expected {
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for {expected} prefetches, saw {}",
                server.stats().prefetch_fetches
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn record_max_active(max: &AtomicUsize, value: usize) {
    let mut current = max.load(Ordering::Acquire);
    while value > current {
        match max.compare_exchange(current, value, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return,
            Err(next) => current = next,
        }
    }
}

fn wait_for_range(ranges: &Mutex<Vec<VideoRange>>, expected: VideoRange) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        {
            let guard = ranges.lock().map_err(|err| err.to_string())?;
            if guard.contains(&expected) {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting for range {expected:?}"));
        }
        thread::sleep(Duration::from_millis(10));
    }
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
    response.split_once("\r\n\r\n").map_or("", |(_, body)| body)
}

fn debug(err: impl std::fmt::Debug) -> String {
    format!("{err:?}")
}
