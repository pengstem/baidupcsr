use super::super::{discover_config_file_from_input, ConfigDiscoveryInput, TargetOs};
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
