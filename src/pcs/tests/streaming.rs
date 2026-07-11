use super::super::*;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

fn transport() -> PcsTransportOptions {
    PcsTransportOptions {
        user_agent: "pcs-stream-test".to_string(),
        pan_user_agent: "pan-stream-test".to_string(),
        proxy: None,
        timeout: Duration::from_secs(5),
    }
}

#[tokio::test]
async fn download_url_range_cancels_while_response_body_is_stalled(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (body_started_tx, body_started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel::<()>();

    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut request = vec![0; 4096];
        let read = socket.read(&mut request).await?;
        let request = String::from_utf8_lossy(&request[..read]);
        let range = request.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("range").then(|| value.trim())
        });
        assert_eq!(range, Some("bytes=0-7"));
        socket
            .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 8\r\nContent-Range: bytes 0-7/8\r\nConnection: close\r\n\r\nab")
            .await?;
        let _ = body_started_tx.send(());
        let _ = release_rx.await;
        socket.write_all(b"cdefgh").await?;
        Ok::<(), std::io::Error>(())
    });

    let client = PcsClient::new(
        PcsClientOptions {
            app_id: 266719,
            pcs_addr: address.to_string(),
            enable_https: false,
        },
        PcsAuth::from_cookie_header("BDUSS=redacted"),
        transport(),
    )?;
    let cancel = CancellationToken::new();
    let request_cancel = cancel.clone();
    let url = format!("http://{address}/video.mp4");
    let download = tokio::spawn(async move {
        client
            .download_url_range(&url, ByteRange::prefix(8)?, &request_cancel)
            .await
    });

    body_started_rx.await?;
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_millis(500), download).await??;
    assert_eq!(
        result,
        Err(PcsError::Cancelled {
            operation: "download".to_string()
        })
    );

    drop(release_tx);
    let _ = server.await;
    Ok(())
}

#[tokio::test]
async fn cancelled_list_and_search_never_open_a_connection(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let client = PcsClient::new(
        PcsClientOptions {
            app_id: 266719,
            pcs_addr: address.to_string(),
            enable_https: false,
        },
        PcsAuth::from_cookie_header("BDUSS=redacted"),
        transport(),
    )?;
    let cancel = CancellationToken::new();
    cancel.cancel();

    assert!(matches!(
        client.list("/", &cancel).await,
        Err(PcsError::Cancelled { .. })
    ));
    assert!(matches!(
        client.search("/", "needle", true, &cancel).await,
        Err(PcsError::Cancelled { .. })
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
    Ok(())
}
