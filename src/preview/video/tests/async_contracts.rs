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
use tokio_util::sync::CancellationToken;

struct BlockingUpstream {
    cancelled: AtomicBool,
}

impl VideoUpstream for BlockingUpstream {
    fn fetch<'a>(
        &'a self,
        _range: VideoRange,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, VideoError>> + Send + 'a>> {
        Box::pin(async move {
            cancel.cancelled().await;
            self.cancelled.store(true, Ordering::Release);
            Err(VideoError::Cancelled)
        })
    }
}

async fn issue_get(local_url: &str) -> std::io::Result<Vec<u8>> {
    let without_scheme = local_url.strip_prefix("http://").expect("loopback URL");
    let (address, path) = without_scheme.split_once('/').expect("token path");
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
        cancelled: AtomicBool::new(false),
    });
    let cancel = CancellationToken::new();
    let server = VideoLoopbackServer::spawn_with_prefetch(
        4096,
        VideoToken::new("session_cancel_123456")?,
        VideoCacheBudget::new(64, 2)?,
        0,
        upstream.clone(),
        cancel.clone(),
    )
    .await?;
    let url = server.local_url().to_string();
    let request = tokio::spawn(async move { issue_get(&url).await });
    tokio::task::yield_now().await;

    cancel.cancel();
    tokio::time::timeout(Duration::from_millis(500), server.shutdown()).await??;
    let _ = tokio::time::timeout(Duration::from_millis(500), request).await?;
    assert!(upstream.cancelled.load(Ordering::Acquire));
    Ok(())
}
