use super::super::*;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

struct BlockingUpstream {
    started: Notify,
    stopped: AtomicBool,
}

struct MarkStoppedOnDrop<'a>(&'a AtomicBool);

impl Drop for MarkStoppedOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl VideoUpstream for BlockingUpstream {
    fn fetch<'a>(
        &'a self,
        _range: VideoRange,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, VideoError>> + Send + 'a>> {
        Box::pin(async move {
            self.started.notify_one();
            let _mark_stopped = MarkStoppedOnDrop(&self.stopped);
            cancel.cancelled().await;
            Err(VideoError::Cancelled)
        })
    }
}

async fn issue_get(local_url: &str) -> std::io::Result<Vec<u8>> {
    let without_scheme = local_url
        .strip_prefix("http://")
        .ok_or_else(|| std::io::Error::other("loopback URL must use HTTP"))?;
    let (address, path) = without_scheme
        .split_once('/')
        .ok_or_else(|| std::io::Error::other("loopback URL must have token path"))?;
    let mut stream = TcpStream::connect(address).await?;
    stream.write_all(format!("GET /{path} HTTP/1.1\r\nHost: {address}\r\nRange: bytes=0-63\r\nConnection: close\r\n\r\n").as_bytes()).await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    Ok(response)
}

#[tokio::test]
async fn cancelling_video_session_stops_blocked_upstream_and_loopback(
) -> Result<(), Box<dyn std::error::Error>> {
    let upstream = Arc::new(BlockingUpstream {
        started: Notify::new(),
        stopped: AtomicBool::new(false),
    });
    let cancel = CancellationToken::new();
    let token = VideoToken::new("session_cancel_123456")
        .map_err(|error| format!("invalid fixture token: {error:?}"))?;
    let budget = VideoCacheBudget::new(64, 2)
        .map_err(|error| format!("invalid fixture budget: {error:?}"))?;
    let server = VideoLoopbackServer::spawn_with_prefetch(
        4096,
        token,
        budget,
        0,
        upstream.clone(),
        cancel.clone(),
    )
    .await?;
    let url = server.local_url().to_string();
    let request_stopped = Arc::new(AtomicBool::new(false));
    let request_stopped_by_task = request_stopped.clone();
    let request = tokio::spawn(async move {
        let result = issue_get(&url).await;
        request_stopped_by_task.store(true, Ordering::Release);
        result
    });
    tokio::time::timeout(Duration::from_millis(500), upstream.started.notified()).await?;

    cancel.cancel();
    let shutdown = tokio::time::timeout(Duration::from_millis(500), server.shutdown()).await?;
    assert!(shutdown.is_ok(), "loopback server must shut down cleanly");
    shutdown?;
    let request_result = tokio::time::timeout(Duration::from_millis(500), request).await?;
    assert!(
        request_result.is_ok(),
        "request task must stop without panicking"
    );
    let _request_io_result = request_result?;
    assert!(request_stopped.load(Ordering::Acquire));
    assert!(upstream.stopped.load(Ordering::Acquire));
    Ok(())
}
