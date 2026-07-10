use std::{ffi::OsString, process::ExitCode};

#[tokio::test]
async fn public_library_entry_accepts_owned_os_arguments() {
    let result = baidupcsr::run([OsString::from("baidupcsr-cli"), OsString::from("--help")]).await;
    assert_eq!(result, ExitCode::SUCCESS);
}
