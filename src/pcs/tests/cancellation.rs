use super::super::*;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn client() -> Result<PcsClient, PcsError> {
    PcsClient::new(
        PcsClientOptions {
            app_id: 266719,
            pcs_addr: "invalid.local".to_string(),
            enable_https: true,
        },
        PcsAuth::from_cookie_header("BDUSS=redacted"),
        PcsTransportOptions {
            user_agent: "baidupcsr-test".to_string(),
            pan_user_agent: "baidupcsr-test".to_string(),
            proxy: None,
            timeout: Duration::from_secs(1),
        },
    )
}

#[tokio::test]
async fn download_range_can_be_cancelled_before_network_work() -> Result<(), PcsError> {
    let client = client()?;
    let cancel = CancellationToken::new();
    cancel.cancel();

    assert_eq!(
        client
            .download_range("/remote.txt", ByteRange::prefix(8)?, &cancel)
            .await,
        Err(PcsError::Cancelled {
            operation: "download".to_string()
        })
    );
    Ok(())
}

#[tokio::test]
async fn download_url_range_can_be_cancelled_before_network_work() -> Result<(), PcsError> {
    let client = client()?;
    let cancel = CancellationToken::new();
    cancel.cancel();

    assert_eq!(
        client
            .download_url_range(
                "https://fixture.invalid/video.mp4",
                ByteRange::prefix(8)?,
                &cancel,
            )
            .await,
        Err(PcsError::Cancelled {
            operation: "download".to_string()
        })
    );
    Ok(())
}
