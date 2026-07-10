use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use super::super::*;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/api")
        .join(name)
}

fn fixture_string(name: &str) -> String {
    let data = fs::read_to_string(fixture_path(name));
    assert!(data.is_ok(), "fixture should be readable");
    data.unwrap_or_default()
}

fn api() -> PcsApi {
    PcsApi {
        options: PcsClientOptions {
            app_id: 266_719,
            pcs_addr: "pcs.baidu.com".to_string(),
            enable_https: true,
        },
    }
}

fn transport() -> PcsTransportOptions {
    PcsTransportOptions {
        user_agent: "pcs-ua".to_string(),
        pan_user_agent: "pan-ua".to_string(),
        proxy: None,
        timeout: Duration::from_secs(30),
    }
}

#[test]
fn client_options_keep_pan_user_agent_separate() {
    let options = transport();
    assert_eq!(options.user_agent, "pcs-ua");
    assert_eq!(options.pan_user_agent, "pan-ua");
}

#[test]
fn blocking_client_clones_share_one_http_client_cache() -> Result<(), PcsError> {
    let client = PcsClient::new(
        api().options,
        PcsAuth::from_cookie_header("BDUSS=fake"),
        transport(),
    )?;
    let cloned = client.clone();
    assert!(std::sync::Arc::ptr_eq(&client.client, &cloned.client));
    Ok(())
}

#[test]
fn list_request_builds_pcs_url_with_ordering() {
    let request = api().list_request("/我的资源");
    assert_eq!(request.url.as_str(), "https://pcs.baidu.com/rest/2.0/pcs/file?app_id=266719&method=list&path=%2F%E6%88%91%E7%9A%84%E8%B5%84%E6%BA%90&by=name&order=asc");
}

#[test]
fn parse_list_response_preserves_file_model_fields() {
    let entries = parse_file_list_for_operation("list", &fixture_string("list_root.json"));
    assert!(entries.is_ok(), "list fixture should parse");
    let Ok(entries) = entries else { return };
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
    let files = parse_file_list_for_operation(
        "list",
        r#"{"list":[{"fs_id":10,"path":"/我的资源/example","server_filename":"example","isdir":1}]}"#,
    )?;
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
    assert!(matches!(error, PcsError::Remote { code: 31066, .. }));
}

#[test]
fn download_request_builds_readonly_pcs_file_url() {
    let request = api().download_request("/我的资源/notes.md");
    assert_eq!(request.url.as_str(), "https://pcs.baidu.com/rest/2.0/pcs/file?app_id=266719&method=download&path=%2F%E6%88%91%E7%9A%84%E8%B5%84%E6%BA%90%2Fnotes.md");
}

#[test]
fn locatedownload_request_builds_signed_pcs_url() {
    let sign = locatedownload_sign_with(
        1_571_140_066,
        "O|1E67351CCE80B2CF48DB511CD77ACD9F",
        10086,
        "test_bduss",
    );
    let request = api().locatedownload_request("/我的资源/movie.mp4", &sign);
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
fn locatedownload_sign_matches_reference_vector() {
    let sign = locatedownload_sign_with(
        1_571_140_066,
        "O|1E67351CCE80B2CF48DB511CD77ACD9F",
        10086,
        "test_bduss",
    );
    assert_eq!(sign.rand, "b6bb7a6f46899e181baea58798d4fdb889775c2c");
    assert_eq!(sign.time, 1_571_140_066);
    assert_eq!(sign.devuid, "O|1E67351CCE80B2CF48DB511CD77ACD9F");
}

#[test]
fn devuid_is_uppercase_md5_plus_suffix() {
    assert_eq!(dev_uid("test_bduss"), "66AE65D3C5D483FC4431FC43670A4673|0");
}

#[test]
fn mkdir_request_builds_post_file_url() {
    let request = api().mkdir_request("/docs/new folder");
    assert!(request
        .url
        .starts_with("https://pcs.baidu.com/rest/2.0/pcs/file?"));
    assert!(request.url.contains("method=mkdir"));
    assert!(request.url.contains("app_id=266719"));
    assert!(request.url.contains("path=%2Fdocs%2Fnew+folder"));
}

#[test]
fn search_request_records_recursive_flag_and_keyword() {
    let request = api().search_request("/docs", "report 2026", true);
    assert!(request.url.contains("method=search"));
    assert!(request.url.contains("path=%2Fdocs"));
    assert!(request.url.contains("wd=report+2026"));
    assert!(request.url.contains("re=1"));
}

#[test]
fn file_operation_request_builds_batch_operation_url() {
    assert!(api()
        .file_operation_request(FileOperation::Move)
        .url
        .contains("method=move"));
}

#[test]
fn delete_param_json_matches_go_reference_shape() -> Result<(), PcsError> {
    let json = delete_param_json(&["/a.txt".to_string(), "/dir/b.txt".to_string()])?;
    assert_eq!(
        json,
        r#"{"list":[{"path":"/a.txt"},{"path":"/dir/b.txt"}]}"#
    );
    Ok(())
}

#[test]
fn copy_move_param_json_matches_go_reference_shape() -> Result<(), PcsError> {
    let json = copy_move_param_json(&[FileOperationPair {
        from: "/a.txt".to_string(),
        to: "/backup/a.txt".to_string(),
    }])?;
    assert_eq!(json, r#"{"list":[{"from":"/a.txt","to":"/backup/a.txt"}]}"#);
    Ok(())
}

#[test]
fn parse_file_operation_response_accepts_errno_zero_and_rejects_remote_error() -> Result<(), String>
{
    parse_file_operation_response("mkdir", r#"{"errno":0}"#).map_err(|err| err.to_string())?;
    let err = parse_file_operation_response(
        "delete",
        r#"{"error_code":31070,"error_msg":"file delete failed"}"#,
    )
    .expect_err("remote delete error should fail");
    assert_eq!(
        err,
        PcsError::Remote {
            operation: "delete".to_string(),
            code: 31070,
            message: "file delete failed".to_string()
        }
    );
    Ok(())
}

#[test]
fn locatedownload_parser_selects_unencrypted_urls() {
    let info = parse_locatedownload(&fixture_string("locatedownload.json"));
    assert!(info.is_ok(), "locatedownload fixture should parse");
    let Ok(info) = info else { return };
    assert_eq!(
        select_download_urls(&info, true),
        vec!["https://fixture.invalid/download/hello.txt"]
    );
}

#[test]
fn locatedownload_parser_accepts_success_without_errno() -> Result<(), PcsError> {
    let info = parse_locatedownload(
        r#"{"urls":[{"url":"http://fixture.invalid/download/movie.mp4","encrypt":0}]}"#,
    )?;
    assert_eq!(select_download_urls(&info, true).len(), 1);
    Ok(())
}

#[test]
fn preview_range_header_is_bounded_to_requested_bytes() -> Result<(), PcsError> {
    let range = ByteRange::prefix(16)?;
    assert_eq!(range.header_value(), "bytes=0-15");
    assert_eq!(range.read_limit_bytes(), 16);
    Ok(())
}

#[test]
fn preview_range_rejects_zero_length_prefix() {
    assert!(ByteRange::prefix(0).is_err());
}

#[test]
fn preview_range_header_supports_seek_offsets() -> Result<(), PcsError> {
    let range = ByteRange::new(1024, 2047)?;
    assert_eq!(range.header_value(), "bytes=1024-2047");
    assert_eq!(range.read_limit_bytes(), 1024);
    Ok(())
}
