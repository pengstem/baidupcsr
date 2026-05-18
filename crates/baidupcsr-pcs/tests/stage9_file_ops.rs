use baidupcsr_pcs::{
    copy_move_param_json, delete_param_json, parse_file_operation_response, FileOperation,
    FileOperationPair, PcsApi, PcsClientOptions, PcsError,
};

fn api() -> PcsApi {
    PcsApi::new(PcsClientOptions {
        app_id: 266719,
        pcs_addr: "pcs.baidu.com".to_string(),
        enable_https: true,
    })
}

#[test]
fn mkdir_request_builds_post_file_url() {
    let request = api().mkdir_request("/docs/new folder");

    assert_eq!(request.method, "POST");
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

    assert_eq!(request.method, "GET");
    assert!(request.url.contains("method=search"));
    assert!(request.url.contains("path=%2Fdocs"));
    assert!(request.url.contains("wd=report+2026"));
    assert!(request.url.contains("re=1"));
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
fn file_operation_request_uses_post_method_for_batch_ops() {
    let request = api().file_operation_request(FileOperation::Move);

    assert_eq!(request.method, "POST");
    assert!(request.url.contains("method=move"));
}

#[test]
fn parse_file_operation_response_accepts_errno_zero_and_rejects_remote_error() -> Result<(), String>
{
    parse_file_operation_response("mkdir", r#"{"errno":0}"#).map_err(|err| err.to_string())?;

    let err = match parse_file_operation_response(
        "delete",
        r#"{"error_code":31070,"error_msg":"file delete failed"}"#,
    ) {
        Ok(()) => return Err("remote delete error should fail".to_string()),
        Err(err) => err,
    };

    assert_eq!(
        err,
        PcsError::Remote {
            operation: "delete".to_string(),
            code: 31070,
            message: "file delete failed".to_string(),
        }
    );
    Ok(())
}
