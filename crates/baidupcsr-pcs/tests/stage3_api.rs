use baidupcsr_pcs::{
    BlockingPcsClient, BlockingPcsClientOptions, ByteRange, PcsAuth, PcsClientOptions, PcsError,
};

#[test]
fn download_range_can_be_cancelled_before_network_work() -> Result<(), PcsError> {
    let client = BlockingPcsClient::new(
        PcsClientOptions {
            app_id: 266719,
            pcs_addr: "invalid.local".to_string(),
            enable_https: true,
        },
        PcsAuth::from_cookie_header("BDUSS=redacted"),
        BlockingPcsClientOptions::new_with_pan("baidupcsr-test", "baidupcsr-test", None),
    );

    assert_eq!(
        client.download_range_with_cancel("/remote.txt", ByteRange::prefix(8)?, || true),
        Err(PcsError::Cancelled {
            operation: "download".to_string()
        })
    );
    Ok(())
}

#[test]
fn download_url_range_can_be_cancelled_before_network_work() -> Result<(), PcsError> {
    let client = BlockingPcsClient::new(
        PcsClientOptions {
            app_id: 266719,
            pcs_addr: "invalid.local".to_string(),
            enable_https: true,
        },
        PcsAuth::from_cookie_header("BDUSS=redacted"),
        BlockingPcsClientOptions::new_with_pan("pcs-ua", "pan-ua", None),
    );

    assert_eq!(
        client.download_url_range_with_cancel(
            "https://fixture.invalid/video.mp4",
            ByteRange::prefix(8)?,
            || true
        ),
        Err(PcsError::Cancelled {
            operation: "download".to_string()
        })
    );
    Ok(())
}
