use std::fs;
use std::path::{Path, PathBuf};

use baidupcsr_pcs::{
    dev_uid, locatedownload_sign_with, parse_file_list, parse_locatedownload, parse_remote_error,
    select_download_urls, BlockingPcsClient, BlockingPcsClientOptions, ByteRange, OrderBy,
    OrderDirection, OrderOptions, PcsApi, PcsAuth, PcsClientOptions, PcsError,
};

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/api")
        .join(name)
}

fn fixture_string(name: &str) -> String {
    let data = fs::read_to_string(fixture_path(name));
    assert!(data.is_ok(), "fixture should be readable");
    data.unwrap_or_default()
}

#[test]
fn parse_list_response_preserves_file_model_fields() {
    let entries = parse_file_list(&fixture_string("list_root.json"));
    assert!(entries.is_ok(), "list fixture should parse");
    let entries = match entries {
        Ok(entries) => entries,
        Err(_) => return,
    };

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].fs_id, 9001);
    assert!(entries[0].is_dir);
    assert_eq!(entries[1].server_filename, "hello.txt");
    assert_eq!(
        entries[1].block_list,
        vec!["5d41402abc4b2a76b9719d911017c592"]
    );
}

#[test]
fn parse_list_response_accepts_real_success_defaults() -> Result<(), PcsError> {
    let response = r#"{
        "list": [
            {
                "fs_id": 10,
                "path": "/我的资源/example",
                "server_filename": "example",
                "isdir": 1
            }
        ]
    }"#;

    let files = parse_file_list(response)?;

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].server_filename, "example");
    assert!(files[0].is_dir);
    assert_eq!(files[0].md5, "");
    assert_eq!(files[0].size, 0);
    Ok(())
}

#[test]
fn parse_remote_error_keeps_remote_code_and_message() {
    let error = parse_remote_error("list", &fixture_string("remote_error.json"));
    assert!(matches!(
        error,
        PcsError::Remote {
            operation: _,
            code: 31066,
            message: _
        }
    ));
}

#[test]
fn list_request_builds_pcs_url_with_ordering() {
    let api = PcsApi::new(PcsClientOptions {
        app_id: 266719,
        pcs_addr: "pcs.baidu.com".to_string(),
        enable_https: true,
    });

    let request = api.list_request(
        "/我的资源",
        OrderOptions {
            by: OrderBy::Name,
            direction: OrderDirection::Asc,
        },
    );

    assert_eq!(request.method, "GET");
    assert_eq!(
        request.url.as_str(),
        "https://pcs.baidu.com/rest/2.0/pcs/file?app_id=266719&method=list&path=%2F%E6%88%91%E7%9A%84%E8%B5%84%E6%BA%90&by=name&order=asc"
    );
}

#[test]
fn download_request_builds_readonly_pcs_file_url() {
    let api = PcsApi::new(PcsClientOptions {
        app_id: 266719,
        pcs_addr: "pcs.baidu.com".to_string(),
        enable_https: true,
    });

    let request = api.download_request("/我的资源/notes.md");

    assert_eq!(request.method, "GET");
    assert_eq!(
        request.url.as_str(),
        "https://pcs.baidu.com/rest/2.0/pcs/file?app_id=266719&method=download&path=%2F%E6%88%91%E7%9A%84%E8%B5%84%E6%BA%90%2Fnotes.md"
    );
}

#[test]
fn locatedownload_request_builds_signed_pcs_url() {
    let api = PcsApi::new(PcsClientOptions {
        app_id: 266719,
        pcs_addr: "pcs.baidu.com".to_string(),
        enable_https: true,
    });
    let sign = locatedownload_sign_with(
        1_571_140_066,
        "O|1E67351CCE80B2CF48DB511CD77ACD9F",
        10086,
        "test_bduss",
    );

    let request = api.locatedownload_request("/我的资源/movie.mp4", &sign);

    assert_eq!(request.method, "POST");
    assert!(request
        .url
        .starts_with("https://pcs.baidu.com/rest/2.0/pcs/file?ant=1&check_blue=1"));
    assert!(request.url.contains("method=locatedownload"));
    assert!(request.url.contains("app_id=250528"));
    assert!(request
        .url
        .contains("path=%2F%E6%88%91%E7%9A%84%E8%B5%84%E6%BA%90%2Fmovie.mp4"));
    assert!(request.url.contains("time=1571140066"));
    assert!(request
        .url
        .contains("devuid=O%7C1E67351CCE80B2CF48DB511CD77ACD9F"));
}

#[test]
fn preview_range_header_is_bounded_to_requested_bytes() {
    let range = baidupcsr_pcs::ByteRange::prefix(16);

    assert_eq!(range.header_value(), "bytes=0-15");
    assert_eq!(range.read_limit_bytes(), 17);
}

#[test]
fn preview_range_header_supports_seek_offsets() -> Result<(), PcsError> {
    let range = ByteRange::new(1024, 2047)?;

    assert_eq!(range.header_value(), "bytes=1024-2047");
    assert_eq!(range.read_limit_bytes(), 1025);
    Ok(())
}

#[test]
fn empty_preview_range_is_rejected() {
    assert_eq!(
        baidupcsr_pcs::ByteRange::try_prefix(0),
        Err(baidupcsr_pcs::PcsError::Network {
            operation: "download".to_string(),
            message: "range length must be greater than zero".to_string(),
        })
    );
}

#[test]
fn download_range_can_be_cancelled_before_network_work() {
    let client = BlockingPcsClient::new(
        PcsApi::new(PcsClientOptions {
            app_id: 266719,
            pcs_addr: "invalid.local".to_string(),
            enable_https: true,
        }),
        PcsAuth::from_cookie_header("BDUSS=redacted"),
        BlockingPcsClientOptions::new("baidupcsr-test", None),
    );

    assert_eq!(
        client.download_range_with_cancel("/remote.txt", ByteRange::prefix(8), || true),
        Err(PcsError::Cancelled {
            operation: "download".to_string()
        })
    );
}

#[test]
fn client_options_keep_pan_user_agent_separate() {
    let options = BlockingPcsClientOptions::new_with_pan("pcs-ua", "pan-ua", None);

    assert_eq!(options.user_agent, "pcs-ua");
    assert_eq!(options.pan_user_agent, "pan-ua");
}

#[test]
fn download_url_range_can_be_cancelled_before_network_work() {
    let client = BlockingPcsClient::new(
        PcsApi::new(PcsClientOptions {
            app_id: 266719,
            pcs_addr: "invalid.local".to_string(),
            enable_https: true,
        }),
        PcsAuth::from_cookie_header("BDUSS=redacted"),
        BlockingPcsClientOptions::new_with_pan("pcs-ua", "pan-ua", None),
    );

    assert_eq!(
        client.download_url_range_with_cancel(
            "https://fixture.invalid/video.mp4",
            ByteRange::prefix(8),
            || true
        ),
        Err(PcsError::Cancelled {
            operation: "download".to_string()
        })
    );
}

#[test]
fn locatedownload_sign_matches_reference_vector() {
    let sign = locatedownload_sign_with(
        1_571_140_066,
        "O|1E67351CCE80B2CF48DB511CD77ACD9F",
        10086,
        "test_bduss",
    );

    assert_eq!(sign.rand, "b6bb7a6f46899e181baea58798d4fdb889775c2c");
    assert_eq!(
        sign.url_param(),
        "time=1571140066&rand=b6bb7a6f46899e181baea58798d4fdb889775c2c&devuid=O|1E67351CCE80B2CF48DB511CD77ACD9F&cuid=O|1E67351CCE80B2CF48DB511CD77ACD9F"
    );
}

#[test]
fn devuid_is_uppercase_md5_plus_suffix() {
    assert_eq!(dev_uid("test_bduss"), "66AE65D3C5D483FC4431FC43670A4673|0");
}

#[test]
fn locatedownload_parser_selects_unencrypted_urls() {
    let info = parse_locatedownload(&fixture_string("locatedownload.json"));
    assert!(info.is_ok(), "locatedownload fixture should parse");
    let info = match info {
        Ok(info) => info,
        Err(_) => return,
    };

    let urls = select_download_urls(&info, true);
    assert_eq!(urls, vec!["https://fixture.invalid/download/hello.txt"]);
}

#[test]
fn locatedownload_parser_accepts_success_without_errno() -> Result<(), PcsError> {
    let info = parse_locatedownload(
        r#"{
            "urls": [
                {
                    "url": "http://fixture.invalid/download/movie.mp4",
                    "encrypt": 0
                }
            ]
        }"#,
    )?;

    assert_eq!(select_download_urls(&info, true).len(), 1);
    Ok(())
}
