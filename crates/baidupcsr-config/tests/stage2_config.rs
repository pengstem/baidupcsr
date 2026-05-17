use std::path::{Path, PathBuf};

use baidupcsr_config::{
    active_user, discover_config_file, load_config, AuthPresence, ConfigDiscoveryInput,
    ConfigError, TargetOs,
};

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/config")
        .join(name)
}

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
        discover_config_file(&input),
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
        discover_config_file(&input),
        PathBuf::from("/home/example/.config/BaiduPCS-Go/pcs_config.json")
    );
}

#[test]
fn load_config_selects_active_user() {
    let config = load_config(&fixture_path("pcs_config_minimal.json"));
    assert!(config.is_ok(), "minimal fixture should parse");
    let config = match config {
        Ok(config) => config,
        Err(_) => return,
    };

    let user = active_user(&config);
    assert!(user.is_some(), "active user should exist");
    let user = match user {
        Some(user) => user,
        None => return,
    };

    assert_eq!(user.uid, 100000001);
    assert_eq!(user.workdir, "/");
    assert_eq!(
        user.auth_presence(),
        AuthPresence {
            bduss: true,
            stoken: true,
            cookies: false,
            access_token: false,
        }
    );
}

#[test]
fn load_config_extracts_auth_presence_from_cookie_string() {
    let config = load_config(&fixture_path("pcs_config_cookie_only.json"));
    assert!(config.is_ok(), "cookie fixture should parse");
    let config = match config {
        Ok(config) => config,
        Err(_) => return,
    };

    let user = active_user(&config);
    assert!(user.is_some(), "active user should exist");
    let user = match user {
        Some(user) => user,
        None => return,
    };

    assert_eq!(
        user.auth_presence(),
        AuthPresence {
            bduss: true,
            stoken: true,
            cookies: true,
            access_token: false,
        }
    );
}

#[test]
fn load_config_rejects_invalid_json_shape() {
    let err = load_config(&fixture_path("pcs_config_invalid.json"));
    assert!(matches!(err, Err(ConfigError::Parse { .. })));
}
