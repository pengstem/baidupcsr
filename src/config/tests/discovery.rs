use super::super::{
    cookie_has_key, discover_config_file_from_input, ConfigDiscoveryInput, TargetOs,
};
use std::path::PathBuf;

#[test]
fn discover_config_prefers_explicit_env_dir() {
    let input = ConfigDiscoveryInput {
        config_dir_env: Some(PathBuf::from("/tmp/baidupcsr-config")),
        executable_dir: PathBuf::from("/opt/baidupcsr"),
        home_dir: Some(PathBuf::from("/home/example")),
        appdata_dir: None,
        target_os: TargetOs::Unix,
    };

    assert_eq!(
        discover_config_file_from_input(&input),
        PathBuf::from("/tmp/baidupcsr-config/pcs_config.json")
    );
}

#[test]
fn discover_config_uses_unix_home_config_dir_when_no_env_or_legacy_file() {
    let input = ConfigDiscoveryInput {
        config_dir_env: None,
        executable_dir: PathBuf::from("/opt/baidupcsr"),
        home_dir: Some(PathBuf::from("/home/example")),
        appdata_dir: None,
        target_os: TargetOs::Unix,
    };

    assert_eq!(
        discover_config_file_from_input(&input),
        PathBuf::from("/home/example/.config/BaiduPCS-Go/pcs_config.json")
    );
}

#[test]
fn discover_config_uses_windows_appdata_when_target_is_windows() {
    let input = ConfigDiscoveryInput {
        config_dir_env: None,
        executable_dir: PathBuf::from("C:/Program Files/baidupcsr"),
        home_dir: Some(PathBuf::from("C:/Users/example")),
        appdata_dir: Some(PathBuf::from("C:/Users/example/AppData/Roaming")),
        target_os: TargetOs::Windows,
    };

    assert_eq!(
        discover_config_file_from_input(&input),
        PathBuf::from("C:/Users/example/AppData/Roaming/BaiduPCS-Go/pcs_config.json")
    );
}

#[test]
fn discover_config_uses_android_download_dir_when_target_is_android() {
    let input = ConfigDiscoveryInput {
        config_dir_env: None,
        executable_dir: PathBuf::from("/data/local/tmp"),
        home_dir: Some(PathBuf::from("/data/data/example")),
        appdata_dir: None,
        target_os: TargetOs::Android,
    };

    assert_eq!(
        discover_config_file_from_input(&input),
        PathBuf::from("/sdcard/Download/BaiduPCS-Go/pcs_config.json")
    );
}

#[test]
fn cookie_has_key_accepts_exact_key_with_nonempty_value() {
    assert!(cookie_has_key("foo=1; BDUSS=secret; bar=2", "BDUSS"));
}

#[test]
fn cookie_has_key_rejects_key_embedded_in_another_cookie_name() {
    assert!(!cookie_has_key("notbduss=x", "bduss"));
}

#[test]
fn cookie_has_key_rejects_empty_value() {
    assert!(!cookie_has_key("bduss=", "bduss"));
}
