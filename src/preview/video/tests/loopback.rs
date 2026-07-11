use super::super::*;
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct FakeUpstream {
    fetches: AtomicUsize,
    active: AtomicUsize,
    max_active: AtomicUsize,
    ranges: Mutex<Vec<VideoRange>>,
    delay: Duration,
}

impl FakeUpstream {
    fn with_delay(delay: Duration) -> Self {
        Self {
            delay,
            ..Self::default()
        }
    }

    fn count(&self) -> usize {
        self.fetches.load(Ordering::Acquire)
    }

    fn ranges(&self) -> Vec<VideoRange> {
        match self.ranges.lock() {
            Ok(ranges) => ranges.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

impl VideoUpstream for FakeUpstream {
    fn fetch<'a>(
        &'a self,
        range: VideoRange,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, VideoError>> + Send + 'a>> {
        Box::pin(async move {
            self.fetches.fetch_add(1, Ordering::AcqRel);
            match self.ranges.lock() {
                Ok(mut ranges) => ranges.push(range),
                Err(poisoned) => poisoned.into_inner().push(range),
            }
            let active = self.active.fetch_add(1, Ordering::AcqRel) + 1;
            self.max_active.fetch_max(active, Ordering::AcqRel);
            if !self.delay.is_zero() {
                tokio::select! {
                    () = tokio::time::sleep(self.delay) => {}
                    () = cancel.cancelled() => {
                        self.active.fetch_sub(1, Ordering::AcqRel);
                        return Err(VideoError::Cancelled);
                    }
                }
            }
            self.active.fetch_sub(1, Ordering::AcqRel);
            Ok((range.start..=range.end_inclusive)
                .map(|offset| u8::try_from(offset % 251).unwrap_or_default())
                .collect())
        })
    }
}

async fn spawn(
    content_length: u64,
    chunk_size: u64,
    chunks: usize,
    prefetch: usize,
    upstream: Arc<FakeUpstream>,
) -> Result<(VideoLoopbackServer, CancellationToken), String> {
    let cancel = CancellationToken::new();
    let server = VideoLoopbackServer::spawn_with_prefetch(
        content_length,
        VideoToken::new("fixture_token_123456").map_err(|error| format!("{error:?}"))?,
        VideoCacheBudget::new(chunk_size, chunks).map_err(|error| format!("{error:?}"))?,
        prefetch,
        upstream,
        cancel.clone(),
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok((server, cancel))
}

fn request_target(local_url: &str) -> io::Result<(&str, &str)> {
    let without_scheme = local_url
        .strip_prefix("http://")
        .ok_or_else(|| io::Error::other("loopback URL must use HTTP"))?;
    without_scheme
        .split_once('/')
        .ok_or_else(|| io::Error::other("loopback URL must have token path"))
}

async fn raw_http(
    local_url: &str,
    method: &str,
    range: Option<&str>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let (address, path) = request_target(local_url)?;
    let mut stream = TcpStream::connect(address).await?;
    let range = range
        .map(|value| format!("Range: {value}\r\n"))
        .unwrap_or_default();
    let request =
        format!("{method} /{path} HTTP/1.1\r\nHost: {address}\r\n{range}Connection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    Ok(response)
}

fn header(response: &[u8]) -> io::Result<&str> {
    let end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| io::Error::other("missing HTTP header terminator"))?;
    std::str::from_utf8(&response[..end])
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn body(response: &[u8]) -> io::Result<&[u8]> {
    let end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| io::Error::other("missing HTTP header terminator"))?;
    Ok(&response[end + 4..])
}

async fn wait_for_fetches(
    upstream: &FakeUpstream,
    expected: usize,
) -> Result<(), tokio::time::error::Elapsed> {
    tokio::time::timeout(Duration::from_secs(2), async {
        while upstream.count() < expected {
            tokio::task::yield_now().await;
        }
    })
    .await
}

#[tokio::test]
async fn loopback_server_serves_head_and_range_through_loopback_port(
) -> Result<(), Box<dyn std::error::Error>> {
    let upstream = Arc::new(FakeUpstream::default());
    let (server, _) = spawn(4096, 64, 2, 0, upstream.clone()).await?;
    assert!(server.local_url().starts_with("http://127.0.0.1:"));

    let head_response = raw_http(server.local_url(), "HEAD", None).await?;
    assert!(header(&head_response)?.contains("200 OK"));
    assert!(header(&head_response)?.contains("Accept-Ranges: bytes"));
    assert_eq!(upstream.count(), 0);

    let get = raw_http(server.local_url(), "GET", Some("bytes=0-63")).await?;
    assert!(header(&get)?.contains("206 Partial Content"));
    assert_eq!(body(&get)?.len(), 64);
    assert_eq!(upstream.count(), 1);
    assert!(server.stats().used_bytes <= 128);
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn loopback_server_serves_open_ended_far_seek_as_bounded_chunk(
) -> Result<(), Box<dyn std::error::Error>> {
    let upstream = Arc::new(FakeUpstream::default());
    let (server, _) = spawn(1_000_000, 64, 4, 0, upstream.clone()).await?;
    let response = raw_http(server.local_url(), "GET", Some("bytes=900000-")).await?;
    assert!(header(&response)?.contains("Content-Range: bytes 900000-900063/1000000"));
    assert_eq!(body(&response)?.len(), 64);
    assert_eq!(
        upstream.ranges(),
        vec![VideoRange {
            start: 900_000,
            end_inclusive: 900_063,
        }]
    );
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn loopback_server_serves_suffix_range_from_file_tail(
) -> Result<(), Box<dyn std::error::Error>> {
    let upstream = Arc::new(FakeUpstream::default());
    let (server, _) = spawn(1000, 64, 4, 0, upstream.clone()).await?;
    let response = raw_http(server.local_url(), "GET", Some("bytes=-32")).await?;
    assert!(header(&response)?.contains("Content-Range: bytes 968-999/1000"));
    assert_eq!(body(&response)?.len(), 32);
    assert_eq!(
        upstream.ranges(),
        vec![VideoRange {
            start: 968,
            end_inclusive: 999,
        }]
    );
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn loopback_server_prefetches_after_open_ended_far_seek(
) -> Result<(), Box<dyn std::error::Error>> {
    let upstream = Arc::new(FakeUpstream::default());
    let (server, _) = spawn(1_000_000, 64, 4, 2, upstream.clone()).await?;
    let _ = raw_http(server.local_url(), "GET", Some("bytes=900000-")).await?;
    wait_for_fetches(&upstream, 3).await?;
    assert!(server.stats().prefetch_fetches >= 2);
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn loopback_server_prefetches_after_open_ended_far_seek_with_existing_window(
) -> Result<(), Box<dyn std::error::Error>> {
    let upstream = Arc::new(FakeUpstream::default());
    let (server, _) = spawn(1_000_000, 64, 6, 2, upstream.clone()).await?;
    let _ = raw_http(server.local_url(), "GET", Some("bytes=0-")).await?;
    wait_for_fetches(&upstream, 3).await?;
    let _ = raw_http(server.local_url(), "GET", Some("bytes=900000-")).await?;
    wait_for_fetches(&upstream, 6).await?;
    assert!(upstream.ranges().iter().any(|range| range.start == 900_000));
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn loopback_server_rejects_wrong_token_without_fetching_upstream(
) -> Result<(), Box<dyn std::error::Error>> {
    let upstream = Arc::new(FakeUpstream::default());
    let (server, _) = spawn(4096, 64, 2, 0, upstream.clone()).await?;
    let wrong_url = server
        .local_url()
        .replace("fixture_token_123456", "wrong_token_12345678");
    let response = raw_http(&wrong_url, "GET", Some("bytes=0-63")).await?;
    assert!(header(&response)?.contains("403 Forbidden"));
    assert_eq!(upstream.count(), 0);
    assert_eq!(server.stats().used_bytes, 0);
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn loopback_server_reuses_chunk_cache_and_reports_stats(
) -> Result<(), Box<dyn std::error::Error>> {
    let upstream = Arc::new(FakeUpstream::default());
    let (server, _) = spawn(4096, 64, 2, 0, upstream.clone()).await?;
    let first = raw_http(server.local_url(), "GET", Some("bytes=0-63")).await?;
    let second = raw_http(server.local_url(), "GET", Some("bytes=0-63")).await?;
    assert_eq!(body(&first)?, body(&second)?);
    assert_eq!(upstream.count(), 1);
    assert_eq!(server.stats().upstream_fetches, 1);
    assert_eq!(server.stats().cache_hits, 1);
    assert!(server.stats().used_bytes <= 128);
    assert_eq!(server.stats().last_range_start, Some(0));
    assert_eq!(server.stats().last_range_end_inclusive, Some(63));
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn loopback_server_waits_for_matching_inflight_prefetch(
) -> Result<(), Box<dyn std::error::Error>> {
    let upstream = Arc::new(FakeUpstream::with_delay(Duration::from_millis(100)));
    let (server, _) = spawn(4096, 64, 2, 0, upstream.clone()).await?;
    let url = server.local_url().to_string();
    let first = tokio::spawn(async move {
        raw_http(&url, "GET", Some("bytes=64-127"))
            .await
            .map_err(|err| err.to_string())
    });
    wait_for_fetches(&upstream, 1).await?;
    let second = raw_http(server.local_url(), "GET", Some("bytes=64-127")).await?;
    let first = first.await??;
    assert_eq!(body(&first)?, body(&second)?);
    let expected_range =
        VideoRange::new(64, 127).map_err(|error| format!("invalid expected range: {error:?}"))?;
    assert_eq!(
        upstream
            .ranges()
            .iter()
            .filter(|range| **range == expected_range)
            .count(),
        1
    );
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn loopback_server_handles_helper_request_while_fetch_is_blocked(
) -> Result<(), Box<dyn std::error::Error>> {
    let upstream = Arc::new(FakeUpstream::with_delay(Duration::from_millis(300)));
    let (server, _) = spawn(4096, 64, 2, 0, upstream.clone()).await?;
    let url = server.local_url().to_string();
    let get = tokio::spawn(async move {
        raw_http(&url, "GET", Some("bytes=0-63"))
            .await
            .map_err(|err| err.to_string())
    });
    wait_for_fetches(&upstream, 1).await?;
    let head_response = tokio::time::timeout(
        Duration::from_millis(100),
        raw_http(server.local_url(), "HEAD", None),
    )
    .await??;
    assert!(header(&head_response)?.contains("200 OK"));
    let _ = get.await??;
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn loopback_server_prefetches_forward_window_without_exceeding_budget(
) -> Result<(), Box<dyn std::error::Error>> {
    let upstream = Arc::new(FakeUpstream::default());
    let (server, _) = spawn(4096, 64, 4, 2, upstream.clone()).await?;
    let _ = raw_http(server.local_url(), "GET", Some("bytes=0-63")).await?;
    wait_for_fetches(&upstream, 3).await?;
    assert!(server.stats().used_bytes <= 256);
    assert!(server.stats().prefetch_fetches >= 2);
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn loopback_server_prefetches_forward_window_in_parallel(
) -> Result<(), Box<dyn std::error::Error>> {
    let upstream = Arc::new(FakeUpstream::with_delay(Duration::from_millis(100)));
    let (server, _) = spawn(4096, 64, 8, 3, upstream.clone()).await?;
    let _ = raw_http(server.local_url(), "GET", Some("bytes=0-63")).await?;
    wait_for_fetches(&upstream, 4).await?;
    assert!(upstream.max_active.load(Ordering::Acquire) >= 3);
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn loopback_server_serves_prefetched_subrange_from_interval_buffer(
) -> Result<(), Box<dyn std::error::Error>> {
    let upstream = Arc::new(FakeUpstream::default());
    let (server, _) = spawn(4096, 64, 4, 1, upstream.clone()).await?;
    let _ = raw_http(server.local_url(), "GET", Some("bytes=0-63")).await?;
    wait_for_fetches(&upstream, 2).await?;
    let before = upstream.count();
    let response = raw_http(server.local_url(), "GET", Some("bytes=80-95")).await?;
    assert_eq!(body(&response)?.len(), 16);
    assert_eq!(upstream.count(), before);
    assert!(server.stats().cache_hits >= 1);
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn loopback_server_does_not_prefetch_far_seek_thumbnail_requests(
) -> Result<(), Box<dyn std::error::Error>> {
    let upstream = Arc::new(FakeUpstream::default());
    let (server, _) = spawn(1_000_000, 64, 4, 2, upstream.clone()).await?;
    let _ = raw_http(server.local_url(), "GET", Some("bytes=0-")).await?;
    wait_for_fetches(&upstream, 3).await?;
    let before = upstream.count();
    let response = raw_http(server.local_url(), "GET", Some("bytes=900000-900007")).await?;
    assert_eq!(body(&response)?.len(), 8);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(upstream.count(), before + 1);
    server.shutdown().await?;
    Ok(())
}
