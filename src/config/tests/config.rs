use std::path::{Path, PathBuf};

use super::super::*;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/config")
        .join(name)
}

#[test]
fn load_config_selects_active_user() {
    let config = load_config(&fixture_path("pcs_config_minimal.json"));
    assert!(config.is_ok(), "minimal fixture should parse");
    let Ok(config) = config else {
        return;
    };

    let user = config.active_user();
    assert!(user.is_some(), "active user should exist");
    let Some(user) = user else {
        return;
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
    let Ok(config) = config else {
        return;
    };

    let user = config.active_user();
    assert!(user.is_some(), "active user should exist");
    let Some(user) = user else {
        return;
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
fn save_active_user_workdir_updates_default_directory() -> Result<(), ConfigError> {
    let dir = std::env::temp_dir().join(format!(
        "baidupcsr-pcs-config-workdir-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|source| ConfigError::Write {
        path: dir.clone(),
        source,
    })?;
    let path = dir.join("pcs_config.json");
    std::fs::copy(fixture_path("pcs_config_minimal.json"), &path).map_err(|source| {
        ConfigError::Write {
            path: path.clone(),
            source,
        }
    })?;

    save_active_user_workdir(&path, "/电影/科幻电影")?;
    let config = load_config(&path)?;
    let user = config
        .active_user()
        .ok_or_else(|| ConfigError::InvalidConfig {
            path: path.clone(),
            message: "active user missing after save".to_string(),
        })?;

    assert_eq!(user.workdir, "/电影/科幻电影");
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(dir);
    Ok(())
}

#[test]
fn load_config_rejects_invalid_json_shape() {
    let err = load_config(&fixture_path("pcs_config_invalid.json"));
    assert!(matches!(err, Err(ConfigError::Parse { .. })));
}

#[test]
fn ui_config_default_uses_user_mpv_profile_with_larger_video_budget() {
    let config = UiConfig::default();

    assert!(config.video_preview.enabled);
    assert_eq!(config.video_preview.chunk_size_bytes, 4 * 1024 * 1024);
    assert_eq!(config.video_preview.max_chunks, 32);
    assert_eq!(
        config.video_preview.player_profile,
        VideoPlayerProfile::User
    );
}

#[test]
fn ui_config_sidecar_can_select_isolated_mpv_profile() -> Result<(), ConfigError> {
    let dir = std::env::temp_dir().join(format!("baidupcsr-ui-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|source| ConfigError::Read {
        path: dir.clone(),
        source,
    })?;
    let path = dir.join("ui_config.toml");
    std::fs::write(
        &path,
        "video_preview_enabled = true\nvideo_chunk_size_bytes = 131072\nvideo_max_chunks = 3\nvideo_player_profile = \"isolated\"\n",
    )
    .map_err(|source| ConfigError::Read {
        path: path.clone(),
        source,
    })?;

    let config = load_ui_config_or_default(&path)?;

    assert!(config.video_preview.enabled);
    assert_eq!(config.video_preview.chunk_size_bytes, 131_072);
    assert_eq!(config.video_preview.max_chunks, 3);
    assert_eq!(
        config.video_preview.player_profile,
        VideoPlayerProfile::Isolated
    );
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(dir);
    Ok(())
}

#[test]
fn ui_config_rejects_zero_video_chunk_budget() {
    let path = std::env::temp_dir().join(format!(
        "baidupcsr-ui-config-invalid-{}.toml",
        std::process::id()
    ));
    let write_result = std::fs::write(
        &path,
        "video_preview_enabled = true\nvideo_chunk_size_bytes = 0\nvideo_max_chunks = 3\n",
    );
    assert!(write_result.is_ok());

    let result = load_ui_config_or_default(&path);

    assert!(matches!(result, Err(ConfigError::InvalidUiConfig { .. })));
    let _ = std::fs::remove_file(path);
}
